use super::*;
use straterust_engine::sim::{Command, Order, Spawn};

fn panels(world: &World) -> [Vec<u32>; 2] {
    let visuals = Visuals::new(world);
    let selected = BTreeSet::from([EntityId(2)]);
    let presentation = Presentation::default();
    let view = View {
        world,
        visuals: &visuals,
        presentation: &presentation,
        cursor: [-1.0, -1.0],
        targeting: false,
        assets: None,
        map_art: None,
        media: None,
        speaking: None,
        mission: None,
        animation_ms: 0,
        portrait_ms: 0,
        camera: Camera {
            x: 800.0,
            y: 200.0,
            zoom: 1.0,
        },
        selected: &selected,
        selected_resource: None,
        drag_box: None,
        paused: false,
        playback: false,
        status: "",
        buttons: &[],
        help: "",
        placement: None,
        placement_type: None,
        ending_hint: "",
    };
    [false, true].map(|native| {
        let mut pixels = vec![0; 1100 * 760];
        let mut canvas = Canvas {
            scene: None,
            pixels: &mut pixels,
            width: 1100,
            height: 760,
            scale: 1.0,
        };
        if native {
            view.draw_native_selection(&mut canvas, [1100.0, 760.0]);
        } else {
            view.draw_hud(&mut canvas, [1100.0, 760.0]);
        }
        pixels
    })
}

#[test]
fn both_selection_panels_show_own_jobs_and_hide_enemy_jobs() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo");
    let original = Package::load(&path).unwrap().world(42).unwrap();
    for owner in [PlayerId(0), PlayerId(1)] {
        let mut map = original.map().clone();
        map.terrain = None;
        map.resources.clear();
        map.fog_of_war = false;
        map.spawns = vec![
            Spawn {
                owner: PlayerId(0),
                unit_type: UnitTypeId(3),
                position: Position { x: 200, y: 200 },
                ..Default::default()
            },
            Spawn {
                owner,
                unit_type: UnitTypeId(3),
                position: Position { x: 800, y: 200 },
                ..Default::default()
            },
        ];
        let mut world = World::new(original.rules().clone(), map, 42).unwrap();
        let idle = panels(&world);
        let mut visuals = Visuals::new(&world);
        let outcome = world
            .step(&[Command {
                tick: world.tick(),
                player: owner,
                sequence: 1,
                order: Order::Train {
                    entity: EntityId(2),
                    unit_type: UnitTypeId(2),
                },
            }])
            .unwrap();
        assert_eq!(outcome[0].rejection, None);
        visuals.update(&world);
        assert_eq!(
            visuals.get(EntityId(2)).unwrap().action,
            visual::VisualAction::Production
        );
        let active = panels(&world);
        for (idle, active) in idle.into_iter().zip(active) {
            if owner == PlayerId(0) {
                assert_ne!(idle, active);
            } else {
                assert_eq!(
                    idle, active,
                    "enemy inspection disclosed its production job"
                );
            }
        }
    }
}

#[test]
fn fog_renders_the_assigned_players_view_instead_of_player_zero() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-demo");
    let server = Package::load(&path).unwrap().world(42).unwrap();
    let presentation = Presentation::default();
    let selected = BTreeSet::new();
    for player in [PlayerId(0), PlayerId(1)] {
        let world = server
            .player_view(player)
            .unwrap()
            .into_world(&server)
            .unwrap();
        let visuals = Visuals::new(&world);
        let home = crate::home_position(&world);
        let view = View {
            world: &world,
            visuals: &visuals,
            presentation: &presentation,
            cursor: [-1.0; 2],
            targeting: false,
            assets: None,
            map_art: None,
            media: None,
            speaking: None,
            mission: None,
            animation_ms: 0,
            portrait_ms: 0,
            camera: Camera {
                x: f64::from(home.x),
                y: f64::from(home.y),
                zoom: 1.0,
            },
            selected: &selected,
            selected_resource: None,
            drag_box: None,
            paused: false,
            playback: false,
            status: "",
            buttons: &[],
            help: "",
            placement: None,
            placement_type: None,
            ending_hint: "",
        };
        let mut pixels = vec![0xffffff; 1100 * 760];
        view.draw_fog(
            &mut Canvas {
                scene: None,
                pixels: &mut pixels,
                width: 1100,
                height: 760,
                scale: 1.0,
            },
            [1100.0, 760.0],
        );
        let point =
            view.camera
                .world_to_screen(f64::from(home.x), f64::from(home.y), [1100.0, 760.0]);
        assert_eq!(
            pixels[point[1] as usize * 1100 + point[0] as usize],
            0xffffff,
            "player {player:?} home is covered by the wrong fog layer"
        );
    }
}
