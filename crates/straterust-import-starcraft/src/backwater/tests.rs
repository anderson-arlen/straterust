use super::*;
fn action() -> SourceAction {
    SourceAction {
        location: 1,
        text: 1,
        sound: 2,
        time: 1000,
        player: 1,
        second: 0,
        unit: 19,
        kind: 7,
        modifier: 9,
        flags: 0,
    }
}
#[test]
fn legacy_transmission_modifiers_preserve_original_executable_semantics() {
    let mut a = action();
    assert_eq!(duration(&a).unwrap(), 1000);
    a.second = 200;
    assert_eq!(duration(&a).unwrap(), 800);
    a.second = 2000;
    assert_eq!(duration(&a).unwrap(), 0);
    a.modifier = 7;
    assert_eq!(duration(&a).unwrap(), 2000);
    a.modifier = 8;
    assert_eq!(duration(&a).unwrap(), 3000);
    a.time = u32::MAX;
    assert!(duration(&a).is_err());
    a.modifier = 1;
    assert!(duration(&a).is_err());
}
#[test]
fn bounded_sections_strings_and_trigger_records_reject_malformed_inputs() {
    assert!(Sections::read(b"TRIG").is_err());
    assert!(Sections::read(b"TRIG\xff\xff\xff\xff").is_err());
    assert!(Sections::read(b"STR \0\0\0\0STR \0\0\0\0").is_err());
    assert!(read_strings(&[1, 0, 50, 0, 0]).is_err());
    assert!(read_strings(&[1, 0, 4, 0, b'a']).is_err());
    assert!(read_triggers(&[0; 2399], false).is_err());
    let mut data = vec![0; 2400];
    data[2373] = 1;
    data[15] = 1;
    data[320 + 26] = 1;
    let triggers = read_triggers(&data, false).unwrap();
    assert_eq!(triggers.len(), 1);
    data[2368] = 4;
    assert!(read_triggers(&data, false).is_err());
}
#[test]
fn source_force_filters_and_one_unit_creates_translate_without_authored_orders() {
    assert_eq!(players(19).unwrap(), [PlayerId(1)]);
    assert_eq!(
        players(17).unwrap(),
        [PlayerId(0), PlayerId(1), PlayerId(2), PlayerId(3)]
    );
    assert_eq!(unit_match(229).unwrap(), MissionUnits::Any);
    let refs = References {
        texts: vec!["original synthetic text".into()],
        text_ids: BTreeMap::from([(1, 0)]),
        sound_ids: BTreeMap::from([(2, 0)]),
    };
    let locations = BTreeMap::from([(
        1,
        MissionLocation {
            excluded_elevations: 0,
            left: 0,
            top: 0,
            right: 256,
            bottom: 256,
        },
    )]);
    let mut create = action();
    create.kind = 11;
    create.player = 3;
    create.unit = 32;
    create.modifier = 0;
    let native = translate_mission(
        &[SourceTrigger {
            owners: vec![1],
            conditions: vec![SourceCondition {
                location: 1,
                player: 1,
                amount: 1,
                unit: 125,
                comparison: 0,
                kind: 3,
                switch: 0,
                flags: 0,
            }],
            actions: vec![create.clone(), create],
        }],
        &locations,
        &refs,
    )
    .unwrap();
    assert_eq!(
        native.triggers[0].actions,
        [
            MissionAction::Create {
                player: PlayerId(3),
                unit_type: UnitTypeId(11),
                location: 0
            },
            MissionAction::Create {
                player: PlayerId(3),
                unit_type: UnitTypeId(11),
                location: 0
            }
        ]
    );
    assert_eq!(native.rescuable_players, [PlayerId(2), PlayerId(3)]);
}
#[test]
fn unit_properties_preserve_burrowed_rescuable_and_resource_placements() {
    use map_formats::PlacedUnit;
    let units = vec![
        PlacedUnit {
            serial: 1,
            x: 128,
            y: 128,
            unit_type: 19,
            owner: 2,
            resource_amount: None,
        },
        PlacedUnit {
            serial: 2,
            x: 256,
            y: 128,
            unit_type: 37,
            owner: 4,
            resource_amount: None,
        },
        PlacedUnit {
            serial: 3,
            x: 128,
            y: 256,
            unit_type: 176,
            owner: 11,
            resource_amount: Some(350),
        },
    ];
    let parsed = ParsedMap {
        width: 16,
        height: 16,
        tiles: vec![0; 256],
        owners: [0; 12],
        races: [0; 12],
        units,
        sections: Vec::new(),
        unsupported: Vec::new(),
    };
    let terrain = DecodedTerrain {
        megatile_indices: vec![0; 256],
        flags: vec![3; 64 * 64],
    };
    let mut raw = vec![0; 108];
    raw[14] = 2;
    raw[17] = 75;
    raw[36 + 12] = 0x12;
    raw[36 + 26] = 2;
    let map = convert_map(&parsed, &terrain, &raw).unwrap();
    assert_eq!(map.spawns.len(), 2);
    assert_eq!(map.spawns[0].owner, PlayerId(2));
    assert_eq!(map.spawns[0].hp_percent, Some(75));
    assert!(map.spawns[1].burrowed);
    assert_eq!(map.spawns[1].position, Position { x: 256, y: 128 });
    assert_eq!(map.resources[0].amount, 350);
    raw[36 + 26] |= 1;
    raw[36 + 12] |= 1;
    assert!(convert_map(&parsed, &terrain, &raw).is_err());
}
#[test]
#[ignore = "requires an owner-provided original disc; checks the complete native image budget"]
fn private_backwater_asset_budget() {
    use straterust_engine::assets::MAX_PACK_RGBA_BYTES;
    let source = std::env::var_os("STRATERUST_BACKWATER_SOURCE")
        .expect("STRATERUST_BACKWATER_SOURCE required");
    let source = Path::new(&source);
    let payload = crate::inspect(source).unwrap();
    let files = convert(&payload, source).unwrap();
    let manifest: AssetManifest = ron::de::from_bytes(&files["assets.ron"]).unwrap();
    let size = |image: &str| files[image].len() - 16;
    let mut usage = vec![("terrain".to_owned(), size(&manifest.terrain.file))];
    usage.push((
        manifest.unit_name.clone(),
        manifest.frames.iter().map(|r| size(&r.file)).sum(),
    ));
    usage.extend(manifest.extra_units.iter().map(|s| {
        (
            s.unit_name.clone(),
            s.frames.iter().map(|r| size(&r.file)).sum(),
        )
    }));
    usage.push((
        "resources".into(),
        manifest.resources.iter().map(|r| size(&r.image.file)).sum(),
    ));
    usage.push((
        "UI".into(),
        manifest.ui.iter().map(|r| size(&r.image.file)).sum(),
    ));
    usage.push((
        "decorations".into(),
        manifest
            .map_images
            .iter()
            .map(|r| size(&r.image.file))
            .sum(),
    ));
    usage.push((
        "scan".into(),
        manifest
            .scan_effect
            .iter()
            .flat_map(|e| &e.frames)
            .map(|r| size(&r.file))
            .sum(),
    ));
    let total: usize = usage.iter().map(|(_, bytes)| bytes).sum();
    for (name, bytes) in usage {
        println!("{name}: {bytes} RGBA bytes");
    }
    println!(
        "Total: {total}; manifest: {} bytes",
        files["assets.ron"].len()
    );
    assert!(
        total <= MAX_PACK_RGBA_BYTES,
        "native image collection exceeds its resident memory budget"
    );
}

