use super::*;

#[test]
fn teams_share_control_resources_and_survive_an_individual_members_loss() {
    let (root, catalog) = fixture();
    let mut settings = catalog.defaults(0);
    for slot in &mut settings.slots[4..] {
        slot.kind = SlotKind::Closed;
    }
    settings.mode = 3;
    settings.random_locations = false;
    let prepared = catalog
        .prepare(root.path(), 0, &settings, &[0, 1, 2, 3], 42)
        .unwrap();
    assert_eq!(
        prepared.humans,
        vec![PlayerId(0), PlayerId(1), PlayerId(0), PlayerId(1)]
    );
    assert_eq!(prepared.world.state().players[0].resources["minerals"], 100);
    assert!(!prepared.world.is_enemy(PlayerId(0), PlayerId(2)));
    assert!(prepared.world.is_enemy(PlayerId(0), PlayerId(1)));
    assert_eq!(
        prepared
            .world
            .state()
            .entities
            .iter()
            .filter(|e| e.owner == PlayerId(0))
            .count(),
        2
    );
    settings.slots[2].kind = SlotKind::Computer;
    assert!(
        catalog
            .prepare(root.path(), 0, &settings, &[0, 1, 3], 42)
            .err()
            .unwrap()
            .to_string()
            .contains("cannot mix human and computer")
    );
}

#[test]
fn arbitrary_teams_keep_separate_armies_economies_and_ai_for_empty_open_seats() {
    let (root, mut catalog) = fixture();
    catalog.modes.push(GameType::Teams);
    let mut settings = catalog.defaults(0);
    settings.mode = catalog.modes.len() - 1;
    for (i, slot) in settings.slots.iter_mut().enumerate() {
        slot.kind = if i < 5 {
            SlotKind::Open
        } else {
            SlotKind::Closed
        };
        slot.team = [4, 4, 6, 6, 2, 0, 0, 0][i];
    }
    let prepared = catalog
        .prepare(root.path(), 0, &settings, &[0, 1], 42)
        .unwrap();
    assert_eq!(prepared.humans, vec![PlayerId(0), PlayerId(1)]);
    assert!(!prepared.world.is_enemy(PlayerId(0), PlayerId(1)));
    assert!(!prepared.world.is_enemy(PlayerId(2), PlayerId(3)));
    assert!(prepared.world.is_enemy(PlayerId(0), PlayerId(2)));
    assert!(prepared.world.is_enemy(PlayerId(2), PlayerId(4)));
    for player in 0..5 {
        assert_eq!(prepared.owners[player], Some(PlayerId(player as u16)));
        assert_eq!(
            prepared.world.state().players[player].resources["minerals"],
            50
        );
        assert!(
            prepared
                .world
                .state()
                .entities
                .iter()
                .any(|e| e.owner == PlayerId(player as u16))
        );
        assert!(
            prepared
                .world
                .map()
                .ai
                .iter()
                .any(|a| a.player == PlayerId(player as u16) && a.active == (player >= 2))
        );
    }
}
