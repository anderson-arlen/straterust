//! Optional data-only map artwork. Network data contains no filenames or code.
use super::*;

const MAX_MAP_RGBA: usize = 20 * 1024 * 1024;

#[derive(Deserialize)]
pub(crate) struct TerrainFields {
    pub terrain: ImageRef,
    pub terrain_grid: Option<TerrainGrid>,
}

#[derive(Deserialize)]
#[serde(field_identifier)]
enum TerrainField {
    #[serde(rename = "schema_version")]
    Schema,
    #[serde(rename = "console_layout")]
    Console,
    #[serde(rename = "player_colors")]
    Colors,
    #[serde(rename = "terrain")]
    Image,
    #[serde(rename = "terrain_grid")]
    Grid,
    #[serde(other)]
    Other,
}

/// Imported manifests put map fields before the large unit roster. Read their
/// prefix with RON's value parser, without visiting every unit animation. A
/// reordered/authored manifest still works through ordinary deserialization.
pub(crate) fn terrain_fields(directory: &Path) -> Result<TerrainFields> {
    let root = directory.canonicalize()?;
    let bytes = read_bounded(
        &package_file(&root, "assets.ron")?,
        MAX_ASSET_MANIFEST_BYTES,
    )?;
    let overlay = ron::de::from_bytes::<SharedAssets>(&bytes).ok();
    let bytes = if let Some(overlay) = &overlay {
        crate::asset_archive::Reader::open(&root)?.read(
            &overlay.shared.file,
            &overlay.shared.blake3,
            MAX_ASSET_MANIFEST_BYTES,
        )?
    } else {
        bytes
    };
    let prefix = || -> Result<TerrainFields> {
        let mut de = ron::de::Deserializer::from_bytes(&bytes)?;
        let mut remainder = de
            .remainder()
            .strip_prefix('(')
            .context("named asset document")?;
        let mut terrain = None;
        while !remainder.trim_start().starts_with(')') {
            de = ron::de::Deserializer::from_str(remainder)?;
            let field = TerrainField::deserialize(&mut de)?;
            remainder = de
                .remainder()
                .trim_start()
                .strip_prefix(':')
                .context("missing asset field separator")?;
            de = ron::de::Deserializer::from_str(remainder)?;
            match field {
                TerrainField::Schema => {
                    u32::deserialize(&mut de)?;
                }
                TerrainField::Console => {
                    Option::<ConsoleLayout>::deserialize(&mut de)?;
                }
                TerrainField::Colors => {
                    std::collections::BTreeMap::<crate::sim::PlayerId, ColorRemap>::deserialize(
                        &mut de,
                    )?;
                }
                TerrainField::Image => terrain = Some(ImageRef::deserialize(&mut de)?),
                TerrainField::Grid if terrain.is_some() => {
                    return Ok(TerrainFields {
                        terrain: terrain.unwrap(),
                        terrain_grid: Option::<TerrainGrid>::deserialize(&mut de)?,
                    });
                }
                _ => {
                    anyhow::bail!("map fields follow unit artwork");
                }
            }
            remainder = de.remainder().trim_start();
            if let Some(rest) = remainder.strip_prefix(',') {
                remainder = rest;
            } else {
                break;
            }
        }
        anyhow::bail!("asset grid was not in the map prefix")
    };
    // Use concrete number types: RON's generic ignored-number lookahead scans
    // the remaining document, which is very expensive for large art manifests.
    let mut fields = prefix().or_else(|_| -> Result<TerrainFields> {
        let manifest: AssetManifest = ron::de::from_bytes(&bytes)?;
        Ok(TerrainFields {
            terrain: manifest.terrain,
            terrain_grid: manifest.terrain_grid,
        })
    })?;
    if let Some(overlay) = overlay {
        fields.terrain_grid = overlay.terrain_grid;
    }
    Ok(fields)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapArtwork {
    pub terrain: Vec<u8>,
    pub grid: Option<TerrainGrid>,
    pub images: Vec<Vec<u8>>,
    pub decorations: Vec<Decoration>,
    #[serde(default)]
    pub player_colors: std::collections::BTreeMap<crate::sim::PlayerId, ColorRemap>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decoration {
    pub position: Position,
    pub anchor: [i32; 2],
    pub image: u16,
}

#[derive(Debug)]
pub struct DecodedMapArtwork {
    pub terrain: Image,
    pub grid: Option<TerrainGrid>,
    pub decorations: Vec<MapImage>,
}

impl MapArtwork {
    pub fn load_thumbnail(directory: &Path, map: &crate::sim::Map) -> Result<Image> {
        let root = directory.canonicalize()?;
        let fields = terrain_fields(&root)?;
        let terrain = crate::asset_archive::Reader::open(&root)?.read(
            &fields.terrain.file,
            &fields.terrain.blake3,
            MAX_IMAGE_BYTES + HEADER_BYTES,
        )?;
        ensure!(
            blake3::hash(&terrain).to_hex().as_str() == fields.terrain.blake3,
            "map image hash mismatch"
        );
        Self {
            terrain,
            grid: fields.terrain_grid,
            images: Vec::new(),
            decorations: Vec::new(),
            player_colors: Default::default(),
        }
        .thumbnail(map)
    }
    /// Small unfogged terrain image for map selection; never creates a world.
    pub fn thumbnail(&self, map: &crate::sim::Map) -> Result<Image> {
        ensure!(
            (1..=32768).contains(&map.width) && (1..=32768).contains(&map.height),
            "invalid map preview size"
        );
        let terrain = decode_image(&self.terrain)?;
        let grid = self.grid.as_ref().context("map has no terrain grid")?;
        grid.validate_atlas(&terrain)?;
        ensure!(
            i64::from(grid.columns) * i64::from(grid.tile_size) == i64::from(map.width)
                && i64::from(grid.rows) * i64::from(grid.tile_size) == i64::from(map.height),
            "map preview dimensions mismatch"
        );
        let extent = map.width.max(map.height) as u32;
        let width = (map.width as u32 * 256 / extent).max(1);
        let height = (map.height as u32 * 256 / extent).max(1);
        let mut image = Image {
            width,
            height,
            rgba: vec![0; (width * height * 4) as usize],
        };
        let across = terrain.width / grid.tile_size;
        for y in 0..height {
            for x in 0..width {
                let wx = x * map.width as u32 / width;
                let wy = y * map.height as u32 / height;
                let tile =
                    grid.tiles[(wy / grid.tile_size * grid.columns + wx / grid.tile_size) as usize];
                let px = tile % across * grid.tile_size + wx % grid.tile_size;
                let py = tile / across * grid.tile_size + wy % grid.tile_size;
                let source = ((py * terrain.width + px) * 4) as usize;
                let target = ((y * width + x) * 4) as usize;
                image.rgba[target..target + 4].copy_from_slice(&terrain.rgba[source..source + 4]);
            }
        }
        Ok(image)
    }

    pub fn load(directory: &Path) -> Result<Option<Self>> {
        if !directory.join("assets.ron").exists() {
            return Ok(None);
        }
        let root = directory.canonicalize()?;
        let manifest = AssetManifest::load_manifest(&root)?;
        manifest.validate()?;
        let source = crate::asset_archive::Reader::open(&root)?;
        let mut total = 0;
        let mut load = |reference: &ImageRef| -> Result<Vec<u8>> {
            let bytes = source.read(
                &reference.file,
                &reference.blake3,
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
        let mut images = Vec::new();
        let mut image_ids = std::collections::BTreeMap::new();
        let decorations = manifest
            .map_images
            .iter()
            .map(|d| {
                let image = if let Some(index) = image_ids.get(&d.image.blake3) {
                    *index
                } else {
                    let index = images.len() as u16;
                    images.push(load(&d.image)?);
                    image_ids.insert(d.image.blake3.clone(), index);
                    index
                };
                Ok(Decoration {
                    position: d.position,
                    anchor: d.anchor,
                    image,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(Self {
            terrain,
            grid: manifest.terrain_grid,
            images,
            decorations,
            player_colors: manifest.player_colors,
        }))
    }

    pub fn validate(&self, world: &World) -> Result<()> {
        self.decode(world).map(|_| ())
    }

    pub fn decode(&self, world: &World) -> Result<DecodedMapArtwork> {
        colors::validate(&self.player_colors)?;
        ensure!(
            self.decorations.len() <= 4096
                && self.images.len() <= 4096
                && self
                    .terrain
                    .len()
                    .saturating_add(self.images.iter().map(Vec::len).sum::<usize>())
                    <= MAX_MAP_RGBA,
            "map artwork exceeds limits"
        );
        let terrain = decode_image(&self.terrain)?;
        let images = self
            .images
            .iter()
            .map(|bytes| decode_image(bytes).map(Arc::new))
            .collect::<Result<Vec<_>>>()?;
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
                    image: Arc::clone(
                        images
                            .get(usize::from(d.image))
                            .context("invalid decoration image index")?,
                    ),
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
