use super::*;
use straterust_engine::{
    content::{Package, read_ron},
    sim::{ResourceAmount, UnitOrder, World},
};

fn demo() -> App {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo");
    let package = Package::load(&path).unwrap();
    let mut app = App::new(
        &package,
        crate::Config::default(),
        read_ron(&path.join("presentation.ron")).unwrap(),
        None,
        None,
    )
    .unwrap();
    // Component control tests intentionally own a headless fixture. Production
    // App initialization is covered separately by the filtered-session tests.
    app.world = package.world(42).unwrap();
    app.simulation = None;
    let mut rules = app.world.rules().clone();
    rules.starting_resources = vec![ResourceAmount {
        kind: "minerals".into(),
        amount: 3000,
    }];
    app.world = World::new(rules, app.world.map().clone(), 42).unwrap();
    app.initial_world = app.world.clone();
    app
}

fn step(app: &mut App) {
    let outcomes = app.world.step(&app.queue.take(app.world.tick())).unwrap();
    assert!(
        outcomes.iter().all(|outcome| outcome.rejection.is_none()),
        "{outcomes:?}"
    );
}

fn damaged_base(resources: u32) -> App {
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    rules.starting_resources[0].amount = resources;
    rules.repair = Some(straterust_engine::sim::RepairRules {
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
    let weapon = rules
        .units
        .iter_mut()
        .find(|unit| unit.id == UnitTypeId(1))
        .unwrap()
        .weapon
        .as_mut()
        .unwrap();
    weapon.range = 2000;
    weapon.damage = 100;
    weapon.cooldown = 1000;
    app.world = World::new(rules, app.world.map().clone(), 42).unwrap();
    let command = crate::Command {
        tick: app.world.tick(),
        player: PlayerId(1),
        sequence: 1,
        order: Order::Attack {
            entity: EntityId(3),
            target: EntityId(1),
        },
    };
    assert!(app.world.step(&[command]).unwrap()[0].rejection.is_none());
    assert!(app.world.state().entities[0].hp < app.world.unit_type(UnitTypeId(3)).unwrap().max_hp);
    app.selected = BTreeSet::from([EntityId(2)]);
    app.visuals = crate::visual::Visuals::new(&app.world);
    app.audio.reset(&app.world);
    app
}

fn add_resource_art(app: &mut App) {
    use straterust_engine::assets::{
        AssetManifest, AssetPack, Image, ImageRef, ResourceImage, ResourceManifest,
    };
    let reference = ImageRef {
        file: "synthetic.srim".into(),
        blake3: "0".repeat(64),
    };
    let resource = ResourceManifest {
        selection_circle: None,
        selection_y: 0,
        kind: "minerals".into(),
        anchor: [32, 48],
        image: reference.clone(),
    };
    let mut image = Image {
        width: 64,
        height: 96,
        rgba: vec![0; 64 * 96 * 4],
    };
    // Original synthetic crystal tip, well above the ground footprint.
    for y in 18..26 {
        for x in 24..40 {
            image.rgba[(y * 64 + x) * 4 + 3] = 255;
        }
    }
    app.assets = Some(AssetPack {
        manifest: AssetManifest {
            schema_version: 1,
            terrain: reference.clone(),
            terrain_grid: None,
            unit_type: UnitTypeId(1),
            unit_name: "Synthetic".into(),
            frame_ms: 100,
            anchor: [0, 0],
            frames: vec![reference],
            clips: vec![],
            extra_units: vec![],
            resources: vec![resource.clone()],
            carried_resources: Vec::new(),
            ui: vec![],
            map_images: vec![],
            scan_effect: None,
            projectiles: Vec::new(),
            damage_effects: None,
            gas_effects: None,
            creep: None,
            indicators: None,
        },
        terrain: Image {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        },
        frames: vec![],
        extra_units: vec![],
        resources: vec![ResourceImage {
            manifest: resource,
            image,
        }],
        carried_resources: Vec::new(),
        ui: vec![],
        map_images: vec![],
        scan_effect: None,
        projectiles: Vec::new(),
        damage_effects: None,
        gas_effects: None,
        creep: None,
        indicators: None,
    });
}

mod concealment;
mod economy;
mod hotkeys;

mod selection;

mod feedback;
mod resources;

mod menus;
