//! Opt-in visual review against the user's locally imported artwork.
use super::*;
use std::io::Write;
use straterust_engine::sim::{Command, Order, ResourceAmount, Spawn, Tick};

#[test]
#[ignore = "requires STRATERUST_CAMPAIGNS pointing to a refreshed retail import"]
fn published_protoss_panels_and_power_field_render_for_review() {
    let root = std::env::var_os("STRATERUST_CAMPAIGNS").unwrap();
    let directory = std::path::Path::new(&root).join("protoss/protoss01");
    let original = Package::load(&directory).unwrap().world(42).unwrap();
    let mut rules = original.rules().clone();
    rules.victory = false;
    rules.starting_resources = vec![ResourceAmount {
        kind: "minerals".into(),
        amount: 1000,
    }];
    let mut map = original.map().clone();
    map.width = 1024;
    map.height = 768;
    map.terrain = None;
    map.mission = None;
    map.ai.clear();
    map.resources.clear();
    map.creation.clear();
    map.start_locations.clear();
    map.initial_explored.clear();
    map.fog_of_war = false;
    map.spawns = [(91, 768), (95, 128), (93, 512)]
        .map(|(kind, x)| Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(kind),
            position: Position { x, y: 384 },
            ..Default::default()
        })
        .to_vec();
    let mut world = World::new(rules, map, 42).unwrap();
    let commands = (1..=5)
        .map(|sequence| Command {
            tick: Tick(0),
            player: PlayerId(0),
            sequence,
            order: Order::Train {
                entity: EntityId(1),
                unit_type: UnitTypeId(65),
            },
        })
        .collect::<Vec<_>>();
    assert!(
        world
            .step(&commands)
            .unwrap()
            .iter()
            .all(|o| o.rejection.is_none())
    );
    let assets = AssetPack::load(&directory).unwrap().unwrap();
    let presentation: Presentation =
        ron::de::from_bytes(&std::fs::read(directory.join("presentation.ron")).unwrap()).unwrap();
    let visuals = Visuals::new(&world);
    for (id, name) in [
        (1, "panel"),
        (2, "unpowered"),
        (3, "coverage"),
        (0, "placement"),
    ] {
        let selected = BTreeSet::from([EntityId(id)]);
        let view = View {
            world: &world,
            visuals: &visuals,
            presentation: &presentation,
            cursor: [-1.0; 2],
            targeting: false,
            assets: Some(&assets),
            map_art: None,
            media: None,
            speaking: None,
            mission: None,
            animation_ms: 0,
            portrait_ms: 0,
            camera: Camera {
                x: 512.0,
                y: 384.0,
                viewport: None,
                zoom: 1.0,
            },
            selected: &selected,
            selected_resource: None,
            drag_box: None,
            paused: true,
            playback: false,
            status: "",
            buttons: &[],
            help: "",
            placement: None,
            placement_type: (id == 0).then_some(UnitTypeId(95)),
            ending_hint: "",
        };
        let mut pixels = vec![0; 1100 * 760];
        view.draw(&mut pixels, 1100, 760, 1.0);
        let mut file =
            std::fs::File::create(format!("/tmp/stratarust-protoss-{name}.ppm")).unwrap();
        file.write_all(b"P6\n1100 760\n255\n").unwrap();
        let bytes = pixels
            .into_iter()
            .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, p as u8])
            .collect::<Vec<_>>();
        file.write_all(&bytes).unwrap();
    }
}
