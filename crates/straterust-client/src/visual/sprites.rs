//! Select and sample source artwork without changing authoritative state.
use super::*;

pub(super) fn sample<'a>(
    sprite: &SpriteRef<'a>,
    kind: ClipKind,
    facing: u8,
    phase_ms: u128,
    construction: Option<usize>,
) -> Option<SpriteFrame<'a>> {
    let clip = sprite.clips.iter().find(|clip| clip.kind == kind)?;
    let directions = usize::from(clip.directions);
    let steps = clip.frames.len() / directions;
    let step = if let Some(stage) = construction {
        stage.min(steps - 1)
    } else {
        let elapsed = phase_ms / u128::from(clip.frame_ms);
        if matches!(
            kind,
            ClipKind::Death
                | ClipKind::Conceal
                | ClipKind::Reveal
                | ClipKind::Lift
                | ClipKind::Land
                | ClipKind::LiftShadow
                | ClipKind::LandShadow
        ) && elapsed >= steps as u128
        {
            return None;
        }
        if kind == ClipKind::Attack && elapsed >= steps as u128 {
            return sample(sprite, ClipKind::Idle, facing, 0, None);
        }
        (elapsed % steps as u128) as usize
    };
    let direction = if directions == 1 {
        0
    } else {
        usize::from(facing % 32)
    };
    let frame = &clip.frames[step * directions + direction];
    let image = &sprite.frames[usize::from(frame.frame)];
    let mut anchor = sprite.anchor;
    if frame.flip_x {
        anchor[0] = image.width as i32 - anchor[0];
    }
    // Imported offsets already resolve the heading. Mirroring changes the
    // image anchor, not the effect's world-facing position.
    anchor[0] -= i32::from(frame.offset[0]);
    anchor[1] -= i32::from(frame.offset[1]);
    Some(SpriteFrame {
        image,
        anchor,
        flip_x: frame.flip_x,
    })
}

pub fn death_image<'a>(assets: &'a AssetPack, death: &DeathVisual) -> Option<SpriteFrame<'a>> {
    let sprite = assets.sprite(death.unit_type)?;
    sample(
        &sprite,
        ClipKind::Death,
        death.facing,
        death.elapsed.as_millis(),
        None,
    )
}

pub fn shadow_image<'a>(
    assets: &'a AssetPack,
    entity: &Entity,
    visual: Option<&UnitVisual>,
    world: &World,
) -> Option<SpriteFrame<'a>> {
    let sprite = assets.sprite(entity.unit_type)?;
    let unit = world.unit_type(entity.unit_type)?;
    if entity.airborne
        && let Some(flight) = &unit.flight
    {
        let (kind, phase) = flight_clip(
            entity,
            flight,
            world.rules().tick_ms,
            world.is_landing(entity),
        );
        let kind = match kind {
            ClipKind::Lift => ClipKind::LiftShadow,
            ClipKind::Land => ClipKind::LandShadow,
            _ => ClipKind::Shadow,
        };
        return sample(
            &sprite,
            kind,
            visual.map_or(0, |visual| visual.facing),
            phase,
            None,
        )
        .or_else(|| sample(&sprite, ClipKind::Shadow, 0, 0, None));
    }
    sample(
        &sprite,
        ClipKind::Shadow,
        visual.map_or(0, |visual| visual.facing),
        0,
        None,
    )
}

fn flight_clip(
    entity: &Entity,
    flight: &straterust_engine::sim::Flight,
    tick_ms: u32,
    landing: bool,
) -> (ClipKind, u128) {
    if entity.flight_transition == 0 {
        return (ClipKind::Airborne, 0);
    }
    let duration = if landing {
        flight.land_ticks
    } else {
        flight.lift_ticks
    };
    (
        if landing {
            ClipKind::Land
        } else {
            ClipKind::Lift
        },
        u128::from(duration.saturating_sub(entity.flight_transition)) * u128::from(tick_ms),
    )
}

/// Clip selection depends on observed work/movement, not a perpetual global walk loop.
pub fn addon_connector<'a>(assets: &'a AssetPack, entity: &Entity) -> Option<SpriteFrame<'a>> {
    if entity.parent.is_none() || entity.construction.is_some() {
        return None;
    }
    sample(
        &assets.sprite(entity.unit_type)?,
        ClipKind::AddonConnector,
        0,
        0,
        None,
    )
}

pub(super) fn action_clip(
    sprite: &SpriteRef<'_>,
    visual: Option<&UnitVisual>,
    world: &World,
) -> (ClipKind, u128) {
    let action = visual.map_or(VisualAction::Idle, |visual| visual.action);
    let kind = match action {
        VisualAction::Idle => ClipKind::Idle,
        VisualAction::Move => ClipKind::Walk,
        VisualAction::Attack => ClipKind::Attack,
        VisualAction::Work => ClipKind::Work,
        VisualAction::Production => ClipKind::Production,
    };
    // Preserve a committed shot after a kill or attack-move resumes. A brief
    // flash can also fall entirely between redraws: retain the latest missed
    // key step for this draw, then consume it in mark_rendered().
    if matches!(kind, ClipKind::Idle | ClipKind::Walk | ClipKind::Attack)
        && let Some(visual) = visual
        && let Some(shot) = visual.shot_tick
        && let Some(clip) = sprite.clip(ClipKind::Attack)
    {
        let tick_ms = u128::from(world.rules().tick_ms);
        let phase = u128::from(world.tick().0.saturating_sub(shot)) * tick_ms;
        let previous = (visual.rendered_tick >= shot)
            .then(|| u128::from(visual.rendered_tick - shot) * tick_ms);
        let missed = clip.key_steps.iter().rev().find_map(|&step| {
            let at = u128::from(step) * u128::from(clip.frame_ms);
            (at <= phase && previous.is_none_or(|last| at > last)).then_some(at)
        });
        let duration =
            (clip.frames.len() / usize::from(clip.directions)) as u128 * u128::from(clip.frame_ms);
        if kind == ClipKind::Attack || phase < duration || missed.is_some() {
            return (ClipKind::Attack, missed.unwrap_or(phase));
        }
    }
    (
        kind,
        visual.map_or(0, |visual| {
            if kind == ClipKind::Attack {
                0
            } else {
                visual.phase_ms(world)
            }
        }),
    )
}

