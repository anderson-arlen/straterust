use super::*;
use straterust_engine::assets::*;

fn palette_fixture() -> (tempfile::TempDir, Catalog) {
    let (root, mut catalog) = fixture();
    catalog.colors = (0..8)
        .map(|i| PlayerColor {
            name: format!("Color {}", i + 1),
            shades: vec![[i * 30, 30, 60], [i * 20, 20, 40]],
        })
        .collect();
    let directory = root.path().join("map");
    let bytes = encode_image(&Image {
        width: 2,
        height: 1,
        rgba: vec![100, 0, 0, 255, 80, 0, 0, 0],
    })
    .unwrap();
    let image = ImageRef {
        file: "image.srim".into(),
        blake3: blake3::hash(&bytes).to_hex().to_string(),
    };
    std::fs::write(directory.join(&image.file), bytes).unwrap();
    let encoded = ron::ser::to_string(&image).unwrap();
    let mut assets: AssetManifest = ron::from_str(&format!("(schema_version:1,terrain:{encoded},unit_type:1,unit_name:\"Test\",frame_ms:100,anchor:(0,0),frames:[{encoded}])")).unwrap();
    assets.player_colors = catalog
        .colors
        .iter()
        .enumerate()
        .map(|(i, color)| {
            (
                PlayerId(i as u16),
                ColorRemap {
                    colors: vec![
                        [[100, 0, 0], color.shades[0]],
                        [[80, 0, 0], color.shades[1]],
                    ],
                },
            )
        })
        .collect();
    write(&directory, "assets.ron", &assets);
    (root, catalog)
}

#[test]
fn color_choices_change_transferred_markings_without_changing_simulation_or_source_files() {
    let (root, catalog) = palette_fixture();
    let mut settings = catalog.defaults(0);
    for slot in &mut settings.slots[2..] {
        slot.kind = SlotKind::Closed;
    }
    let original = catalog
        .prepare(root.path(), 0, &settings, &[0, 1], 42)
        .unwrap();
    let before = std::fs::read(root.path().join("map/assets.ron")).unwrap();
    catalog.choose_color(&mut settings, 0, 7).unwrap();
    catalog.choose_color(&mut settings, 1, 0).unwrap();
    let chosen = catalog
        .prepare(root.path(), 0, &settings, &[0, 1], 42)
        .unwrap();
    assert_eq!(original.world.state_hash(), chosen.world.state_hash());
    let transfer = chosen.map_transfer().unwrap();
    let art = transfer.artwork.unwrap();
    for (player, color) in [(0, 7), (1, 0)] {
        let remap = &art.player_colors[&PlayerId(player)];
        assert_eq!(remap.apply([100, 0, 0]), catalog.colors[color].shades[0]);
        assert_eq!(remap.apply([80, 0, 0]), catalog.colors[color].shades[1]);
        assert_eq!(remap.apply([0, 10, 20]), [0, 10, 20]);
    }
    assert_eq!(
        std::fs::read(root.path().join("map/assets.ron")).unwrap(),
        before
    );
    settings.slots[1].color = Some(7);
    assert!(
        catalog
            .validate_settings(0, &settings)
            .unwrap_err()
            .to_string()
            .contains("already in use")
    );
}

#[test]
fn old_catalogs_and_settings_recover_palettes_and_defaults_without_reimporting() {
    let (root, catalog) = palette_fixture();
    let mut legacy = catalog.clone();
    legacy.colors.clear();
    legacy.load_legacy_colors(root.path()).unwrap();
    assert_eq!(legacy.colors.len(), 8);
    for (old, new) in catalog.colors.iter().zip(&legacy.colors) {
        assert_eq!(old.shades, new.shades);
    }
    let mut settings = catalog.defaults(0);
    for slot in &mut settings.slots {
        slot.color = None;
    }
    let encoded = ron::ser::to_string(&settings).unwrap();
    assert!(
        !encoded.contains("color:"),
        "old settings have no color field"
    );
    let mut settings: Settings = ron::from_str(&encoded).unwrap();
    legacy.assign_colors(&mut settings).unwrap();
    assert_eq!(
        settings.slots.iter().map(|s| s.color).collect::<Vec<_>>(),
        (0..8).map(Some).collect::<Vec<_>>()
    );
    assert!(
        legacy.choose_color(&mut settings, 0, 1).is_err(),
        "automatic assignments reserve colors too"
    );
    settings.slots[1].kind = SlotKind::Closed;
    legacy.choose_color(&mut settings, 0, 1).unwrap();
    legacy.assign_colors(&mut settings).unwrap();
    assert_eq!(settings.slots[1].color, None);
    legacy.validate_settings(0, &settings).unwrap();
}
