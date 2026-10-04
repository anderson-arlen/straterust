use super::*;
use straterust_engine::sim::Spawn;

fn flash(app: &App, id: EntityId) -> bool {
    let assets = app.assets.as_ref().unwrap();
    let actor = entity(app, id);
    let observed = app.visuals.get(id).unwrap();
    let body = visual::unit_image(assets, actor, Some(observed), &app.world).unwrap();
    let sprite = assets.sprite(UnitTypeId(1)).unwrap();
    let heading = usize::from(observed.facing);
    let heading = if heading > 16 { 32 - heading } else { heading };
    std::ptr::eq(body.image, &sprite.frames[51 + heading])
}

#[test]
#[ignore = "requires private mission 5; reproduces synchronized group fire at reduced redraw cadence"]
fn native_group_muzzle_flashes_survive_skipped_redraw_ticks() {
    let directory = PathBuf::from(std::env::var_os("STRATERUST_ASSET_PACKAGE").unwrap());
    let mut app = App::load(
        &directory,
        Config {
            audio: false,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    app.mission_ui = None;
    let mut map = app.world.map().clone();
    map.mission = None;
    map.ai.clear();
    map.terrain = None;
    map.fog_of_war = false;
    map.initial_explored.clear();
    map.creation.clear();
    map.resources.clear();
    map.start_locations.clear();
    map.players = 2;
    let mut results = Vec::new();
    for (target_type, title) in [(34, "Control Tower"), (21, "Goliath"), (23, "Wraith")] {
        for count in [9, 12] {
            let mut rules = app.world.rules().clone();
            rules.victory = false;
            rules
                .units
                .iter_mut()
                .find(|u| u.id == UnitTypeId(target_type))
                .unwrap()
                .max_hp = 10000;
            map.spawns = (0..count)
                .map(|i| {
                    let angle = i as f64 * std::f64::consts::TAU / count as f64;
                    Spawn {
                        unit_type: UnitTypeId(1),
                        position: Position {
                            x: 700 + (140.0 * angle.cos()).round() as i32,
                            y: 700 + (140.0 * angle.sin()).round() as i32,
                        },
                        ..Default::default()
                    }
                })
                .collect();
            map.spawns.push(Spawn {
                owner: PlayerId(1),
                unit_type: UnitTypeId(target_type),
                position: Position { x: 700, y: 700 },
                ..Default::default()
            });
            app.world = World::new(rules, map.clone(), 42).unwrap();
            app.visuals = Visuals::new(&app.world);
            let target = EntityId(count + 1);
            for id in 1..=count {
                app.issue(Order::Attack {
                    entity: EntityId(id),
                    target,
                })
                .unwrap();
            }
            let mut flashed = BTreeSet::new();
            for tick in 1..=7 {
                for outcome in app.world.step(&app.queue.take(app.world.tick())).unwrap() {
                    assert!(outcome.rejection.is_none(), "{outcome:?}");
                }
                app.visuals.update(&app.world);
                // Render at ~12 FPS instead of the simulation's ~24 ticks/sec.
                if tick % 2 == 1 {
                    for id in 1..=count {
                        if flash(&app, EntityId(id)) {
                            flashed.insert(id);
                        }
                    }
                    if target_type == 34 && count == 12 && tick == 3 {
                        app.camera.x = 700.0;
                        app.camera.y = 700.0;
                        capture(&app, "marine-group-skipped-frames", [1100, 760], None);
                    }
                    app.visuals.mark_rendered();
                }
            }
            assert!(entity(&app, target).hp > 0);
            assert!(
                (1..=count).all(|id| app.visuals.get(EntityId(id)).unwrap().shot_tick.is_some())
            );
            println!(
                "{count} Marines vs {title}: {} showed a flash",
                flashed.len()
            );
            results.push((title, count, flashed.len()));
        }
    }
    assert!(
        results
            .iter()
            .all(|(_, count, seen)| *count as usize == *seen),
        "missed group flashes: {results:?}"
    );
}
