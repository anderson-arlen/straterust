use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "straterust-native-assets-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn manifest(&self) -> AssetManifest {
        let bytes = encode_image(&image()).unwrap();
        fs::write(self.0.join("sample.srim"), &bytes).unwrap();
        let reference = ImageRef {
            file: "sample.srim".into(),
            blake3: blake3::hash(&bytes).to_hex().to_string(),
        };
        AssetManifest {
            console_layout: None,
            player_colors: Default::default(),
            schema_version: 1,
            terrain: reference.clone(),
            terrain_grid: None,
            unit_type: UnitTypeId(1),
            unit_name: "Synthetic sample".into(),
            frame_ms: 100,
            anchor: [1, 0],
            frames: vec![reference],
            clips: vec![],
            extra_units: vec![],
            resources: vec![],
            carried_resources: Vec::new(),
            ui: vec![],
            map_images: vec![],
            scan_effect: None,
            projectiles: Vec::new(),
            damage_effects: None,
            gas_effects: None,
            creep: None,
            indicators: None,
        }
    }

    fn write_manifest(&self, manifest: &AssetManifest) {
        fs::write(
            self.0.join("assets.ron"),
            ron::ser::to_string(manifest).unwrap(),
        )
        .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn image() -> Image {
    Image {
        width: 2,
        height: 1,
        rgba: vec![255, 0, 0, 255, 0, 255, 0, 0],
    }
}

#[test]
fn projectile_trails_load_optional_art_and_bound_emissions() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest();
    let effect = EffectManifest {
        frame_ms: 42,
        anchor: [1, 0],
        frames: manifest.frames.clone(),
        sequence: vec![0],
    };
    manifest.projectiles.push(ProjectileManifest {
        ability: None,
        unit_type: UnitTypeId(1),
        targets_air: true,
        directional: false,
        speed_fp8: 2560,
        forward_offset: 10,
        launch_offsets: Vec::new(),
        arc_height: 0,
        on_target: false,
        charge: None,
        marker: None,
        flight: effect.clone(),
        impact: effect.clone(),
        trail: None,
    });
    let old = ron::ser::to_string(&manifest).unwrap();
    assert!(!old.contains("trail"));
    assert!(
        ron::from_str::<AssetManifest>(&old).unwrap().projectiles[0]
            .trail
            .is_none()
    );
    manifest.projectiles[0].trail = Some(ProjectileTrailManifest {
        rear_offset: 0,
        directional: false,
        start_ms: 84,
        interval_ms: 42,
        effect,
    });
    fixture.write_manifest(&manifest);
    let loaded = AssetPack::load(&fixture.0).unwrap().unwrap();
    assert_eq!(
        loaded.projectiles[0].trail.as_ref().unwrap().frames[0].rgba,
        image().rgba
    );
    let trail = manifest.projectiles[0].trail.as_mut().unwrap();
    trail.interval_ms = 0;
    assert!(manifest.validate().is_err());
    let trail = manifest.projectiles[0].trail.as_mut().unwrap();
    trail.interval_ms = 42;
    trail.effect.sequence = vec![0; 65];
    assert!(
        manifest.validate().is_err(),
        "untrusted assets cannot create unbounded live trails"
    );
}

