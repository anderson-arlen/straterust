use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectileManifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ability: Option<crate::sim::AbilityId>,
    pub unit_type: UnitTypeId,
    #[serde(default)]
    pub targets_air: bool,
    #[serde(default)]
    pub directional: bool,
    /// Pixels per simulation tick, with eight fractional bits.
    pub speed_fp8: u32,
    pub forward_offset: u32,
    /// Optional per-heading muzzle attachment, clockwise from north.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub launch_offsets: Vec<[i16; 2]>,
    #[serde(default)]
    pub arc_height: u32,
    #[serde(default)]
    pub on_target: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charge: Option<EffectManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<EffectManifest>,
    pub flight: EffectManifest,
    pub impact: EffectManifest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trail: Option<ProjectileTrailManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectileTrailManifest {
    /// First emission and interval, measured from the start of flight.
    pub start_ms: u32,
    pub interval_ms: u32,
    /// Distance behind the projectile along its heading, in world pixels.
    #[serde(default)]
    pub rear_offset: u16,
    #[serde(default)]
    pub directional: bool,
    pub effect: EffectManifest,
}

#[derive(Debug)]
pub struct Projectile {
    pub manifest: ProjectileManifest,
    pub charge: Option<Effect>,
    pub marker: Option<Effect>,
    pub flight: Effect,
    pub impact: Effect,
    pub trail: Option<Effect>,
}

impl ProjectileManifest {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            (1..=256 * 1024).contains(&self.speed_fp8)
                && self.forward_offset <= 1024
                && self.arc_height <= 1024,
            "invalid projectile motion"
        );
        ensure!(
            (self.launch_offsets.is_empty() || self.launch_offsets.len() == 32)
                && self
                    .launch_offsets
                    .iter()
                    .flatten()
                    .all(|v| v.unsigned_abs() <= 1024),
            "invalid projectile launch attachments"
        );
        if let Some(trail) = &self.trail {
            ensure!(
                trail.start_ms <= 60000
                    && (1..=60000).contains(&trail.interval_ms)
                    && trail.rear_offset <= 1024
                    && trail.effect.sequence.len() as u64 * u64::from(trail.effect.frame_ms)
                        <= u64::from(trail.interval_ms) * 64,
                "invalid projectile trail timing"
            );
        }
        for effect in [&self.flight, &self.impact]
            .into_iter()
            .chain(self.trail.as_ref().map(|trail| &trail.effect))
            .chain(self.charge.as_ref())
            .chain(self.marker.as_ref())
        {
            validate_sprite("projectile effect", effect.frame_ms, &effect.frames)?;
            ensure!(
                !effect.sequence.is_empty()
                    && effect.sequence.len() <= MAX_FRAMES
                    && effect
                        .sequence
                        .iter()
                        .all(|&frame| usize::from(frame) < effect.frames.len()),
                "invalid projectile effect sequence"
            );
        }
        ensure!(
            !self.directional
                || self
                    .flight
                    .sequence
                    .iter()
                    .all(|&frame| usize::from(frame) + 16 < self.flight.frames.len()),
            "directional projectile needs all 17 headings"
        );
        ensure!(
            self.trail.as_ref().is_none_or(|trail| !trail.directional
                || trail
                    .effect
                    .sequence
                    .iter()
                    .all(|&frame| usize::from(frame) + 16 < trail.effect.frames.len())),
            "directional trail needs all 17 headings"
        );
        Ok(())
    }
}
