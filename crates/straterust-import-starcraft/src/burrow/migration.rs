//! Adapt older private imports at the importer boundary. Legacy StarCraft
//! fields do not become aliases or extra gameplay state in the engine.
use super::*;
use std::{fs, path::Path};
use straterust_engine::sim::{Rules, UnitTypeId};

#[derive(Deserialize)]
struct LegacyRules {
    units: Vec<LegacyUnit>,
}
#[derive(Deserialize)]
struct LegacyUnit {
    id: UnitTypeId,
    #[serde(default)]
    unburrow_ticks: u32,
}

pub(crate) fn upgrade(directory: &Path) -> Result<()> {
    let path = directory.join("rules.ron");
    let original = fs::read_to_string(&path)?;
    let legacy: LegacyRules = ron::from_str(&original)?;
    let text = rename_tokens(&strip_duration(&original));
    let mut rules: Rules = ron::from_str(&text)?;
    for old in legacy.units.into_iter().filter(|u| u.unburrow_ticks != 0) {
        let unit = rules
            .units
            .iter_mut()
            .find(|u| u.id == old.id)
            .context("missing legacy unit")?;
        unit.cloak = Some(super::rules(old.unburrow_ticks));
    }
    let map_path = directory.join("map.ron");
    let original_map = fs::read_to_string(&map_path)?;
    let map = rename_tokens(&original_map);
    let _: straterust_engine::sim::Map = ron::from_str(&map)?;
    let assets_path = directory.join("assets.ron");
    let original_assets = fs::read_to_string(&assets_path)?;
    let assets = rename_tokens(&original_assets);
    let manifest: AssetManifest = ron::from_str(&assets)?;
    manifest.validate()?;
    // Validate every adapted manifest before writing. Each file can need an
    // upgrade independently, including after an interrupted previous refresh.
    if text != original {
        fs::write(path, ron_bytes(&rules)?)?;
    }
    if map != original_map {
        fs::write(map_path, map)?;
    }
    if assets != original_assets {
        fs::write(assets_path, assets)?;
    }
    Ok(())
}

/// Change identifiers, not quoted names/paths or comments. RON formatting may
/// omit spaces after colons, so whole-line/string replacement is insufficient.
fn rename_tokens(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let (mut quoted, mut escaped, mut comment) = (false, false, false);
    let mut block_depth = 0;
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        if !quoted && !comment {
            let delimiter = if rest.starts_with("/*") {
                block_depth += 1;
                Some("/*")
            } else if block_depth > 0 && rest.starts_with("*/") {
                block_depth -= 1;
                Some("*/")
            } else {
                None
            };
            if let Some(delimiter) = delimiter {
                output.push_str(delimiter);
                at += 2;
                continue;
            }
            if block_depth == 0 && rest.starts_with("//") {
                comment = true;
            }
        }
        let c = rest.chars().next().unwrap();
        if !quoted && !comment && block_depth == 0 && (c.is_ascii_alphabetic() || c == '_') {
            let end = rest
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(rest.len());
            output.push_str(match &rest[..end] {
                "Burrow" => "Conceal",
                "Unburrow" => "Reveal",
                "burrowed" => "cloaked",
                "burrow_ticks" => "conceal_ticks",
                "unburrow_ticks" => "reveal_ticks",
                token => token,
            });
            at += end;
            continue;
        }
        output.push(c);
        at += c.len_utf8();
        if comment {
            comment = c != '\n';
        } else if block_depth > 0 {
            continue;
        } else if escaped {
            escaped = false;
        } else if quoted && c == '\\' {
            escaped = true;
        } else if c == '"' {
            quoted = !quoted;
        }
    }
    output
}