#[test]
fn carried_resources_load_arbitrary_kinds_and_partial_loads() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest();
    // Empty optional mappings remain absent in old/native fixture packages.
    let old = ron::ser::to_string(&manifest).unwrap();
    assert!(!old.contains("carried_resources"));
    assert!(
        ron::from_str::<AssetManifest>(&old)
            .unwrap()
            .carried_resources
            .is_empty()
    );
    let full = SpriteManifest {
        unit_type: UnitTypeId(1),
        unit_name: "Worker with wood".into(),
        frame_ms: 100,
        anchor: [1, 0],
        frames: manifest.frames.clone(),
        clips: vec![],
    };
    let mut partial = full.clone();
    partial.unit_name = "Worker with partial wood".into();
    manifest.carried_resources.push(CarriedResourceManifest {
        replaces_body: false,
        kind: "wood".into(),
        full_amount: 100,
        full,
        partial: Some(partial),
    });
    fixture.write_manifest(&manifest);
    let loaded = AssetPack::load(&fixture.0).unwrap().unwrap();
    let cargo = &loaded.carried_resources[0];
    assert_eq!(cargo.sprite(100).name, "Worker with wood");
    assert_eq!(cargo.sprite(20).name, "Worker with partial wood");
    assert_eq!(cargo.sprite(100).frames[0], image());
    manifest.carried_resources[0].partial = None;
    fixture.write_manifest(&manifest);
    let loaded = AssetPack::load(&fixture.0).unwrap().unwrap();
    assert_eq!(
        loaded.carried_resources[0].sprite(20).name,
        "Worker with wood"
    );
    manifest
        .carried_resources
        .push(manifest.carried_resources[0].clone());
    assert!(
        manifest.validate().is_err(),
        "duplicate carrier/kind must fail"
    );
    manifest.carried_resources.pop();
    let mut partial = manifest.carried_resources[0].full.clone();
    partial.unit_type = UnitTypeId(2);
    manifest.carried_resources[0].partial = Some(partial);
    assert!(
        manifest.validate().is_err(),
        "variants must use the same carrier"
    );
}

#[test]
fn finite_scan_effect_validates_timeline_and_every_canvas_anchor() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest();
    manifest.scan_effect = Some(EffectManifest {
        frame_ms: 42,
        anchor: [1, 0],
        frames: manifest.frames.clone(),
        sequence: vec![0, 0, 0],
    });
    fixture.write_manifest(&manifest);
    let effect = AssetPack::load(&fixture.0)
        .unwrap()
        .unwrap()
        .scan_effect
        .unwrap();
    assert_eq!(effect.sequence, [0, 0, 0]);
    assert_eq!(effect.frames.len(), 1);
    manifest.scan_effect.as_mut().unwrap().sequence.push(1);
    assert!(manifest.validate().is_err());
    manifest.scan_effect.as_mut().unwrap().sequence = vec![0; MAX_FRAMES + 1];
    assert!(manifest.validate().is_err());
    manifest.scan_effect.as_mut().unwrap().sequence = vec![0];
    manifest.scan_effect.as_mut().unwrap().anchor = [2, 0];
    fixture.write_manifest(&manifest);
    assert!(AssetPack::load(&fixture.0).is_err());
}

