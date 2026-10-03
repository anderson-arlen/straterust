//! The addon research enabled by the first five retail campaign CHKs.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize)]
struct Labels {
    #[serde(default)]
    build_buttons: BTreeMap<UnitTypeId, BuildButton>,
    #[serde(default)]
    research_names: BTreeMap<ResearchId, String>,
    #[serde(default)]
    research_keys: BTreeMap<ResearchId, String>,
    #[serde(default)]
    train_keys: BTreeMap<UnitTypeId, String>,
}

#[derive(Serialize, Deserialize)]
struct BuildButton {
    advanced: bool,
    slot: u8,
    key: String,
}

pub(crate) fn refresh_research(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    assets: &mut AssetManifest,
    rules: &mut Rules,
    chk: &[u8],
) -> Result<()> {
    // Apply the same production prerequisites to refreshed packages as fresh imports.
    let known_units: BTreeSet<_> = rules.units.iter().map(|unit| unit.id).collect();
    for unit in &mut rules.units {
        if unit.id == UnitTypeId(2)
            && known_units.contains(&UnitTypeId(16))
            && !unit.builds.contains(&UnitTypeId(16))
        {
            unit.builds.push(UnitTypeId(16));
        }
        let requirements: &[u16] = match unit.id.0 {
            21 => &[32, 36],
            22 => &[32, 35],
            36 => &[32],
            _ => continue,
        };
        unit.prerequisites = requirements
            .iter()
            .copied()
            .map(UnitTypeId)
            .filter(|id| known_units.contains(id))
            .collect();
    }
    let sections = crate::backwater::Sections::read(chk)?;
    let owners = sections.exact("OWNR", 12)?;
    let human = owners
        .iter()
        .position(|owner| *owner == 6)
        .context("mission has no human")?;
    let tech_flags = sections.exact("PTEC", 912)?;
    let upgrade_flags = sections.exact("UPGR", 1748)?;
    let tech = archive.read_file("arr\\techdata.dat", 432)?;
    let upgrades = archive.read_file("arr\\upgrades.dat", 920)?;
    ensure!(
        tech.len() == 432 && upgrades.len() == 920,
        "unsupported research DAT layout"
    );
    let available = |technology: bool, source: usize| {
        if technology {
            if tech_flags[624 + human * 24 + source] != 0 {
                (tech_flags[576 + source], tech_flags[600 + source])
            } else {
                (
                    tech_flags[human * 24 + source],
                    tech_flags[288 + human * 24 + source],
                )
            }
        } else if upgrade_flags[1196 + human * 46 + source] != 0 {
            (upgrade_flags[1104 + source], upgrade_flags[1150 + source])
        } else {
            (
                upgrade_flags[human * 46 + source],
                upgrade_flags[552 + human * 46 + source],
            )
        }
    };
    let known = |id| rules.units.iter().any(|unit| unit.id == UnitTypeId(id));
    let mut definitions = Vec::new();
    for (id, source, technology, facility, targets, effect, name, key) in [
        (
            5,
            9,
            true,
            34,
            vec![UnitTypeId(23)],
            ResearchEffect::Cloak {
                units: vec![UnitTypeId(23)],
            },
            "Cloaking Field",
            "C",
        ),
        (
            6,
            22,
            false,
            34,
            vec![UnitTypeId(23)],
            ResearchEffect::EnergyCapacity {
                units: vec![UnitTypeId(23)],
                amount: 50,
            },
            "Apollo Reactor",
            "A",
        ),
        (
            7,
            17,
            false,
            35,
            vec![UnitTypeId(20)],
            ResearchEffect::MovementSpeed {
                units: vec![UnitTypeId(20)],
                percent: 150,
                acceleration_percent: 200,
            },
            "Ion Thrusters",
            "I",
        ),
        (
            8,
            3,
            true,
            35,
            vec![UnitTypeId(20)],
            ResearchEffect::Mines {
                units: vec![UnitTypeId(20)],
            },
            "Spider Mines",
            "M",
        ),
    ] {
        let (maximum, initial) = available(technology, source);
        if maximum == 0 || !known(facility) || !targets.iter().all(|id| known(id.0)) {
            continue;
        }
        ensure!(
            initial == 0,
            "initial addon research requires explicit native starting research conversion"
        );
        let (data, count) = if technology {
            (&tech, 24)
        } else {
            (&upgrades, 46)
        };
        let cost = [
            ("minerals", word(data, source * 2)),
            (
                "gas",
                word(data, count * (if technology { 2 } else { 4 }) + source * 2),
            ),
        ]
        .into_iter()
        .filter(|(_, amount)| *amount > 0)
        .map(|(kind, amount)| ResourceAmount {
            kind: kind.into(),
            amount: u32::from(amount),
        })
        .collect();
        let ticks = u32::from(word(
            data,
            count * (if technology { 4 } else { 8 }) + source * 2,
        ));
        let icon = usize::from(word(
            data,
            count * (if technology { 12 } else { 14 }) + source * 2,
        ));
        definitions.push((
            Research {
                id: ResearchId(id),
                facility: UnitTypeId(facility),
                cost,
                ticks,
                effect,
            },
            icon,
            name,
            key,
        ));
    }
    rules
        .research
        .retain(|research| !(5..=8).contains(&research.id.0));
    let bytes = files
        .get("presentation.ron")
        .context("missing research presentation")?;
    let mut presentation = std::str::from_utf8(bytes)?.to_owned();
    let mut labels: Labels = ron::de::from_str(&presentation)?;
    for id in 5..=8 {
        labels.research_names.remove(&ResearchId(id));
        labels.research_keys.remove(&ResearchId(id));
    }
    let palette =
        formats::decode_pcx(&archive.read_file("unit\\cmdbtns\\ticon.pcx", 1024 * 1024)?)?;
    let icons = formats::decode_grp(
        &archive.read_file("unit\\cmdbtns\\cmdicons.grp", 1024 * 1024)?,
        &crate::terran_ui::command_palette(&palette)?,
    )?;
    for (research, icon, name, key) in definitions {
        labels.research_names.insert(research.id, name.into());
        labels.research_keys.insert(research.id, key.into());
        let key = format!("research.{}", research.id.0);
        assets.ui.retain(|entry| entry.key != key);
        assets.ui.push(straterust_engine::assets::UiImageManifest {
            image: crate::add_image(
                files,
                &format!("ui-{key}.srim"),
                icons.get(icon).context("missing research icon")?,
            )?,
            key,
        });
        rules.research.push(research);
    }
    for (unit, key) in [(20, "V"), (21, "G"), (22, "T"), (23, "W"), (53, "D")] {
        labels.train_keys.insert(UnitTypeId(unit), key.into());
    }
    for (id, advanced, slot, key) in [
        (3, false, 0, "C"),
        (4, false, 1, "S"),
        (14, false, 2, "R"),
        (5, false, 3, "B"),
        (15, false, 4, "E"),
        (13, false, 5, "T"),
        (12, false, 6, "A"),
        (16, false, 7, "U"),
        (32, true, 0, "F"),
        (33, true, 1, "S"),
        (36, true, 3, "A"),
    ] {
        labels.build_buttons.insert(
            UnitTypeId(id),
            BuildButton {
                advanced,
                slot,
                key: key.into(),
            },
        );
    }
    set_map(
        &mut presentation,
        "build_buttons",
        &ron::ser::to_string(&labels.build_buttons)?,
    )?;
    set_map(
        &mut presentation,
        "research_names",
        &ron::ser::to_string(&labels.research_names)?,
    )?;
    set_map(
        &mut presentation,
        "research_keys",
        &ron::ser::to_string(&labels.research_keys)?,
    )?;
    set_map(
        &mut presentation,
        "train_keys",
        &ron::ser::to_string(&labels.train_keys)?,
    )?;
    let key = "command.advanced-build";
    assets.ui.retain(|entry| entry.key != key);
    assets.ui.push(straterust_engine::assets::UiImageManifest {
        key: key.into(),
        image: crate::add_image(files, "ui-command-advanced-build.srim", &icons[235])?,
    });
    files.insert("presentation.ron".into(), presentation.into_bytes());
    Ok(())
}

fn set_map(text: &mut String, field: &str, value: &str) -> Result<()> {
    let Some(field_at) = text.find(&format!("{field}:")) else {
        let end = text.rfind(')').context("invalid presentation")?;
        text.insert_str(end, &format!("    {field}: {value},\n"));
        return Ok(());
    };
    let begin = field_at
        + text[field_at..]
            .find('{')
            .context("invalid presentation map")?;
    let (mut depth, mut quoted, mut escaped) = (0, false, false);
    for (offset, character) in text[begin..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && character == '\\' {
            escaped = true;
            continue;
        }
        if character == '"' {
            quoted = !quoted;
        }
        if quoted {
            continue;
        }
        if character == '{' {
            depth += 1;
        }
        if character == '}' {
            depth -= 1;
            if depth == 0 {
                text.replace_range(begin..begin + offset + 1, value);
                return Ok(());
            }
        }
    }
    anyhow::bail!("unterminated presentation map")
}
