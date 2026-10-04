use super::*;
use crate::terran::crop;

#[test]
fn scanner_pulses_keep_signed_offsets_and_end_before_detection() {
    let pulses: Vec<_> = (0..8)
        .map(|pose| Image {
            width: 48,
            height: 48,
            rgba: [pose + 1, 2, 3, 255].repeat(48 * 48),
        })
        .collect();
    let (frames, sequence) = scan_frames(&pulses).unwrap();
    assert_eq!(sequence.len(), 156);
    let sample = |tick: usize, x: usize, y: usize| {
        let image = &frames[usize::from(sequence[tick])];
        image.rgba[(y * 144 + x) * 4]
    };
    assert_eq!(sample(0, 48, 48), 1);
    assert_eq!(sample(0, 47, 48), 0);
    assert_eq!(sample(2, 48, 48), 2);
    assert_eq!(sample(22, 0, 46), 1);
    assert_eq!(sample(46, 51, 96), 8);
    assert!(
        frames[usize::from(sequence[47])]
            .rgba
            .iter()
            .all(|&v| v == 0)
    );
    assert!(sequence[47..].iter().all(|&frame| frame == sequence[47]));
    assert!(scan_frames(&pulses[..7]).is_err());
}
#[test]
#[ignore = "requires STRATERUST_SOURCE pointing to the authorized original disc and FFmpeg"]
fn original_mission_roles_convert_with_bounded_native_art_and_media() {
    let source = std::env::var_os("STRATERUST_SOURCE").expect("set STRATERUST_SOURCE");
    let source = Path::new(&source);
    let payload = crate::inspect(source).unwrap();
    let mut files = crate::terran::convert(&payload, source).unwrap();
    crate::zerg::convert(source, &mut files).unwrap();
    convert(source, &mut files).unwrap();
    let stage = tempfile::tempdir().unwrap();
    for (name, bytes) in &files {
        std::fs::write(stage.path().join(name), bytes).unwrap();
    }
    let assets = straterust_engine::assets::AssetPack::load(stage.path())
        .unwrap()
        .unwrap();
    let media = straterust_engine::media::MediaPack::load(stage.path())
        .unwrap()
        .unwrap();
    let rules: Rules = ron::de::from_bytes(&files["rules.ron"]).unwrap();
    assert_eq!(rules.units.len(), 17);
    assert_eq!(assets.extra_units.len(), 16);
    assert_eq!(assets.ui.len(), 66);
    assert!(
        media
            .portraits
            .iter()
            .any(|p| p.unit_type == UnitTypeId(1000) && p.portrait_only)
    );
    for unit in [6, 7] {
        let sprite = assets
            .extra_units
            .iter()
            .find(|s| s.manifest.unit_type == UnitTypeId(unit))
            .unwrap();
        assert_eq!(sprite.frames.len(), 301);
        assert!(
            sprite
                .manifest
                .clips
                .iter()
                .any(|c| c.kind == ClipKind::Reveal)
        );
    }
    let firebat = rules.units.iter().find(|u| u.id == UnitTypeId(11)).unwrap();
    assert_eq!(firebat.weapon.as_ref().unwrap().splash, Some([15, 20, 25]));
    assert_eq!(
        firebat
            .weapon
            .as_ref()
            .unwrap()
            .strikes
            .iter()
            .map(|s| s.delay)
            .collect::<Vec<_>>(),
        [0, 2, 3]
    );
}
#[test]
fn tight_crop_preserves_exact_world_pixels_in_both_orientations() {
    let mut source = Image {
        width: 12,
        height: 10,
        rgba: vec![0; 12 * 10 * 4],
    };
    for (x, y, color) in [(1, 2, [1, 2, 3, 255]), (4, 7, [4, 5, 6, 99])] {
        let i = (y * 12 + x) * 4;
        source.rgba[i..i + 4].copy_from_slice(&color);
    }
    let (cropped, offset) = crop(&source).unwrap();
    assert_eq!((cropped.width, cropped.height, offset), (4, 6, [-5, -3]));
    for flip in [false, true] {
        let source_pixels: Vec<_> = source
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, p)| p[3] != 0)
            .map(|(i, p)| {
                let x = i as i32 % 12;
                let y = i as i32 / 12;
                ([if flip { 5 - x } else { x - 6 }, y - 5], *p)
            })
            .collect();
        let cropped_pixels: Vec<_> = cropped
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, p)| p[3] != 0)
            .map(|(i, p)| {
                let x = i as i32 % 4;
                let y = i as i32 / 4;
                (
                    [
                        if flip {
                            3 - x - 4 - i32::from(offset[0])
                        } else {
                            x + i32::from(offset[0])
                        },
                        y + i32::from(offset[1]),
                    ],
                    *p,
                )
            })
            .collect();
        assert_eq!(source_pixels, cropped_pixels);
    }
}
#[test]
fn empty_crop_and_invalid_sources_are_bounded() {
    let image = Image {
        width: 2,
        height: 2,
        rgba: vec![0; 16],
    };
    let (cropped, offset) = crop(&image).unwrap();
    assert_eq!((cropped.width, cropped.height, offset), (1, 1, [0, 0]));
    let mut invalid = image;
    invalid.rgba.pop();
    assert!(crop(&invalid).is_err());
}