/// Unit records are at depth two in native rules. Nested deployment timings
/// stay intact and are renamed separately, without affecting other fields.
fn strip_duration(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let (mut depth, mut quoted, mut escaped, mut comment) = (0, false, false, false);
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let c = rest.chars().next().unwrap();
        if !quoted && !comment && rest.starts_with("//") {
            comment = true;
        }
        if !quoted
            && !comment
            && depth == 2
            && rest
                .strip_prefix("unburrow_ticks")
                .is_some_and(|tail| tail.trim_start().starts_with(':'))
        {
            let end = rest.find([',', ')']).expect("validated scalar field");
            at += end + usize::from(rest.as_bytes()[end] == b',');
            continue;
        }
        output.push(c);
        at += c.len_utf8();
        if comment {
            if c == '\n' {
                comment = false;
            }
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && c == '\\' {
            escaped = true;
            continue;
        }
        if c == '"' {
            quoted = !quoted;
        }
        if !quoted {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use straterust_engine::assets::{ClipFrame, ClipKind, SpriteClip};

    fn fixture() -> tempfile::TempDir {
        let image = crate::Image {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 255],
        };
        let files = crate::native_files(&image, std::slice::from_ref(&image)).unwrap();
        let directory = tempfile::tempdir().unwrap();
        for (name, bytes) in files {
            fs::write(directory.path().join(name), bytes).unwrap();
        }
        directory
    }

    #[test]
    fn updated_rules_do_not_skip_legacy_map_and_compact_animation_clips() {
        let directory = fixture();
        let path = directory.path();
        let original_rules = fs::read(path.join("rules.ron")).unwrap();
        let mut map: straterust_engine::sim::Map =
            ron::de::from_bytes(&fs::read(path.join("map.ron")).unwrap()).unwrap();
        map.spawns[0].cloaked = true;
        fs::write(
            path.join("map.ron"),
            ron::ser::to_string(&map)
                .unwrap()
                .replace("cloaked:", "burrowed:"),
        )
        .unwrap();
        let mut assets: AssetManifest =
            ron::de::from_bytes(&fs::read(path.join("assets.ron")).unwrap()).unwrap();
        assets.unit_name = "Burrow".into();
        assets.clips = [ClipKind::Conceal, ClipKind::Reveal]
            .into_iter()
            .map(|kind| SpriteClip {
                kind,
                directions: 1,
                frame_ms: 42,
                frames: vec![ClipFrame {
                    frame: 0,
                    flip_x: false,
                    offset: [0, 0],
                }],
                key_steps: Vec::new(),
            })
            .collect();
        fs::write(
            path.join("assets.ron"),
            ron::ser::to_string(&assets)
                .unwrap()
                .replace("kind:Conceal", "kind:Burrow")
                .replace("kind:Reveal", "kind:Unburrow"),
        )
        .unwrap();
        upgrade(path).unwrap();
        let world = crate::Package::load(path).unwrap().world(42).unwrap();
        let assets = crate::AssetPack::load(path).unwrap().unwrap();
        assets.validate_for_world(&world).unwrap();
        assert_eq!(assets.manifest.clips[0].kind, ClipKind::Conceal);
        assert_eq!(assets.manifest.clips[1].kind, ClipKind::Reveal);
        assert_eq!(assets.manifest.unit_name, "Burrow");
        assert!(world.map().spawns[0].cloaked);
        assert_eq!(fs::read(path.join("rules.ron")).unwrap(), original_rules);
        let files: Vec<_> = ["rules.ron", "map.ron", "assets.ron"]
            .into_iter()
            .map(|name| (name, fs::read(path.join(name)).unwrap()))
            .collect();
        upgrade(path).unwrap();
        for (name, bytes) in files {
            assert_eq!(fs::read(path.join(name)).unwrap(), bytes);
        }
    }

    #[test]
    fn legacy_rules_preserve_stationary_concealment_and_failed_upgrade_writes_nothing() {
        let directory = fixture();
        let path = directory.path();
        let mut rules: Rules =
            ron::de::from_bytes(&fs::read(path.join("rules.ron")).unwrap()).unwrap();
        rules.units[0].cloak = None;
        let text =
            ron::ser::to_string(&rules)
                .unwrap()
                .replacen("id:1,", "id:1,unburrow_ticks :7,", 1);
        fs::write(path.join("rules.ron"), &text).unwrap();
        let original_assets = fs::read(path.join("assets.ron")).unwrap();
        fs::write(path.join("assets.ron"), "invalid").unwrap();
        assert!(upgrade(path).is_err());
        assert_eq!(fs::read_to_string(path.join("rules.ron")).unwrap(), text);
        fs::write(path.join("assets.ron"), original_assets).unwrap();
        upgrade(path).unwrap();
        let rules: Rules = ron::de::from_bytes(&fs::read(path.join("rules.ron")).unwrap()).unwrap();
        let ability = rules.units[0].cloak.as_ref().unwrap();
        assert!(!ability.can_move && !ability.can_attack && !ability.blocks_movement);
        assert_eq!(ability.reveal_ticks, 7);
    }

    #[test]
    fn legacy_identifiers_preserve_quoted_text_comments_and_longer_names() {
        let text = "(name:\"Burrow \\\"burrowed\\\"\",kind:Burrow,other:BurrowLong,\n// Unburrow\n/* burrowed /* Burrow */ Unburrow */kind:Unburrow)";
        assert_eq!(
            rename_tokens(text),
            "(name:\"Burrow \\\"burrowed\\\"\",kind:Conceal,other:BurrowLong,\n// Unburrow\n/* burrowed /* Burrow */ Unburrow */kind:Reveal)"
        );
    }

    #[test]
    fn legacy_duration_removal_preserves_nested_deployment_and_quoted_text() {
        let text = r#"(id:"unburrow_ticks: 8,",units:[(id:6,unburrow_ticks:7,
            mine:Some((unburrow_ticks:3,burrow_ticks:4))),
            (id:7,unburrow_ticks:7)],tick_ms:42)"#;
        let actual = strip_duration(text);
        assert_eq!(actual.matches("unburrow_ticks:").count(), 2);
        assert!(actual.contains("mine:Some((unburrow_ticks:3,burrow_ticks:4))"));
        assert!(actual.contains("(id:7,)"));
    }
}
