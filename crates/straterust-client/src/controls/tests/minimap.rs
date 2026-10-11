use super::*;
use straterust_engine::assets::{ConsoleLayout, ConsoleViewport};
use winit::event::MouseButton;

fn app(authored: bool) -> App {
    let mut app = demo();
    app.selected = [EntityId(2)].into();
    if authored {
        add_resource_art(&mut app);
        app.assets.as_mut().unwrap().manifest.console_layout = Some(ConsoleLayout {
            viewport: ConsoleViewport {
                canvas: [640, 480],
                margins: [176, 16, 16, 16],
            },
            menu: [0, 0, 176, 24],
            minimap: [24, 26, 128, 128],
            selection: [8, 166, 160, 164],
            group: [9, 170, 46, 34],
            group_step: [56, 40],
            group_columns: 3,
            buttons: [9, 340, 46, 38],
            button_step: [56, 47],
        });
    }
    app
}

fn point(app: &App, target: Position) -> [f64; 2] {
    let map = [app.world.map().width, app.world.map().height];
    let [x, y, w, h] = minimap_rect(app.logical_size(), map, app.assets.as_ref());
    [
        x + w * (f64::from(target.x) + 0.25) / f64::from(map[0]),
        y + h * (f64::from(target.y) + 0.25) / f64::from(map[1]),
    ]
}

#[test]
fn minimap_right_click_moves_and_shift_queues_without_panning_or_changing_selection() {
    for authored in [false, true] {
        for size in [(640, 480), (1280, 800), (1920, 1080)] {
            let mut app = app(authored);
            (app.config.width, app.config.height) = size;
            let target = Position { x: 1152, y: 720 };
            let camera = (app.camera.x, app.camera.y);
            let before = app.world.state_hash();
            assert!(
                app.minimap_click(MouseButton::Right, point(&app, target))
                    .unwrap()
            );
            assert_eq!(app.world.state_hash(), before);
            assert_eq!(
                app.recorded.last().unwrap().order,
                Order::Move {
                    entity: EntityId(2),
                    target
                }
            );
            app.keys.insert(KeyCode::ShiftLeft);
            let next = Position { x: 1024, y: 600 };
            assert!(
                app.minimap_click(MouseButton::Right, point(&app, next))
                    .unwrap()
            );
            assert_eq!(
                app.recorded.last().unwrap().order,
                Order::Queue {
                    entity: EntityId(2),
                    order: UnitOrder::Move { target: next }
                }
            );
            assert_eq!((app.camera.x, app.camera.y), camera);
            assert_eq!(app.selected, [EntityId(2)].into());
            assert!(!app.minimap_drag);
            step(&mut app);
            let worker = &app.world.state().entities[1];
            assert_eq!(worker.order, UnitOrder::Move { target });
            assert_eq!(worker.queued_orders, vec![UnitOrder::Move { target: next }]);
        }
    }
}

#[test]
fn minimap_targeted_commands_use_world_positions_and_plain_left_click_still_pans() {
    for authored in [false, true] {
        for mode in [
            TargetMode::Move,
            TargetMode::AttackMove,
            TargetMode::Patrol,
            TargetMode::Rally,
        ] {
            let mut app = app(authored);
            if matches!(mode, TargetMode::Rally) {
                app.selected = [EntityId(1)].into();
            }
            app.target_mode = Some(mode);
            let target = Position { x: 1152, y: 720 };
            let camera = (app.camera.x, app.camera.y);
            assert!(
                app.minimap_click(MouseButton::Left, point(&app, target))
                    .unwrap()
            );
            let expected = match mode {
                TargetMode::Move => Order::Move {
                    entity: EntityId(2),
                    target,
                },
                TargetMode::AttackMove => Order::AttackMove {
                    entity: EntityId(2),
                    target,
                },
                TargetMode::Patrol => Order::Patrol {
                    entity: EntityId(2),
                    target,
                },
                TargetMode::Rally => Order::Rally {
                    entity: EntityId(1),
                    target,
                },
                _ => unreachable!(),
            };
            assert_eq!(app.recorded.last().unwrap().order, expected);
            assert!(app.target_mode.is_none());
            assert!(!app.minimap_drag);
            assert_eq!((app.camera.x, app.camera.y), camera);
            let count = app.recorded.len();
            assert!(
                app.minimap_click(MouseButton::Left, point(&app, target))
                    .unwrap()
            );
            assert!(app.minimap_drag);
            assert_eq!(app.recorded.len(), count);
            assert_ne!((app.camera.x, app.camera.y), camera);
        }
    }
}

#[test]
fn minimap_context_orders_and_target_cancel_match_main_view_and_ignore_letterboxing() {
    let mut app = app(true);
    let target = app.world.state().entities[2].position;
    assert!(
        app.minimap_click(MouseButton::Right, point(&app, target))
            .unwrap()
    );
    assert_eq!(
        app.recorded.last().unwrap().order,
        Order::Attack {
            entity: EntityId(2),
            target: EntityId(3)
        }
    );
    app.target_mode = Some(TargetMode::Move);
    let count = app.recorded.len();
    assert!(
        app.minimap_click(MouseButton::Right, point(&app, target))
            .unwrap()
    );
    assert!(app.target_mode.is_none());
    assert_eq!(app.recorded.len(), count);
    let map = [app.world.map().width, app.world.map().height];
    let [x, y, w, _] = minimap_rect(app.logical_size(), map, app.assets.as_ref());
    assert!(
        !app.minimap_click(MouseButton::Right, [x + w / 2.0, y - 1.0])
            .unwrap()
    );
    assert_eq!(app.recorded.len(), count);
}
