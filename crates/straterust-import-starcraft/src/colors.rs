//! The source eight-shade team ramps, keyed by converted mission player IDs.
use crate::{Archive, backwater::Sections, formats};
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, io::Cursor};
use straterust_engine::{assets::ColorRemap, sim::PlayerId};

pub(crate) fn players(
    archive: &mut Archive<Cursor<Vec<u8>>>,
    ids: &BTreeMap<u8, PlayerId>,
    sections: Option<&Sections<'_>>,
) -> Result<BTreeMap<PlayerId, ColorRemap>> {
    let ramps = formats::decode_pcx(&archive.read_file("game\\tunit.pcx", 8192)?)?;
    ensure!(
        (ramps.width, ramps.height) == (128, 1),
        "unsupported StarCraft player palette table"
    );
    // Unit GRPs are decoded against Badlands even on other terrain sets. Use
    // that same RGB palette for both source markings and their replacements.
    let palette = formats::palette(&archive.read_file("tileset\\badlands.wpe", 1024)?)?;
    mappings(
        &ramps.pixels,
        &palette,
        ids,
        sections.and_then(|s| s.optional("COLR")),
    )
}

fn mappings(
    ramps: &[u8],
    palette: &[[u8; 4]; 256],
    ids: &BTreeMap<u8, PlayerId>,
    overrides: Option<&[u8]>,
) -> Result<BTreeMap<PlayerId, ColorRemap>> {
    ensure!(ramps.len() == 128, "invalid player palette ramps");
    if let Some(colors) = overrides {
        ensure!(
            colors.len() == 8 && colors.iter().all(|c| *c < 16),
            "invalid CHK COLR player colors"
        );
    }
    ids.iter()
        .map(|(&source, &native)| {
            ensure!(source < 16, "unsupported source player color slot {source}");
            let color = overrides
                .and_then(|colors| colors.get(usize::from(source)))
                .copied()
                .unwrap_or(source);
            let colors = (0..8)
                .map(|shade| {
                    [
                        palette[8 + shade][..3].try_into().unwrap(),
                        palette[usize::from(ramps[usize::from(color) * 8 + shade])][..3]
                            .try_into()
                            .unwrap(),
                    ]
                })
                .collect();
            Ok((native, ColorRemap { colors }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_colors_preserve_source_slots_and_explicit_map_overrides() -> Result<()> {
        let palette = std::array::from_fn(|i| [i as u8, 0, 0, 255]);
        let ramps: Vec<_> = (64..192).collect();
        let ids = BTreeMap::from([(1, PlayerId(0)), (4, PlayerId(1)), (11, PlayerId(2))]);
        let colors = mappings(&ramps, &palette, &ids, None)?;
        assert_eq!(colors[&PlayerId(0)].apply([8, 0, 0]), [72, 0, 0]);
        assert_eq!(colors[&PlayerId(1)].apply([15, 0, 0]), [103, 0, 0]);
        assert_eq!(colors[&PlayerId(2)].apply([8, 0, 0]), [152, 0, 0]);
        assert_eq!(colors[&PlayerId(0)].apply([16, 0, 0]), [16, 0, 0]);
        let colors = mappings(&ramps, &palette, &ids, Some(&[0, 7, 2, 3, 6, 5, 4, 1]))?;
        assert_eq!(colors[&PlayerId(0)].apply([8, 0, 0]), [120, 0, 0]);
        assert_eq!(colors[&PlayerId(1)].apply([8, 0, 0]), [112, 0, 0]);
        assert_eq!(colors[&PlayerId(2)].apply([8, 0, 0]), [152, 0, 0]);
        assert!(mappings(&ramps, &palette, &ids, Some(&[0; 7])).is_err());
        assert!(mappings(&ramps, &palette, &ids, Some(&[16; 8])).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires STRATERUST_SOURCE pointing to the authorized retail disc"]
    fn retail_player_colors_preserve_all_campaign_source_slots() -> Result<()> {
        let path = std::env::var_os("STRATERUST_SOURCE").expect("set STRATERUST_SOURCE");
        let source = crate::Source::open(std::path::Path::new(&path))?;
        let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
        let mut archive =
            Archive::from_bytes(installer.read_file("files\\stardat.mpq", 128 * 1024 * 1024)?)?;
        let original = players(
            &mut archive,
            &(0..16).map(|p| (p, PlayerId(u16::from(p)))).collect(),
            None,
        )?;
        let red = original[&PlayerId(0)].colors[0][1];
        let blue = original[&PlayerId(1)].colors[0][1];
        assert!(red[0] > red[2] && blue[2] > blue[0]);
        for race in ["terran", "zerg", "protoss"] {
            for mission in 1..=10 {
                let number = if race == "terran" && mission >= 7 {
                    mission + 1
                } else {
                    mission
                };
                let chk = installer.read_file(
                    &format!("campaign\\{race}\\{race}{number:02}\\staredit\\scenario.chk"),
                    8 * 1024 * 1024,
                )?;
                let sections = Sections::read(&chk)?;
                let parsed = crate::map_formats::parse_chk(&chk)?;
                let human = parsed.owners.iter().position(|p| *p == 6).unwrap() as u8;
                let ids = crate::campaign::placed_players(&parsed, sections.get("THG2")?, human)?
                    .into_iter()
                    .enumerate()
                    .map(|(native, source)| (source, PlayerId(native as u16)))
                    .collect();
                let converted = players(&mut archive, &ids, Some(&sections))?;
                for (source, native) in ids {
                    assert_eq!(
                        converted[&native].colors,
                        original[&PlayerId(u16::from(source))].colors
                    );
                }
            }
        }
        Ok(())
    }
}
