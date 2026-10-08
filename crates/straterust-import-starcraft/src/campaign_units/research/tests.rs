use super::*;

#[test]
#[ignore = "requires the owner's locally extracted retail executable"]
fn all_three_races_building_research_rows_match_retail_executable() -> Result<()> {
    let exe = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../local/source/starcraft/files_starcraft.exe"
    ))?;
    let mut upgrades = BTreeSet::new();
    let mut technologies = BTreeSet::new();
    for source in 106..=175 {
        let directory = 0xe5cf0 + source * 12;
        let count = dword(&exe, directory) as usize;
        let pointer = dword(&exe, directory + 4) as usize;
        if count == 0 {
            continue;
        }
        let offset = pointer
            .checked_sub(0x402200)
            .context("unexpected source button address")?;
        for row in 0..count {
            let at = offset + row * 20;
            let slot = (word(&exe, at) - 1) as u8;
            let action = dword(&exe, at + 8);
            let operand = usize::from(word(&exe, at + 14));
            match action {
                0x473090 => {
                    let facility = match operand {
                        24..=26 => 132,
                        4 | 12 => 141,
                        _ => source as u16,
                    };
                    upgrades.insert((operand, facility, slot));
                }
                0x472db0 => {
                    let facility = if operand == 11 { 131 } else { source as u16 };
                    technologies.insert((operand, facility, slot));
                }
                _ => {}
            }
        }
    }
    assert_eq!(upgrades, catalog::UPGRADES.iter().copied().collect());
    assert_eq!(
        technologies,
        catalog::TECHNOLOGIES
            .iter()
            .map(|&(s, f, p, _)| (s, f, p))
            .collect()
    );
    assert_eq!(
        upgrades.len() + technologies.len(),
        faction_research::SOURCES.len()
    );
    Ok(())
}