#[test]
fn image_round_trip_and_format_bounds() {
    let bytes = encode_image(&image()).unwrap();
    assert_eq!(decode_image(&bytes).unwrap(), image());
    for end in 0..bytes.len() {
        assert!(decode_image(&bytes[..end]).is_err());
    }
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(decode_image(&extra).is_err());
    for (offset, word) in [(0, 0), (4, 2), (8, 0), (12, u32::MAX)] {
        let mut bad = bytes.clone();
        bad[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        assert!(decode_image(&bad).is_err());
    }
    let mut bad = image();
    bad.rgba.pop();
    assert!(encode_image(&bad).is_err());
}

#[test]
fn absent_is_optional_but_bad_supplied_assets_fail() {
    let fixture = Fixture::new();
    assert!(AssetPack::load(&fixture.0).unwrap().is_none());
    let mut manifest = fixture.manifest();
    fixture.write_manifest(&manifest);
    let loaded = AssetPack::load(&fixture.0).unwrap().unwrap();
    assert_eq!(loaded.terrain, image());
    assert_eq!(loaded.frames, vec![image()]);
    manifest.anchor = [2, 0];
    fixture.write_manifest(&manifest);
    assert!(AssetPack::load(&fixture.0).is_err());
    manifest.anchor = [1, 0];
    manifest.frames[0].blake3 = "0".repeat(64);
    fixture.write_manifest(&manifest);
    assert!(AssetPack::load(&fixture.0).is_err());
    fs::write(fixture.0.join("assets.ron"), "broken").unwrap();
    assert!(AssetPack::load(&fixture.0).is_err());
}

#[test]
fn oversized_manifest_is_rejected_before_parsing() {
    let fixture = Fixture::new();
    let file = fs::File::create(fixture.0.join("assets.ron")).unwrap();
    file.set_len(MAX_ASSET_MANIFEST_BYTES as u64 + 1).unwrap();
    assert!(
        AssetPack::load(&fixture.0)
            .unwrap_err()
            .to_string()
            .contains(&format!("{MAX_ASSET_MANIFEST_BYTES} byte limit"))
    );
}

#[test]
fn optional_ui_images_load_by_key_and_validate_references() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest();
    manifest.ui.push(UiImageManifest {
        key: "command.repair".into(),
        image: manifest.terrain.clone(),
    });
    fixture.write_manifest(&manifest);
    let loaded = AssetPack::load(&fixture.0).unwrap().unwrap();
    assert_eq!(loaded.ui_image("command.repair"), Some(&image()));
    assert!(loaded.ui_image("command.unknown").is_none());

    manifest.ui[0].image.blake3 = "0".repeat(64);
    fixture.write_manifest(&manifest);
    assert!(AssetPack::load(&fixture.0).is_err());
    manifest.ui[0].image = manifest.terrain.clone();
    manifest.ui[0].image.file = "../outside.srim".into();
    assert!(manifest.validate().is_err());
}

#[test]
fn ui_image_keys_are_unique_and_bounded() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest();
    let entry = UiImageManifest {
        key: "selection.1".into(),
        image: manifest.terrain.clone(),
    };
    manifest.ui = vec![entry.clone(), entry.clone()];
    assert!(manifest.validate().is_err());
    manifest.ui = vec![entry];
    for key in [
        String::new(),
        "a".repeat(65),
        "bad/key".into(),
        "bad key".into(),
    ] {
        manifest.ui[0].key = key;
        assert!(manifest.validate().is_err());
    }
    manifest.ui = (0..1025)
        .map(|n| UiImageManifest {
            key: format!("selection.{n}"),
            image: manifest.terrain.clone(),
        })
        .collect();
    assert!(manifest.validate().is_err());
    manifest.ui.pop();
    assert!(manifest.validate().is_ok());
}

#[test]
fn rejects_paths_and_unbounded_animation_metadata() {
    let fixture = Fixture::new();
    let manifest = fixture.manifest();
    for name in ["", ".", "..", "../outside", "/absolute", "dir/a", "dir\\a"] {
        let mut bad = manifest.clone();
        bad.terrain.file = name.into();
        assert!(bad.validate().is_err(), "accepted {name:?}");
    }
    let mut bad = manifest.clone();
    bad.frames.clear();
    assert!(bad.validate().is_err());
    bad.frames = vec![manifest.terrain.clone(); MAX_FRAMES + 1];
    assert!(bad.validate().is_err());
    bad = manifest;
    bad.frame_ms = 0;
    assert!(bad.validate().is_err());
}

