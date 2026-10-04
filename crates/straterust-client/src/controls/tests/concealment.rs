use super::*;
use straterust_engine::session::ServerSession;

#[test]
fn package_control_toggles_guest_group_and_plays_transition_cues() {
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(1))
        .unwrap()
        .cloak = Some(straterust_engine::sim::Cloak {
        can_move: false,
        can_attack: false,
        blocks_movement: false,
        reveal_ticks: 3,
        reveal_on_order: true,
        auto_reveal: true,
        ..Default::default()
    });
    let mut map = app.world.map().clone();
    map.spawns = ron::from_str(
        r"[
        (owner:0,unit_type:1,position:(x:100,y:100)),
        (owner:1,unit_type:1,position:(x:800,y:100)),
        (owner:1,unit_type:1,position:(x:832,y:100)),
        (owner:1,unit_type:2,position:(x:900,y:100)),
    ]",
    )
    .unwrap();
    let definitions = World::new(rules, map, 42).unwrap();
    let mut server =
        ServerSession::new(definitions.clone(), 42, vec![PlayerId(0), PlayerId(1)]).unwrap();
    app.world = server
        .update(PlayerId(1), &[])
        .unwrap()
        .view
        .into_world(&definitions)
        .unwrap();
    app.selected = BTreeSet::from([EntityId(2), EntityId(3), EntityId(4)]);
    assert!(
        !app.buttons()
            .iter()
            .any(|b| matches!(b.action, Action::Cloak(_))),
        "the client does not invent a game's control layout"
    );
    app.presentation.command_buttons = ron::from_str(
        r#"{
        "cloak.1.on":(slot:7,key:"J",label:"Hide",tip:"Go underground",icon:"test.hide"),
        "cloak.1.off":(slot:7,key:"K",label:"Emerge",tip:"Return above ground",icon:"test.emerge"),
    }"#,
    )
    .unwrap();
    app.presentation.validate().unwrap();
    let buttons = app.buttons();
    let button = buttons
        .iter()
        .find(|b| b.action == Action::Cloak(true))
        .unwrap();
    assert_eq!(
        (button.slot, button.key.as_str(), button.label.as_str()),
        (7, "J", "Hide")
    );
    app.audio.reset(&app.world);
    app.activate(Action::Cloak(true)).unwrap();
    assert_eq!(
        app.recorded.len(),
        2,
        "only capable owned members receive orders"
    );
    for command in app.recorded.drain(..) {
        server.submit(PlayerId(1), command).unwrap();
    }
    let outcomes = server.advance(&[]).unwrap();
    assert!(outcomes.iter().all(|o| o.rejection.is_none()));
    app.world = server
        .update(PlayerId(1), &outcomes)
        .unwrap()
        .view
        .into_world(&definitions)
        .unwrap();
    app.audio.observe(&app.world);
    assert_eq!(
        app.audio
            .events
            .iter()
            .filter(|e| e.0 == crate::audio::Cue::Conceal)
            .count(),
        2
    );
    let buttons = app.buttons();
    let button = buttons
        .iter()
        .find(|b| b.action == Action::Cloak(false))
        .unwrap();
    assert_eq!(button.key, "K");
    app.activate(Action::Cloak(false)).unwrap();
    for command in app.recorded.drain(..) {
        server.submit(PlayerId(1), command).unwrap();
    }
    let outcomes = server.advance(&[]).unwrap();
    assert!(outcomes.iter().all(|o| o.rejection.is_none()));
    app.world = server
        .update(PlayerId(1), &outcomes)
        .unwrap()
        .view
        .into_world(&definitions)
        .unwrap();
    app.audio.observe(&app.world);
    app.audio.observe(&app.world);
    assert_eq!(
        app.audio
            .events
            .iter()
            .filter(|e| e.0 == crate::audio::Cue::Reveal)
            .count(),
        2
    );
    assert!(
        app.buttons()
            .iter()
            .find(|b| b.action == Action::Cloak(true))
            .unwrap()
            .disabled
            .is_some()
    );
    app.presentation
        .command_buttons
        .get_mut("cloak.1.on")
        .unwrap()
        .slot = 9;
    assert!(app.presentation.validate().is_err());
}
