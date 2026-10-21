//! Logical tiles, 32px megatiles and native collision grids.
use super::{
    gfx::Palette,
    pud::{Pud, word},
    war::WarArchive,
};
use anyhow::{Context, Result, ensure};
use straterust_engine::{
    assets::{Image, TerrainGrid},
    map::{BLOCKS_SIGHT, BUILDABLE, Terrain, WALKABLE, WATER},
};

pub struct Tileset {
    pub palette: Palette,
    pub atlas: Image,
    mapping: Vec<u8>,
}

impl Tileset {
    pub fn load(archive: &WarArchive, era: usize) -> Result<Self> {
        let base = [2, 18, 10, 438][era];
        let palette = super::gfx::palette(&archive.entry(base)?)?;
        let mega = archive.entry(base + 1)?;
        let mini = archive.entry(base + 2)?;
        let mapping = archive.entry(base + 3)?;
        ensure!(
            mega.len().is_multiple_of(32) && mega.len() / 32 <= 4096,
            "invalid megatile count"
        );
        ensure!(
            mapping.len().is_multiple_of(42),
            "invalid logical tile mapping"
        );
        let count = mega.len() / 32;
        let columns = 32;
        let rows = count.div_ceil(columns);
        let width = columns * 32;
        let height = rows * 32;
        ensure!(height <= 2048, "terrain atlas exceeds limit");
        let mut rgba = vec![0; width * height * 4];
        for tile in 16..count {
            for cell in 0..16 {
                let index = usize::from(word(&mega, tile * 32 + cell * 2)?);
                let base = (index & !3) * 16;
                let pixels = mini
                    .get(base..base + 64)
                    .context("minitile outside archive record")?;
                for y in 0..8 {
                    for x in 0..8 {
                        let sx = if index & 2 != 0 { 7 - x } else { x };
                        let sy = if index & 1 != 0 { 7 - y } else { y };
                        let px = tile % columns * 32 + cell % 4 * 8 + x;
                        let py = tile / columns * 32 + cell / 4 * 8 + y;
                        let dest = (py * width + px) * 4;
                        rgba[dest..dest + 4]
                            .copy_from_slice(&palette[pixels[sy * 8 + sx] as usize]);
                    }
                }
            }
        }
        Ok(Self {
            palette,
            atlas: Image {
                width: width as u32,
                height: height as u32,
                rgba,
            },
            mapping,
        })
    }

    pub fn forest(
        &self,
        root: &std::path::Path,
    ) -> Result<straterust_engine::assets::ResourceManifest> {
        // Forest remains in the map's original tiles. Native megatile 126 is
        // harvested forest/stumps in every era; it replaces known depleted cells.
        Ok(straterust_engine::assets::ResourceManifest {
            kind: "wood".into(),
            positions: Vec::new(),
            terrain: true,
            terrain_edges: Some(self.forest_edges()?),
            anchor: [16, 16],
            image: super::gfx::write_image(root, &self.tile_image(125))?,
            depleted_image: Some(super::gfx::write_image(root, &self.tile_image(126))?),
            active_image: None,
            selection_circle: None,
            selection_y: 0,
        })
    }

    pub fn rocks(
        &self,
        root: &std::path::Path,
        era: usize,
    ) -> Result<straterust_engine::assets::ResourceManifest> {
        let (live, dead) = super::rocks::tiles(era);
        Ok(super::rocks::manifest(
            era,
            super::gfx::write_image(root, &self.tile_image(live))?,
            super::gfx::write_image(root, &self.tile_image(dead))?,
        ))
    }