#[test]
fn directional_clips_load_and_reject_partial_or_invalid_image_references() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest();
    let idle = SpriteClip {
        key_steps: Vec::new(),
        kind: ClipKind::Idle,
        directions: 32,
        frame_ms: 100,
        frames: (0..32)
            .map(|direction| ClipFrame {
                frame: 0,
                flip_x: direction > 16,
                offset: [0, 0],
            })
            .collect(),
        loop_start: None,
        progress_starts: vec![],
    };
    manifest.clips.push(idle.clone());
    fixture.write_manifest(&manifest);
    let loaded = AssetPack::load(&fixture.0).unwrap().unwrap();
    let sprite = loaded.sprite(UnitTypeId(1)).unwrap();
    let clip = sprite.clip(ClipKind::Idle).unwrap();
    assert!(!clip.frames[8].flip_x);
    assert!(clip.frames[24].flip_x);
    assert!(sprite.clip(ClipKind::Walk).is_none());
    let older_frame: ClipFrame = ron::de::from_str("(frame:0,flip_x:false)").unwrap();
    assert_eq!(older_frame.offset, [0, 0]);
    let older_clip: SpriteClip =
        ron::de::from_str("(kind:Attack,directions:1,frame_ms:42,frames:[(frame:0,flip_x:false)])")
            .unwrap();
    assert!(older_clip.key_steps.is_empty());
    let attack = SpriteClip {
        kind: ClipKind::Attack,
        frames: vec![idle.frames[0]; 7 * 32],
        key_steps: vec![1, 3, 5],
        ..idle.clone()
    };
    manifest.clips = vec![attack.clone()];
    assert!(manifest.validate().is_ok());
    for key_steps in [vec![7], vec![1, 1], vec![3, 1], vec![u16::MAX]] {
        manifest.clips = vec![SpriteClip {
            key_steps,
            ..attack.clone()
        }];
        assert!(manifest.validate().is_err());
    }
    for invalid in [
        SpriteClip {
            key_steps: vec![0],
            ..idle.clone()
        },
        SpriteClip {
            directions: 0,
            ..idle.clone()
        },
        SpriteClip {
            directions: 17,
            ..idle.clone()
        },
        SpriteClip {
            frame_ms: 0,
            ..idle.clone()
        },
        SpriteClip {
            frames: vec![],
            ..idle.clone()
        },
        SpriteClip {
            frames: vec![idle.frames[0]; 31],
            ..idle.clone()
        },
        SpriteClip {
            frames: vec![idle.frames[0]; 32 * (MAX_FRAMES + 1)],
            ..idle.clone()
        },
        SpriteClip {
            frames: vec![
                ClipFrame {
                    frame: 1,
                    flip_x: false,
                    offset: [0, 0],
                };
                32
            ],
            ..idle.clone()
        },
        SpriteClip {
            frames: vec![
                ClipFrame {
                    frame: 0,
                    flip_x: false,
                    offset: [i16::MIN, 0]
                };
                32
            ],
            ..idle.clone()
        },
    ] {
        manifest.clips = vec![invalid];
        assert!(manifest.validate().is_err());
    }
    manifest.clips = vec![idle.clone(), idle];
    assert!(manifest.validate().is_err());
}

#[test]
fn terrain_atlas_grid_validates_layout_and_preserves_older_packs() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest();
    let older = ron::ser::to_string(&manifest)
        .unwrap()
        .replace("terrain_grid:None,", "");
    fs::write(fixture.0.join("assets.ron"), older).unwrap();
    assert!(
        AssetPack::load(&fixture.0)
            .unwrap()
            .unwrap()
            .manifest
            .terrain_grid
            .is_none()
    );
    manifest.terrain_grid = Some(TerrainGrid {
        tile_size: 1,
        columns: 256,
        rows: 256,
        tiles: vec![1; 256 * 256],
    });
    fixture.write_manifest(&manifest);
    let loaded = AssetPack::load(&fixture.0).unwrap().unwrap();
    assert_eq!(loaded.manifest.terrain_grid.unwrap().tiles.len(), 65_536);
    let grid = manifest.terrain_grid.as_mut().unwrap();
    grid.tiles[0] = 2;
    fixture.write_manifest(&manifest);
    assert!(
        AssetPack::load(&fixture.0)
            .unwrap_err()
            .to_string()
            .contains("outside its atlas")
    );
    let grid = manifest.terrain_grid.as_mut().unwrap();
    grid.tiles[0] = 1;
    grid.tile_size = 2;
    fixture.write_manifest(&manifest);
    assert!(
        AssetPack::load(&fixture.0)
            .unwrap_err()
            .to_string()
            .contains("multiples of tile_size")
    );
    let mut grid = manifest.terrain_grid.unwrap();
    grid.tile_size = 0;
    assert!(grid.validate().is_err());
    grid.tile_size = 1;
    grid.columns = 257;
    assert!(grid.validate().is_err());
    grid.columns = 256;
    grid.tiles.pop();
    assert!(grid.validate().is_err());
}

