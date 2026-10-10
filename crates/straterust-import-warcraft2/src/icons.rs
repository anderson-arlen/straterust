//! Convert the original tileset-specific command and portrait icon sheet.
use super::{gfx, presentation, spells, war::WarArchive};
use anyhow::{Context, Result};
use std::path::Path;
use straterust_engine::{assets::*, sim::*};

pub fn add(
    archive: &WarArchive,
    root: &Path,
    palette: &gfx::Palette,
    era: usize,
    rules: &Rules,
    assets: &mut AssetManifest,
) -> Result<()> {
    let sheet = gfx::sprites(&archive.entry([356, 357, 358, 471][era])?, palette)?;
    let mut add = |key: String, index: usize| -> Result<()> {
        let image = sheet
            .get(index)
            .with_context(|| format!("missing source icon {index}"))?;
        assets.ui.push(UiImageManifest {
            key,
            image: gfx::write_image(root, image)?,
        });
        Ok(())
    };
    for unit in &rules.units {
        add(
            format!("unit.{}", unit.id.0),
            presentation::unit_icon(usize::from(unit.id.0 - 1)),
        )?;
    }
    let spells = spells::definitions();
    for spell in &spells {
        add(
            format!("ability.{}", spell.id.0),
            presentation::spell_icon(spell.id.0),
        )?;
    }
    for research in &rules.research {
        let ordinal = usize::from(research.id.0);
        let race = usize::from(ordinal > 64);
        let ordinal = ordinal - race * 64;
        let index = if ordinal <= 17 {
            let choices = if race == 0 {
                [
                    117, 118, 165, 166, 125, 126, 145, 146, 154, 155, 140, 141, 6, 132, 133, 134,
                    10,
                ]
            } else {
                [
                    120, 121, 168, 169, 128, 129, 148, 149, 151, 152, 138, 139, 7, 135, 136, 137,
                    11,
                ]
            };
            choices[ordinal - 1]
        } else {
            presentation::spell_icon(
                spells
                    .iter()
                    .find(|s| s.research.is_some_and(|r| r.0 == research.id))
                    .context("research has no source icon")?
                    .id
                    .0,
            )
        };
        add(format!("research.{}", research.id.0), index)?;
    }
    for (name, index) in [
        ("move", 83),
        // Warcraft II has no native rally button. Use its destination arrow
        // rather than leaving the engine's supported rally command blank.
        ("rally", 83),
        ("repair", 85),
        ("gather", 86),
        ("build", 87),
        ("advanced-build", 88),
        ("cancel", 91),
        ("back", 91),
        ("unload", 162),
        ("patrol", 178),
        ("hold", 180),
        ("attack", 182),
        ("stop", 91),
    ] {
        add(format!("command.{name}"), index)?;
    }
    Ok(())
}
