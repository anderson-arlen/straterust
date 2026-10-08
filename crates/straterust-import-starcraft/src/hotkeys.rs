//! Retail button rows reference one-based stat_txt.tbl strings. Their first byte
//! is the shortcut; following bytes contain display text/color codes. Original
//! formats and unit IDs stop here, never in the client. See executable button
//! rows at file offsets 0xe4e60 (SCV), 0xe4f18/0xe4fd0 (build), 0xe54b8..0xe5cc0
//! (Terran facilities), and 0xe37d8 (common commands).
use crate::{Archive, Files, campaign_units};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Seek},
    path::Path,
};
use straterust_engine::sim::{ResearchId, UnitTypeId};

const TRAIN: &[(u16, u32)] = &[
    (0, 586),
    (1, 587),
    (7, 592),
    (32, 588),
    (2, 589),
    (3, 590),
    (5, 591),
    (8, 593),
    (9, 594),
    (11, 595),
    (12, 596),
    (14, 597),
    (37, 575),
    (38, 576),
    (41, 578),
    (42, 579),
    (43, 580),
    (39, 577),
    (50, 585),
    (132, 624),
    (133, 625),
    (137, 626),
    (144, 627),
    (146, 628),
    (44, 581),
    (45, 582),
    (46, 583),
    (47, 584),
    (64, 599),
    (65, 600),
    (66, 601),
    (67, 602),
    (69, 603),
    (70, 604),
    (71, 605),
    (72, 606),
    (73, 607),
    (83, 608),
    (85, 609),
];
const RESEARCH: &[(u16, u32)] = &[
    (1, 463),
    (2, 456),
    (3, 472),
    (4, 324),
    (5, 332),
    (6, 478),
    (7, 473),
    (8, 327),
    (9, 364),
    (10, 480),
    (11, 482),
    (12, 483),
    (13, 485),
    (14, 486),
    (15, 489),
    (16, 490),
    (17, 493),
    (18, 326),
    (19, 330),
    (20, 475),
    (21, 328),
];
const COMMAND: &[(&str, u32)] = &[
    ("move", 664),
    ("stop", 665),
    ("attack", 666),
    ("patrol", 667),
    ("hold", 668),
    ("lift", 670),
    ("land", 671),
    ("rally", 672),
    ("gather", 675),
    ("repair", 677),
    ("build", 678),
    ("advanced-build", 679),
    ("unload", 684),
    ("back", 688),
    ("cancel", 693),
    ("stim", 334),
    ("mine", 336),
    ("scan", 337),
];
const BUILD: &[(u16, bool, u8, u32)] = &[
    (106, false, 0, 646),
    (109, false, 1, 647),
    (110, false, 2, 648),
    (111, false, 3, 649),
    (122, false, 4, 650),
    (124, false, 5, 651),
    (112, false, 6, 652),
    (125, false, 7, 653),
    (113, true, 0, 654),
    (114, true, 1, 655),
    (116, true, 2, 656),
    (123, true, 3, 657),
    (131, false, 0, 613),
    (143, false, 1, 614),
    (149, false, 2, 615),
    (142, false, 3, 616),
    (139, false, 4, 617),
    (135, false, 5, 618),
    (141, true, 1, 620),
    (134, true, 0, 619),
    (138, true, 2, 621),
    (140, true, 3, 622),
    (136, true, 4, 623),
    (154, false, 0, 630),
    (156, false, 1, 631),
    (157, false, 2, 632),
    (160, false, 3, 633),
    (166, false, 4, 634),
    (162, false, 5, 635),
    (164, false, 6, 636),
    (172, false, 7, 637),
    (155, true, 0, 638),
    (159, true, 1, 639),
    (163, true, 2, 640),
    (165, true, 3, 641),
    (167, true, 4, 642),
    (170, true, 5, 643),
    (169, true, 6, 644),
    (171, true, 7, 645),
];
const ADDON: &[(u16, u8, u32)] = &[
    (107, 6, 658),
    (108, 7, 659),
    (115, 6, 660),
    (117, 6, 661),
    (118, 7, 662),
    (120, 6, 663),
];

