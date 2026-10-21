use super::*;

#[test]
#[ignore = "verifies retail Human/Orc worker-only openings; requires STRATERUST_GAME_ROOT"]
fn retail_six_player_ai_establishes_its_economy() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(
        std::env::var_os("STRATERUST_GAME_ROOT").expect("native game directory"),
    );
    let catalog = Catalog::load(&root)?;
    let map = catalog.maps.iter().position(|m| m.players == 6).unwrap();
    for faction in 0..2 {
        let mut settings = catalog.defaults(map);
        settings.map_units = false;
        settings.random_locations = false;
        for (i, slot) in settings.slots.iter_mut().enumerate() {
            slot.kind = if i < 5 {
                SlotKind::Open
            } else if i == 5 {
                SlotKind::Computer
            } else {
                SlotKind::Closed
            };
            slot.faction = Some(faction);
            slot.team = if i < 3 { 0 } else { 1 };
        }
        let mut world = catalog
            .prepare(&root, map, &settings, &[0, 1, 2, 3, 4], 42)?
            .world;
        let player = PlayerId(5);
        let worker = catalog.factions[faction].worker;
        let mut complete_at = None;
        for tick in 0..5000 {
            world.step(&[])?;
            let center = world.state().entities.iter().find(|e| {
                e.owner == player
                    && world
                        .unit_type(e.unit_type)
                        .is_some_and(|u| !u.dropoff.is_empty() && u.trains.contains(&worker))
            });
            if center.is_some_and(|e| e.construction.is_none()) {
                complete_at.get_or_insert(tick);
            }
            if complete_at.is_none() {
                assert!(
                    world
                        .state()
                        .entities
                        .iter()
                        .filter(|e| e.owner == player)
                        .all(|e| {
                            !world.unit_type(e.unit_type).unwrap().structure
                                || !world.unit_type(e.unit_type).unwrap().dropoff.is_empty()
                        }),
                    "military/supply spending before hall completion"
                );
            }
            if complete_at.is_some()
                && world
                    .state()
                    .entities
                    .iter()
                    .filter(|e| e.owner == player && e.unit_type == worker)
                    .count()
                    > 1
                && world.state().statistics[5]
                    .resources_collected
                    .get("gold")
                    .copied()
                    .unwrap_or(0)
                    > 0
            {
                break;
            }
        }
        let count = world
            .state()
            .entities
            .iter()
            .filter(|e| e.owner == player && e.unit_type == worker)
            .count();
        let gathered = world.state().statistics[5]
            .resources_collected
            .get("gold")
            .copied()
            .unwrap_or(0);
        assert!(
            complete_at.is_some(),
            "{} never completed its hall",
            catalog.factions[faction].name
        );
        if count <= 1 {
            eprintln!(
                "AI resources={:?} supply={:?} requests={:?}",
                world.state().players[5],
                world.supply(player),
                world.state().ai
            );
            for entity in world.state().entities.iter().filter(|e| e.owner == player) {
                eprintln!("AI entity={entity:?}");
            }
            for node in world
                .state()
                .resources
                .iter()
                .filter(|n| n.kind == "wood")
                .take(4)
            {
                let worker_id = world
                    .state()
                    .entities
                    .iter()
                    .find(|e| e.owner == player && e.unit_type == worker)
                    .unwrap()
                    .id;
                eprintln!(
                    "wood={node:?} rejection={:?}",
                    world.gather_rejection(worker_id, node.id)
                );
            }
        }
        assert!(
            count > 1 && gathered > 0,
            "AI economy failed: workers={count} gold={gathered}"
        );
        println!(
            "faction={} map={:?} hall_completed_tick={complete_at:?} workers={count} gold_delivered={gathered}",
            catalog.factions[faction].name, catalog.maps[map].title
        );
    }
    Ok(())
}
