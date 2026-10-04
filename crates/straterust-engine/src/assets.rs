//! Optional native presentation assets. These bytes never enter gameplay identities or ticks.
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
mod animation;
use animation::validate_clips;
pub use animation::{ClipFrame, ClipKind, SpriteClip};
mod indicators;
pub use indicators::{CursorManifest, IndicatorsManifest, IndicatorsPack, UnitIndicator};
mod map_artwork;
mod resources;
pub use map_artwork::{DecodedMapArtwork, MapArtwork};
pub use resources::{
    CarriedResourceManifest, CarriedResourcePack, ResourceImage, ResourceManifest,
};

use crate::sim::{Position, UnitTypeId, World};

pub const MAX_IMAGE_DIMENSION: u32 = 2048;
pub const MAX_FRAMES: usize = 512;
pub const MAX_PACK_RGBA_BYTES: usize = 128 * 1024 * 1024;
const HEADER_BYTES: usize = 16;
const MAX_IMAGE_BYTES: usize = MAX_IMAGE_DIMENSION as usize * MAX_IMAGE_DIMENSION as usize * 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Row-major, straight (not premultiplied) RGBA8, starting at the top left.
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageRef {
    pub file: String,
    /// Lowercase hexadecimal BLAKE3 of the complete encoded SRIM file.
    pub blake3: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainGrid {
    pub tile_size: u32,
    pub columns: u32,
    pub rows: u32,
    /// Map cells in row-major order, each indexing a row-major tile in the terrain atlas.
    pub tiles: Vec<u32>,
}

impl TerrainGrid {
    fn validate(&self) -> Result<()> {
        ensure!(
            (1..=256).contains(&self.columns) && (1..=256).contains(&self.rows),
            "terrain grid dimensions must be 1..=256 tiles"
        );
        ensure!(
            (1..=MAX_IMAGE_DIMENSION).contains(&self.tile_size),
            "terrain tile size must be 1..={MAX_IMAGE_DIMENSION}"
        );
        ensure!(
            self.tiles.len() == (self.columns * self.rows) as usize,
            "terrain grid tile count does not match its dimensions"
        );
        Ok(())
    }

    fn validate_atlas(&self, image: &Image) -> Result<()> {
        self.validate()?;
        ensure!(
            image.width.is_multiple_of(self.tile_size)
                && image.height.is_multiple_of(self.tile_size),
            "terrain atlas dimensions must be multiples of tile_size"
        );
        let tile_count = (image.width / self.tile_size) * (image.height / self.tile_size);
        ensure!(
            self.tiles.iter().all(|tile| *tile < tile_count),
            "terrain grid references a tile outside its atlas"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetManifest {
    pub schema_version: u32,
    pub terrain: ImageRef,
    #[serde(default)]
    pub terrain_grid: Option<TerrainGrid>,
    pub unit_type: UnitTypeId,
    pub unit_name: String,
    pub frame_ms: u32,
    /// Shared canvas anchor. Clip offsets can position individually cropped frames.
    pub anchor: [i32; 2],
    pub frames: Vec<ImageRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<SpriteClip>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_units: Vec<SpriteManifest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<ResourceManifest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub carried_resources: Vec<CarriedResourceManifest>,
    /// Named HUD images; the client defines their layout and interaction.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ui: Vec<UiImageManifest>,
    /// Static world decorations, independent of gameplay entities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub map_images: Vec<MapImageManifest>,
    /// Finite detection effect, independent of any gameplay unit sprite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan_effect: Option<EffectManifest>,
    /// Weapon artwork and travel belong to presentation, not simulation rules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projectiles: Vec<ProjectileManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub damage_effects: Option<DamageEffectsManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gas_effects: Option<GasEffectsManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creep: Option<CreepManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indicators: Option<IndicatorsManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreepManifest {
    pub tiles: Vec<ImageRef>,
    pub edges: Vec<ImageRef>,
    pub mask_frames: Vec<u8>,
}
#[derive(Debug)]
pub struct CreepPack {
    pub tiles: Vec<Image>,
    pub edges: Vec<Image>,
    pub mask_frames: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GasEffectsManifest {
    pub plumes: [EffectManifest; 3],
    pub depleted: EffectManifest,
    pub geyser_spots: Vec<EffectSpot>,
    pub units: Vec<UnitEffectManifest>,
}

#[derive(Debug)]
pub struct GasEffects {
    pub manifest: GasEffectsManifest,
    pub plumes: [Effect; 3],
    pub depleted: Effect,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectSpot {
    pub offset: [i32; 2],
    pub variant: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitEffectManifest {
    pub unit_type: UnitTypeId,
    pub spots: Vec<EffectSpot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DamageEffectsManifest {
    pub small: [EffectManifest; 3],
    pub large: [EffectManifest; 3],
    pub units: Vec<UnitEffectManifest>,
}

#[derive(Debug)]
pub struct DamageEffects {
    pub manifest: DamageEffectsManifest,
    pub small: [Effect; 3],
    pub large: [Effect; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectManifest {
    pub frame_ms: u32,
    pub anchor: [i32; 2],
    pub frames: Vec<ImageRef>,
    /// Finite timeline, with repeated indices representing held source poses.
    pub sequence: Vec<u16>,
}

#[derive(Debug)]
pub struct Effect {
    pub frame_ms: u32,
    pub anchor: [i32; 2],
    pub frames: Vec<Image>,
    pub sequence: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapImageManifest {
    pub position: Position,
    pub anchor: [i32; 2],
    pub image: ImageRef,
}

#[derive(Debug)]
pub struct MapImage {
    pub position: Position,
    pub anchor: [i32; 2],
    pub image: Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiImageManifest {
    pub key: String,
    pub image: ImageRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpriteManifest {
    pub unit_type: UnitTypeId,
    pub unit_name: String,
    pub frame_ms: u32,
    pub anchor: [i32; 2],
    pub frames: Vec<ImageRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<SpriteClip>,
}

impl AssetManifest {
    pub fn validate(&self) -> Result<()> {
        resources::validate_carried(&self.carried_resources)?;
        if let Some(indicators) = &self.indicators {
            indicators.validate()?;
        }
        ensure!(self.schema_version == 1, "unsupported native asset schema");
        if let Some(grid) = &self.terrain_grid {
            grid.validate()?;
        }
        validate_reference(&self.terrain)?;
        validate_sprite(&self.unit_name, self.frame_ms, &self.frames)?;
        validate_clips(&self.clips, self.frames.len())?;
        ensure!(
            self.extra_units.len() <= 64 && self.resources.len() <= 64,
            "native pack supports up to 64 additional unit and resource mappings each"
        );
        let mut ids = BTreeSet::from([self.unit_type]);
        for sprite in &self.extra_units {
            ensure!(
                ids.insert(sprite.unit_type),
                "duplicate native unit art mapping"
            );
            validate_sprite(&sprite.unit_name, sprite.frame_ms, &sprite.frames)?;
            validate_clips(&sprite.clips, sprite.frames.len())?;
        }
        let mut kinds = BTreeSet::new();
        for resource in &self.resources {
            ensure!(
                resource.selection_y.unsigned_abs() <= 256
                    && resource.selection_circle.is_none_or(|circle| self
                        .indicators
                        .as_ref()
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
                kinds.insert(&resource.kind),
                "duplicate native resource art mapping"
            );
            validate_reference(&resource.image)?;
        }
        ensure!(
            self.ui.len() <= 192,
            "native pack supports up to 192 UI images"
        );
        let mut keys = BTreeSet::new();
        for entry in &self.ui {
            ensure!(
                !entry.key.is_empty()
                    && entry.key.len() <= 64
                    && entry
                        .key
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)),
                "invalid UI image key"
            );
            ensure!(keys.insert(&entry.key), "duplicate UI image key");
            validate_reference(&entry.image)?;
        }
        ensure!(
            self.map_images.len() <= 4096,
            "too many world decoration images"
        );
        for entry in &self.map_images {
            ensure!(
                entry.position.x >= 0 && entry.position.y >= 0,
                "negative world decoration position"
            );
            validate_reference(&entry.image)?;
        }
        if let Some(effect) = &self.scan_effect {
            validate_sprite("scan effect", effect.frame_ms, &effect.frames)?;
            ensure!(
                !effect.sequence.is_empty()
                    && effect.sequence.len() <= MAX_FRAMES
                    && effect
                        .sequence
                        .iter()
                        .all(|&frame| usize::from(frame) < effect.frames.len()),
                "invalid finite scan effect sequence"
            );
        }
        ensure!(
            self.projectiles.len() <= 128,
            "too many projectile mappings"
        );
        let mut units = BTreeSet::new();
        for projectile in &self.projectiles {
            ensure!(
                units.insert((projectile.unit_type, projectile.targets_air)),
                "duplicate projectile unit"
            );
            projectile.validate()?;
        }
        if let Some(damage) = &self.damage_effects {
            ensure!(
                damage.units.len() <= 128,
                "too many damage overlay mappings"
            );
            let mut ids = BTreeSet::new();
            for unit in &damage.units {
                ensure!(
                    ids.insert(unit.unit_type) && !unit.spots.is_empty() && unit.spots.len() <= 24,
                    "invalid damage overlay mapping"
                );
                ensure!(
                    unit.spots.iter().all(|spot| spot.variant < 3
                        && spot
                            .offset
                            .iter()
                            .all(|value| (-1024..=1024).contains(value))),
                    "invalid damage attachment"
                );
            }
            for effect in damage.small.iter().chain(&damage.large) {
                validate_sprite("damage effect", effect.frame_ms, &effect.frames)?;
                ensure!(
                    !effect.sequence.is_empty()
                        && effect.sequence.len() <= MAX_FRAMES
                        && effect
                            .sequence
                            .iter()
                            .all(|&frame| usize::from(frame) < effect.frames.len()),
                    "invalid damage effect sequence"
                );
            }
        }
        if let Some(creep) = &self.creep {
            ensure!(
                creep.tiles.len() == 13
                    && !creep.edges.is_empty()
                    && creep.edges.len() <= 128
                    && creep.mask_frames.len() == 256
                    && creep
                        .mask_frames
                        .iter()
                        .all(|&frame| usize::from(frame) <= creep.edges.len()),
                "invalid creep art"
            );
            for image in creep.tiles.iter().chain(&creep.edges) {
                validate_reference(image)?;
            }
        }
        if let Some(gas) = &self.gas_effects {
            ensure!(
                gas.units.len() <= 128 && gas.geyser_spots.len() <= 3,
                "invalid gas overlay mappings"
            );
            let mut ids = BTreeSet::new();
            for unit in &gas.units {
                ensure!(
                    ids.insert(unit.unit_type) && unit.spots.len() <= 3,
                    "invalid gas overlay unit"
                );
            }
            for spot in gas
                .geyser_spots
                .iter()
                .chain(gas.units.iter().flat_map(|unit| &unit.spots))
            {
                ensure!(
                    spot.variant < 3 && spot.offset.iter().all(|v| (-1024..=1024).contains(v)),
                    "invalid gas attachment"
                );
            }
            for effect in gas.plumes.iter().chain(std::iter::once(&gas.depleted)) {
                validate_sprite("gas effect", effect.frame_ms, &effect.frames)?;
                ensure!(
                    !effect.sequence.is_empty()
                        && effect.sequence.len() <= MAX_FRAMES
                        && effect
                            .sequence
                            .iter()
                            .all(|&frame| usize::from(frame) < effect.frames.len()),
                    "invalid gas effect sequence"
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct AssetPack {
    pub manifest: AssetManifest,
    pub terrain: Image,
    pub frames: Vec<Image>,
    pub extra_units: Vec<SpritePack>,
    pub resources: Vec<ResourceImage>,
    pub carried_resources: Vec<CarriedResourcePack>,
    pub ui: Vec<UiImage>,
    pub map_images: Vec<MapImage>,
    pub scan_effect: Option<Effect>,
    pub projectiles: Vec<Projectile>,
    pub damage_effects: Option<DamageEffects>,
    pub gas_effects: Option<GasEffects>,
    pub creep: Option<CreepPack>,
    pub indicators: Option<IndicatorsPack>,
}

#[derive(Debug)]
pub struct UiImage {
    pub key: String,
    pub image: Image,
}

#[derive(Debug)]
pub struct SpritePack {
    pub manifest: SpriteManifest,
    pub frames: Vec<Image>,
}

/// One borrowed sprite view for both the original primary mapping and added units.
pub struct SpriteRef<'a> {
    pub name: &'a str,
    pub frame_ms: u32,
    pub anchor: [i32; 2],
    pub frames: &'a [Image],
    pub clips: &'a [SpriteClip],
}

impl SpriteRef<'_> {
    pub fn clip(&self, kind: ClipKind) -> Option<&SpriteClip> {
        self.clips.iter().find(|clip| clip.kind == kind)
    }
}

impl AssetPack {
    pub fn projectile_for(&self, unit_type: UnitTypeId, air: bool) -> Option<&Projectile> {
        self.projectiles
            .iter()
            .find(|p| p.manifest.unit_type == unit_type && p.manifest.targets_air == air)
    }
    pub fn projectile(&self, unit_type: UnitTypeId) -> Option<&Projectile> {
        self.projectiles
            .iter()
            .find(|effect| effect.manifest.unit_type == unit_type)
    }
    pub fn ui_image(&self, key: &str) -> Option<&Image> {
        self.ui
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| &entry.image)
    }

    pub fn sprite(&self, unit_type: UnitTypeId) -> Option<SpriteRef<'_>> {
        if self.manifest.unit_type == unit_type {
            return Some(SpriteRef {
                name: &self.manifest.unit_name,
                frame_ms: self.manifest.frame_ms,
                anchor: self.manifest.anchor,
                frames: &self.frames,
                clips: &self.manifest.clips,
            });
        }
        self.extra_units
            .iter()
            .find(|sprite| sprite.manifest.unit_type == unit_type)
            .map(|sprite| SpriteRef {
                name: &sprite.manifest.unit_name,
                frame_ms: sprite.manifest.frame_ms,
                anchor: sprite.manifest.anchor,
                frames: &sprite.frames,
                clips: &sprite.manifest.clips,
            })
    }

    /// Cross-file validation shared by package publication and the client.
    pub fn validate_for_world(&self, world: &World) -> Result<()> {
        ensure!(
            self.carried_resources
                .iter()
                .all(|cargo| world.unit_type(cargo.manifest.full.unit_type).is_some()),
            "carried resource references unknown unit"
        );
        if let Some(indicators) = &self.indicators {
            ensure!(
                indicators
                    .manifest
                    .units
                    .iter()
                    .all(|entry| world.unit_type(entry.unit_type).is_some()),
                "selection indicator references unknown unit"
            );
        }
        for id in std::iter::once(self.manifest.unit_type).chain(
            self.manifest
                .extra_units
                .iter()
                .map(|sprite| sprite.unit_type),
        ) {
            ensure!(
                world.rules().units.iter().any(|unit| unit.id == id),
                "presentation asset references unknown unit type {}",
                id.0
            );
        }
        for decoration in &self.map_images {
            ensure!(
                world.map().contains(decoration.position),
                "world decoration lies outside the gameplay map"
            );
        }
        for projectile in &self.projectiles {
            ensure!(
                world
                    .unit_type(projectile.manifest.unit_type)
                    .is_some_and(|unit| unit.weapon.is_some()),
                "projectile references an unarmed or unknown unit"
            );
        }
        if let Some(damage) = &self.damage_effects {
            ensure!(
                damage
                    .manifest
                    .units
                    .iter()
                    .all(|overlay| world.unit_type(overlay.unit_type).is_some()),
                "damage overlay references unknown unit"
            );
        }
        if let Some(gas) = &self.gas_effects {
            ensure!(
                gas.manifest.units.iter().all(|overlay| world
                    .unit_type(overlay.unit_type)
                    .is_some_and(|unit| unit.extracts.is_some())),
                "gas overlay references unknown extractor"
            );
        }
        if let Some(grid) = &self.manifest.terrain_grid {
            ensure!(
                i64::from(grid.columns) * i64::from(grid.tile_size) == i64::from(world.map().width)
                    && i64::from(grid.rows) * i64::from(grid.tile_size)
                        == i64::from(world.map().height),
                "presentation terrain grid dimensions do not match the gameplay map"
            );
        }
        Ok(())
    }

    /// Missing assets.ron means geometric fallback; a supplied but invalid pack is an error.
    pub fn load(directory: &Path) -> Result<Option<Self>> {
        match fs::symlink_metadata(directory.join("assets.ron")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            result => {
                result.context("cannot inspect assets.ron")?;
            }
        }
        let root = directory
            .canonicalize()
            .with_context(|| format!("cannot resolve package {}", directory.display()))?;
        // Complete campaign rosters include explicit directional/action clip steps.
        let manifest_bytes = read_bounded(&package_file(&root, "assets.ron")?, 4 * 1024 * 1024)?;
        let manifest: AssetManifest =
            ron::de::from_bytes(&manifest_bytes).context("invalid assets.ron")?;
        manifest.validate()?;
        let mut rgba_bytes = 0;
        let mut load_image = |reference: &ImageRef| -> Result<Image> {
            let bytes = read_bounded(
                &package_file(&root, &reference.file)?,
                MAX_IMAGE_BYTES + HEADER_BYTES,
            )?;
            ensure!(
                blake3::hash(&bytes).to_hex().as_str() == reference.blake3,
                "asset hash mismatch: {}",
                reference.file
            );
            // Charge bytes before decoding, so the resident image collection stays bounded.
            rgba_bytes += bytes.len().saturating_sub(HEADER_BYTES);
            ensure!(
                rgba_bytes <= MAX_PACK_RGBA_BYTES,
                "native asset pack exceeds the 128 MiB RGBA limit"
            );
            decode_image(&bytes).with_context(|| format!("invalid asset {}", reference.file))
        };
        let terrain = load_image(&manifest.terrain)?;
        if let Some(grid) = &manifest.terrain_grid {
            grid.validate_atlas(&terrain)?;
        }
        let frames = manifest
            .frames
            .iter()
            .map(&mut load_image)
            .collect::<Result<Vec<_>>>()?;
        validate_frames(&frames, manifest.anchor)?;
        let mut extra_units = Vec::new();
        for sprite in &manifest.extra_units {
            let frames = sprite
                .frames
                .iter()
                .map(&mut load_image)
                .collect::<Result<Vec<_>>>()?;
            validate_frames(&frames, sprite.anchor)?;
            extra_units.push(SpritePack {
                manifest: sprite.clone(),
                frames,
            });
        }
        let mut resources = Vec::new();
        for resource in &manifest.resources {
            let image = load_image(&resource.image)?;
            validate_frames(std::slice::from_ref(&image), resource.anchor)?;
            resources.push(ResourceImage {
                manifest: resource.clone(),
                image,
            });
        }
        let ui = manifest
            .ui
            .iter()
            .map(|entry| {
                Ok(UiImage {
                    key: entry.key.clone(),
                    image: load_image(&entry.image)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let mut map_images = Vec::new();
        for entry in &manifest.map_images {
            let image = load_image(&entry.image)?;
            validate_frames(std::slice::from_ref(&image), entry.anchor)?;
            map_images.push(MapImage {
                position: entry.position,
                anchor: entry.anchor,
                image,
            });
        }
        let scan_effect = if let Some(effect) = &manifest.scan_effect {
            let frames = effect
                .frames
                .iter()
                .map(&mut load_image)
                .collect::<Result<Vec<_>>>()?;
            validate_frames(&frames, effect.anchor)?;
            Some(Effect {
                frame_ms: effect.frame_ms,
                anchor: effect.anchor,
                frames,
                sequence: effect.sequence.clone(),
            })
        } else {
            None
        };
        let mut load_effect = |effect: &EffectManifest| -> Result<Effect> {
            let frames = effect
                .frames
                .iter()
                .map(&mut load_image)
                .collect::<Result<Vec<_>>>()?;
            validate_frames(&frames, effect.anchor)?;
            Ok(Effect {
                frame_ms: effect.frame_ms,
                anchor: effect.anchor,
                frames,
                sequence: effect.sequence.clone(),
            })
        };
        let mut projectiles = Vec::new();
        for projectile in &manifest.projectiles {
            projectiles.push(Projectile {
                manifest: projectile.clone(),
                flight: load_effect(&projectile.flight)?,
                impact: load_effect(&projectile.impact)?,
            });
        }
        let damage_effects = manifest
            .damage_effects
            .as_ref()
            .map(|damage| -> Result<_> {
                let mut load_three = |effects: &[EffectManifest; 3]| -> Result<[Effect; 3]> {
                    Ok([
                        load_effect(&effects[0])?,
                        load_effect(&effects[1])?,
                        load_effect(&effects[2])?,
                    ])
                };
                Ok(DamageEffects {
                    manifest: damage.clone(),
                    small: load_three(&damage.small)?,
                    large: load_three(&damage.large)?,
                })
            })
            .transpose()?;
        let gas_effects = manifest
            .gas_effects
            .as_ref()
            .map(|gas| -> Result<_> {
                Ok(GasEffects {
                    manifest: gas.clone(),
                    plumes: [
                        load_effect(&gas.plumes[0])?,
                        load_effect(&gas.plumes[1])?,
                        load_effect(&gas.plumes[2])?,
                    ],
                    depleted: load_effect(&gas.depleted)?,
                })
            })
            .transpose()?;
        let creep = manifest
            .creep
            .as_ref()
            .map(|creep| -> Result<_> {
                let tiles = creep
                    .tiles
                    .iter()
                    .map(&mut load_image)
                    .collect::<Result<Vec<_>>>()?;
                let edges = creep
                    .edges
                    .iter()
                    .map(&mut load_image)
                    .collect::<Result<Vec<_>>>()?;
                ensure!(
                    tiles
                        .iter()
                        .chain(&edges)
                        .all(|image| image.width == 32 && image.height == 32),
                    "creep art must use 32px source tiles"
                );
                Ok(CreepPack {
                    tiles,
                    edges,
                    mask_frames: creep.mask_frames.clone(),
                })
            })
            .transpose()?;
        let indicators = manifest
            .indicators
            .as_ref()
            .map(|indicators| IndicatorsPack::load(indicators, &mut load_image))
            .transpose()?;
        let carried_resources =
            resources::load_carried(&manifest.carried_resources, &mut load_image)?;
        Ok(Some(Self {
            manifest,
            terrain,
            frames,
            extra_units,
            resources,
            carried_resources,
            ui,
            map_images,
            scan_effect,
            projectiles,
            damage_effects,
            gas_effects,
            creep,
            indicators,
        }))
    }
}

fn validate_sprite(name: &str, frame_ms: u32, frames: &[ImageRef]) -> Result<()> {
    ensure!(
        !name.trim().is_empty() && name.len() <= 128 && !name.chars().any(char::is_control),
        "asset unit_name must contain 1..=128 bytes without control characters"
    );
    ensure!(
        (1..=10_000).contains(&frame_ms),
        "asset frame_ms must be 1..=10000"
    );
    ensure!(
        (1..=MAX_FRAMES).contains(&frames.len()),
        "native animation must have 1..={MAX_FRAMES} frames"
    );
    for reference in frames {
        validate_reference(reference)?;
    }
    Ok(())
}

pub(crate) fn validate_reference(reference: &ImageRef) -> Result<()> {
    validate_filename(&reference.file)?;
    ensure!(
        reference.blake3.len() == 64
            && reference
                .blake3
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "invalid BLAKE3 digest for {}",
        reference.file
    );
    Ok(())
}

fn validate_frames(frames: &[Image], anchor: [i32; 2]) -> Result<()> {
    ensure!(
        !frames.is_empty()
            && anchor[0] >= 0
            && anchor[1] >= 0
            && frames
                .iter()
                .all(|frame| anchor[0] < frame.width as i32 && anchor[1] < frame.height as i32),
        "native animation anchor is outside its frame canvas"
    );
    Ok(())
}

fn validate_filename(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name.len() <= 128
            && name != "."
            && name != ".."
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)),
        "asset path must be a simple package filename: {name:?}"
    );
    Ok(())
}

pub(crate) fn package_file(root: &Path, name: &str) -> Result<PathBuf> {
    validate_filename(name)?;
    let path = root
        .join(name)
        .canonicalize()
        .with_context(|| format!("cannot resolve asset {name}"))?;
    ensure!(path.starts_with(root), "asset {name} escapes its package");
    Ok(path)
}

pub(crate) fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    // Check before open so a named pipe cannot block package loading.
    ensure!(
        fs::metadata(path)?.is_file(),
        "asset must be a regular file"
    );
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    ensure!(file.metadata()?.is_file(), "asset must be a regular file");
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("cannot read {}", path.display()))?;
    ensure!(bytes.len() <= limit, "asset exceeds {limit} byte limit");
    Ok(bytes)
}

#[cfg(test)]
mod tests;

mod image;
pub use image::{decode_image, encode_image};

mod projectile;
pub use projectile::{Projectile, ProjectileManifest};
