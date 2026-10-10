//! Warcraft II Battle.net Edition data conversion; original formats never
//! enter the engine or client runtime.
mod animation;
mod art;
mod availability;
#[cfg(test)]
mod campaign_tests;
mod colors;
mod console;
mod construction;
mod effects;
#[cfg(test)]
mod feedback_tests;
#[cfg(test)]
mod gameplay_tests;
mod gfx;
mod icons;
mod indicators;
mod media;
mod missions;
mod opponent;
mod presentation;
mod publish;
mod pud;
mod source;
mod spell_art;
mod spells;
mod stats;
mod technology;
mod terrain;
mod voices;
mod walls;
mod war;

use anyhow::Result;
use std::path::Path;

pub fn import_game(source: &Path, output: &Path, progress: &dyn Fn(&str)) -> Result<()> {
    let mut source = source::Source::open(source, progress)?;
    publish::publish(&mut source, output, progress)
}

pub fn inspect(path: &Path) -> Result<()> {
    let source = source::Source::open(path, &|s| println!("{s}"))?;
    let rules = stats::rules(&source.main.entry(472)?)?;
    println!(
        "Warcraft II Battle.net Edition: 52 campaign maps, {} unit definitions",
        rules.units.len()
    );
    for unit in &rules.units {
        let source_id = usize::from(unit.id.0 - 1);
        if let Some(index) = art::graphic(source_id, 0) {
            let bytes = source.main.entry(index)?;
            println!(
                "unit {source_id}: graphic {index}, {} frames, {}x{}",
                pud::word(&bytes, 0)?,
                pud::word(&bytes, 2)?,
                pud::word(&bytes, 4)?
            );
        }
    }
    Ok(())
}
