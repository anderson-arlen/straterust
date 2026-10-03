//! Ground resources and worker cargo art. Resource kinds and unit IDs are
//! ruleset data; these optional mappings never affect harvesting or deposits.
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceManifest {
    pub kind: String,
    pub anchor: [i32; 2],
    pub image: ImageRef,
    #[serde(default)]
    pub selection_circle: Option<u8>,
    #[serde(default)]
    pub selection_y: i16,
}

#[derive(Debug)]
pub struct ResourceImage {
    pub manifest: ResourceManifest,
    pub image: Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarriedResourceManifest {
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
