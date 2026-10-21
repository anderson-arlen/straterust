//! A real TCP proxy with controlled interruptions in either direction.
use super::*;
use std::{
    collections::VecDeque,
    net::{TcpStream, UdpSocket},
};

struct Proxy {
    address: SocketAddr,
    hold: Arc<AtomicBool>,
    hold_up: Arc<AtomicBool>,
    loss: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Proxy {
    fn new(server: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let hold = Arc::new(AtomicBool::new(false));
        let hold_up = Arc::new(AtomicBool::new(false));
        let loss = Arc::new(AtomicBool::new(false));
        let losing = Arc::clone(&loss);
        let stop = Arc::new(AtomicBool::new(false));
        let (holding, stopped) = (Arc::clone(&hold), Arc::clone(&stop));
        let holding_up = Arc::clone(&hold_up);
        let thread = std::thread::spawn(move || {
            let guest = loop {
                if stopped.load(Ordering::Acquire) {
                    return;
                }
                match listener.accept() {
                    Ok((guest, _)) => break guest,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1))
                    }
                    Err(e) => panic!("proxy accept: {e}"),
                }
            };
            let mut host = net::Connection::new(TcpStream::connect(server).unwrap()).unwrap();
            let mut guest = net::Connection::new(guest).unwrap();
            let socket = UdpSocket::bind(address).unwrap();
            socket.set_nonblocking(true).unwrap();
            let (mut up, mut down) = (VecDeque::new(), VecDeque::new());
            let mut guest_udp = None;
            let mut packets = 0;
            'proxy: while !stopped.load(Ordering::Acquire) {
                for _ in 0..64 {
                    match guest.receive::<net::ClientMessage>() {
                        Ok(Some(message)) => {
                            if host.send(&message).is_err() {
                                break 'proxy;
                            }
                        }
                        Ok(None) => break,
                        Err(_) => break 'proxy,
                    }
                }
                for _ in 0..64 {
                    match host.receive::<ServerMessage>() {
                        Ok(Some(mut message)) => {
                            if let ServerMessage::Welcome { udp, .. } = &mut message {
                                udp.port = address.port();
                            }
                            if guest.send(&message).is_err() {
                                break 'proxy;
                            }
                        }
                        Ok(None) => break,
                        Err(_) => break 'proxy,
                    }
                }
                let _ = host.flush();
                let _ = guest.flush();
                let mut buffer = [0; 1500];
                while let Ok((n, source)) = socket.recv_from(&mut buffer) {
                    packets += 1;
                    if losing.load(Ordering::Acquire) && packets % 3 == 0 {
                        continue;
                    }
                    if source == server {
                        down.push_back(buffer[..n].to_vec());
                    } else {
                        guest_udp = Some(source);
                        up.push_back(buffer[..n].to_vec());
                    }
                }
                if !holding_up.load(Ordering::Acquire) {
                    while let Some(packet) = up.pop_front() {
                        let _ = socket.send_to(&packet, server);
                    }
                }
                if !holding.load(Ordering::Acquire)
                    && let Some(destination) = guest_udp
                {
                    while let Some(packet) = down.pop_front() {
                        let _ = socket.send_to(&packet, destination);
                        if losing.load(Ordering::Acquire) {
                            let _ = socket.send_to(&packet, destination);
                        }
                    }
                }
                assert!(up.len() + down.len() < 4096);
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        Self {
            address,
            hold,
            hold_up,
            loss,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap();
    }
}
#[test]
fn prediction_moves_through_a_second_without_updates_and_recovers_from_a_burst() {
    interrupted_match(Duration::from_millis(1100), false);
}

#[test]
fn hosting_display_keeps_accepting_updates_with_an_incorrect_clock_estimate() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-demo");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let config = Config {
        audio: false,
        ..Config::default()
    };
    let mut host = App::load_network(&directory, config.clone()).unwrap();
    host.start_network(&directory, address, true, None).unwrap();
    let mut guest = App::load_network(&directory, config).unwrap();
    guest
        .start_network(&directory, address, false, None)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while host.paused || host.network.as_ref().unwrap().prediction.is_none() {
        host.advance_network().unwrap();
        guest.advance_network().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let before = host.world.tick();
    let mut clock = net::clock::ClockSync::default();
    clock.observe(
        net::clock::Pong {
            echo_ms: 0,
            received_ms: 1_000_000,
            sent_ms: 1_000_000,
        },
        0,
    );
    *host.network.as_ref().unwrap().clock.lock().unwrap() = clock;
    let until = Instant::now() + Duration::from_secs(2);
    while host.world.tick().0 < before.0 + 10 {
        host.advance_network().unwrap();
        guest.advance_network().unwrap();
        assert!(Instant::now() < until, "host froze behind its age filter");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        host.network
            .as_ref()
            .unwrap()
            .applied
            .load(Ordering::Acquire)
            > 0
    );
}

#[test]
fn both_peers_survive_an_eleven_second_interruption_and_resume_commands() {
    interrupted_match(Duration::from_secs(11), true);
}