#[test]
fn cross_file_validation_rejects_unknown_units_and_wrong_map_coverage() {
    let fixture = Fixture::new();
    fixture.write_manifest(&fixture.manifest());
    let mut assets = AssetPack::load(&fixture.0).unwrap().unwrap();
    let package_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures");
    let world = crate::content::Package::load(&package_path)
        .unwrap()
        .world(42)
        .unwrap();
    assets.validate_for_world(&world).unwrap();
    assets.manifest.unit_type = UnitTypeId(u16::MAX);
    assert!(assets.validate_for_world(&world).is_err());
    assets.manifest.unit_type = UnitTypeId(1);
    assets.manifest.terrain_grid = Some(TerrainGrid {
        tile_size: 1,
        columns: 2,
        rows: 1,
        tiles: vec![0, 1],
    });
    assert!(assets.validate_for_world(&world).is_err());
}

#[test]
fn additional_unit_and_resource_art_load_with_unique_validated_mappings() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest();
    manifest.extra_units.push(SpriteManifest {
        unit_type: UnitTypeId(2),
        unit_name: "Synthetic worker".into(),
        frame_ms: 100,
        anchor: [1, 0],
        frames: manifest.frames.clone(),
        clips: vec![],
    });
    manifest.resources.push(ResourceManifest {
        terrain: false,
        terrain_edges: None,
        depleted_image: None,
        active_image: None,
        positions: Vec::new(),
        selection_circle: None,
        selection_y: 0,
        kind: "minerals".into(),
        anchor: [1, 0],
        image: manifest.terrain.clone(),
    });
    fixture.write_manifest(&manifest);
    let loaded = AssetPack::load(&fixture.0).unwrap().unwrap();
    assert_eq!(
        loaded.sprite(UnitTypeId(2)).unwrap().name,
        "Synthetic worker"
    );
    assert_eq!(loaded.sprite(UnitTypeId(1)).unwrap().frames, [image()]);
    assert!(loaded.sprite(UnitTypeId(3)).is_none());
    assert_eq!(loaded.resources[0].image, image());
    manifest.extra_units[0].unit_type = UnitTypeId(1);
    assert!(manifest.validate().is_err());
    manifest.extra_units[0].unit_type = UnitTypeId(2);
    manifest.resources.push(manifest.resources[0].clone());
    assert!(manifest.validate().is_err());
    manifest.resources.pop();
    manifest.extra_units[0].anchor = [2, 0];
    fixture.write_manifest(&manifest);
    assert!(AssetPack::load(&fixture.0).is_err());
    manifest.extra_units[0].anchor = [1, 0];
    manifest.resources[0].image.blake3 = "0".repeat(64);
    fixture.write_manifest(&manifest);
    assert!(AssetPack::load(&fixture.0).is_err());
}

#[test]
fn rejects_out_of_canvas_anchor_and_invalid_encoded_pixels() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest();
    let bytes = encode_image(&Image {
        width: 1,
        height: 1,
        rgba: vec![255; 4],
    })
    .unwrap();
    fs::write(fixture.0.join("second.srim"), &bytes).unwrap();
    manifest.frames.push(ImageRef {
        file: "second.srim".into(),
        blake3: blake3::hash(&bytes).to_hex().to_string(),
    });
    fixture.write_manifest(&manifest);
    let error = AssetPack::load(&fixture.0).unwrap_err().to_string();
    assert!(error.contains("anchor is outside"), "{error}");
    fs::write(fixture.0.join("second.srim"), b"invalid image").unwrap();
    manifest.frames[1].blake3 = blake3::hash(b"invalid image").to_hex().to_string();
    fixture.write_manifest(&manifest);
    let error = AssetPack::load(&fixture.0).unwrap_err();
    assert!(format!("{error:#}").contains("truncated SRIM header"));
}

