//! Opt-in visual review artifacts driven through the playable controls.
use super::*;
use crate::controls::{Action, BUILD_GRID, button_rect};
use crate::visual::VisualAction;
use straterust_engine::sim::{Entity, UnitTypeId};

fn advance(app: &mut App) {
    app.visuals.mark_rendered();
    app.visuals.advance_effects(
        Duration::from_millis(u64::from(app.world.rules().tick_ms)),
        app.assets.as_ref(),
    );
    for outcome in app.world.step(&app.queue.take(app.world.tick())).unwrap() {
        assert!(outcome.rejection.is_none(), "{:?}", outcome);
    }
    app.visuals.update(&app.world);
    if let (Some(mission), Some(media)) = (&mut app.mission_ui, &app.media) {
        mission.advance(
            Duration::from_millis(u64::from(app.world.rules().tick_ms)),
            media,
            &mut app.audio,
        );
        if let Some(position) = mission.observe(&app.world, media, &mut app.audio) {
            app.camera.x = f64::from(position.x);
            app.camera.y = f64::from(position.y);
        }
    }
}

fn until(app: &mut App, predicate: impl Fn(&App) -> bool, limit: usize) {
    for _ in 0..limit {
        if predicate(app) {
            return;
        }
        advance(app);
    }
    assert!(
        predicate(app),
        "review scenario stalled at tick {}",
        app.world.tick().0
    );
}

fn completed(app: &App, unit_type: u16) -> Option<EntityId> {
    app.world
        .state()
        .entities
        .iter()
        .find(|entity| {
            entity.owner == PlayerId(0)
                && entity.unit_type.0 == unit_type
                && entity.construction.is_none()
        })
        .map(|entity| entity.id)
}

fn entity(app: &App, id: EntityId) -> &Entity {
    app.world
        .state()
        .entities
        .iter()
        .find(|entity| entity.id == id)
        .expect("review entity survives")
}

/// Compare the actual drawn native pixels and placement, not just clip labels.
/// Source bytes remain in this opt-in test's memory and private screenshots.
fn pose(app: &App, id: EntityId) -> Option<Vec<u8>> {
    let assets = app.assets.as_ref()?;
    let entity = entity(app, id);
    let observed = app.visuals.get(id);
    let body = visual::unit_image(assets, entity, observed, &app.world)
        .expect("reviewed unit has native body art");
    let effect = visual::work_effect(assets, entity, observed, &app.world);
    let mut bytes = Vec::new();
    for frame in [Some(body), effect].into_iter().flatten() {
        bytes.extend(frame.image.width.to_le_bytes());
        bytes.extend(frame.image.height.to_le_bytes());
        bytes.extend(frame.anchor[0].to_le_bytes());
        bytes.extend(frame.anchor[1].to_le_bytes());
        bytes.push(u8::from(frame.flip_x));
        bytes.extend_from_slice(&frame.image.rgba);
    }
    Some(bytes)
}

fn assert_work_effect_visible(app: &App, id: EntityId) {
    if let Some(assets) = &app.assets {
        let frame = visual::work_effect(assets, entity(app, id), app.visuals.get(id), &app.world)
            .expect("actual native work selects a drill effect");
        assert!(
            frame
                .image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 0),
            "work review must not capture a transparent effect"
        );
    }
}

fn capture(app: &App, name: &str, size: [u32; 2], hover: Option<Action>) {
    capture_at(
        app,
        name,
        size,
        hover,
        None,
        u128::from(app.world.tick().0) * 50,
    );
}

