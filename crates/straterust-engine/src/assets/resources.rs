//! Ground resources and worker cargo art. Resource kinds and unit IDs are
//! ruleset data; these optional mappings never affect harvesting or deposits.
use super::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceManifest {
    /// Optional map positions using this artwork; empty is the kind default.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub positions: Vec<Position>,
    pub kind: String,
    pub anchor: [i32; 2],
    pub image: ImageRef,
    /// Live artwork is already part of map terrain; only depletion is overlaid.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub terrain: bool,
    /// Optional corner tiles for redraws next to depleted terrain resources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terrain_edges: Option<ResourceTerrainEdges>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depleted_image: Option<ImageRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_image: Option<ImageRef>,
    #[serde(default)]
    pub selection_circle: Option<u8>,
    #[serde(default)]
    pub selection_y: i16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceTerrainEdges {
    /// Atlas tile to occupied corners: TL=8, TR=4, BR=2, BL=1.
    /// Optional narrow-column connections: lower=16, upper=32.
    pub corners: BTreeMap<u32, u8>,
    /// Replacement atlas tiles indexed by the four occupied corner bits.
    pub tiles: [Option<u32>; 16],
    /// Narrow column tiles indexed by live neighbours above (1) and below (2).
    pub isolated: [Option<u32>; 4],
}

#[derive(Debug)]
pub struct ResourceImage {
    pub manifest: ResourceManifest,
    pub image: Image,
    pub depleted_image: Option<Image>,
    pub active_image: Option<Image>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarriedResourceManifest {
    /// Full carrier artwork replaces its body instead of being an overlay.
    #[serde(default)]
    pub replaces_body: bool,
    pub kind: String,
    /// Amount at which the full-load sprite is used.
    pub full_amount: u32,
    /// Action clips follow the carrier's animation and position their artwork
    /// at imported attachment offsets, independently of its body atlas.
    pub full: SpriteManifest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial: Option<SpriteManifest>,
}

#[derive(Debug)]
pub struct CarriedResourcePack {
    pub manifest: CarriedResourceManifest,
    pub full: SpritePack,
    pub partial: Option<SpritePack>,
}

impl CarriedResourcePack {
    pub fn sprite(&self, amount: u32) -> SpriteRef<'_> {
        let sprite = if amount < self.manifest.full_amount {
            self.partial.as_ref().unwrap_or(&self.full)
        } else {
            &self.full
        };
        SpriteRef {
            name: &sprite.manifest.unit_name,
            frame_ms: sprite.manifest.frame_ms,
            anchor: sprite.manifest.anchor,
            frames: &sprite.frames,
            clips: &sprite.manifest.clips,
        }
    }
}

pub(super) fn validate_carried(entries: &[CarriedResourceManifest]) -> Result<()> {
    ensure!(entries.len() <= 128, "too many carried resource mappings");
    let mut keys = BTreeSet::new();
    for entry in entries {
        ensure!(
            entry.full_amount > 0
                && !entry.kind.is_empty()
                && entry.kind.len() <= 64
                && entry
                    .kind
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                && keys.insert((entry.full.unit_type, &entry.kind)),
            "invalid or duplicate carried resource mapping"
        );
        for sprite in std::iter::once(&entry.full).chain(entry.partial.iter()) {
            ensure!(
                sprite.unit_type == entry.full.unit_type,
                "cargo carrier IDs differ"
            );
            validate_sprite(&sprite.unit_name, sprite.frame_ms, &sprite.frames)?;
            validate_clips(&sprite.clips, sprite.frames.len())?;
        }
    }
    Ok(())
}

pub(super) fn load_carried(
    entries: &[CarriedResourceManifest],
    load: &mut impl FnMut(&ImageRef) -> Result<Image>,
) -> Result<Vec<CarriedResourcePack>> {
    let mut sprite = |manifest: &SpriteManifest| -> Result<SpritePack> {
        let frames = manifest
            .frames
            .iter()
            .map(&mut *load)
            .collect::<Result<Vec<_>>>()?;
        validate_frames(&frames, manifest.anchor)?;
        Ok(SpritePack {
            manifest: manifest.clone(),
            frames,
        })
    };
    entries
        .iter()
        .map(|manifest| {
            Ok(CarriedResourcePack {
                manifest: manifest.clone(),
                full: sprite(&manifest.full)?,
                partial: manifest.partial.as_ref().map(&mut sprite).transpose()?,
            })
        })
        .collect()
}

pub(super) fn validate_resources(
    entries: &[ResourceManifest],
    indicators: Option<&IndicatorsManifest>,
) -> Result<()> {
    let mut kinds = BTreeSet::new();
    let mut positions = BTreeSet::new();
    for resource in entries {
        if let Some(edges) = &resource.terrain_edges {
            ensure!(
                resource.terrain
                    && !edges.corners.is_empty()
                    && edges.corners.len() <= 4096
                    && edges
                        .corners
                        .iter()
                        .all(|(tile, mask)| *tile < 4096 && (1..=63).contains(mask))
                    && edges
                        .tiles
                        .iter()
                        .chain(&edges.isolated)
                        .flatten()
                        .all(|tile| *tile < 4096),
                "invalid resource terrain edges"
            );
        }
        ensure!(
            resource.selection_y.unsigned_abs() <= 256
                && resource.selection_circle.is_none_or(|circle| indicators
                    .is_some_and(|pack| usize::from(circle) < pack.circles.len())),
            "invalid resource selection indicator"
        );
        ensure!(
            !resource.kind.is_empty()
                && resource.kind.len() <= 64
                && resource
                    .kind
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)),
            "invalid native resource art kind"
        );
        ensure!(
            resource.positions.len() <= 16384
                && (if resource.positions.is_empty() {
                    kinds.insert(&resource.kind)
                } else {
                    resource
                        .positions
                        .iter()
                        .all(|p| positions.insert((&resource.kind, p)))
                })
                && positions.len() <= 16384,
            "duplicate native resource art mapping"
        );
        validate_reference(&resource.image)?;
        for image in resource.depleted_image.iter().chain(&resource.active_image) {
            validate_reference(image)?;
        }
    }
    Ok(())
}
