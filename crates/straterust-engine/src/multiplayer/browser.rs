//! Read-only map browser metadata. Original formats and gameplay stay outside UI.
use super::*;
use crate::assets::{Image, MapArtwork};

#[derive(Clone, Debug)]
pub struct Overview {
    pub layout: String,
    pub settings: String,
    pub fields: Vec<(String, String)>,
    pub army: String,
    pub resources: String,
    pub terrain: String,
}

pub struct Preview {
    pub image: Image,
    pub size: [i32; 2],
    pub starts: Vec<StartLocation>,
}

#[derive(Deserialize, Default)]
struct Names {
    #[serde(default)]
    unit_names: BTreeMap<UnitTypeId, String>,
}

fn geometry(directory: &Path, map: &Map) -> Result<Vec<u8>> {
    if directory.join("terrain.srtm").exists() {
        let bytes = crate::assets::read_bounded(
            &directory.join("terrain.srtm"),
            crate::map::MAX_TERRAIN_BYTES,
        )?;
        crate::map::decode_terrain(&bytes)?.validate_coverage(map.width, map.height)?;
        Ok(bytes)
    } else if let Some(terrain) = &map.terrain {
        crate::map::encode_terrain(terrain)
    } else {
        Ok(Vec::new())
    }
}

fn native_map(directory: &Path) -> Result<Map> {
    let map: Map = read_ron(&directory.join("map.ron"))?;
    ensure!(
        (1..=32768).contains(&map.width)
            && (1..=32768).contains(&map.height)
            && map.start_locations.len() <= 16
            && map.resources.len() <= 65536,
        "invalid map browser dimensions or placements"
    );
    Ok(map)
}