pub fn unit_image<'a>(
    assets: &'a AssetPack,
    entity: &Entity,
    visual: Option<&UnitVisual>,
    world: &World,
) -> Option<SpriteFrame<'a>> {
    let sprite = assets.sprite(entity.unit_type)?;
    let unit = world.unit_type(entity.unit_type)?;
    let facing = visual.map_or(0, |visual| visual.facing);
    if entity.airborne
        && let Some(flight) = &unit.flight
    {
        let (kind, phase) = flight_clip(
            entity,
            flight,
            world.rules().tick_ms,
            world.is_landing(entity),
        );
        return sample(&sprite, kind, facing, phase, None)
            .or_else(|| sample(&sprite, ClipKind::Airborne, facing, 0, None));
    }
    if let (Some(state), Some(stats)) = (&entity.mine_state, &unit.mine) {
        let (kind, phase) = match state.phase {
            MinePhase::Concealing => (
                ClipKind::Conceal,
                u128::from(stats.conceal_ticks.saturating_sub(state.remaining))
                    * u128::from(world.rules().tick_ms),
            ),
            MinePhase::Emerging => (
                ClipKind::Reveal,
                u128::from(stats.reveal_ticks.saturating_sub(state.remaining))
                    * u128::from(world.rules().tick_ms),
            ),
            MinePhase::Armed => {
                let clip = sprite.clip(ClipKind::Conceal)?;
                (
                    ClipKind::Conceal,
                    (clip.frames.len() / usize::from(clip.directions) - 1) as u128
                        * u128::from(clip.frame_ms),
                )
            }
            MinePhase::Chasing if visual.is_some_and(|visual| visual.moving) => (
                ClipKind::Walk,
                u128::from(world.tick().0) * u128::from(world.rules().tick_ms),
            ),
            _ => (ClipKind::Idle, 0),
        };
        return sample(&sprite, kind, facing, phase, None);
    }
    if let Some(visual) = visual
        && let Some(tick) = visual.concealment_changed
    {
        let phase =
            u128::from(world.tick().0.saturating_sub(tick)) * u128::from(world.rules().tick_ms);
        let kind = if entity.cloaked {
            ClipKind::Conceal
        } else {
            ClipKind::Reveal
        };
        if let Some(frame) = sample(&sprite, kind, visual.facing, phase, None) {
            return Some(frame);
        }
    }
    if entity.doodad_enabled == Some(false) {
        return sample(&sprite, ClipKind::Disabled, 0, 0, Some(usize::MAX));
    }
    if entity.cloaked
        && let Some(clip) = sprite.clip(ClipKind::Conceal)
    {
        let final_step = clip.frames.len() / usize::from(clip.directions) - 1;
        return sample(
            &sprite,
            ClipKind::Conceal,
            visual.map_or(0, |visual| visual.facing),
            final_step as u128 * u128::from(clip.frame_ms),
            None,
        );
    }
    let stage = construction_stage(entity);
    if stage.is_some() {
        // A missing construction clip must use the original scaffold fallback,
        // never the finished building image.
        return sample(&sprite, ClipKind::Construction, 0, 0, stage);
    }
    let action = visual.map_or(VisualAction::Idle, |v| v.action);
    let (kind, phase) = action_clip(&sprite, visual, world);
    let facing = visual.map_or(8, |v| {
        if kind == ClipKind::Attack && matches!(action, VisualAction::Idle | VisualAction::Move) {
            v.shot_facing
        } else {
            v.facing
        }
    });
    if !sprite.clips.is_empty() {
        return sample(&sprite, kind, facing, phase, None)
            .or_else(|| sample(&sprite, ClipKind::Idle, facing, 0, None));
    }
    // Legacy preview packs only have a walking sequence. Hold frame zero at
    // rest; no fabricated rotations of a single east-facing image.
    let index = if action == VisualAction::Move {
        (phase / u128::from(sprite.frame_ms) % sprite.frames.len() as u128) as usize
    } else {
        0
    };
    Some(SpriteFrame {
        image: &sprite.frames[index],
        anchor: sprite.anchor,
        flip_x: false,
    })
}

pub fn work_effect<'a>(
    assets: &'a AssetPack,
    entity: &Entity,
    visual: Option<&UnitVisual>,
    world: &World,
) -> Option<SpriteFrame<'a>> {
    let visual = visual.filter(|visual| visual.action == VisualAction::Work)?;
    let sprite = assets.sprite(entity.unit_type)?;
    sample(
        &sprite,
        ClipKind::WorkEffect,
        visual.facing,
        visual.phase_ms(world),
        None,
    )
}
