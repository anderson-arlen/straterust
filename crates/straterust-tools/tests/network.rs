//! Two independent client processes complete a short original Terran fight.
use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
};
use straterust_engine::{
    content::Package,
    net::{self, ServerMessage},
    sim::{PlayerId, ViewedEntity},
};

#[test]
fn two_process_clients_finish_combat_and_the_host_replay_matches() {
    let binary = env!("CARGO_BIN_EXE_straterust-session");
    let package = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-duel");
    let client_package = Path::new("/tmp/straterust-client-package");
    std::fs::create_dir_all(client_package).unwrap();
    std::fs::copy(package.join("rules.ron"), client_package.join("rules.ron")).unwrap();
    // A custom map exists only on the host. Neither client may read these files.
    std::fs::write(client_package.join("map.ron"), "THIS MUST NOT BE PARSED").unwrap();
    std::fs::write(
        client_package.join("mission.ron"),
        "THIS MUST NOT BE PARSED",
    )
    .unwrap();
    let replay_path = "/tmp/straterust-process-replay.ron";
    let mut host = Command::new(binary)
        .args(["host", "--package"])
        .arg(&package)
        .args(["--address", "127.0.0.1:0", "--record-replay", replay_path])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(host.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let address = line
        .trim()
        .strip_prefix("host=")
        .expect("host started on an ephemeral port");
    let first = Command::new(binary)
        .args(["join", "--package"])
        .arg(client_package)
        .args([
            "--address",
            address,
            "--player",
            "0",
            "--attack-move",
            "448,240",
            "--wire",
            "/tmp/straterust-client0-wire.ron",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let second = Command::new(binary)
        .args(["join", "--package"])
        .arg(client_package)
        .args([
            "--address",
            address,
            "--player",
            "1",
            "--wire",
            "/tmp/straterust-client1-wire.ron",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let outputs = [
        first.wait_with_output().unwrap(),
        second.wait_with_output().unwrap(),
    ];
    let server = host.wait_with_output().unwrap();
    assert!(
        server.status.success(),
        "{}",
        String::from_utf8_lossy(&server.stderr)
    );
    for output in outputs {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("winner=Some(PlayerId(0))"));
    }
    for (player, path) in [
        (PlayerId(0), "/tmp/straterust-client0-wire.ron"),
        (PlayerId(1), "/tmp/straterust-client1-wire.ron"),
    ] {
        let wire = std::fs::read_to_string(path).unwrap();
        assert!(!wire.contains("rng_state") && !wire.contains("last_sequences"));
        for line in wire.lines() {
            let message: ServerMessage = ron::from_str(line).unwrap();
            let view = match message {
                ServerMessage::Welcome { initial, .. } => Some(initial.view),
                ServerMessage::Update(update) => Some(update.view),
                _ => None,
            };
            if let Some(view) = view {
                assert!(view.entities.iter().all(|e| match e {
                    ViewedEntity::Owned(e) => e.owner == player,
                    ViewedEntity::Visible(e) => e.owner != player,
                }));
            }
        }
    }
    let replay = net::load_replay(Path::new(replay_path)).unwrap();
    assert!(!replay.ticks.iter().all(|tick| tick.commands.is_empty()));
    let world = replay
        .play(&Package::load(&package).unwrap().world(42).unwrap())
        .unwrap();
    assert_eq!(world.state().winner, Some(PlayerId(0)));
    assert_eq!(
        world.state_hash().to_hex().as_str(),
        replay.ticks.last().unwrap().hash
    );
}
