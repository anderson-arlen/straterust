//! Headless host, protocol client and replay verifier. Uses the same session and
//! transport as the window; useful for direct LAN diagnostics without graphics.
use anyhow::{Context, Result, bail, ensure};
use std::{
    net::{SocketAddr, TcpListener},
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};
use straterust_engine::{
    content::Package,
    net::{self, RemoteClient, ServerMessage},
    session::INPUT_LEAD,
    sim::{Command, Order, PlayerId, Position, Tick},
};

fn main() -> std::process::ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            log::error!("{error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "help".into());
    if ["help", "--help", "-h"].contains(&mode.as_str()) {
        println!(
            "straterust-session host|join|replay|discover --package DIR\n  host: --address IP:port [--record-replay FILE]\n  join: --address IP:port --player 0|1 [--ticks N] [--attack-move X,Y] [--wire FILE]\n  replay: --replay FILE\n  discover: list matching and incompatible LAN hosts\nNetwork diagnostics use the same authoritative server/player protocol as the native client."
        );
        return Ok(());
    }
    let mut directory = PathBuf::from("content/lan-demo");
    let mut address: SocketAddr = "127.0.0.1:6112".parse()?;
    let mut player = PlayerId(1);
    let mut ticks = 100;
    let mut replay = None;
    let mut wire_path = None;
    let mut attack_move = None;
    while let Some(arg) = args.next() {
        let value = args
            .next()
            .with_context(|| format!("missing value for {arg}"))?;
        match arg.as_str() {
            "--package" => directory = value.into(),
            "--address" => address = value.parse()?,
            "--player" => player = PlayerId(value.parse()?),
            "--ticks" => ticks = value.parse::<u64>()?,
            "--record-replay" | "--replay" => replay = Some(PathBuf::from(value)),
            "--wire" => wire_path = Some(PathBuf::from(value)),
            "--attack-move" => {
                let (x, y) = value.split_once(',').context("use X,Y")?;
                attack_move = Some(Position {
                    x: x.parse()?,
                    y: y.parse()?,
                });
            }
            _ => bail!("unknown argument {arg}"),
        }
    }
    ensure!((1..=1_000_000).contains(&ticks), "invalid tick limit");
    let world = if mode == "join" || mode == "discover" {
        Package::client_definitions(&directory)?
    } else {
        Package::load(&directory)?.world(42)?
    };
    match mode.as_str() {
        "host" => {
            let listener = TcpListener::bind(address).context("cannot bind host")?;
            println!("host={}", listener.local_addr()?);
            let map = net::MapTransfer::load(&directory, &world)?;
            net::run_host_with_map(
                listener,
                world,
                42,
                Arc::new(AtomicBool::new(false)),
                replay.as_deref(),
                map,
            )
        }
        "replay" => {
            let replay = net::load_replay(&replay.context("--replay FILE required")?)?;
            let result = replay.play(&world)?;
            println!(
                "replay tick={} hash={} winner={:?}",
                result.tick().0,
                result.state_hash(),
                result.state().winner
            );
            Ok(())
        }
        "discover" => {
            for game in net::lan::discover(
                &straterust_engine::sim::GameplayIdentity::of(&world),
                Duration::from_secs(1),
            )? {
                println!(
                    "{} {} compatible={}",
                    game.address, game.name, game.compatible
                );
            }
            Ok(())
        }
        "join" => {
            let (mut client, initial) = RemoteClient::connect(address, &world, player)?;
            let mut wire = Vec::new();
            if wire_path.is_some() {
                wire.push(ron::ser::to_string(&ServerMessage::Welcome {
                    handshake: client.handshake.clone(),
                    initial: initial.clone(),
                })?);
            }
            let mut view = initial
                .view
                .into_world(&client.map.definitions(&world, player)?)?;
            let mut ordered = false;
            let mut started = false;
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                if let Some(message) = client.poll()? {
                    if wire_path.is_some() {
                        wire.push(ron::ser::to_string(&message)?);
                    }
                    match message {
                        ServerMessage::Status { started: value, .. } => started = value,
                        ServerMessage::Update(update) => {
                            for outcome in update.outcomes {
                                ensure!(
                                    outcome.rejection.is_none(),
                                    "command rejected: {:?}",
                                    outcome.rejection
                                );
                            }
                            view = update.view.into_world(&view)?;
                            if view.tick().0 >= ticks {
                                break;
                            }
                        }
                        ServerMessage::End(reason) => {
                            println!("end={reason} winner={:?}", view.state().winner);
                            break;
                        }
                        ServerMessage::Rejected(reason) => bail!("order rejected: {reason}"),
                        ServerMessage::Welcome { .. }
                        | ServerMessage::MapBegin { .. }
                        | ServerMessage::MapChunk { .. } => {
                            bail!("duplicate handshake or map transfer")
                        }
                    }
                }
                if started && !ordered {
                    if let Some(target) = attack_move {
                        let mut sequence = 0;
                        for entity in &view.state().entities {
                            if entity.owner == player
                                && view
                                    .unit_type(entity.unit_type)
                                    .is_some_and(|u| !u.structure)
                            {
                                sequence += 1;
                                client.command(Command {
                                    tick: Tick(view.tick().0 + INPUT_LEAD),
                                    player,
                                    sequence,
                                    order: Order::AttackMove {
                                        entity: entity.id,
                                        target,
                                    },
                                })?;
                            }
                        }
                    }
                    ordered = true;
                }
                ensure!(
                    Instant::now() < deadline,
                    "session verification deadline reached"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            if let Some(path) = wire_path {
                std::fs::write(path, wire.join("\n"))?;
            }
            println!(
                "player={} tick={} visible_entities={} winner={:?}",
                player.0,
                view.tick().0,
                view.state().entities.len(),
                view.state().winner
            );
            client.leave();
            Ok(())
        }
        _ => bail!("unknown mode {mode}"),
    }
}