fn capture_at(
    app: &App,
    name: &str,
    size: [u32; 2],
    hover: Option<Action>,
    speaking: Option<UnitTypeId>,
    animation_ms: u128,
) -> Vec<u32> {
    let gameplay_hash = app.world.state_hash();
    let buttons = app.buttons();
    let logical = size.map(f64::from);
    let cursor = hover
        .and_then(|action| buttons.iter().find(|button| button.action == action))
        .map_or_else(
            || app.logical_cursor(),
            |button| {
                let [x, y, w, h] = button_rect(
                    button.slot,
                    logical,
                    controls::native_ui(app.assets.as_ref()),
                );
                [x + w / 2.0, y + h / 2.0]
            },
        );
    let mut camera = app.camera;
    camera.clamp_to_map([app.world.map().width, app.world.map().height], logical);
    let mut pixels = vec![0; (size[0] * size[1]) as usize];
    View {
        world: &app.world,
        visuals: &app.visuals,
        cursor,
        targeting: app.target_mode.is_some(),
        presentation: &app.presentation,
        assets: app.assets.as_ref(),
        media: app.media.as_ref(),
        speaking,
        mission: app.mission_ui.as_ref(),
        animation_ms,
        portrait_ms: animation_ms,
        camera,
        selected: &app.selected,
        selected_resource: app.selected_resource,
        drag_box: None,
        paused: app.paused
            || app
                .world
                .state()
                .mission
                .as_ref()
                .is_some_and(|mission| mission.paused),
        playback: false,
        status: &app.status,
        buttons: &buttons,
        help: "SHIFT QUEUE  CTRL+0-9 GROUPS  F5 RESTART",
        placement: app.placement(),
        ending_hint: "F5 RESTART",
    }
    .draw(&mut pixels, size[0], size[1], 1.0);
    assert_eq!(
        app.world.state_hash(),
        gameplay_hash,
        "painting changed gameplay"
    );
    let path = format!("/tmp/straterust-review-{name}.ppm");
    let mut file = BufWriter::new(File::create(&path).unwrap());
    writeln!(file, "P6\n{} {}\n255", size[0], size[1]).unwrap();
    for pixel in &pixels {
        file.write_all(&[(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8])
            .unwrap();
    }
    file.flush().unwrap();
    eprintln!(
        "visual review tick={} screenshot={path}",
        app.world.tick().0
    );
    pixels
}

fn review_portrait(app: &App, name: &str) {
    let Some(media) = &app.media else { return };
    let selected = entity(app, *app.selected.first().unwrap());
    let portrait = media.portrait(selected.unit_type).unwrap();
    let first = &portrait.idle[0].rgba;
    let different = portrait
        .idle
        .iter()
        .position(|image| image.rgba != *first)
        .expect("source idle portrait contains distinct poses");
    let hash = app.world.state_hash();
    let idle = capture_at(
        app,
        &format!("portrait-{name}-idle"),
        [1280, 800],
        None,
        None,
        0,
    );
    let animated = capture_at(
        app,
        &format!("portrait-{name}-animated"),
        [1280, 800],
        None,
        None,
        different as u128 * u128::from(portrait.frame_ms),
    );
    let talk = capture_at(
        app,
        &format!("portrait-{name}-talk"),
        [1280, 800],
        None,
        Some(selected.unit_type),
        0,
    );
    // Inspect only the portrait panel so world animation cannot satisfy the check.
    let [x, y, width, height] = if controls::native_ui(app.assets.as_ref()) {
        let scale = controls::native_ui_scale([1280.0, 800.0]);
        [
            1280.0 - 225.0 * scale,
            800.0 - 69.0 * scale,
            60.0 * scale,
            56.0 * scale,
        ]
        .map(|value| value as usize)
    } else {
        [228, 610, 76, 70]
    };
    let panel = |pixels: &[u32]| -> Vec<u32> {
        (y..y + height)
            .flat_map(|y| pixels[y * 1280 + x..y * 1280 + x + width].iter().copied())
            .collect()
    };
    assert!(
        panel(&idle) != panel(&animated),
        "idle portrait must visibly animate"
    );
    assert!(
        panel(&idle) != panel(&talk),
        "talking portrait must visibly change"
    );
    assert_eq!(
        app.world.state_hash(),
        hash,
        "portraits are presentation only"
    );
}

fn review_repair_and_deaths(app: &mut App) {
    use straterust_engine::sim::{RepairRules, ResourceAmount, Spawn, Weapon};
    let mut rules = app.initial_world.rules().clone();
    rules.starting_resources = vec![ResourceAmount {
        kind: "minerals".into(),
        amount: 1000,
    }];
    rules.repair = Some(RepairRules {
        rate_numerator: 1,
        rate_denominator: 1,
        cost_divisor: 4,
        range: 5,
    });
    rules
        .units
        .iter_mut()
        .find(|unit| unit.id == UnitTypeId(2))
        .unwrap()
        .repairs = vec![UnitTypeId(2), UnitTypeId(3), UnitTypeId(4), UnitTypeId(5)];
    let mut shooter = rules
        .units
        .iter()
        .find(|unit| unit.id == UnitTypeId(1))
        .unwrap()
        .clone();
    shooter.id = UnitTypeId(6);
    shooter.weapon = Some(Weapon {
        cooldown_jitter: None,
        targets_air: false,
        damage_kind: Default::default(),
        splash: None,
        strikes: Vec::new(),
        damage: 100,
        range: 2000,
        cooldown: 1000,
    });
    rules.units.push(shooter);
    let mut map = app.initial_world.map().clone();
    map.spawns[2].unit_type = UnitTypeId(6);
    app.world = World::new(rules.clone(), map.clone(), 42).unwrap();
    app.queue = CommandQueue::default();
    app.target_mode = None;
    app.build_menu = false;
    let damage = Command {
        tick: app.world.tick(),
        player: PlayerId(1),
        sequence: 1,
        order: Order::Attack {
            entity: EntityId(3),
            target: EntityId(1),
        },
    };
    assert!(app.world.step(&[damage]).unwrap()[0].rejection.is_none());
    app.visuals = Visuals::new(&app.world);
    app.selected = BTreeSet::from([EntityId(2)]);
    app.camera.x = 280.0;
    app.camera.y = 256.0;
    capture(app, "repair-command", [1280, 800], Some(Action::Repair));
    app.activate(Action::Repair).unwrap();
    app.targeting_click(Position { x: 224, y: 256 }).unwrap();
    until(
        app,
        |app| app.visuals.get(EntityId(2)).unwrap().action == VisualAction::Work,
        200,
    );
    assert_work_effect_visible(app, EntityId(2));
    capture(app, "repair-working", [1280, 800], None);
    let hp = entity(app, EntityId(1)).hp;
    until(app, |app| entity(app, EntityId(1)).hp > hp, 10);
    app.activate(Action::Stop).unwrap();
    advance(app);
    assert_ne!(
        app.visuals.get(EntityId(2)).unwrap().action,
        VisualAction::Work
    );
    capture(app, "repair-stopped", [1280, 800], None);

    // Each source death is reviewed separately, with an original test attacker
    // kept off-camera. These worlds only drive presentation verification.
    rules.units.iter_mut().for_each(|unit| unit.weapon = None);
    rules.units.last_mut().unwrap().weapon = Some(Weapon {
        cooldown_jitter: None,
        targets_air: false,
        damage_kind: Default::default(),
        splash: None,
        strikes: Vec::new(),
        damage: 10000,
        range: 2000,
        cooldown: 1000,
    });
    map.resources.clear();
    for unit_type in 1..=5 {
        map.spawns = vec![
            Spawn {
                owner: PlayerId(0),
                unit_type: UnitTypeId(unit_type),
                position: Position { x: 224, y: 256 },
                ..Spawn::default()
            },
            Spawn {
                owner: PlayerId(1),
                unit_type: UnitTypeId(6),
                position: Position { x: 1280, y: 256 },
                ..Spawn::default()
            },
        ];
        app.world = World::new(rules.clone(), map.clone(), 42).unwrap();
        app.visuals = Visuals::new(&app.world);
        app.selected.clear();
        app.world.step(&[]).unwrap();
        app.visuals.update(&app.world);
        assert_eq!(app.visuals.deaths().len(), 1);
        if let Some(assets) = &app.assets {
            assert!(
                visual::death_image(assets, &app.visuals.deaths()[0]).is_some(),
                "unit {unit_type} must have native death art"
            );
        }
        capture(app, &format!("death-{unit_type}-a"), [1280, 800], None);
        app.paused = true;
        let hash = app.world.state_hash();
        let initial = app.assets.as_ref().map(|assets| {
            visual::death_image(assets, &app.visuals.deaths()[0])
                .unwrap()
                .image
                .rgba
                .clone()
        });
        app.visuals
            .advance_effects(Duration::from_millis(350), app.assets.as_ref());
        if let Some(assets) = &app.assets {
            assert_ne!(
                initial.as_ref().unwrap(),
                &visual::death_image(assets, &app.visuals.deaths()[0])
                    .unwrap()
                    .image
                    .rgba,
                "unit {unit_type} death must visibly advance while paused"
            );
        }
        capture(app, &format!("death-{unit_type}-b"), [1280, 800], None);
        app.visuals
            .advance_effects(Duration::from_secs(600), app.assets.as_ref());
        assert!(app.visuals.deaths().is_empty());
        assert_eq!(app.world.state_hash(), hash);
    }
}

mod commands;
mod effects;
mod indicators;

mod units;

mod buildings;
mod campaign;
mod carried;
mod combat;

mod aircraft;
mod muzzle;
mod transports;
