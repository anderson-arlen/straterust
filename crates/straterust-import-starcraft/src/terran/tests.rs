use super::*;
use crate::terran_data::ReferenceWeapon;

fn reference() -> Vec<ReferenceUnit> {
    UNIT_MAPPING
        .iter()
        .map(|&(source_id, _)| ReferenceUnit {
            source_id,
            minerals: 75,
            gas: 0,
            hitpoints: 90,
            armor: 2,
            unit_size: 1,
            build_frames: 400,
            supply_required_half_units: 2,
            supply_provided_half_units: 4,
            collision_extents: [10, 4, 12, 8],
            placement_size: [32, 64],
            weapon: None,
        })
        .collect()
}

fn template() -> Rules {
    ron::de::from_str(include_str!("../../../../content/terran-demo/rules.ron")).unwrap()
}

#[test]
fn fire_intensity_keeps_dark_edges_translucent_and_preserves_emission() {
    for source in [
        [0, 0, 0, 255],
        [8, 3, 1, 255],
        [80, 32, 4, 255],
        [255, 240, 80, 255],
    ] {
        let rgba = fire_color(source);
        for channel in 0..3 {
            let over_black = (u32::from(rgba[channel]) * u32::from(rgba[3]) + 127) / 255;
            assert!(over_black.abs_diff(u32::from(source[channel])) <= 1);
        }
        if source[0] < 255 {
            assert!(rgba[3] < 255);
            let terrain = [70, 100, 50];
            for channel in 0..3 {
                let blended = (u32::from(rgba[channel]) * u32::from(rgba[3])
                    + terrain[channel] * (255 - u32::from(rgba[3]))
                    + 127)
                    / 255;
                assert!(
                    blended > u32::from(source[channel]),
                    "dark fire must retain terrain instead of a black mat"
                );
            }
        }
    }
    assert_eq!(fire_color([0, 0, 0, 255]), [0; 4]);
}

#[test]
fn translucent_flames_composite_over_bodies_and_keep_alpha_outside_them() {
    let body = Image {
        width: 3,
        height: 1,
        rgba: vec![20, 60, 100, 255, 0, 0, 0, 0, 40, 80, 120, 128],
    };
    let flame = Image {
        width: 3,
        height: 1,
        rgba: [255, 60, 0, 128].repeat(3),
    };
    let result = composite(&body, &flame).unwrap();
    assert_eq!(&result.rgba[..4], &[138, 60, 50, 255]);
    assert_eq!(&result.rgba[4..8], &[255, 60, 0, 128]);
    assert_eq!(&result.rgba[8..12], &[184, 67, 40, 192]);
}

#[test]
fn finite_deaths_retain_source_poses_with_bounded_decay() {
    let marine = marine_death();
    assert_eq!(marine.kind, ClipKind::Death);
    assert_eq!(marine.directions, 1);
    assert_eq!(marine.frames.len() as u32 * marine.frame_ms, 8_850);
    assert_eq!(marine.frames[0].frame, 221);
    assert_eq!(marine.frames[7].frame, 228);
    for (index, frame) in [(8, 229), (25, 230), (42, 231)] {
        assert!(
            marine.frames[index..index + 17]
                .iter()
                .all(|pose| pose.frame == frame)
        );
    }
    let building = building_death(11);
    assert_eq!(building.frames.len(), 54);
    assert_eq!(building.frames[13].frame, 24);
    assert_eq!(building.frames[14].frame, 25);
    assert_eq!(building.frames.last().unwrap().frame, 28);
    assert_eq!(building.frames.len() as u32 * building.frame_ms, 8_100);
}