pub fn inspect(directory: &Path, entry: &MapEntry, catalog: &Catalog) -> Result<Overview> {
    let mut map = native_map(directory)?;
    let mut setup: MapSetup = read_ron(&directory.join("match.ron"))?;
    let mut layout = blake3::Hasher::new();
    layout.update(&map.width.to_le_bytes());
    layout.update(&map.height.to_le_bytes());
    let geometry = geometry(directory, &map)?;
    layout.update(&geometry);
    let mut artwork = blake3::Hasher::new();
    if directory.join("assets.ron").exists() {
        let manifest = crate::assets::map_artwork::terrain_fields(directory)?;
        artwork.update(manifest.terrain.blake3.as_bytes());
        if let Some(grid) = manifest.terrain_grid {
            grid.validate()?;
            let grid = ron::ser::to_string(&grid)?;
            artwork.update(grid.as_bytes());
            // Native geometry includes movement, height and building restrictions.
            // Cosmetic tile changes are versions of that layout. Without native
            // geometry, artwork is the only available layout comparison.
            if geometry.is_empty() {
                layout.update(grid.as_bytes());
            }
        }
    }
    let terrain_hash = artwork.finalize().to_hex().to_string();
    // Deposits can be repositioned/configured without changing the terrain.
    // Keep these as selectable settings versions with different preview markers.
    let resource_hash = blake3::hash(ron::ser::to_string(&map.resources)?.as_bytes())
        .to_hex()
        .to_string();

    let mut totals = BTreeMap::<String, u64>::new();
    for r in &map.resources {
        *totals.entry(r.kind.clone()).or_default() += u64::from(r.amount);
    }
    let amounts = |values: &[ResourceAmount]| {
        values
            .iter()
            .map(|r| format!("{} {}", r.amount, r.kind))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let resources = if setup.resources.windows(2).all(|r| r[0] == r[1]) {
        setup
            .resources
            .first()
            .map_or_else(String::new, |r| amounts(r))
    } else {
        setup
            .resources
            .iter()
            .enumerate()
            .map(|(i, r)| format!("P{}: {}", i + 1, amounts(r)))
            .collect::<Vec<_>>()
            .join("; ")
    };
    let army = setup.scenario_spawns.as_ref().unwrap_or(&map.spawns);
    let mut armies = vec![BTreeMap::<u16, usize>::new(); usize::from(entry.players)];
    for s in army {
        if let Some(counts) = armies.get_mut(usize::from(s.owner.0)) {
            *counts.entry(s.unit_type.0).or_default() += 1;
        }
    }
    let names: Names = if directory.join("presentation.ron").exists() {
        read_ron(&directory.join("presentation.ron"))?
    } else {
        Names::default()
    };
    let army_label = armies
        .iter()
        .enumerate()
        .map(|(i, units)| {
            format!(
                "P{}: {}",
                i + 1,
                units
                    .iter()
                    .map(|(id, count)| format!(
                        "{count} {}",
                        names
                            .unit_names
                            .get(&UnitTypeId(*id))
                            .cloned()
                            .unwrap_or_else(|| "units".into())
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let army_hash = blake3::hash(ron::ser::to_string(&armies)?.as_bytes())
        .to_hex()
        .to_string();
    let mut starts = map.start_locations.clone();
    starts.sort_by_key(|s| s.player);
    let mut fields = vec![
        ("Players".into(), entry.players.to_string()),
        (
            "Terrain".into(),
            catalog
                .tilesets
                .get(usize::from(entry.tileset))
                .cloned()
                .unwrap_or_else(|| format!("Terrain set {}", entry.tileset + 1)),
        ),
        (
            "Races".into(),
            setup
                .factions
                .iter()
                .map(|f| {
                    catalog
                        .factions
                        .get(*f)
                        .map_or("Unknown", |r| r.name.as_str())
                })
                .collect::<Vec<_>>()
                .join(", "),
        ),
        ("Starting resources".into(), resources),
        (
            "Starting army".into(),
            if armies.iter().all(BTreeMap::is_empty) {
                "Standard match starting units".into()
            } else {
                army_label
            },
        ),
        (
            "Total resources".into(),
            totals
                .iter()
                .map(|(k, n)| format!("{n} {k}"))
                .collect::<Vec<_>>()
                .join(", "),
        ),
        (
            "Resource deposits".into(),
            map.resources
                .iter()
                .filter(|r| r.kind != "wood")
                .take(128)
                .map(|r| {
                    format!(
                        "{} {} at ({},{})",
                        r.amount, r.kind, r.position.x, r.position.y
                    )
                })
                .collect::<Vec<_>>()
                .join(", "),
        ),
        (
            "Starting locations".into(),
            starts
                .iter()
                .map(|s| format!("P{} ({},{})", s.player.0 + 1, s.position.x, s.position.y))
                .collect::<Vec<_>>()
                .join(", "),
        ),
    ];
    fields.push((
        "Use Map Settings".into(),
        setup
            .scenario_error
            .clone()
            .unwrap_or_else(|| "Available".into()),
    ));
    // IDs and generated terrain-package filenames do not change map settings.
    map.id.clear();
    map.terrain = None;
    setup.variants.clear();
    if directory.join("mission.ron").exists() {
        map.mission = Some(read_ron(&directory.join("mission.ron"))?);
    }
    let mut settings = blake3::Hasher::new();
    settings.update(ron::ser::to_string(&(entry.players, entry.tileset, &map, &setup))?.as_bytes());
    // Map keeps mission data outside map.ron and skips it during serialization.
    settings.update(ron::ser::to_string(&map.mission)?.as_bytes());
    settings.update(terrain_hash.as_bytes());
    // Unsupported scenarios may differ in source data that has no native form.
    // Keep those versions separate rather than claiming their settings are equal.
    if setup.scenario_error.is_some() {
        settings.update(entry.package.as_bytes());
    }
    Ok(Overview {
        layout: layout.finalize().to_hex().to_string(),
        settings: settings.finalize().to_hex().to_string(),
        fields,
        army: army_hash,
        resources: resource_hash,
        terrain: terrain_hash,
    })
}

pub fn preview(directory: &Path) -> Result<Preview> {
    let map = native_map(directory)?;
    let mut image = MapArtwork::load_thumbnail(directory, &map)?;
    for resource in &map.resources {
        if resource.amount == 0 || resource.kind == "wood" {
            continue;
        }
        let x = i64::from(resource.position.x) * i64::from(image.width) / i64::from(map.width);
        let y = i64::from(resource.position.y) * i64::from(image.height) / i64::from(map.height);
        for dy in -1..=1 {
            for dx in -1..=1 {
                if x + dx >= 0
                    && y + dy >= 0
                    && x + dx < i64::from(image.width)
                    && y + dy < i64::from(image.height)
                {
                    let offset = (((y + dy) as u32 * image.width + (x + dx) as u32) * 4) as usize;
                    image.rgba[offset..offset + 4].copy_from_slice(&[230, 203, 87, 255]);
                }
            }
        }
    }
    Ok(Preview {
        image,
        size: [map.width, map.height],
        starts: map.start_locations,
    })
}
