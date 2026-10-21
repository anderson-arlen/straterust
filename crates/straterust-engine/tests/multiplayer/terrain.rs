use super::*;
use straterust_engine::{asset_archive, assets::*, content::read_ron};

#[test]
fn terrain_choice_assembles_and_transfers_in_memory_from_shared_data_without_writing_variants() {
    let (source, mut catalog) = fixture();
    let map_directory = source.path().join("map");
    let pixels: Vec<_> = (0..32)
        .flat_map(|_| {
            (0..64).flat_map(|x| {
                if x < 32 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                }
            })
        })
        .collect();
    let bytes = encode_image(&Image {
        width: 64,
        height: 32,
        rgba: pixels,
    })
    .unwrap();
    let reference = ImageRef {
        file: "tiles.srim".into(),
        blake3: blake3::hash(&bytes).to_hex().to_string(),
    };
    std::fs::write(map_directory.join(&reference.file), &bytes).unwrap();
    let grid = TerrainGrid {
        tile_size: 32,
        columns: 36,
        rows: 32,
        tiles: vec![0; 36 * 32],
    };
    let encoded = ron::ser::to_string(&reference).unwrap();
    let mut assets: AssetManifest = ron::from_str(&format!("(schema_version:1,terrain:{encoded},unit_type:1,unit_name:\"Test\",frame_ms:100,anchor:(0,0),frames:[{encoded}])")).unwrap();
    assets.terrain_grid = Some(grid.clone());
    write(&map_directory, "assets.ron", &assets);
    let mut setup: MapSetup = read_ron(&map_directory.join("match.ron")).unwrap();
    setup.terrain_grid = Some(grid);
    write(&map_directory, "match.ron", &setup);
    catalog.tilesets = vec!["Red".into(), "Blue".into()];
    catalog.terrain_mappings = vec![
        TileMapping {
            tiles: vec![0],
            color_sources: Vec::new(),
        },
        TileMapping {
            tiles: vec![1],
            color_sources: Vec::new(),
        },
    ];
    for faction in &mut catalog.factions {
        faction.tileset_presentations = vec!["map".into(), "map".into()];
    }
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join("game");
    asset_archive::copy_game(source.path(), &root, &|_| {}).unwrap();
    let original_files: Vec<_> = std::fs::read_dir(root.join("map"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    let mut settings = catalog.defaults(0);
    settings.slots[2..]
        .iter_mut()
        .for_each(|s| s.kind = SlotKind::Closed);
    settings.random_locations = false;
    let mut worlds = Vec::new();
    for era in 0..2 {
        settings.tileset = Some(era);
        let prepared = catalog.prepare(&root, 0, &settings, &[0, 1], 42).unwrap();
        let transfer = prepared.map_transfer().unwrap();
        let artwork = transfer.artwork.as_ref().unwrap();
        assert!(
            artwork
                .grid
                .as_ref()
                .unwrap()
                .tiles
                .iter()
                .all(|t| *t == era as u32)
        );
        let decoded = transfer.definitions(&prepared.world, PlayerId(0)).unwrap();
        let preview = artwork.thumbnail(decoded.map()).unwrap();
        assert_eq!(
            &preview.rgba[..4],
            if era == 0 {
                &[255, 0, 0, 255]
            } else {
                &[0, 0, 255, 255]
            }
        );
        worlds.push(prepared.world.state_hash());
    }
    assert_eq!(worlds[0], worlds[1]);
    let files: Vec<_> = std::fs::read_dir(root.join("map"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(original_files, files);
    assert!(!root.join("map/tiles.srim").exists());
    let manifest = AssetManifest::load_manifest(&root.join("map")).unwrap();
    assert!(manifest.terrain_grid.unwrap().tiles.iter().all(|t| *t == 0));
    catalog.terrain_mappings[0].tiles.clear();
    settings.tileset = Some(0);
    assert!(catalog.prepare(&root, 0, &settings, &[0, 1], 42).is_err());
}
