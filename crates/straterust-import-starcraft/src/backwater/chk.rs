use super::*;

pub(crate) struct Sections<'a>(BTreeMap<&'a str, &'a [u8]>);
impl<'a> Sections<'a> {
    pub(crate) fn read(data: &'a [u8]) -> Result<Self> {
        ensure!(data.len() <= 8 * 1024 * 1024, "campaign CHK exceeds8MiB");
        let mut at = 0;
        let mut map = BTreeMap::new();
        while at < data.len() {
            let h = data.get(at..at + 8).context("truncated CHK header")?;
            let name = std::str::from_utf8(&h[..4])?;
            let end = at
                .checked_add(8)
                .and_then(|n| n.checked_add(word(h, 4) as usize))
                .context("CHK length overflow")?;
            let body = data.get(at + 8..end).context("truncated CHK body")?;
            ensure!(
                map.len() < 256 && map.insert(name, body).is_none(),
                "duplicate/excessive CHK sections"
            );
            at = end;
        }
        Ok(Self(map))
    }
    pub(crate) fn get(&self, name: &str) -> Result<&'a [u8]> {
        self.0
            .get(name)
            .copied()
            .with_context(|| format!("missing CHK {name}"))
    }
    pub(crate) fn exact(&self, name: &str, length: usize) -> Result<&'a [u8]> {
        let b = self.get(name)?;
        ensure!(b.len() == length, "invalid CHK {name} length");
        Ok(b)
    }
}
pub(crate) fn short(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
pub(crate) fn word(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("validated record"))
}
pub(crate) fn read_strings(b: &[u8]) -> Result<Vec<String>> {
    ensure!(b.len() >= 2 && b.len() <= 1024 * 1024, "invalid STR size");
    let count = usize::from(short(b, 0));
    ensure!(
        count <= 4096 && b.len() >= 2 + count * 2,
        "invalid STR offset table"
    );
    let mut result = vec![String::new()];
    for i in 0..count {
        let offset = usize::from(short(b, 2 + i * 2));
        let value = b.get(offset..).context("STR offset outside section")?;
        let end = value
            .iter()
            .position(|b| *b == 0)
            .context("unterminated STR string")?;
        ensure!(end <= 8192, "STR string too long");
        result.push(
            std::str::from_utf8(&value[..end])
                .context("mission text is not UTF-8/ASCII")?
                .into(),
        );
    }
    Ok(result)
}
pub(crate) fn read_locations(b: &[u8]) -> Result<BTreeMap<u16, MissionLocation>> {
    ensure!(b.len() == 1280, "expected64 source locations");
    let mut result = BTreeMap::new();
    for (i, r) in b.as_chunks::<20>().0.iter().enumerate() {
        if r.iter().all(|b| *b == 0) {
            continue;
        }
        ensure!(
            matches!(short(r, 18), 0 | 7),
            "unsupported location elevation filtering"
        );
        let v: Vec<i32> = (0..4)
            .map(|i| i32::try_from(word(r, i * 4)).context("location coordinate overflow"))
            .collect::<Result<_>>()?;
        ensure!(v[0] < v[2] && v[1] < v[3], "inverted/empty source location");
        result.insert(
            (i + 1) as u16,
            MissionLocation {
                excluded_elevations: short(r, 18) as u8,
                left: v[0],
                top: v[1],
                right: v[2],
                bottom: v[3],
            },
        );
    }
    Ok(result)
}
pub(crate) fn read_triggers(b: &[u8], briefing: bool) -> Result<Vec<SourceTrigger>> {
    ensure!(
        b.len().is_multiple_of(2400) && b.len() / 2400 <= 256,
        "invalid campaign trigger section"
    );
    let mut result = Vec::new();
    for r in b.as_chunks::<2400>().0 {
        ensure!(word(r, 2368) == 0, "unsupported trigger flags");
        let owners: Vec<_> = r[2372..2399]
            .iter()
            .enumerate()
            .filter(|(_, v)| **v != 0)
            .map(|(i, _)| i as u8)
            .collect();
        ensure!(
            !owners.is_empty() && owners.iter().all(|p| *p <= 21),
            "invalid trigger owners"
        );
        let mut conditions = Vec::new();
        for c in r[..320].as_chunks::<20>().0 {
            if c[15] == 0 {
                break;
            }
            ensure!(
                c[17] == 0 && short(c, 18) == 0,
                "unsupported condition flags"
            );
            conditions.push(SourceCondition {
                location: word(c, 0),
                player: word(c, 4),
                amount: word(c, 8),
                unit: short(c, 12),
                comparison: c[14],
                kind: c[15],
                switch: c[16],
                flags: c[17],
            });
        }
        let mut actions = Vec::new();
        for a in r[320..2368].as_chunks::<32>().0 {
            if a[26] == 0 {
                break;
            }
            ensure!(
                a[28] & !6 == 0 && a[29..32].iter().all(|b| *b == 0),
                "unsupported action flags"
            );
            if a[28] & 2 != 0 {
                continue;
            }
            actions.push(SourceAction {
                location: word(a, 0),
                text: word(a, 4),
                sound: word(a, 8),
                time: word(a, 12),
                player: word(a, 16),
                second: word(a, 20),
                unit: short(a, 24),
                kind: a[26],
                modifier: a[27],
                flags: a[28],
            });
        }
        if briefing {
            ensure!(
                conditions.len() == 1 && conditions[0].kind == 13,
                "invalid briefing marker"
            );
        }
        result.push(SourceTrigger {
            conditions,
            actions,
            owners,
        });
    }
    Ok(result)
}
pub(crate) fn check_defaults(s: &Sections<'_>) -> Result<()> {
    for (name, length, count) in [("UNIS", 4048, 228), ("UPGS", 598, 46), ("TECS", 216, 24)] {
        ensure!(
            s.exact(name, length)?[..count].iter().all(|b| *b == 1),
            "custom campaign {name} values require explicit conversion"
        );
    }
    ensure!(
        s.exact("MASK", 4096)?.iter().all(|b| *b == 255),
        "unsupported initially revealed mission"
    );
    ensure!(
        s.exact("UPUS", 64)?.iter().all(|b| *b == 0),
        "unsupported campaign property slots"
    );
    Ok(())
}
pub(crate) fn read_availability(s: &Sections<'_>) -> Result<Vec<Availability>> {
    let units = s.exact("PUNI", 5700)?;
    let upgrades = s.exact("UPGR", 1748)?;
    let tech = s.exact("PTEC", 912)?;
    let mut result = Vec::new();
    for (player, _) in PLAYER_IDS {
        let p = usize::from(player);
        let enabled_units = (0..228)
            .filter(|&u| {
                if units[2964 + p * 228 + u] != 0 {
                    units[2736 + u] != 0
                } else {
                    units[p * 228 + u] != 0
                }
            })
            .map(|u| u as u16)
            .collect();
        let u = (0..46)
            .map(|i| {
                if upgrades[1196 + p * 46 + i] != 0 {
                    [upgrades[1150 + i], upgrades[1104 + i]]
                } else {
                    [upgrades[552 + p * 46 + i], upgrades[p * 46 + i]]
                }
            })
            .collect();
        let t = (0..24)
            .map(|i| {
                if tech[624 + p * 24 + i] != 0 {
                    [tech[576 + i], tech[600 + i]]
                } else {
                    [tech[p * 24 + i], tech[288 + p * 24 + i]]
                }
            })
            .collect();
        result.push(Availability {
            source_player: player,
            enabled_units,
            upgrades: u,
            technologies: t,
        });
    }
    Ok(result)
}
pub(crate) fn convert_map(parsed: &ParsedMap, terrain: &DecodedTerrain, raw: &[u8]) -> Result<Map> {
    ensure!(raw.len() == parsed.units.len() * 36, "invalid UNIT data");
    let mut map = Map {
        id: "straterust.backwater-station".into(),
        width: i32::from(parsed.width) * 32,
        height: i32::from(parsed.height) * 32,
        players: 4,
        fog_of_war: true,
        spawns: Vec::new(),
        start_locations: Vec::new(),
        resources: Vec::new(),
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: Vec::new(),
        mission: None,
        terrain: Some(Terrain {
            cell_size: 8,
            columns: u32::from(parsed.width) * 4,
            rows: u32::from(parsed.height) * 4,
            flags: terrain.flags.clone(),
        }),
    };
    for (unit, r) in parsed.units.iter().zip(raw.as_chunks::<36>().0) {
        let position = Position {
            x: i32::from(unit.x),
            y: i32::from(unit.y),
        };
        match unit.unit_type {
            176..=178 | 188 => {
                ensure!(unit.owner == 11, "nonneutral resource");
                let gas = unit.unit_type == 188;
                map.resources.push(ResourceSpawn {
                    requires_extractor: gas,
                    footprint: Footprint {
                        width: if gas { 128 } else { 64 },
                        height: if gas { 64 } else { 32 },
                    },
                    kind: if gas { "gas" } else { "minerals" }.into(),
                    position,
                    amount: unit.resource_amount.context("resource quantity missing")?,
                });
            }
            214 => map.start_locations.push(StartLocation {
                player: player_id(u32::from(unit.owner))?,
                position,
            }),
            _ => {
                let valid_states = short(r, 12);
                let valid_fields = short(r, 14);
                let states = short(r, 26) & valid_states;
                ensure!(
                    short(r, 10) == 0 && word(r, 32) == 0,
                    "linked source placements unsupported"
                );
                ensure!(
                    states & !0x12 == 0 && valid_fields & 0x28 == 0,
                    "unsupported UNIT energy/hangar/cloak properties"
                );
                ensure!(
                    valid_fields & 4 == 0 || r[18] == 0,
                    "unsupported UNIT shields"
                );
                map.spawns.push(Spawn {
                    doodad_enabled: None,
                    owner: player_id(u32::from(unit.owner))?,
                    unit_type: unit_id(unit.unit_type)?,
                    position,
                    hp_percent: (valid_fields & 2 != 0).then_some(r[17]),
                    energy_percent: (valid_fields & 8 != 0).then_some(r[19]),
                    invincible: states & 0x10 != 0,
                    cloaked: states & 2 != 0,
                });
            }
        }
    }
    map.start_locations.sort_by_key(|s| s.player);
    Ok(map)
}
