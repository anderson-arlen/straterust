use super::*;

/// Small presentation refresh for already imported campaigns; publish the
/// referring manifest last so a partial write cannot expose missing frames.
pub(super) fn update_effects(source_path: &Path, output: &Path, update_rules: bool) -> Result<()> {
    let source = Source::open(source_path)?;
    let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
    let mut archive =
        Archive::from_bytes(installer.read_file("files\\stardat.mpq", 128 * 1024 * 1024)?)?;
    let directories = crate::campaign::package_directories(output)?;
    if output.join("campaign.ron").is_file() {
        crate::menus::refresh(source_path, output)?;
    }
    for directory in directories {
        crate::burrow::upgrade(&directory)?;
        let bytes = fs::read(directory.join("assets.ron"))?;
        ensure!(
            bytes.len() <= 64 * 1024 * 1024,
            "refresh manifest exceeds limit"
        );
        let mut assets: AssetManifest = ron::de::from_bytes(&bytes)?;
        let package = Package::load(&directory)?;
        let world = package.world(0)?;
        let mut files = Files::new();
        if directory.join("media.ron").is_file() {
            files.insert("media.ron".into(), fs::read(directory.join("media.ron"))?);
        }
        files.insert(
            "presentation.ron".into(),
            fs::read(directory.join("presentation.ron"))?,
        );
        mission_terran::add_grenade_projectiles(
            &mut archive,
            &mut Vec::new(),
            &mut files,
            &mut assets,
            world.rules(),
        )?;
        let mut rules = world.rules().clone();
        if update_rules
            && rules
                .units
                .iter()
                .any(|u| u.id == straterust_engine::sim::UnitTypeId(22))
            && !rules
                .units
                .iter()
                .any(|u| u.id == straterust_engine::sim::UnitTypeId(56))
        {
            files.insert("rules.ron".into(), ron_bytes(&rules)?);
            files.insert("assets.ron".into(), ron_bytes(&assets)?);
            files.insert("map.ron".into(), ron_bytes(world.map())?);
            campaign_units::convert(&mut archive, &mut files, &[30])?;
            rules = ron::de::from_bytes(&files["rules.ron"])?;
            assets = ron::de::from_bytes(&files["assets.ron"])?;
        }
        if update_rules
            && rules
                .units
                .iter()
                .any(|u| Some(u.id) == campaign_units::native_id(35))
            && !rules
                .units
                .iter()
                .any(|u| Some(u.id) == campaign_units::native_id(59))
        {
            files.insert("rules.ron".into(), ron_bytes(&rules)?);
            files.insert("assets.ron".into(), ron_bytes(&assets)?);
            files.insert("map.ron".into(), ron_bytes(world.map())?);
            campaign_units::convert(&mut archive, &mut files, &[59])?;
            rules = ron::de::from_bytes(&files["rules.ron"])?;
            assets = ron::de::from_bytes(&files["assets.ron"])?;
        }
        if update_rules
            && matches!(
                world.map().id.as_str(),
                "straterust.terran-05" | "stratarust.terran-05"
            )
            && !rules
                .units
                .iter()
                .any(|unit| unit.id == straterust_engine::sim::UnitTypeId(53))
        {
            files.insert("rules.ron".into(), ron_bytes(&rules)?);
            files.insert("assets.ron".into(), ron_bytes(&assets)?);
            campaign_units::convert(&mut archive, &mut files, &[11])?;
            rules = ron::de::from_bytes(&files["rules.ron"])?;
            assets = ron::de::from_bytes(&files["assets.ron"])?;
        }
        if update_rules {
            let missing: Vec<_> = [14, 73, 85]
                .into_iter()
                .filter(|source| {
                    !rules
                        .units
                        .iter()
                        .any(|u| Some(u.id) == campaign_units::native_id(*source))
                })
                .collect();
            if !missing.is_empty() {
                files.insert("rules.ron".into(), ron_bytes(&rules)?);
                files.insert("assets.ron".into(), ron_bytes(&assets)?);
                files.insert("map.ron".into(), ron_bytes(world.map())?);
                campaign_units::convert(&mut archive, &mut files, &missing)?;
                rules = ron::de::from_bytes(&files["rules.ron"])?;
                assets = ron::de::from_bytes(&files["assets.ron"])?;
            }
            campaign_units::apply_faction_rules(&mut archive, &mut rules)?;
            if rules
                .units
                .iter()
                .any(|u| Some(u.id) == campaign_units::native_id(35))
            {
                let mut presentation = std::str::from_utf8(&files["presentation.ron"])?.to_owned();
                campaign_units::set_map(&mut presentation, "supply_divisor", "2")?;
                files.insert("presentation.ron".into(), presentation.into_bytes());
            }
            campaign_units::apply_creep_rules(&mut rules);
            campaign_units::apply_combat_rules(&mut archive, &mut rules)?;
            campaign_units::apply_transport_rules(&mut archive, &mut rules)?;
        }
        campaign_units::refresh_effects(&mut archive, &mut files, &mut assets, &rules)?;
        campaign_units::refresh_combat(&mut archive, &mut files, &mut assets, &rules)?;
        campaign_units::refresh_buildings(&mut archive, &mut files, &mut assets, &rules)?;
        campaign_units::refresh_morphs(&mut archive, &mut files, &mut assets, &rules)?;
        campaign_units::refresh_protoss(&mut archive, &mut files, &mut assets, &rules)?;
        campaign_units::refresh_wireframes(&mut archive, &mut files, &mut assets, &rules)?;
        campaign_units::refresh_indicators(&mut archive, &mut files, &mut assets, &rules)?;
        crate::flight::refresh(&mut archive, &mut files, &mut assets, &mut rules)?;
        if update_rules {
            let mut map = world.map().clone();
            if let Some((race, number)) = crate::campaign::source_mission(&world.map().id) {
                let chk = installer.read_file(
                    &format!("campaign\\{race}\\{race}{number}\\staredit\\scenario.chk"),
                    8 * 1024 * 1024,
                )?;
                campaign_units::refresh_research(
                    &mut archive,
                    &mut files,
                    &mut assets,
                    &mut rules,
                    &chk,
                )?;
                campaign_units::refresh_map_properties(&chk, &rules, &mut map)?;
                if let Some(mission) = &mut map.mission {
                    refresh_initial_research(&chk, &rules, mission)?;
                    files.insert("mission.ron".into(), ron_bytes(mission)?);
                }
            }
            refresh_energy_properties(&mut installer, &mut map)?;
            let ai = archive.read_file("scripts\\aiscript.bin", 65536)?;
            for controller in &mut map.ai {
                if controller.program.is_empty() {
                    continue;
                }
                let id = match (
                    number_for_map(&map.id),
                    controller
                        .program
                        .iter()
                        .any(|i| matches!(i, straterust_engine::sim::AiInstruction::Attack)),
                ) {
                    (Some("03"), _) => *b"Ter3",
                    (Some("05"), true) => *b"Ter5",
                    (Some("05"), false) => *b"Te5H",
                    _ => continue,
                };
                controller.program = crate::ai::translate(&ai, id, campaign_units::MAPPING)?;
            }
            files.insert("map.ron".into(), ron_bytes(&map)?);
            rules.prioritize_threats = true;
            let mut verified_map = map;
            verified_map.terrain = world.map().terrain.clone();
            straterust_engine::sim::World::new(rules.clone(), verified_map, 0)
                .context("validate refreshed campaign gameplay before publishing")?;
            files.insert("rules.ron".into(), ron_bytes(&rules)?);
        }
        crate::hotkeys::refresh(&mut archive, &mut files)?;
        crate::carried_resources::refresh(&mut archive, &mut files, &mut assets, &rules)?;
        crate::burrow::refresh(
            &mut archive,
            &mut files,
            &mut assets,
            &rules,
            &mut Vec::new(),
        )?;
        // Combat refresh installs source effects, including the base Zerg
        // nuclear warning. Reapply the listener's advisor after every refresh,
        // including presentation-only updates that do not rebuild research.
        if let Some((race, number)) = crate::campaign::source_mission(&world.map().id) {
            let chk = installer.read_file(
                &format!("campaign\\{race}\\{race}{number}\\staredit\\scenario.chk"),
                8 * 1024 * 1024,
            )?;
            let sections = backwater::Sections::read(&chk)?;
            let human = sections
                .exact("OWNR", 12)?
                .iter()
                .position(|owner| *owner == 6)
                .context("mission has no human controller")?;
            campaign_units::research::refresh_announcements(
                &mut archive,
                &mut files,
                &rules,
                sections.exact("SIDE", 12)?[human],
            )?;
        }
        if assets.unit_type == straterust_engine::sim::UnitTypeId(1) {
            crate::terran::mark_marine_flashes(&mut assets.clips);
        }
        assets.validate()?;
        ensure!(
            ron_bytes(&assets)?.len() <= straterust_engine::assets::MAX_ASSET_MANIFEST_BYTES,
            "assets manifest exceeds runtime limit"
        );
        if let Some(bytes) = files.get("media.ron") {
            let media: straterust_engine::media::MediaManifest = ron::de::from_bytes(bytes)?;
            media.validate()?;
        }
        for (name, bytes) in files {
            fs::write(directory.join(name), bytes)?;
        }
        let temporary = directory.join(".weapon-effects-assets.ron");
        fs::write(&temporary, ron_bytes(&assets)?)?;
        fs::rename(temporary, directory.join("assets.ron"))?;
        println!("Updated campaign effects: {}", directory.display());
    }
    Ok(())
}

