//! Passenger shots remain visible at the container's firing ports.
use super::*;

pub fn garrison_frames<'a>(
    assets: &'a AssetPack,
    world: &World,
    visuals: &Visuals,
    container: &Entity,
) -> Vec<SpriteFrame<'a>> {
    let Some(garrison) = world
        .unit_type(container.unit_type)
        .and_then(|unit| unit.garrison.as_ref())
    else {
        return Vec::new();
    };
    world
        .state()
        .entities
        .iter()
        .filter(|passenger| {
            passenger.garrisoned_in == Some(container.id)
                && garrison.attackers.contains(&passenger.unit_type)
        })
        .filter_map(|passenger| {
            let visual = visuals.get(passenger.id)?;
            let tick = visual.shot_tick?;
            let phase =
                u128::from(world.tick().0.saturating_sub(tick)) * u128::from(world.rules().tick_ms);
            let sprite = assets
                .sprite(passenger.unit_type)
                .filter(|sprite| sprite.clip(ClipKind::GarrisonAttack).is_some())
                .or_else(|| assets.sprite(container.unit_type))?;
            let clip = sprite.clip(ClipKind::GarrisonAttack)?;
            if phase
                >= (clip.frames.len() / usize::from(clip.directions)) as u128
                    * u128::from(clip.frame_ms)
            {
                return None;
            }
            sample(
                &sprite,
                ClipKind::GarrisonAttack,
                visual.facing,
                phase,
                None,
            )
        })
        .collect()
}
