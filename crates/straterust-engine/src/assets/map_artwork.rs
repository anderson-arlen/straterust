//! Optional data-only map artwork. Network data contains no filenames or code.
use super::*;

const MAX_MAP_RGBA: usize = 20 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapArtwork {
    pub terrain: Vec<u8>,
    pub grid: Option<TerrainGrid>,
    pub decorations: Vec<Decoration>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decoration {
    pub position: Position,
    pub anchor: [i32; 2],
    pub image: Vec<u8>,
}

#[derive(Debug)]
pub struct DecodedMapArtwork {
    pub terrain: Image,
    pub grid: Option<TerrainGrid>,
    pub decorations: Vec<MapImage>,
}

impl MapArtwork {
    pub fn load(directory: &Path) -> Result<Option<Self>> {
        if !directory.join("assets.ron").exists() {
            return Ok(None);
        }
        let root = directory.canonicalize()?;
        let manifest: AssetManifest = ron::de::from_bytes(&read_bounded(
            &package_file(&root, "assets.ron")?,
            4 * 1024 * 1024,
        )?)?;
        manifest.validate()?;
        let mut total = 0;
        let mut load = |reference: &ImageRef| -> Result<Vec<u8>> {
            let bytes = read_bounded(
                &package_file(&root, &reference.file)?,
                MAX_IMAGE_BYTES + HEADER_BYTES,
            )?;
            ensure!(
                blake3::hash(&bytes).to_hex().as_str() == reference.blake3,
                "map image hash mismatch"
            );
            total += bytes.len();
            ensure!(total <= MAX_MAP_RGBA, "map artwork exceeds 20 MiB limit");
            decode_image(&bytes)?;
            Ok(bytes)
        };
        let terrain = load(&manifest.terrain)?;
        let decorations = manifest
            .map_images
            .iter()
            .map(|d| {
                Ok(Decoration {
                    position: d.position,
                    anchor: d.anchor,
                    image: load(&d.image)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(Self {
            terrain,
            grid: manifest.terrain_grid,
            decorations,
        }))
    }

    pub fn validate(&self, world: &World) -> Result<()> {
        self.decode(world).map(|_| ())
    }

    pub fn decode(&self, world: &World) -> Result<DecodedMapArtwork> {
        ensure!(
            self.decorations.len() <= 4096
                && self.terrain.len().saturating_add(
                    self.decorations
                        .iter()
                        .map(|d| d.image.len())
                        .sum::<usize>()
                ) <= MAX_MAP_RGBA,
            "map artwork exceeds limits"
        );
        let terrain = decode_image(&self.terrain)?;
        if let Some(grid) = &self.grid {
            grid.validate_atlas(&terrain)?;
            ensure!(
                i64::from(grid.columns) * i64::from(grid.tile_size) == i64::from(world.map().width)
                    && i64::from(grid.rows) * i64::from(grid.tile_size)
                        == i64::from(world.map().height),
                "map artwork dimensions mismatch"
            );
        }
        let decorations = self
            .decorations
            .iter()
            .map(|d| {
                ensure!(
                    world.map().contains(d.position)
                        && d.anchor.iter().all(|v| (-32768..=32768).contains(v)),
                    "invalid map decoration position"
                );
                Ok(MapImage {
                    position: d.position,
                    anchor: d.anchor,
                    image: decode_image(&d.image)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(DecodedMapArtwork {
            terrain,
            grid: self.grid.clone(),
            decorations,
        })
    }
}
