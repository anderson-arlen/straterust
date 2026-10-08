use super::*;
use straterust_engine::sim::{AbilityId, PlayerId, StrikeAppearance, StrikeStage, Tick};

#[test]
fn strike_sounds_follow_filtered_stages_without_a_visible_caster() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures");
    let base = Package::load(&path).unwrap().world(42).unwrap();
    let mut audio = Audio::new(false);
    audio.reset(&base);
    let mut view = base.player_view(PlayerId(0)).unwrap();
    view.entities.clear();
    view.tick = Tick(1);
    view.strikes = vec![StrikeAppearance {
        ability: AbilityId(6),
        started: Tick(0),
        stage: None,
        elapsed: 0,
        position: None,
        marker: None,
        caster: None,
        heading: [0, 0],
        velocity_fp8: 0,
        warning: true,
    }];
    let projected = view.clone().into_world(&base).unwrap();
    assert!(projected.state().entities.is_empty());
    audio.observe(&projected);
    assert_eq!(audio.events, [(Cue::AbilityWarning(AbilityId(6)), None)]);
    view.tick = Tick(2);
    audio.observe(&view.clone().into_world(&base).unwrap());
    assert_eq!(audio.events.len(), 1, "warning plays only once");
    view.tick = Tick(3);
    view.strikes[0].stage = Some(StrikeStage::Impact);
    view.strikes[0].position = Some(Position { x: 64, y: 64 });
    audio.observe(&view.into_world(&base).unwrap());
    assert_eq!(
        audio.events.last(),
        Some(&(Cue::StrikeStage(AbilityId(6), StrikeStage::Impact), None))
    );
}