#[test]
fn repeated_images_cannot_exceed_the_resident_memory_limit() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest();
    let reference = {
        let bytes = encode_image(&Image {
            width: MAX_IMAGE_DIMENSION,
            height: MAX_IMAGE_DIMENSION,
            rgba: vec![0; MAX_IMAGE_BYTES],
        })
        .unwrap();
        fs::write(fixture.0.join("large.srim"), &bytes).unwrap();
        ImageRef {
            file: "large.srim".into(),
            blake3: blake3::hash(&bytes).to_hex().to_string(),
        }
    };
    manifest.terrain = reference.clone();
    manifest.frames = vec![reference; MAX_PACK_RGBA_BYTES / MAX_IMAGE_BYTES];
    fixture.write_manifest(&manifest);
    let error = AssetPack::load(&fixture.0).unwrap_err().to_string();
    assert!(error.contains("512 MiB RGBA limit"), "{error}");
}

#[cfg(unix)]
#[test]
fn rejects_symlinks_outside_the_package_including_manifest() {
    let fixture = Fixture::new();
    let outside = Fixture::new();
    let mut manifest = outside.manifest();
    outside.write_manifest(&manifest);
    std::os::unix::fs::symlink(outside.0.join("assets.ron"), fixture.0.join("assets.ron")).unwrap();
    assert!(AssetPack::load(&fixture.0).is_err());
    fs::remove_file(fixture.0.join("assets.ron")).unwrap();
    std::os::unix::fs::symlink(
        outside.0.join("sample.srim"),
        fixture.0.join("escaped.srim"),
    )
    .unwrap();
    manifest.terrain.file = "escaped.srim".into();
    fixture.write_manifest(&manifest);
    assert!(AssetPack::load(&fixture.0).is_err());
}
#[test]
fn world_decorations_load_and_validate_position_anchor_and_hash() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest();
    manifest.map_images.push(MapImageManifest {
        position: Position { x: 5, y: 6 },
        anchor: [1, 0],
        image: manifest.terrain.clone(),
    });
    fixture.write_manifest(&manifest);
    let loaded = AssetPack::load(&fixture.0).unwrap().unwrap();
    assert_eq!(loaded.map_images[0].position, Position { x: 5, y: 6 });
    assert_eq!(loaded.map_images[0].image, image());
    manifest.map_images[0].anchor = [2, 0];
    fixture.write_manifest(&manifest);
    assert!(AssetPack::load(&fixture.0).is_err());
    manifest.map_images[0].anchor = [1, 0];
    manifest.map_images[0].image.blake3 = "0".repeat(64);
    fixture.write_manifest(&manifest);
    assert!(AssetPack::load(&fixture.0).is_err());
    manifest.map_images[0].image = manifest.terrain.clone();
    manifest.map_images[0].position.x = -1;
    assert!(manifest.validate().is_err());
    manifest.map_images[0].position.x = 0;
    manifest.map_images = vec![manifest.map_images[0].clone(); 4097];
    assert!(manifest.validate().is_err());
}

#[test]
fn individually_cropped_frames_validate_every_anchor() {
    let frames = [
        Image {
            width: 3,
            height: 5,
            rgba: vec![0; 60],
        },
        Image {
            width: 1,
            height: 2,
            rgba: vec![0; 8],
        },
    ];
    assert!(validate_frames(&frames, [0, 0]).is_ok());
    assert!(validate_frames(&frames, [1, 0]).is_err());
    assert!(validate_frames(&frames, [0, 2]).is_err());
    assert!(validate_frames(&frames, [-1, 0]).is_err());
    assert!(validate_frames(&[], [0, 0]).is_err());
}
