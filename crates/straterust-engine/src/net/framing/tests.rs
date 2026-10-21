use super::*;

#[test]
fn partial_frames_allow_thirty_seconds_to_complete() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut sending = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let mut receiving = Connection::new(listener.accept().unwrap().0).unwrap();
    let frame = encode_frame(&42_u32).unwrap();
    receiving.incoming.extend_from_slice(&frame[..4]);
    receiving.partial_since = Some(Instant::now() - Duration::from_secs(29));
    assert!(receiving.receive::<u32>().unwrap().is_none());
    sending.write_all(&frame[4..]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(value) = receiving.receive::<u32>().unwrap() {
            assert_eq!(value, 42);
            break;
        }
        assert!(Instant::now() < deadline);
    }
    receiving.incoming.extend_from_slice(&frame[..4]);
    receiving.partial_since = Some(Instant::now() - Duration::from_secs(31));
    assert!(
        receiving
            .receive::<u32>()
            .unwrap_err()
            .to_string()
            .contains("timed out")
    );
}

#[test]
fn compression_roundtrips_counts_wire_bytes_and_rejects_expansion_over_limit() {
    let text = "resource:wood,position:(x:100,y:200),amount:100;".repeat(1000);
    let frame = encode_frame(&text).unwrap();
    assert!(frame.len() < text.len() / 5);
    assert!(u32::from_le_bytes(frame[..4].try_into().unwrap()) & COMPRESSED != 0);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut sending =
        Connection::new(TcpStream::connect(listener.local_addr().unwrap()).unwrap()).unwrap();
    let mut receiving = Connection::new(listener.accept().unwrap().0).unwrap();
    let up = Arc::new(Traffic::default());
    let down = Arc::new(Traffic::default());
    sending.track(Arc::clone(&up));
    receiving.track(Arc::clone(&down));
    sending.send(&text).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(decoded) = receiving.receive::<String>().unwrap() {
            assert_eq!(decoded, text);
            break;
        }
        assert!(Instant::now() < deadline);
    }
    assert_eq!(up.totals(), [frame.len() as u64, 0]);
    assert_eq!(down.totals(), [0, frame.len() as u64]);
    let mut invalid = frame[4..].to_vec();
    invalid[..4].copy_from_slice(&(MAX_FRAME_BYTES as u32 + 1).to_le_bytes());
    assert!(decode::<String>(&invalid, true).is_err());
    assert!(decode::<String>(&[1, 0, 0, 0, 255], true).is_err());
    assert_eq!(
        u32::from_le_bytes(encode_frame(&42).unwrap()[..4].try_into().unwrap()) & COMPRESSED,
        0
    );
}