    fn forest_edges(&self) -> Result<straterust_engine::assets::ResourceTerrainEdges> {
        let mut corners = std::collections::BTreeMap::new();
        let mut tiles = [None; 16];
        // Native mixed forest rows describe occupied tile corners. Resolve
        // logical IDs through this era's mapping, including visual variants.
        for (logical, mask) in std::iter::once((0x70, 15)).chain(
            [8, 4, 12, 1, 9, 5, 13, 2, 10, 6, 14, 3, 11, 7]
                .into_iter()
                .enumerate()
                .map(|(i, mask)| (0x700 + i * 16, mask)),
        ) {
            for variant in 0..16 {
                let tile = u32::from(word(&self.mapping, (logical >> 4) * 42 + variant * 2)?);
                if tile != 0 {
                    corners.insert(tile, mask);
                    tiles[usize::from(mask)].get_or_insert(tile);
                }
            }
        }
        corners.extend([(121, 3 + 32), (122, 15 + 48), (123, 12 + 16)]);
        Ok(straterust_engine::assets::ResourceTerrainEdges {
            corners,
            tiles,
            isolated: [None, Some(123), Some(121), Some(122)],
        })
    }

    pub(super) fn tile_image(&self, tile: u32) -> Image {
        let columns = self.atlas.width / 32;
        let x = tile % columns * 32;
        let y = tile / columns * 32;
        let mut rgba = Vec::with_capacity(32 * 32 * 4);
        for row in y..y + 32 {
            let start = ((row * self.atlas.width + x) * 4) as usize;
            rgba.extend_from_slice(&self.atlas.rgba[start..start + 32 * 4]);
        }
        Image {
            width: 32,
            height: 32,
            rgba,
        }
    }

