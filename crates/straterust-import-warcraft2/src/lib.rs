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
mod descriptions;
mod effects;
#[cfg(test)]
mod effects_tests;
#[cfg(test)]
mod feedback_tests;
#[cfg(test)]
mod gameplay_tests;
mod gfx;
mod icons;
mod indicators;
mod media;
mod missions;
mod multiplayer;
mod opponent;
mod presentation;
mod publish;
mod pud;
mod rocks;
#[cfg(test)]
mod rules_tests;
mod source;
mod spell_art;
mod spells;
mod stats;
mod technology;
mod terrain;
mod upgrade;
mod voices;
mod walls;
mod war;

use anyhow::Result;
use std::path::Path;

pub const IMPORTER_BUILD: u32 = 12;

/// Older native campaign imports used a center-based rescue check which could
/// never reach a 4×4 Town Hall. Keep the fix at the importer boundary.
pub fn upgrade_campaign_map(
    rules: &straterust_engine::sim::Rules,
    map: &mut straterust_engine::sim::Map,
) {
    if rules.id == "warcraft2-bne"
        && map.id.starts_with("warcraft2.")
        && let Some(mission) = &mut map.mission
        && mission.rescue_margin.is_none()
    {
        mission.rescue_margin = Some(32);
    }
}

pub fn upgrade_multiplayer_ai(catalog: &mut straterust_engine::multiplayer::Catalog) {
    multiplayer::upgrade_catalog(catalog);
}

pub fn upgrade_installed_maps(
    root: &Path,
    original: Option<&Path>,
    progress: &dyn Fn(&str),
) -> Result<()> {
    multiplayer::upgrade(root, progress)?;
    let source = original
        .map(|path| source::Source::open(path, progress))
        .transpose()?;
    upgrade::definitions(root, source.as_ref(), progress)
}

pub fn import_game(source: &Path, output: &Path, progress: &dyn Fn(&str)) -> Result<()> {
    let mut source = source::Source::open(source, progress)?;
    application_metadata(&mut source, output, progress)?;
    publish::publish(&mut source, output, progress)
}

pub fn update_game_metadata(path: &Path, output: &Path, progress: &dyn Fn(&str)) -> Result<()> {
    let mut source = source::Source::open(path, progress)?;
    application_metadata(&mut source, output, progress)
}

fn application_metadata(
    source: &mut source::Source,
    output: &Path,
    progress: &dyn Fn(&str),
) -> Result<()> {
    progress("Reading original Warcraft II application version and icon");
    let exe = source.application_exe()?;
    let version = straterust_import_formats::pe::windows_version(&exe)?;
    let icon = straterust_import_formats::pe::windows_icon(&exe)?;
    straterust_engine::game::GameInfo::imported(
        output,
        version.product_label.unwrap_or(version.product_version),
        "warcraft2",
        IMPORTER_BUILD,
        straterust_engine::assets::Image {
            width: icon.width,
            height: icon.height,
            rgba: icon.rgba,
        },
    )
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
