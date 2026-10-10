use super::*;

#[test]
#[ignore = "requires STRATERUST_SOURCE pointing to the authorized retail disc"]
fn inventory_retail_campaigns() {
    let path = std::env::var_os("STRATERUST_SOURCE").expect("set STRATERUST_SOURCE");
    let source = Source::open(Path::new(&path)).unwrap();
    let mut installer = Archive::open_region(&source.path, source.offset, source.len).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/source/starcraft");
    std::fs::create_dir_all(&root).unwrap();
    let mut inventory = Vec::new();
    for race in ["terran", "zerg", "protoss"] {
        for number in 1..=if race == "terran" { 11 } else { 10 } {
            let member = format!("campaign\\{race}\\{race}{number:02}\\staredit\\scenario.chk");
            let chk = installer.read_file(&member, 8 * 1024 * 1024).unwrap();
            std::fs::write(
                root.join(format!(
                    "campaign_{race}_{race}{number:02}_staredit_scenario.chk"
                )),
                &chk,
            )
            .unwrap();
            let sections = Sections::read(&chk).unwrap();
            let map = map_formats::parse_chk(&chk).unwrap();
            let triggers = backwater::read_triggers(sections.get("TRIG").unwrap(), false).unwrap();
            let strings = backwater::read_strings(sections.get("STR ").unwrap()).unwrap();
            let units: BTreeSet<_> = map.units.iter().map(|u| u.unit_type).collect();
            let conditions: BTreeSet<_> = triggers
                .iter()
                .flat_map(|t| t.conditions.iter().map(|c| c.kind))
                .collect();
            let actions: BTreeSet<_> = triggers
                .iter()
                .flat_map(|t| t.actions.iter().map(|a| a.kind))
                .collect();
            let scripts: BTreeSet<_> = triggers
                .iter()
                .flat_map(|t| t.actions.iter())
                .filter(|a| matches!(a.kind, 15 | 16))
                .map(|a| String::from_utf8_lossy(&a.second.to_le_bytes()).into_owned())
                .collect();
            let objectives: BTreeSet<_> = triggers
                .iter()
                .flat_map(|t| t.actions.iter())
                .filter(|a| a.kind == 12)
                .map(|a| strings.get(a.text as usize - 1).cloned())
                .collect();
            inventory.push((
                race,
                number,
                map.tileset,
                units,
                conditions,
                actions,
                scripts,
                objectives,
            ));
        }
    }
    std::fs::write(
        root.join("campaign-inventory.ron"),
        ron_bytes(&inventory).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "requires STRATERUST_SOURCE pointing to the authorized retail disc"]
fn later_campaign_ai_programs_translate() -> Result<()> {
    let path = std::env::var_os("STRATERUST_SOURCE").context("set STRATERUST_SOURCE")?;
    let source = Source::open(Path::new(&path))?;
    let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
    let mut archive =
        Archive::from_bytes(installer.read_file("files\\stardat.mpq", 128 * 1024 * 1024)?)?;
    let ai = archive.read_file("scripts\\aiscript.bin", 65536)?;
    // Check script role requirements before the expensive artwork conversion.
    let dat = archive.read_file("arr\\units.dat", 19192)?;
    let fixture = straterust_engine::content::Package::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"),
    )?
    .world(42)?;
    let worker = fixture.unit_type(UnitTypeId(2)).unwrap().worker.clone();
    let mut rules = fixture.rules().clone();
    rules.units = MAPPING
        .iter()
        .map(|&(source, native)| {
            let n = usize::from(source);
            let subunit = usize::from(short(&dat, 228 + n * 2));
            let weapon = if subunit < 228 { subunit } else { n };
            let structure =
                u32::from_le_bytes(dat[0x19b0 + n * 4..0x19b4 + n * 4].try_into().unwrap()) & 1
                    != 0;
            UnitType {
                id: UnitTypeId(native),
                structure,
                speed: if structure { 0 } else { 4 },
                worker: if matches!(source, 7 | 41 | 64) {
                    worker.clone()
                } else {
                    None
                },
                weapon: (dat[0x1704 + weapon] < 100 || dat[0x17e8 + weapon] < 100).then_some(
                    Weapon {
                        friendly_splash: false,
                        projectile_speed: 0,
                        damage: 1,
                        range: 32,
                        cooldown: 1,
                        targets_air: true,
                        target_classes: Vec::new(),
                        cooldown_jitter: None,
                        damage_kind: DamageKind::Normal,
                        splash: None,
                        strikes: vec![],
                    },
                ),
                ..UnitType::default()
            }
        })
        .collect();
    rules.research.clear();
    for race in Race::ALL {
        for mission in 6..=10 {
            let number = race.source_number(mission);
            let chk = installer.read_file(
                &format!(
                    "campaign\\{0}\\{0}{number:02}\\staredit\\scenario.chk",
                    race.folder()
                ),
                8 * 1024 * 1024,
            )?;
            let sections = Sections::read(&chk)?;
            let parsed = map_formats::parse_chk(&chk)?;
            for source in parsed
                .units
                .iter()
                .filter(|u| !matches!(u.unit_type, 176..=178 | 188 | 214))
                .map(|u| u.unit_type)
                .chain(
                    sections
                        .get("THG2")?
                        .as_chunks::<10>()
                        .0
                        .iter()
                        .filter(|r| short(*r, 8) & 0x1000 == 0)
                        .map(|r| short(r, 0)),
                )
            {
                ensure!(
                    campaign_units::native_id(source).is_some(),
                    "{} {mission}: unsupported placed unit {source}",
                    race.folder()
                );
            }
            let human = parsed.owners.iter().position(|p| *p == 6).unwrap() as u8;
            let players = placed_players(&parsed, sections.get("THG2")?, human)?;
            let ids: BTreeMap<_, _> = players
                .iter()
                .enumerate()
                .map(|(n, p)| (*p, PlayerId(n as u16)))
                .collect();
            let mut map = fixture.map().clone();
            map.players = players.len() as u16;
            map.terrain = None;
            map.spawns.truncate(1);
            map.start_locations.clear();
            map.resources.clear();
            map.ai.clear();
            let source_triggers = backwater::read_triggers(sections.get("TRIG")?, false)?;
            let briefing = backwater::read_triggers(sections.get("MBRF")?, true)?;
            let strings = backwater::read_strings(sections.get("STR ")?)?;
            let refs = References::collect(&source_triggers, &briefing, &strings)?;
            backwater::briefing_actions(&briefing, &refs)
                .with_context(|| format!("{} {mission}: native briefing", race.folder()))?;
            let locations = backwater::read_locations(sections.exact("MRGN", 1280)?)?;
            map.width = i32::from(parsed.width) * 32;
            map.height = i32::from(parsed.height) * 32;
            let mut alliances = Vec::new();
            for a in 0..players.len() {
                for b in a + 1..players.len() {
                    if players[a] == 11
                        || players[b] == 11
                        || matches!(parsed.owners[usize::from(players[a])], 0 | 3 | 7)
                        || matches!(parsed.owners[usize::from(players[b])], 0 | 3 | 7)
                    {
                        alliances.push([PlayerId(a as u16), PlayerId(b as u16)]);
                    }
                }
            }
            let rescue = players
                .iter()
                .filter(|p| parsed.owners[usize::from(**p)] == 3)
                .map(|p| ids[p])
                .collect();
            let trigger_players: Vec<_> = players
                .iter()
                .copied()
                .filter(|p| parsed.owners[usize::from(*p)] != 0)
                .collect();
            map.mission = Some(triggers::translate(
                &source_triggers,
                sections.get("UPRP")?,
                &locations,
                &refs,
                &trigger_players,
                &ids,
                sections.get("FORC")?,
                &ai,
                &mut map,
                rescue,
                alliances,
            )?);
            World::new(rules.clone(), map, 42)
                .with_context(|| format!("{} mission {mission}: native triggers", race.folder()))?;
            for trigger in backwater::read_triggers(sections.get("TRIG")?, false)? {
                for action in trigger.actions.iter().filter(|a| matches!(a.kind, 15 | 16)) {
                    let script = action.second.to_le_bytes();
                    if matches!(
                        &script,
                        b"Suic" | b"SuiR" | b"Rscu" | b"EnBk" | b"ClrC" | b"MvTe" | b"VluA"
                    ) {
                        continue;
                    }
                    let program = crate::ai::translate(&ai, script, MAPPING)
                        .with_context(|| format!("{} {mission}: {:?}", race.folder(), script))?;
                    let mut map = fixture.map().clone();
                    map.spawns.truncate(1);
                    map.resources.clear();
                    map.ai = vec![AiController {
                        research: Vec::new(),
                        abilities: Vec::new(),
                        harvest_weights: Vec::new(),
                        player: PlayerId(1),
                        home: Position { x: 800, y: 400 },
                        radius: 640,
                        active: true,
                        program,
                    }];
                    World::new(rules.clone(), map, 42)
                        .with_context(|| format!("{} {mission}: {:?}", race.folder(), script))?;
                }
            }
        }
    }
    Ok(())
}