    pub fn map(&self, pud: &Pud) -> Result<(TerrainGrid, Terrain, Vec<usize>)> {
        let logical = pud.words(b"MTXM")?;
        let regions = pud.words(b"REGM")?;
        let squares = pud.words(b"SQM ")?;
        ensure!(
            logical.len() == regions.len() && regions.len() == squares.len(),
            "map grids differ in length"
        );
        let mut tiles = Vec::with_capacity(logical.len());
        let mut flags = Vec::with_capacity(logical.len());
        let mut trees = Vec::new();
        for (index, ((tile, region), square)) in
            logical.iter().zip(&regions).zip(&squares).enumerate()
        {
            tiles.push(u32::from(word(
                &self.mapping,
                usize::from(tile >> 4) * 42 + usize::from(tile & 15) * 2,
            )?));
            let tree = *region == 0xfffe;
            if tree {
                trees.push(index);
            }
            let flag = if tree {
                WALKABLE | BUILDABLE
            } else if *region == 0xfffd {
                BLOCKS_SIGHT
            } else if *square == 0x82 || *square == 2 {
                WALKABLE | WATER
            } else if square & 0x80 != 0 {
                BLOCKS_SIGHT
            } else if square & 0x40 != 0 {
                WATER
            } else if *square == 0 {
                WALKABLE | WATER
            } else if square & 1 != 0 {
                // SQM dirt (0x0011), including ground/dirt edges, permits
                // walking but forbids construction. Walkability alone is not
                // enough to mark a tile buildable.
                WALKABLE | if square & 0x10 == 0 { BUILDABLE } else { 0 }
            } else {
                0
            };
            flags.push(flag);
        }
        Ok((
            TerrainGrid {
                tile_size: 32,
                columns: u32::from(pud.width),
                rows: u32::from(pud.height),
                tiles,
            },
            Terrain {
                cell_size: 32,
                columns: u32::from(pud.width),
                rows: u32::from(pud.height),
                flags,
            },
            trees,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use straterust_engine::{assets::AssetPack, content::Campaign};

    #[test]
    fn dirt_rejects_building_footprints_without_blocking_ground_movement() {
        use straterust_engine::{
            map::{Footprint, MovementClass, PlacementSurface},
            sim::{Map, Position},
        };
        // A three-tile building is valid on ground; moving it one tile right
        // overlaps a walkable dirt edge. The second row is all dirt; the last
        // row supplies water, coast and blocked terrain as controls.
        let squares: [u16; 12] = [1, 1, 1, 0x11, 0x11, 0x11, 0x11, 0x11, 0x40, 0x82, 0x81, 0];
        let logical: [u16; 12] = [
            0x50, 0x50, 0x60, 0x500, 0x30, 0x40, 0x300, 0x500, 0x10, 0x200, 0x80, 0x50,
        ];
        let regions: [u16; 12] = [
            0x4000, 0x4000, 0x4000, 0x4000, 0x4000, 0x4000, 0x4000, 0x4000, 0, 0, 0xfffd, 0x4000,
        ];
        let mut mapping = vec![0; 0x51 * 42];
        for tile in logical {
            let offset = usize::from(tile >> 4) * 42 + usize::from(tile & 15) * 2;
            mapping[offset..offset + 2].copy_from_slice(&200_u16.to_le_bytes());
        }
        let set = Tileset {
            palette: [[0; 4]; 256],
            atlas: Image {
                width: 1,
                height: 1,
                rgba: vec![0; 4],
            },
            mapping,
        };
        let pud = Pud {
            chunks: [(b"MTXM", logical), (b"REGM", regions), (b"SQM ", squares)]
                .into_iter()
                .map(|(key, values)| {
                    (
                        *key,
                        values.into_iter().flat_map(u16::to_le_bytes).collect(),
                    )
                })
                .collect(),
            width: 4,
            height: 3,
            era: 2,
            units: Vec::new(),
            owners: [3; 16],
            sides: [0; 16],
            local: 0,
            title: "Public dirt placement fixture".into(),
        };
        let (_, terrain, _) = set.map(&pud).unwrap();
        let map: Map =
            ron::from_str("(id: \"dirt\", width: 128, height: 96, players: 1, spawns: [])")
                .unwrap();
        let map = Map {
            terrain: Some(terrain),
            ..map
        };
        let building = Footprint {
            width: 96,
            height: 32,
        };
        assert!(map.can_build(Position { x: 48, y: 16 }, building));
        assert!(!map.can_build(Position { x: 80, y: 16 }, building));
        assert!(!map.can_build(Position { x: 48, y: 48 }, building));
        for x in [16, 48, 80, 112] {
            let position = Position { x, y: 48 };
            assert!(map.can_move(position, Footprint::default(), MovementClass::Ground));
            assert!(!map.can_build(position, Footprint::default()));
        }
        let coast = Position { x: 48, y: 80 };
        assert!(map.can_build_on(coast, Footprint::default(), PlacementSurface::Shore));
        assert!(!map.can_build(coast, Footprint::default()));
        let water = Position { x: 16, y: 80 };
        assert!(map.can_move(water, Footprint::default(), MovementClass::Water));
        assert!(!map.can_move(water, Footprint::default(), MovementClass::Ground));
        let rocks = Position { x: 80, y: 80 };
        assert!(!map.can_move(rocks, Footprint::default(), MovementClass::Ground));
        assert!(!map.can_build(rocks, Footprint::default()));
    }

    #[test]
    #[ignore = "requires STRATERUST_WARCRAFT2 and STRATERUST_WARCRAFT2_SOURCE (extracted retail directory)"]
    fn installed_campaign_dirt_forbids_building_and_older_saves_use_current_terrain() {
        use straterust_engine::{
            content::Package,
            map::{Footprint, MovementClass},
            session::{SavedGame, ServerSession},
            sim::*,
        };
        let root = super::super::gameplay_tests::root();
        let source =
            std::path::PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2_SOURCE").unwrap());
        let archive = WarArchive::open(&source.join("Support/TOMES/TOME.1"), 1000).unwrap();
        let mut checked = std::collections::BTreeSet::new();
        let mut dirt_cells = 0;
        for (folder, race, expansion) in [
            ("human", 0, false),
            ("orc", 1, false),
            ("human-expansion", 0, true),
            ("orc-expansion", 1, true),
        ] {
            let campaign = Campaign::load(&root.join(folder)).unwrap();
            for (index, mission) in campaign.missions.iter().enumerate() {
                let pud = Pud::decode(
                    &archive
                        .entry(if expansion { 446 } else { 192 } + index * 2 + race)
                        .unwrap(),
                )
                .unwrap();
                let world = Package::load(&root.join(folder).join(&mission.package))
                    .unwrap()
                    .world(7)
                    .unwrap();
                let map = world.map();
                for (cell, square) in pud.words(b"SQM ").unwrap().into_iter().enumerate() {
                    if square != 0x11 {
                        continue;
                    }
                    let position = Position {
                        x: cell as i32 % i32::from(pud.width) * 32 + 16,
                        y: cell as i32 / i32::from(pud.width) * 32 + 16,
                    };
                    assert!(map.can_move(position, Footprint::default(), MovementClass::Ground));
                    assert!(
                        !map.can_build(position, Footprint::default()),
                        "{folder}/{index}/{cell}"
                    );
                    dirt_cells += 1;
                }
                if checked.contains(&(pud.era, race)) {
                    continue;
                }
                let building = world.unit_type(super::super::stats::id(60 + race)).unwrap();
                let mut rules = world.rules().clone();
                rules.victory = false;
                rules.starting_resources = ["gold", "wood", "oil"]
                    .map(|kind| ResourceAmount {
                        kind: kind.into(),
                        amount: 10000,
                    })
                    .to_vec();
                let mut map = map.clone();
                map.ai.clear();
                map.mission = None;
                map.creation.clear();
                map.initial_explored.clear();
                map.fog_of_war = false;
                map.resources.clear();
                let mut legacy_map = map.clone();
                for flags in &mut legacy_map.terrain.as_mut().unwrap().flags {
                    if *flags == WALKABLE {
                        *flags |= BUILDABLE;
                    }
                }
                let Some(target) =
                    (0..map.terrain.as_ref().unwrap().flags.len()).find_map(|cell| {
                        let position = Position {
                            x: cell as i32 % i32::from(pud.width) * 32 + 16,
                            y: cell as i32 / i32::from(pud.width) * 32 + 16,
                        };
                        (legacy_map.can_build(position, building.placement)
                            && map.can_move(position, building.placement, MovementClass::Ground)
                            && !map.can_build(position, building.placement))
                        .then_some(position)
                    })
                else {
                    continue;
                };
                checked.insert((pud.era, race));
                map.spawns = vec![Spawn {
                    unit_type: super::super::stats::id(2 + race),
                    position: target,
                    ..Default::default()
                }];
                let current = World::new(rules.clone(), map.clone(), 7).unwrap();
                let order = Order::Build {
                    entity: EntityId(1),
                    unit_type: building.id,
                    position: target,
                };
                legacy_map.spawns = map.spawns;
                let mut old = World::new(rules, legacy_map, 7).unwrap();
                assert!(
                    old.build_rejection(PlayerId(0), EntityId(1), building.id, target)
                        .is_none(),
                    "{folder}/{index}: {:?}",
                    old.build_rejection(PlayerId(0), EntityId(1), building.id, target)
                );
                let outcomes = old
                    .step(&[Command {
                        tick: old.tick(),
                        player: PlayerId(0),
                        sequence: 1,
                        order: order.clone(),
                    }])
                    .unwrap();
                assert!(outcomes[0].rejection.is_none());
                assert!(
                    old.state()
                        .entities
                        .iter()
                        .any(|e| e.unit_type == building.id)
                );
                let save =
                    SavedGame::capture(&ServerSession::new(old, 7, vec![PlayerId(0)]).unwrap())
                        .unwrap();
                let mut restored = ServerSession::restore_saved(&current, save).unwrap();
                let remaining = |world: &World| {
                    world
                        .state()
                        .entities
                        .iter()
                        .find(|e| e.unit_type == building.id)
                        .unwrap()
                        .construction
                        .as_ref()
                        .unwrap()
                        .remaining
                };
                let before = remaining(restored.world());
                restored.advance(&[]).unwrap();
                assert!(remaining(restored.world()) < before);
                let mut rejected = current;
                let balances = rejected.state().players[0].resources.clone();
                let outcomes = rejected
                    .step(&[Command {
                        tick: rejected.tick(),
                        player: PlayerId(0),
                        sequence: 1,
                        order,
                    }])
                    .unwrap();
                assert_eq!(outcomes[0].rejection, Some(Rejection::InvalidPlacement));
                assert_eq!(rejected.state().players[0].resources, balances);
                assert_eq!(rejected.state().entities.len(), 1);
            }
        }
        assert_eq!(checked.len(), 8, "both races in every era");
        assert!(dirt_cells > 1000);
    }

    #[test]
    #[ignore = "requires STRATERUST_WARCRAFT2 and STRATERUST_WARCRAFT2_SOURCE (extracted retail directory)"]
    fn installed_forest_stumps_and_occupied_mines_match_original_art_in_every_era() {
        let root = std::path::PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2").unwrap());
        let source =
            std::path::PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2_SOURCE").unwrap());
        let archive = WarArchive::open(
            &super::super::source::case_path(&source, "Support/TOMES/TOME.1").unwrap(),
            1000,
        )
        .unwrap();
        let mut eras = std::collections::BTreeSet::new();
        for (folder, race, expansion) in [
            ("human", 0, false),
            ("orc", 1, false),
            ("human-expansion", 0, true),
            ("orc-expansion", 1, true),
        ] {
            let campaign = Campaign::load(&root.join(folder)).unwrap();
            for (i, mission) in campaign.missions.iter().enumerate() {
                let pud = Pud::decode(
                    &archive
                        .entry(if expansion { 446 } else { 192 } + i * 2 + race)
                        .unwrap(),
                )
                .unwrap();
                let set = Tileset::load(&archive, pud.era).unwrap();
                let (grid, _, trees) = set.map(&pud).unwrap();
                let native_edges = set.forest_edges().unwrap();
                let assets = AssetPack::load(&root.join(folder).join(&mission.package))
                    .unwrap()
                    .unwrap();
                let actual = assets.manifest.terrain_grid.as_ref().unwrap();
                for tree in trees {
                    assert!(native_edges.corners.contains_key(&grid.tiles[tree]));
                    assert_eq!(
                        actual.tiles[tree], grid.tiles[tree],
                        "forest was replaced by ground"
                    );
                }
                if !eras.insert(pud.era) {
                    continue;
                }
                let wood = assets
                    .resources
                    .iter()
                    .find(|art| art.manifest.kind == "wood")
                    .unwrap();
                assert!(wood.manifest.terrain);
                assert_eq!(
                    wood.manifest.terrain_edges.as_ref().unwrap().isolated[0],
                    None
                );
                assert_eq!(wood.depleted_image.as_ref().unwrap(), &set.tile_image(126));
                let source_icons = super::super::gfx::sprites(
                    &archive.entry([356, 357, 358, 471][pud.era]).unwrap(),
                    &set.palette,
                )
                .unwrap();
                assert_eq!(assets.ui_image("command.rally").unwrap(), &source_icons[83]);
                let source_gold = super::super::gfx::sprites(
                    &archive
                        .entry(super::super::art::graphic(92, pud.era).unwrap())
                        .unwrap(),
                    &set.palette,
                )
                .unwrap();
                let gold = assets
                    .resources
                    .iter()
                    .find(|art| art.manifest.kind == "gold")
                    .unwrap();
                assert_eq!(&gold.image, &source_gold[0]);
                assert_eq!(gold.active_image.as_ref().unwrap(), &source_gold[1]);
                assert_ne!(gold.image, gold.active_image.as_ref().unwrap().clone());
            }
        }
        assert_eq!(eras.len(), 4);
    }
}
