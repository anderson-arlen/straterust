//! Reusable cargo presentation, selected from actual resource state.
use super::*;

pub fn carried_resource_frame<'a>(
    assets: &'a AssetPack,
    entity: &Entity,
    visual: Option<&UnitVisual>,
    world: &World,
) -> Option<SpriteFrame<'a>> {
    let cargo = entity.cargo.as_ref().filter(|cargo| cargo.amount > 0)?;
    if entity.garrisoned_in.is_some() || entity.burrowed || entity.hp == 0 {
        return None;
    }
    let mapping = assets.carried_resources.iter().find(|mapping| {
        mapping.manifest.full.unit_type == entity.unit_type && mapping.manifest.kind == cargo.kind
    })?;
    let sprite = mapping.sprite(cargo.amount);
    let kind = match visual.map_or(VisualAction::Idle, |visual| visual.action) {
        VisualAction::Idle => ClipKind::Idle,
        VisualAction::Move => ClipKind::Walk,
        VisualAction::Attack => ClipKind::Attack,
        VisualAction::Work => ClipKind::Work,
        VisualAction::Production => ClipKind::Production,
    };
    let facing = visual.map_or(8, |visual| visual.facing);
    let phase = visual.map_or(0, |visual| visual.phase_ms(world));
    sample(&sprite, kind, facing, phase, None)
        .or_else(|| sample(&sprite, ClipKind::Idle, facing, 0, None))
}
