use super::*;

fn endpoint() -> Endpoint {
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").unwrap());
    Endpoint::new(
        socket,
        Offer {
            port: 1,
            token: [7; 16],
        },
        "127.0.0.1".parse().unwrap(),
        None,
        Arc::default(),
    )
    .unwrap()
}
fn data(seed: u64) -> Vec<u8> {
    (0..300)
        .flat_map(|i| *blake3::hash(&(seed * 1000 + i).to_le_bytes()).as_bytes())
        .collect()
}

#[test]
fn newer_snapshot_completes_despite_missing_older_fragment_and_reordered_duplicates() {
    let mut receiver = endpoint();
    let source = "127.0.0.1:1234".parse().unwrap();
    let older = packets(&[7; 16], 1, true, &framing::encode_frame(&data(1)).unwrap());
    let newer = packets(&[7; 16], 2, true, &framing::encode_frame(&data(2)).unwrap());
    assert!(older.len() > 2 && newer.len() > 2);
    assert!(receiver.receive::<Vec<u8>>(&older[0], source).is_none());
    let mut got = None;
    for packet in newer.iter().rev() {
        got = receiver.receive::<Vec<u8>>(packet, source).or(got);
        assert!(receiver.receive::<Vec<u8>>(packet, source).is_none());
    }
    assert_eq!(got.unwrap(), data(2));
    for packet in older {
        assert!(receiver.receive::<Vec<u8>>(&packet, source).is_none());
    }
}

#[test]
fn malformed_oversized_wrong_session_and_wrong_source_packets_are_ignored() {
    let mut receiver = endpoint();
    let source = "127.0.0.1:1234".parse().unwrap();
    let valid = packets(&[7; 16], 1, false, &framing::encode_frame(&42_u32).unwrap()).remove(0);
    for packet in [
        vec![0; 4],
        vec![0; PACKET_BYTES + 1],
        {
            let mut p = valid.clone();
            p[4] ^= 1;
            p
        },
        {
            let mut p = valid.clone();
            p[33..37].copy_from_slice(&u32::MAX.to_le_bytes());
            p
        },
    ] {
        assert!(receiver.receive::<u32>(&packet, source).is_none());
    }
    assert!(
        receiver
            .receive::<u32>(&valid, "127.0.0.2:1234".parse().unwrap())
            .is_none()
    );
    assert_eq!(receiver.receive::<u32>(&valid, source), Some(42));
}
