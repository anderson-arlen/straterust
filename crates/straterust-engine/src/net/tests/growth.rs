//! Opt-in end-to-end tick costs on a growing native eight-player match.
use super::*;
use crate::multiplayer::{Catalog, SlotKind};
use crate::sim::{Map, Rules, WorldSnapshot};

#[test]
#[ignore = "requires STRATERUST_GAME_ROOT; release-mode native eight-player profiling"]
fn retail_eight_player_match_profile() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("STRATERUST_GAME_ROOT").unwrap());
    let catalog = Catalog::load(&root)?;
    let map = catalog
        .maps
        .iter()
        .position(|m| m.package.contains("bac6eef937ed93cf"))
        .unwrap();
    let mut settings = catalog.defaults(map);
    settings.random_locations = false;
    for (index, slot) in settings.slots.iter_mut().enumerate() {
        slot.kind = if index == 0 {
            SlotKind::Open
        } else {
            SlotKind::Computer
        };
        slot.faction = Some(index % 2);
    }
    let mut world = catalog.prepare(&root, map, &settings, &[0], 42)?.world;
    let resume = std::env::var_os("STRATERUST_PROFILE_RESUME").is_some();
    if resume {
        let (rules, map, snapshot): (Rules, Map, WorldSnapshot) = ron::from_str(
            &std::fs::read_to_string("/tmp/stratarust-profile-world.ron")?,
        )?;
        world = World::new(rules, map, 42)?.restore_saved_snapshot(snapshot, None)?;
    }
    let first_tick = world.tick().0;
    let mut server = ServerSession::new(world, 42, vec![PlayerId(0)])?;
    let initial = server.update(PlayerId(0), &[])?;
    let mut confirmed = initial.view.clone().into_world(server.world())?;
    let mut receiver = replication::Receiver::new(&initial);
    let mut sender = replication::Sender::new(initial);
    let ticks = std::env::var("STRATERUST_PROFILE_TICKS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50000);
    let mut samples: [Vec<u128>; 4] = Default::default();
    let mut bytes = 0usize;
    for tick in 1..=ticks {
        let start = Instant::now();
        let outcomes = server.advance(&[])?;
        samples[0].push(start.elapsed().as_micros());
        let start = Instant::now();
        sender.observe(server.update(PlayerId(0), &outcomes)?)?;
        samples[1].push(start.elapsed().as_micros());
        let start = Instant::now();
        let packet = framing::encode_frame(&ServerMessage::Delta(sender.packet().unwrap()))?;
        bytes += packet.len();
        let ServerMessage::Delta(delta) = framing::decode_frame(&packet)? else {
            unreachable!()
        };
        samples[2].push(start.elapsed().as_micros());
        let start = Instant::now();
        if let Some(update) = receiver.apply(delta)? {
            if tick % 5 == 0 {
                sender.acknowledge(update.sequence)?;
            }
            confirmed = update.view.into_world(&confirmed)?;
        }
        samples[3].push(start.elapsed().as_micros());
        if tick % 2500 == 0 {
            let costs: Vec<_> = samples
                .iter_mut()
                .map(|s| {
                    s.sort_unstable();
                    let result = (
                        s.iter().sum::<u128>() / s.len() as u128,
                        s[s.len() * 95 / 100],
                        *s.last().unwrap(),
                    );
                    s.clear();
                    result
                })
                .collect();
            eprintln!(
                "tick={tick} actors={} resources={} result={} average/p95/max_us=[step,view,codec,restore]={costs:?} bytes/tick={}",
                server.world().state().entities.len(),
                server.world().state().resources.len(),
                server.match_result().is_some(),
                bytes / 2500
            );
            bytes = 0;
        }
    }
    assert_eq!(confirmed.tick().0, first_tick + ticks);
    eprintln!("final_hash={}", server.world().state_hash());
    if !resume && std::env::var_os("STRATERUST_PROFILE_PHASES").is_some() {
        let saved = (
            server.world().rules(),
            server.world().map(),
            server.world().save_snapshot()?,
        );
        std::fs::write(
            "/tmp/stratarust-profile-world.ron",
            ron::ser::to_string(&saved)?,
        )?;
    }
    Ok(())
}