#[test]
fn source_headings_share_mirrored_images_and_idle_holds_one_pose() {
    let idle = directional(ClipKind::Idle, &[68], 100);
    assert_eq!(idle.frames.len(), 32);
    assert_eq!(
        idle.frames[0],
        ClipFrame {
            frame: 68,
            flip_x: false,
            offset: [0, 0],
        }
    );
    assert_eq!(
        idle.frames[8],
        ClipFrame {
            frame: 76,
            flip_x: false,
            offset: [0, 0],
        }
    );
    assert_eq!(
        idle.frames[16],
        ClipFrame {
            frame: 84,
            flip_x: false,
            offset: [0, 0],
        }
    );
    assert_eq!(
        idle.frames[24],
        ClipFrame {
            frame: 76,
            flip_x: true,
            offset: [0, 0],
        }
    );
    assert_eq!(
        idle.frames[31],
        ClipFrame {
            frame: 69,
            flip_x: true,
            offset: [0, 0],
        }
    );
    let walk = directional(ClipKind::Walk, &[68, 85], 100);
    assert_eq!(walk.frames.len(), 64);
    assert_eq!(walk.frames[32 + 8].frame, 93);
    let construction = single_direction(ClipKind::Construction, &[1, 2, 3, 4], 100);
    assert_eq!(construction.directions, 1);
    assert_eq!(construction.frames[3].frame, 4);
}

#[test]
fn construction_canvases_preserve_center_anchor_and_transparent_padding() {
    let body = Image {
        width: 2,
        height: 2,
        rgba: [1, 2, 3, 255].repeat(4),
    };
    let padded = center_canvas(&body, 4, 4).unwrap();
    assert_eq!(&padded.rgba[(4 + 1) * 4..(4 + 3) * 4], &body.rgba[..8]);
    assert!(padded.rgba[..4 * 4].iter().all(|value| *value == 0));
    assert!(center_canvas(&body, 1, 4).is_err());
    assert!(center_canvas(&body, 2048, 4).is_err());
}

#[test]
fn normal_building_overlays_preserve_body_pixels_and_reject_remapped_art() {
    let body = Image {
        width: 2,
        height: 1,
        rgba: vec![1, 2, 3, 255, 4, 5, 6, 255],
    };
    let overlay = Image {
        width: 2,
        height: 1,
        rgba: vec![0, 0, 0, 0, 7, 8, 9, 255],
    };
    assert_eq!(
        composite(&body, &overlay).unwrap().rgba,
        [1, 2, 3, 255, 7, 8, 9, 255]
    );
    let mut definitions = vec![0; 28690];
    for (image, script) in [(276, 103_u32), (279, 106)] {
        let offset = 755 * 10 + image * 4;
        definitions[offset..offset + 4].copy_from_slice(&script.to_le_bytes());
    }
    verify_building_overlay_definitions(&definitions).unwrap();
    definitions[755 * 8 + 279] = 9;
    definitions[755 * 9 + 279] = 1;
    assert!(verify_building_overlay_definitions(&definitions).is_err());
    assert!(verify_building_overlay_definitions(&definitions[..100]).is_err());
    assert_eq!(wait_steps(&[(7, 4), (8, 2)]), [7, 7, 7, 7, 8, 8]);
    assert_eq!(wait_steps(&BARRACKS_WORK).len(), 18);
}

#[test]
fn source_direction_table_places_sparks_forward_without_mirroring_them() {
    // Mathematical fixture, not extracted executable bytes. The original table is
    // exactly the rounded 256-angle unit circle in eight fractional bits.
    let mut executable = vec![0; 0xd9b28];
    for heading in 0..256 {
        let angle = heading as f64 * std::f64::consts::PI / 128.0;
        for value in [
            (angle.sin() * 256.0).round() as i32,
            (-angle.cos() * 256.0).round() as i32,
        ] {
            executable.extend(value.to_le_bytes());
        }
    }
    let offsets = source_work_offsets(&executable).unwrap();
    for (heading, expected) in [
        (0, [0, -20]),
        (4, [14, -15]),
        (8, [20, 0]),
        (16, [0, 20]),
        (20, [-15, 14]),
        (24, [-20, 0]),
    ] {
        assert_eq!(offsets[heading], expected);
    }
    let clip = work_effect_clip(&offsets);
    assert_eq!(clip.frames.len(), 320);
    assert_eq!(clip.frames[8].offset, [20, 0]);
    assert_eq!(clip.frames[24].offset, [-20, 0]);
    assert_eq!(clip.frames[8].frame, clip.frames[24].frame);
    assert!(clip.frames.iter().all(|frame| !frame.flip_x));
    assert_eq!(clip.frames[9 * 32].frame, 60);
    assert!(source_work_offsets(&executable[..executable.len() - 1]).is_err());
    *executable.last_mut().unwrap() ^= 1;
    assert!(source_work_offsets(&executable).is_err());
}