#[derive(Default, Deserialize)]
struct Keys {
    #[serde(default)]
    train_keys: BTreeMap<UnitTypeId, String>,
    #[serde(default)]
    research_keys: BTreeMap<ResearchId, String>,
    #[serde(default)]
    command_keys: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct BuildButton {
    advanced: bool,
    slot: u8,
    key: String,
}

pub(crate) fn refresh<R: Read + Seek>(archive: &mut Archive<R>, files: &mut Files) -> Result<()> {
    let table = archive.read_file("rez\\stat_txt.tbl", 65536)?;
    apply(files, &table)
}

/// Control data can be upgraded even when an older package's artwork cannot
/// pass today's effect conversion. Validate every generated file before writes.
pub(crate) fn update(source_path: &Path, output: &Path) -> Result<()> {
    let source = crate::Source::open(source_path)?;
    let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
    let mut archive =
        Archive::from_bytes(installer.read_file("files\\stardat.mpq", 128 * 1024 * 1024)?)?;
    let table = archive.read_file("rez\\stat_txt.tbl", 65536)?;
    let directories = crate::campaign::package_directories(output)?;
    let mut pending = Vec::new();
    for directory in directories {
        let path = directory.join("presentation.ron");
        let bytes = fs::read(&path)?;
        ensure!(
            bytes.len() <= 1024 * 1024,
            "presentation exceeds hotkey refresh limit"
        );
        let mut files = Files::from([("presentation.ron".into(), bytes)]);
        apply(&mut files, &table)?;
        pending.push((path, files.remove("presentation.ron").unwrap()));
    }
    for (path, bytes) in pending {
        fs::write(&path, bytes)?;
        println!("Updated game hotkeys: {}", path.display());
    }
    Ok(())
}

fn apply(files: &mut Files, table: &[u8]) -> Result<()> {
    let bytes = files
        .get("presentation.ron")
        .context("missing command presentation")?;
    let mut text = std::str::from_utf8(bytes)?.to_owned();
    let mut keys: Keys = ron::from_str(&text)?;
    for &(source, string) in TRAIN {
        if let Some(id) = campaign_units::native_id(source) {
            keys.train_keys.insert(id, hotkey(table, string)?);
        }
    }
    for &(id, string) in RESEARCH {
        keys.research_keys
            .insert(ResearchId(id), hotkey(table, string)?);
    }
    for &(name, string) in COMMAND {
        keys.command_keys
            .insert(name.into(), hotkey(table, string)?);
    }
    let mut builds = BTreeMap::new();
    for &(source, slot, string) in ADDON {
        if let Some(id) = campaign_units::native_id(source) {
            let key = hotkey(table, string)?;
            keys.command_keys
                .insert(format!("build.{}", id.0), key.clone());
            builds.insert(
                id,
                BuildButton {
                    advanced: false,
                    slot,
                    key,
                },
            );
        }
    }
    for &(source, advanced, slot, string) in BUILD {
        if let Some(id) = campaign_units::native_id(source) {
            builds.insert(
                id,
                BuildButton {
                    advanced,
                    slot,
                    key: hotkey(table, string)?,
                },
            );
        }
    }
    for (name, value) in [
        ("train_keys", ron::ser::to_string(&keys.train_keys)?),
        ("research_keys", ron::ser::to_string(&keys.research_keys)?),
        ("command_keys", ron::ser::to_string(&keys.command_keys)?),
        ("build_buttons", ron::ser::to_string(&builds)?),
    ] {
        campaign_units::set_map(&mut text, name, &value)?;
    }
    let _: Keys = ron::from_str(&text).context("validate refreshed hotkey presentation")?;
    files.insert("presentation.ron".into(), text.into_bytes());
    Ok(())
}

fn hotkey(table: &[u8], index: u32) -> Result<String> {
    ensure!(
        (2..=65536).contains(&table.len()),
        "invalid hotkey table length"
    );
    let count = usize::from(u16::from_le_bytes(table[..2].try_into()?));
    let index = usize::try_from(index)?;
    let header = 2 + count * 2;
    ensure!(
        header <= table.len() && (1..=count).contains(&index),
        "invalid hotkey string reference"
    );
    let offset = usize::from(u16::from_le_bytes(
        table[index * 2..index * 2 + 2].try_into()?,
    ));
    ensure!(
        offset >= header && offset < table.len(),
        "invalid hotkey string offset"
    );
    let tail = &table[offset..];
    ensure!(tail.contains(&0), "unterminated hotkey string");
    Ok(match tail[0] {
        0x1b => "Esc".into(),
        byte if byte.is_ascii_alphabetic() => char::from(byte.to_ascii_uppercase()).to_string(),
        _ => anyhow::bail!("unsupported shortcut in stat_txt.tbl string {index}"),
    })
}

#[cfg(test)]
mod tests;