fn refresh_initial_research(
    chk: &[u8],
    rules: &straterust_engine::sim::Rules,
    mission: &mut straterust_engine::sim::Mission,
) -> Result<()> {
    use straterust_engine::sim::{
        MissionAction, MissionComparison, MissionCondition, MissionTrigger, PlayerId,
    };
    let sections = crate::backwater::Sections::read(chk)?;
    let parsed = crate::map_formats::parse_chk(chk)?;
    let human = parsed
        .owners
        .iter()
        .position(|owner| *owner == 6)
        .context("missing human controller")? as u8;
    let players = crate::campaign::placed_players(&parsed, sections.get("THG2")?, human)?;
    let mut actions = Vec::new();
    for (native, source_player) in players.into_iter().enumerate() {
        for &(id, technology, source) in campaign_units::research::faction_research::SOURCES {
            let player = PlayerId(native as u16);
            let initial = campaign_units::research::faction_research::available(
                &sections,
                usize::from(source_player),
                technology,
                source,
            )?
            .1;
            for level in 1..=initial.min(3) {
                let research = campaign_units::research::faction_research::level_id(id, level);
                if rules.research.iter().any(|r| r.id == research)
                && !mission.triggers.iter().flat_map(|t| &t.actions).any(|a| matches!(a, MissionAction::GrantResearch { player: p, research: r } if *p == player && *r == research)) {
                actions.push(MissionAction::GrantResearch { player, research });
            }
            }
        }
    }
    for chunk in actions.chunks(64).rev() {
        mission.triggers.insert(
            0,
            MissionTrigger {
                conditions: vec![MissionCondition::Elapsed {
                    comparison: MissionComparison::AtLeast,
                    milliseconds: 0,
                }],
                actions: chunk.to_vec(),
            },
        );
    }
    Ok(())
}

