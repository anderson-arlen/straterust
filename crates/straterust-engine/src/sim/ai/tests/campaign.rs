//! Bounded original-script checks; imported content stays private.
use super::*;

#[test]
#[ignore = "requires refreshed private first-five campaign packages"]
fn native_campaign_ai_source_settings_and_first_parties() {
    let root = std::path::PathBuf::from(std::env::var_os("STRATERUST_CAMPAIGN_PACKAGE").unwrap());
    for number in [3, 5] {
        let source = Package::load(&root.join(format!("terran{number:02}")))
            .unwrap()
            .world(42)
            .unwrap();
        let mut controller = source
            .map
            .ai
            .iter()
            .find(|c| c.program.iter().any(|i| matches!(i, AiInstruction::Attack)))
            .unwrap()
            .clone();
        let first_attack = controller
            .program
            .iter()
            .position(|i| matches!(i, AiInstruction::Attack))
            .unwrap();
        let waits: u32 = controller.program[..first_attack]
            .iter()
            .filter_map(|i| {
                if let AiInstruction::Wait(t) = i {
                    Some(*t)
                } else {
                    None
                }
            })
            .sum();
        assert_eq!(waits, if number == 3 { 1252 } else { 6502 });
        assert!(
            controller
                .program
                .iter()
                .any(|i| matches!(i, AiInstruction::Defense { .. }))
        );
        if number == 5 {
            assert!(source.creation_allowed(controller.player, UnitTypeId(53)));
            let mut funded = source.clone();
            funded.state.players[usize::from(controller.player.0)].resources =
                BTreeMap::from([("minerals".into(), 6000), ("gas".into(), 6000)]);
            let starport = funded
                .state
                .entities
                .iter()
                .find(|e| {
                    e.owner == controller.player
                        && e.unit_type == UnitTypeId(33)
                        && rts::distance(e.position, controller.home)
                            <= i64::from(controller.radius).pow(2)
                })
                .unwrap()
                .id;
            let addon = funded.addon_position(starport).unwrap();
            assert_eq!(
                funded.build_rejection(controller.player, starport, UnitTypeId(34), addon),
                None,
                "original AI Starport must be able to attach its transport prerequisite"
            );

            let mut party = AiState::new(&controller);
            party.attack.insert(UnitTypeId(11), 4);
            assert!(
                source.ai_needs_transport(&controller, &party),
                "original mission-5 ground party needs a transport route"
            );

            assert!(
                controller.program[..first_attack].contains(&AiInstruction::AttackAdd {
                    unit_type: UnitTypeId(11),
                    count: 4
                })
            );
        }
        // Exercise original requests/party counts on a small isolated town. Shorten
        // timers only here; assertions above verify the unchanged source waits.
        controller.active = true;
        controller.program.truncate(first_attack + 1);
        for instruction in &mut controller.program {
            if let AiInstruction::Wait(ticks) = instruction {
                *ticks = 16;
            }
        }
        let mut rules = source.rules().clone();
        for unit in &mut rules.units {
            unit.build_ticks = 16;
        }
        let mut map = source.map().clone();
        map.mission = None;
        map.terrain = None;
        map.fog_of_war = false;
        map.initial_explored.clear();
        map.resources.clear();
        map.spawns.retain(|s| {
            s.owner == controller.player
                && rts::distance(s.position, controller.home) <= 900_i64.pow(2)
        });
        map.spawns.push(Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(3),
            position: map
                .start_locations
                .iter()
                .find(|s| s.player == PlayerId(0))
                .unwrap()
                .position,
            ..Default::default()
        });
        map.ai = vec![controller.clone()];
        let mut world = World::new(rules, map, 42).unwrap();
        world.state.players[usize::from(controller.player.0)].resources =
            BTreeMap::from([("minerals".into(), 6000), ("gas".into(), 6000)]);
        let started = std::time::Instant::now();
        for _ in 0..320 {
            world.step(&[]).unwrap();
            if !world.state.ai[0].deployed.is_empty() {
                break;
            }
        }
        let state = &world.state.ai[0];
        assert!(
            !state.deployed.is_empty(),
            "mission={number} state={state:?}"
        );
        assert!(world.resource_balance(controller.player, "minerals") < 6000);
        eprintln!(
            "source mission={number} first party deployed={} tick={} elapsed={:?}",
            state.deployed.len(),
            world.tick().0,
            started.elapsed()
        );
    }
}