#[test]
fn playable_terran_order_skips_the_cut_scenario_and_includes_the_finale() {
    assert_eq!(
        (1..=10)
            .map(|n| Race::Terran.source_number(n))
            .collect::<Vec<_>>(),
        [1, 2, 3, 4, 5, 6, 8, 9, 10, 11]
    );
    for race in [Race::Zerg, Race::Protoss] {
        assert_eq!(
            (1..=10).map(|n| race.source_number(n)).collect::<Vec<_>>(),
            (1..=10).collect::<Vec<_>>()
        );
    }
}

#[test]
#[ignore = "requires STRATERUST_SOURCE pointing to the authorized retail disc"]
fn later_campaign_created_properties_translate() -> Result<()> {
    let path = std::env::var_os("STRATERUST_SOURCE").context("set STRATERUST_SOURCE")?;
    let source = Source::open(Path::new(&path))?;
    let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
    for race in Race::ALL {
        for mission in 6..=10 {
            let number = race.source_number(mission);
            let chk = installer.read_file(
                &format!(
                    "campaign\\{0}\\{0}{number:02}\\staredit\\scenario.chk",
                    race.folder()
                ),
                8 * 1024 * 1024,
            )?;
            let sections = Sections::read(&chk)?;
            for trigger in backwater::read_triggers(sections.get("TRIG")?, false)? {
                for action in trigger.actions.iter().filter(|a| a.kind == 11) {
                    triggers::created_properties(sections.get("UPRP")?, action.second)
                        .with_context(|| {
                            format!(
                                "{} mission {mission} property {}",
                                race.folder(),
                                action.second
                            )
                        })?;
                }
            }
        }
    }
    Ok(())
}