fn refresh_energy_properties(
    archive: &mut Archive<std::fs::File>,
    map: &mut straterust_engine::sim::Map,
) -> Result<()> {
    let Some((race, number)) = crate::campaign::source_mission(&map.id) else {
        return Ok(());
    };
    let chk = archive.read_file(
        &format!("campaign\\{race}\\{race}{number}\\staredit\\scenario.chk"),
        8 * 1024 * 1024,
    )?;
    let sections = backwater::Sections::read(&chk)?;
    if race == "terran" && number == "05" {
        let parsed = map_formats::parse_chk(&chk)?;
        let human = parsed
            .owners
            .iter()
            .position(|owner| *owner == 6)
            .context("mission has no human controller")?;
        let mut players = vec![human];
        players.extend((0..8).filter(|p| *p != human && parsed.owners[*p] != 0));
        let availability = sections.exact("PUNI", 5700)?;
        for (native, source) in players.into_iter().enumerate() {
            let enabled = if availability[2964 + source * 228 + 11] != 0 {
                availability[2736 + 11] != 0
            } else {
                availability[source * 228 + 11] != 0
            };
            let allowed = map
                .creation
                .entry(straterust_engine::sim::PlayerId(native as u16))
                .or_default();
            let dropship = straterust_engine::sim::UnitTypeId(53);
            allowed.retain(|unit| *unit != dropship);
            if enabled {
                allowed.push(dropship);
                allowed.sort();
            }
        }
    }
    for record in sections.get("UNIT")?.as_chunks::<36>().0 {
        let word = |offset| u16::from_le_bytes(record[offset..offset + 2].try_into().unwrap());
        if word(14) & 8 == 0 {
            continue;
        }
        let Some(unit_type) = campaign_units::native_id(word(8)) else {
            continue;
        };
        let position = straterust_engine::sim::Position {
            x: i32::from(word(4)),
            y: i32::from(word(6)),
        };
        for spawn in &mut map.spawns {
            if spawn.unit_type == unit_type && spawn.position == position {
                spawn.energy_percent = Some(record[19]);
            }
        }
    }
    Ok(())
}

fn number_for_map(id: &str) -> Option<&str> {
    id.strip_prefix("straterust.terran-")
        .or_else(|| id.strip_prefix("stratarust.terran-"))
}
