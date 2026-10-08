//! Original map links and stored production are data, not executable game logic.
use super::*;

pub(crate) fn refresh(chk: &[u8], rules: &Rules, map: &mut Map) -> Result<()> {
    let sections = crate::backwater::Sections::read(chk)?;
    let parsed = crate::map_formats::parse_chk(chk)?;
    let human = parsed
        .owners
        .iter()
        .position(|o| *o == 6)
        .context("missing human controller")? as u8;
    let players = crate::campaign::placed_players(&parsed, sections.get("THG2")?, human)?;
    let availability = sections.exact("PUNI", 5700)?;
    for (player, source) in players.into_iter().enumerate() {
        let p = usize::from(source);
        let enabled = MAPPING
            .iter()
            .filter(|(source, native)| {
                let u = usize::from(*source);
                let allowed = if availability[2964 + p * 228 + u] != 0 {
                    availability[2736 + u]
                } else {
                    availability[p * 228 + u]
                };
                allowed != 0
                    && rules
                        .units
                        .iter()
                        .any(|unit| unit.id == UnitTypeId(*native))
            })
            .map(|(_, native)| UnitTypeId(*native))
            .collect();
        map.creation.insert(PlayerId(player as u16), enabled);
    }
    let records = sections.get("UNIT")?.as_chunks::<36>().0;
    for record in records {
        if matches!(word(record, 8), 72 | 82 | 83 | 108) && word(record, 14) & 32 != 0 {
            let count = word(record, 24);
            ensure!(count <= 64, "unsupported original stored unit count");
            let position = Position {
                x: i32::from(word(record, 4)),
                y: i32::from(word(record, 6)),
            };
            if let Some(spawn) = map
                .spawns
                .iter_mut()
                .find(|s| s.position == position && Some(s.unit_type) == native_id(word(record, 8)))
            {
                spawn.stored_units = count as u8;
            }
        }
    }
    for record in records
        .iter()
        .filter(|r| word(*r, 8) == 134 && word(*r, 10) & 1024 != 0)
    {
        let serial = dword(record, 32);
        let peer = records
            .iter()
            .find(|r| dword(*r, 0) == serial && word(*r, 8) == 134)
            .context("missing original Nydus endpoint")?;
        let position = |r: &[u8]| Position {
            x: i32::from(word(r, 4)),
            y: i32::from(word(r, 6)),
        };
        if let Some(spawn) = map
            .spawns
            .iter_mut()
            .find(|s| s.position == position(record) && Some(s.unit_type) == native_id(134))
        {
            spawn.linked_to = Some(position(peer));
        }
    }
    Ok(())
}
