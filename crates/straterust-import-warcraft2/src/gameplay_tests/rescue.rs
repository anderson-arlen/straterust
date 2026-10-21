use super::*;
use straterust_engine::session::{SavedGame, ServerSession};

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to an existing retail import"]
fn tyrs_hand_captures_town_hall_and_returns_gold_when_resuming_older_progress() {
    let directory = root().join("human/mission08");
    let original_mission = std::fs::read(directory.join("mission.ron")).unwrap();
    let legacy = Package::load(&directory).unwrap().world(7).unwrap();
    let mut map = legacy.map().clone();
    map.ai.clear(); // Isolate capture and economy from unrelated campaign AI.
    map.mission.as_mut().unwrap().rescue_margin = None;
    let mut stuck = World::new(legacy.rules().clone(), map.clone(), 7).unwrap();
    let town = stuck
        .state()
        .entities
        .iter()
        .find(|e| e.owner == PlayerId(2) && e.unit_type == super::super::stats::id(74))
        .unwrap()
        .clone();
    let knight = entity(&stuck, 6);
    send(
        &mut stuck,
        Order::Move {
            entity: knight,
            target: town.position,
        },
    );
    for _ in 0..900 {
        stuck.step(&[]).unwrap();
    }
    assert_eq!(
        stuck
            .state()
            .entities
            .iter()
            .find(|e| e.id == town.id)
            .unwrap()
            .owner,
        PlayerId(2),
        "old center check cannot capture the hall"
    );
    assert_eq!(stuck.resource_balance(PlayerId(0), "gold"), 0);
    let server = ServerSession::new(stuck, 7, vec![PlayerId(0)]).unwrap();
    let bytes = SavedGame::capture(&server).unwrap().encode().unwrap();

    super::super::upgrade_campaign_map(legacy.rules(), &mut map);
    assert_eq!(map.mission.as_ref().unwrap().rescue_margin, Some(32));
    let updated = map.mission.clone();
    super::super::upgrade_campaign_map(legacy.rules(), &mut map);
    assert_eq!(map.mission, updated, "compatibility update is idempotent");
    let definitions = World::new(legacy.rules().clone(), map, 7).unwrap();
    let mut resumed =
        ServerSession::restore_saved(&definitions, SavedGame::decode(&bytes).unwrap()).unwrap();
    for _ in 0..2 {
        resumed.advance(&[]).unwrap();
    }
    assert_eq!(
        resumed
            .world()
            .state()
            .entities
            .iter()
            .find(|e| e.id == town.id)
            .unwrap()
            .owner,
        PlayerId(0)
    );

    let mut w = resumed.world().snapshot();
    let worker = entity(&w, 2);
    let mine = w
        .state()
        .resources
        .iter()
        .filter(|r| r.kind == "gold" && r.amount > 0)
        .min_by_key(|r| {
            i64::from(r.position.x - town.position.x).pow(2)
                + i64::from(r.position.y - town.position.y).pow(2)
        })
        .unwrap()
        .id;
    send(
        &mut w,
        Order::Gather {
            entity: worker,
            resource: mine,
        },
    );
    until(&mut w, 1800, |w| {
        w.resource_balance(PlayerId(0), "gold") > 0
    });
    assert_eq!(
        std::fs::read(directory.join("mission.ron")).unwrap(),
        original_mission,
        "installed import is unchanged"
    );
}
