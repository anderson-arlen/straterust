use super::*;

/// Small presentation refresh for already imported campaigns; publish the
/// referring manifest last so a partial write cannot expose missing frames.
pub(super) fn update_effects(source_path: &Path, output: &Path, update_rules: bool) -> Result<()> {
    let source = Source::open(source_path)?;
    let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
    let mut archive =
        Archive::from_bytes(installer.read_file("files\\stardat.mpq", 128 * 1024 * 1024)?)?;
    let directories = if output.join("campaign.ron").is_file() {
        straterust_engine::content::Campaign::load(output)?
            .missions
            .iter()
            .map(|entry| output.join(&entry.package))
            .collect::<Vec<_>>()
    } else {
        vec![output.to_path_buf()]
    };
    for directory in directories {
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
            campaign_units::apply_creep_rules(&mut rules);
            campaign_units::apply_combat_rules(&mut archive, &mut rules)?;
            campaign_units::apply_transport_rules(&mut archive, &mut rules)?;
        }
        campaign_units::refresh_effects(&mut archive, &mut files, &mut assets, &rules)?;
        campaign_units::refresh_combat(&mut archive, &mut files, &mut assets, &rules)?;
        campaign_units::refresh_buildings(&mut archive, &mut files, &mut assets, &rules)?;
        campaign_units::refresh_wireframes(&mut archive, &mut files, &mut assets, &rules)?;
        campaign_units::refresh_indicators(&mut archive, &mut files, &mut assets, &rules)?;
        crate::flight::refresh(&mut archive, &mut files, &mut assets, &mut rules)?;
        if update_rules {
            if let Some(number) = world.map().id.strip_prefix("straterust.terran-") {
                let chk = installer.read_file(
                    &format!("campaign\\terran\\terran{number}\\staredit\\scenario.chk"),
                    8 * 1024 * 1024,
                )?;
                campaign_units::refresh_research(
                    &mut archive,
                    &mut files,
                    &mut assets,
                    &mut rules,
                    &chk,
                )?;
            }
            let mut map = world.map().clone();
            refresh_energy_properties(&mut installer, &mut map)?;
            files.insert("map.ron".into(), ron_bytes(&map)?);
            rules.prioritize_threats = true;
            let mut verified_map = map;
            verified_map.terrain = world.map().terrain.clone();
            verified_map.mission = world.map().mission.clone();
            straterust_engine::sim::World::new(rules.clone(), verified_map, 0)
                .context("validate refreshed campaign gameplay before publishing")?;
            files.insert("rules.ron".into(), ron_bytes(&rules)?);
        }
        assets.validate()?;
        ensure!(
            ron_bytes(&assets)?.len() <= 4 * 1024 * 1024,
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

fn refresh_energy_properties(
    archive: &mut Archive<std::fs::File>,
    map: &mut straterust_engine::sim::Map,
) -> Result<()> {
    let Some(number) = map.id.strip_prefix("straterust.terran-") else {
        return Ok(());
    };
    let chk = archive.read_file(
        &format!("campaign\\terran\\terran{number}\\staredit\\scenario.chk"),
        8 * 1024 * 1024,
    )?;
    let sections = backwater::Sections::read(&chk)?;
    if number == "05" {
        // Native player zero is the source human controller, which is not
        // necessarily source slot zero. Retain the actual mission PUNI flag.
        let parsed = map_formats::parse_chk(&chk)?;
        let human = parsed
            .owners
            .iter()
            .position(|owner| *owner == 6)
            .context("mission has no human controller")?;
        let availability = sections.exact("PUNI", 5700)?;
        let source_unit = 11;
        let enabled = if availability[2964 + human * 228 + source_unit] != 0 {
            availability[2736 + source_unit] != 0
        } else {
            availability[human * 228 + source_unit] != 0
        };
        let allowed = map
            .creation
            .entry(straterust_engine::sim::PlayerId(0))
            .or_default();
        let dropship = straterust_engine::sim::UnitTypeId(53);
        allowed.retain(|unit| *unit != dropship);
        if enabled {
            allowed.push(dropship);
            allowed.sort();
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
