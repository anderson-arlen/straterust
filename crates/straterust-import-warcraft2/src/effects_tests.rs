//! Compare converted effects with source frames, keeping retail art private.
use super::*;
use straterust_engine::assets::*;

#[test]
fn directional_animation_steps_advance_poses_without_rotating_the_heading() {
    let frame = |i| ImageRef {
        file: format!("{i}.srim"),
        blake3: format!("{i:064x}"),
    };
    let mut effect = EffectManifest {
        frame_ms: 66,
        anchor: [16, 16],
        frames: (0..15).map(frame).collect(),
        sequence: (0..15).collect(),
    };
    effects::directional(&mut effect);
    assert_eq!(effect.sequence, [0, 17, 34]);
    for pose in 0..3 {
        for heading in 0..17 {
            assert_eq!(
                effect.frames[usize::from(effect.sequence[pose]) + heading].file,
                format!("{}.srim", pose * 5 + (heading + 2) / 4)
            );
        }
    }
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 and STRATERUST_WARCRAFT2_SOURCE (retail installer or directory)"]
fn native_effect_records_and_upgrade_match_all_spell_and_flyer_headings() {
    let root = gameplay_tests::root();
    let source = source::Source::open(
        &std::path::PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2_SOURCE").unwrap()),
        &|_| {},
    )
    .unwrap();
    let cache = tempfile::tempdir().unwrap();
    let reviews = std::path::Path::new("/tmp/stratarust-warcraft-effects");
    std::fs::create_dir_all(reviews).unwrap();
    for era in 0..4 {
        let set = terrain::Tileset::load(&source.main, era).unwrap();
        let directory = root.join(format!("multiplayer/art-0-{era}"));
        let mut assets = AssetManifest::load_manifest(&directory).unwrap();
        effects::upgrade(&mut assets);
        let first = ron::ser::to_string(&assets).unwrap();
        effects::upgrade(&mut assets);
        assert_eq!(
            first,
            ron::ser::to_string(&assets).unwrap(),
            "upgrade must be idempotent"
        );
        let statuses = spell_art::status_images(&source.main, &set.palette).unwrap();
        assert_eq!(statuses.len(), 5);
        for (ability, index) in [(5, 0), (15, 1), (8, 2), (10, 3), (16, 4)] {
            let p = assets
                .projectiles
                .iter()
                .find(|p| p.ability == Some(straterust_engine::sim::AbilityId(ability)))
                .unwrap();
            let marker = p
                .marker
                .as_ref()
                .expect("new import retains native status symbols");
            assert_eq!(
                marker.frames[0].blake3,
                blake3::hash(&encode_image(&statuses[index]).unwrap())
                    .to_hex()
                    .to_string()
            );
        }
        let shield = assets
            .projectiles
            .iter()
            .find(|p| p.ability == Some(straterust_engine::sim::AbilityId(9)))
            .unwrap();
        assert!(matches!(
            shield.pattern,
            Some(EffectPattern::Orbit {
                count: 5,
                radius: 32,
                period_ms: 1188,
                phase_degrees: 210,
                spacing_degrees: -70
            })
        ));
        assert_eq!(shield.flight.anchor, [16, 32]);
        for (unit, record) in [(23, 325), (36, 326), (43, 325), (44, 326)] {
            let projectile = assets
                .projectiles
                .iter()
                .find(|p| p.unit_type.0 == unit && p.ability.is_none())
                .unwrap();
            let mut expected =
                effects::effect(&source.main, cache.path(), &set.palette, record).unwrap();
            effects::directional(&mut expected);
            assert_eq!(projectile.flight.sequence, expected.sequence);
            assert_eq!(
                projectile
                    .flight
                    .frames
                    .iter()
                    .map(|r| &r.blake3)
                    .collect::<Vec<_>>(),
                expected
                    .frames
                    .iter()
                    .map(|r| &r.blake3)
                    .collect::<Vec<_>>()
            );
            let blast = effects::effect(&source.main, cache.path(), &set.palette, 347).unwrap();
            assert_eq!(projectile.impact.frames[0].blake3, blast.frames[0].blake3);
        }
        for (id, record) in [
            (1, 346),
            (2, 333),
            (3, 332),
            (4, 346),
            (5, 346),
            (6, 335),
            (7, 327),
            (8, 346),
            (9, 328),
            (10, 346),
            (11, 346),
            (12, 329),
            (13, 334),
            (14, 346),
            (15, 346),
            (16, 346),
            (17, 336),
            (18, 330),
            (19, 347),
        ] {
            let projectile = assets
                .projectiles
                .iter()
                .find(|p| p.ability == Some(straterust_engine::sim::AbilityId(id)))
                .unwrap();
            let mut expected =
                effects::effect(&source.main, cache.path(), &set.palette, record).unwrap();
            if projectile.directional {
                effects::directional(&mut expected);
            }
            if id == 13 {
                expected.sequence = vec![0];
            }
            if id == 12 {
                expected.sequence = vec![0, 1];
            }
            let art = if matches!(id, 6 | 7 | 9 | 12 | 13 | 17 | 18) {
                &projectile.flight
            } else {
                &projectile.impact
            };
            assert_eq!(art.sequence, expected.sequence, "spell {id} era {era}");
            assert_eq!(
                art.frames.iter().map(|r| &r.blake3).collect::<Vec<_>>(),
                expected
                    .frames
                    .iter()
                    .map(|r| &r.blake3)
                    .collect::<Vec<_>>(),
                "spell {id} era {era}"
            );
        }
        for record in [
            325, 326, 327, 328, 329, 330, 332, 333, 334, 335, 336, 346, 347,
        ] {
            for (i, image) in gfx::sprites(&source.main.entry(record).unwrap(), &set.palette)
                .unwrap()
                .iter()
                .enumerate()
            {
                std::fs::write(
                    reviews.join(format!("{era}-{record}-{i}.srim")),
                    encode_image(image).unwrap(),
                )
                .unwrap();
            }
        }
        let edges = rocks::edges(era);
        for logical in [
            0x80, 0x400, 0x410, 0x420, 0x430, 0x440, 0x450, 0x460, 0x470, 0x480, 0x490, 0x4a0,
            0x4b0, 0x4c0, 0x4d0,
        ] {
            // Test source logical mapping through ordinary PUD import, not duplicated image assumptions.
            let mut chunks = std::collections::BTreeMap::new();
            for (key, value) in [(b"MTXM", logical), (b"REGM", 0xfffd), (b"SQM ", 0x81)] {
                chunks.insert(*key, (value as u16).to_le_bytes().to_vec());
            }
            let pud = pud::Pud {
                chunks,
                width: 1,
                height: 1,
                era,
                units: Vec::new(),
                owners: [3; 16],
                sides: [0; 16],
                local: 0,
                title: "Rock fixture".into(),
            };
            let (grid, _, _) = set.map(&pud).unwrap();
            assert!(
                edges.corners.contains_key(&grid.tiles[0]),
                "era {era} logical {logical:x} tile {}",
                grid.tiles[0]
            );
        }
    }
}

#[test]
fn healing_upgrade_prunes_targets_missing_from_limited_campaign_rosters() {
    use straterust_engine::sim::*;
    let mut rules = Rules {
        units: vec![
            UnitType {
                id: UnitTypeId(1),
                ..Default::default()
            },
            UnitType {
                id: UnitTypeId(13),
                abilities: vec![TargetedAbility {
                    id: AbilityId(2),
                    research: None,
                    range: 192,
                    energy: 0,
                    effect: AbilityEffect::Heal {
                        affected: vec![UnitTypeId(999)],
                        amount: 1,
                        bulk: false,
                    },
                }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    spells::upgrade(&mut rules);
    let AbilityEffect::Heal { affected, bulk, .. } = &rules.units[1].abilities[0].effect else {
        panic!("healing must remain healing")
    };
    assert!(*bulk);
    assert_eq!(affected, &[UnitTypeId(1), UnitTypeId(13)]);
}
