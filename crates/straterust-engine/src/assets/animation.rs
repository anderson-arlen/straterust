//! Native directional animation metadata and validation.
use super::{MAX_FRAMES, MAX_IMAGE_DIMENSION};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Presentation states selected by the client, never simulation callbacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ClipKind {
    Disabled,
    Idle,
    Walk,
    Attack,
    /// A separate muzzle/effect layer emitted by a passenger in a container.
    GarrisonAttack,
    Work,
    Production,
    /// Separate source bridge at the origin of a completed, attached addon.
    AddonConnector,
    Construction,
    WorkEffect,
    Death,
    Conceal,
    Reveal,
    Lift,
    Land,
    Airborne,
    /// A separate ground-anchored layer beneath flying bodies.
    Shadow,
    LiftShadow,
    LandShadow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipFrame {
    /// Index into the sprite's shared image list. Mirroring does not duplicate image bytes.
    pub frame: u16,
    pub flip_x: bool,
    /// Screen-pixel displacement from the unit origin, resolved for this heading.
    /// This displacement is applied after image mirroring and is never mirrored again.
    #[serde(default)]
    pub offset: [i16; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpriteClip {
    pub kind: ClipKind,
    /// One direction, or 32 clockwise headings: north=0, east=8, south=16, west=24.
    pub directions: u8,
    pub frame_ms: u32,
    /// Time-major, then direction. Construction steps are chosen by progress, not time.
    /// WorkEffect is drawn separately; each frame's offset locates the contact point.
    pub frames: Vec<ClipFrame>,
    /// Attack steps to display once if their brief poses fall between redraws.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_steps: Vec<u16>,
}

pub(super) fn validate_clips(clips: &[SpriteClip], frame_count: usize) -> Result<()> {
    ensure!(clips.len() <= 13, "too many sprite clips");
    let mut kinds = BTreeSet::new();
    for clip in clips {
        ensure!(kinds.insert(clip.kind), "duplicate sprite clip state");
        ensure!(
            matches!(clip.directions, 1 | 32),
            "sprite clips require 1 or 32 directions"
        );
        ensure!(
            (1..=10_000).contains(&clip.frame_ms),
            "clip frame_ms must be 1..=10000"
        );
        let directions = usize::from(clip.directions);
        ensure!(
            !clip.frames.is_empty()
                && clip.frames.len().is_multiple_of(directions)
                && clip.frames.len() / directions <= MAX_FRAMES,
            "sprite clip must have 1..={MAX_FRAMES} complete directional steps"
        );
        let steps = clip.frames.len() / directions;
        ensure!(
            (clip.key_steps.is_empty() || clip.kind == ClipKind::Attack)
                && clip.key_steps.len() <= steps
                && clip.key_steps.iter().all(|step| usize::from(*step) < steps)
                && clip.key_steps.windows(2).all(|pair| pair[0] < pair[1]),
            "attack key steps must be ordered, unique and within the clip"
        );
        ensure!(
            clip.frames
                .iter()
                .all(|frame| usize::from(frame.frame) < frame_count),
            "sprite clip references an image outside its frame list"
        );
        ensure!(
            clip.frames.iter().all(|frame| frame
                .offset
                .iter()
                .all(|offset| i32::from(*offset).abs() <= MAX_IMAGE_DIMENSION as i32)),
            "sprite clip offset exceeds the image dimension limit"
        );
    }
    Ok(())
}