#[test]
fn selected_legacy_script_pointers_are_bounded_and_instructions_are_verified() {
    let mut bytes = vec![0; 67];
    bytes[..4].copy_from_slice(&[42, 0, 8, 0]);
    bytes[4..6].copy_from_slice(&u16::MAX.to_le_bytes());
    bytes[8..16].copy_from_slice(b"SCPE\x0f\0\0\0");
    bytes[46..48].copy_from_slice(&64_u16.to_le_bytes());
    bytes[64..].copy_from_slice(&[0, 1, 0]);
    expect_animation(&bytes, 42, 15, &[0, 1, 0]).unwrap();
    assert!(expect_animation(&bytes, 42, 15, &[0, 2, 0]).is_err());
    assert!(script_animation(&bytes, 41, 15).is_err());
    assert!(script_animation(&bytes, 42, 16).is_err());
    for end in 0..bytes.len() {
        assert!(expect_animation(&bytes[..end], 42, 15, &[0, 1, 0]).is_err());
    }
    bytes[46..48].copy_from_slice(&65535_u16.to_le_bytes());
    assert!(script_animation(&bytes, 42, 15).is_err());
    bytes[46..48].copy_from_slice(&0_u16.to_le_bytes());
    assert!(script_animation(&bytes, 42, 15).is_err());
    bytes[46..48].copy_from_slice(&64_u16.to_le_bytes());
    bytes[8] = b'X';
    assert!(script_animation(&bytes, 42, 15).is_err());
    bytes[8] = b'S';
    bytes[16..18].copy_from_slice(&64_u16.to_le_bytes());
    for kind in [0, 1] {
        bytes[12] = kind;
        expect_animation(&bytes, 42, 0, &[0, 1, 0]).unwrap();
        assert!(script_animation(&bytes, 42, 2).is_err());
    }
}

#[test]
fn overrides_selected_fields_and_preserves_authored_behavior() {
    let mut rules = template();
    apply_reference(&mut rules, &reference()).unwrap();
    let marine = &rules.units[0];
    assert_eq!(
        (marine.max_hp, marine.armor, marine.build_ticks),
        (90, 2, 400)
    );
    assert_eq!(
        marine.footprint,
        Footprint {
            width: 23,
            height: 13
        }
    );
    assert_eq!(
        marine.placement,
        Footprint {
            width: 32,
            height: 64
        }
    );
    assert_eq!((marine.supply_used, marine.supply_provided), (1, 2));
    assert_eq!(marine.cost.len(), 1);
    assert_eq!(marine.cost[0].amount, 75);
    assert!(marine.weapon.is_none());
    assert_eq!(marine.speed, 4);
    assert_eq!(marine.prerequisites, vec![UnitTypeId(5)]);
    assert_eq!(rules.units[1].worker.as_ref().unwrap().harvest_ticks, 75);
    assert_eq!(rules.tick_ms, 50);
}

#[test]
fn rejects_supply_rounding_and_unhandled_minimum_range() {
    let mut units = reference();
    units[0].supply_required_half_units = 3;
    assert!(apply_reference(&mut template(), &units).is_err());
    units[0].supply_required_half_units = 2;
    units[0].weapon = Some(ReferenceWeapon {
        id: 0,
        damage_type: 3,
        behavior: 2,
        effect: 1,
        splash_radii: [0; 3],
        forward_offset: 0,
        target_flags: 3,
        damage: 7,
        cooldown_frames: 12,
        minimum_range: 1,
        maximum_range: 96,
    });
    assert!(apply_reference(&mut template(), &units).is_err());
    units[0].weapon.as_mut().unwrap().minimum_range = 0;
    let mut rules = template();
    apply_reference(&mut rules, &units).unwrap();
    let weapon = rules.units[0].weapon.as_ref().unwrap();
    assert_eq!((weapon.damage, weapon.range, weapon.cooldown), (7, 96, 12));
}