#[test]
fn final_results_wait_for_udp_recovery_instead_of_ending_before_delivery() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-duel");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let mut host = App::load_network(
        &directory,
        Config {
            audio: false,
            ..Config::default()
        },
    )
    .unwrap();
    host.start_network(&directory, address, true, None).unwrap();
    let proxy = Proxy::new(address);
    let mut guest = App::load_network(
        &directory,
        Config {
            audio: false,
            ..Config::default()
        },
    )
    .unwrap();
    guest
        .start_network(&directory, proxy.address, false, None)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while guest.network.as_ref().unwrap().prediction.is_none() || guest.paused {
        host.advance_network().unwrap();
        guest.advance_network().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    proxy.hold.store(true, Ordering::Release);
    host.issue(Order::AttackMove {
        entity: EntityId(1),
        target: Position { x: 448, y: 240 },
    })
    .unwrap();
    while host.match_result.is_none() {
        host.advance_network().unwrap();
        guest.advance_network().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let until = Instant::now() + Duration::from_millis(1100);
    while Instant::now() < until {
        host.advance_network().unwrap();
        guest.advance_network().unwrap();
        assert!(guest.match_result.is_none());
        assert!(!guest.network.as_ref().unwrap().ended, "{}", guest.status);
        std::thread::sleep(Duration::from_millis(2));
    }
    proxy.hold.store(false, Ordering::Release);
    while guest.match_result.is_none() || !guest.network.as_ref().unwrap().ended {
        host.advance_network().unwrap();
        guest.advance_network().unwrap();
        assert!(Instant::now() < deadline, "{}", guest.status);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(host.match_result, guest.match_result);
}

fn interrupted_match(gap: Duration, both_directions: bool) {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-demo");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let mut host = App::load_network(
        &directory,
        Config {
            audio: false,
            ..Config::default()
        },
    )
    .unwrap();
    host.start_network(&directory, address, true, None).unwrap();
    let proxy = Proxy::new(address);
    let mut guest = App::load_network(
        &directory,
        Config {
            audio: false,
            ..Config::default()
        },
    )
    .unwrap();
    guest
        .start_network(&directory, proxy.address, false, None)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8) + gap;
    while guest.network.as_ref().unwrap().prediction.is_none() || guest.paused {
        host.advance_network().unwrap();
        guest.advance_network().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let actor = guest
        .world
        .state()
        .entities
        .iter()
        .find(|e| {
            e.owner == guest.world.view_player()
                && guest.world.unit_type(e.unit_type).unwrap().worker.is_some()
        })
        .unwrap()
        .id;
    guest
        .issue(Order::Move {
            entity: actor,
            target: Position { x: 1184, y: 200 },
        })
        .unwrap();
    let until = Instant::now() + Duration::from_millis(250);
    while Instant::now() < until {
        host.advance_network().unwrap();
        guest.advance_network().unwrap();
        std::thread::sleep(Duration::from_millis(4));
    }
    proxy.hold.store(true, Ordering::Release);
    proxy.hold_up.store(both_directions, Ordering::Release);
    let before = guest
        .world
        .state()
        .entities
        .iter()
        .find(|e| e.id == actor)
        .unwrap()
        .position;
    let until = Instant::now() + gap;
    while Instant::now() < until {
        host.advance_network().unwrap();
        guest.advance_network().unwrap();
        assert!(!host.network.as_ref().unwrap().ended, "{}", host.status);
        assert!(!guest.network.as_ref().unwrap().ended, "{}", guest.status);
        std::thread::sleep(Duration::from_millis(4));
    }
    let after = guest
        .world
        .state()
        .entities
        .iter()
        .find(|e| e.id == actor)
        .unwrap()
        .position;
    assert!(
        after.y < before.y - 50,
        "prediction stopped during the gap: {before:?} -> {after:?}"
    );
    proxy.hold.store(false, Ordering::Release);
    proxy.hold_up.store(false, Ordering::Release);
    let until = Instant::now() + Duration::from_millis(500);
    let mut previous = after.y;
    while Instant::now() < until {
        host.advance_network().unwrap();
        guest.advance_network().unwrap();
        let y = guest
            .world
            .state()
            .entities
            .iter()
            .find(|e| e.id == actor)
            .unwrap()
            .position
            .y;
        if !both_directions {
            assert!(
                y <= previous + 5,
                "burst rewound movement: {previous} -> {y}"
            );
        }
        previous = y;
        std::thread::sleep(Duration::from_millis(4));
    }
    assert!(!guest.network.as_ref().unwrap().ended);
    proxy.loss.store(true, Ordering::Release);
    guest.issue(Order::Stop { entity: actor }).unwrap();
    let stop_id = guest.recorded.last().unwrap().sequence;
    loop {
        host.advance_network().unwrap();
        guest.advance_network().unwrap();
        let confirmed = &guest
            .network
            .as_ref()
            .unwrap()
            .prediction
            .as_ref()
            .unwrap()
            .confirmed;
        if guest
            .network
            .as_ref()
            .unwrap()
            .command_acks
            .lock()
            .unwrap()
            .contains(stop_id)
            && confirmed
                .state()
                .entities
                .iter()
                .any(|e| e.id == actor && matches!(e.order, UnitOrder::Idle))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "commands did not resume: {}",
            guest.status
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