#[test]
#[ignore = "requires an owner-provided original disc and completed native campaign package"]
fn private_backwater_source_inventory() {
    use straterust_engine::{
        assets::AssetPack,
        content::{Package, read_ron},
        media::MediaPack,
    };
    let source_path = std::env::var_os("STRATERUST_BACKWATER_SOURCE")
        .expect("STRATERUST_BACKWATER_SOURCE required");
    let package_path = std::env::var_os("STRATERUST_BACKWATER_PACKAGE")
        .expect("STRATERUST_BACKWATER_PACKAGE required");
    let source = Source::open(Path::new(&source_path)).unwrap();
    let mut archive = Archive::open_region(&source.path, source.offset, source.len).unwrap();
    let chk = archive.read_file(MEMBER, 8 * 1024 * 1024).unwrap();
    let parsed = map_formats::parse_chk(&chk).unwrap();
    let sections = Sections::read(&chk).unwrap();
    let strings = read_strings(sections.get("STR ").unwrap()).unwrap();
    let triggers = read_triggers(sections.get("TRIG").unwrap(), false).unwrap();
    let briefing = read_triggers(sections.get("MBRF").unwrap(), true).unwrap();
    let refs = References::collect(&triggers, &briefing, &strings).unwrap();
    let locations = read_locations(sections.get("MRGN").unwrap()).unwrap();
    let package = Package::load(Path::new(&package_path)).unwrap();
    let world = package.world(42).unwrap();
    let map = world.map();
    assert_eq!((map.width, map.height, map.players), (2048, 2048, 4));
    assert_eq!(
        (
            map.spawns.len(),
            map.resources.len(),
            map.start_locations.len()
        ),
        (79, 15, 4)
    );
    assert!(map.fog_of_war);
    assert_eq!(world.rules().tick_ms, 42);
    for (id, speed, acceleration, steps, strikes) in [
        (1, 1024, 0, vec![], vec![1]),
        (2, 1280, 67, vec![], vec![1]),
        (6, 1426, 0, vec![2, 8, 9, 5, 6, 7, 2], vec![2]),
        (7, 951, 0, vec![2, 2, 2, 6, 6, 6, 2], vec![1]),
        (10, 1707, 100, vec![], vec![1]),
        (11, 1024, 0, vec![], vec![1, 3, 4]),
    ] {
        let unit = world.unit_type(UnitTypeId(id)).unwrap();
        let motion = unit.motion.as_ref().expect("source motion missing");
        assert_eq!(
            (motion.speed, motion.acceleration, &motion.steps),
            (speed, acceleration, &steps)
        );
        let weapon = unit.weapon.as_ref().unwrap();
        assert_eq!(weapon.cooldown_jitter, Some([-1, 2]));
        assert_eq!(
            weapon.strikes.iter().map(|s| s.delay).collect::<Vec<_>>(),
            strikes
        );
    }
    let worker = world
        .unit_type(UnitTypeId(2))
        .unwrap()
        .worker
        .as_ref()
        .unwrap();
    assert_eq!(
        (worker.capacity, worker.harvest_amount, worker.harvest_ticks),
        (8, 8, 75)
    );
    assert!(!world.rules().victory);
    assert_eq!(map.spawns.iter().filter(|s| s.burrowed).count(), 13);
    let terrain = map.terrain.as_ref().unwrap();
    let expected = convert_map(
        &parsed,
        &DecodedTerrain {
            megatile_indices: Vec::new(),
            flags: terrain.flags.clone(),
        },
        sections.get("UNIT").unwrap(),
    )
    .unwrap();
    for (actual, source) in map.spawns.iter().zip(&expected.spawns) {
        assert_eq!(
            (
                actual.owner,
                actual.unit_type,
                actual.position,
                actual.hp_percent,
                actual.invincible,
                actual.burrowed
            ),
            (
                source.owner,
                source.unit_type,
                source.position,
                source.hp_percent,
                source.invincible,
                source.burrowed
            )
        );
    }
    for (actual, source) in map.resources.iter().zip(&expected.resources) {
        assert_eq!(
            (
                &actual.kind,
                actual.position,
                actual.amount,
                actual.requires_extractor
            ),
            (
                &source.kind,
                source.position,
                source.amount,
                source.requires_extractor
            )
        );
    }
    let mission = map.mission.as_ref().unwrap();
    assert_eq!(mission.triggers.len(), 15);
    assert_eq!(
        *mission,
        translate_mission(&triggers, &locations, &refs).unwrap()
    );
    let assets = AssetPack::load(Path::new(&package_path)).unwrap().unwrap();
    assets.validate_for_world(&world).unwrap();
    assert_eq!(assets.map_images.len(), 23);
    for (image, record) in assets
        .map_images
        .iter()
        .zip(sections.get("THG2").unwrap().as_chunks::<10>().0)
    {
        assert_eq!(
            image.position,
            Position {
                x: i32::from(short(record, 2)),
                y: i32::from(short(record, 4))
            }
        );
    }
    let media = MediaPack::load(Path::new(&package_path)).unwrap().unwrap();
    media.validate_world(&world).unwrap();
    assert_eq!(media.mission_texts, refs.texts);
    assert_eq!(media.mission_audio.len(), 21);
    assert_eq!(media.briefing.len(), 20);
    assert_eq!(world.rules().research.len(), 4);
    assert_eq!(
        world
            .rules()
            .research
            .iter()
            .map(|r| r.ticks)
            .collect::<Vec<_>>(),
        [4000, 4000, 1500, 1200]
    );
    let raw_media: MediaManifest = read_ron(&Path::new(&package_path).join("media.ron")).unwrap();
    let mut files = Files::from([("media.ron".into(), ron_bytes(&raw_media).unwrap())]);
    write_presentation(&mut files, &briefing, &refs).unwrap();
    let converted: MediaManifest = ron::de::from_bytes(&files["media.ron"]).unwrap();
    assert_eq!(
        ron_bytes(&converted.briefing).unwrap(),
        ron_bytes(&raw_media.briefing).unwrap()
    );
    // Source mission branches, with only their tested condition changed.
    let mut rescue = package.world(42).unwrap();
    for _ in 0..12 {
        rescue.step(&[]).unwrap();
    }
    assert!(
        rescue
            .state()
            .entities
            .iter()
            .any(|e| e.unit_type == UnitTypeId(10) && e.owner == PlayerId(0))
    );
    let mut outpost_map = map.clone();
    outpost_map.spawns.push(Spawn {
        owner: PlayerId(0),
        unit_type: UnitTypeId(1),
        position: Position { x: 256, y: 208 },
        ..Spawn::default()
    });
    let mut outpost = World::new(world.rules().clone(), outpost_map, 42).unwrap();
    for _ in 0..600 {
        outpost.step(&[]).unwrap();
    }
    assert!(
        outpost
            .state()
            .entities
            .iter()
            .all(|entity| entity.owner != PlayerId(3)),
        "rescuing the source depot must transfer its complete outpost and generated reinforcements"
    );
    assert_eq!(
        outpost
            .state()
            .entities
            .iter()
            .filter(|entity| entity.unit_type == UnitTypeId(11))
            .count(),
        5,
        "original bunker rescue actions must create exactly five Firebats"
    );
    assert_eq!(
        outpost
            .state()
            .entities
            .iter()
            .filter(|entity| entity.unit_type == UnitTypeId(2))
            .count(),
        4,
        "original rescue actions must add exactly two SCVs"
    );
    let mut defeat_map = map.clone();
    defeat_map.spawns.retain(|s| s.unit_type != UnitTypeId(10));
    let mut defeat = World::new(world.rules().clone(), defeat_map, 42).unwrap();
    for _ in 0..300 {
        defeat.step(&[]).unwrap();
        if defeat.state().defeated.contains(&PlayerId(0)) {
            break;
        }
    }
    assert!(
        defeat.state().defeated.contains(&PlayerId(0)),
        "original Raynor-loss trigger did not defeat human"
    );
    let mut victory_map = map.clone();
    victory_map.spawns.retain(|s| s.unit_type != UnitTypeId(8));
    let mut victory = World::new(world.rules().clone(), victory_map, 42).unwrap();
    for _ in 0..3000 {
        victory.step(&[]).unwrap();
        if victory.state().winner.is_some() {
            break;
        }
    }
    assert_eq!(
        victory.state().winner,
        Some(PlayerId(0)),
        "original ending transmissions/victory did not complete"
    );
}
