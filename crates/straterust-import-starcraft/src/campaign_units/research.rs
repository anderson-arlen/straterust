//! Retail building research, limited by each campaign map’s technology settings.
use super::*;
mod announcements;
pub(crate) use announcements::refresh as refresh_announcements;
use serde::Deserialize;
mod catalog;
pub(crate) mod faction_research;
#[cfg(test)]
mod tests;

#[derive(Default, Deserialize)]
struct Labels {
    #[serde(default)]
    research_names: BTreeMap<ResearchId, String>,
    #[serde(default)]
    research_keys: BTreeMap<ResearchId, String>,
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
    catalog::refresh(archive, files, assets, rules, &sections, human)?;
    crate::hotkeys::refresh(archive, files)
}

pub(crate) fn set_map(text: &mut String, field: &str, value: &str) -> Result<()> {
    let Some(field_at) = text.find(&format!("{field}:")) else {
        let end = text.rfind(')').context("invalid presentation")?;
        text.insert_str(end, &format!("    {field}: {value},\n"));
        return Ok(());
    };
    if value.bytes().all(|byte| byte.is_ascii_digit()) {
        let begin = field_at + field.len() + 1;
        let end = begin
            + text[begin..]
                .find(',')
                .context("invalid presentation scalar")?;
        ensure!(
            text[begin..end].trim().parse::<u32>().is_ok(),
            "invalid presentation scalar"
        );
        text.replace_range(begin..end, &format!(" {value}"));
        return Ok(());
    }
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
