//! Correct importer-owned rules inside the installer's atomic replacement.
use anyhow::Result;
use std::{
    fs,
    path::{Path, PathBuf},
};
use straterust_engine::{
    asset_archive,
    content::{SharedDocument, read_ron},
    sim::Rules,
};

/// Refresh native definitions only inside a staged, managed installation.
pub fn upgrade_installed_rules(root: &Path, progress: &dyn Fn(&str)) -> Result<()> {
    fn collect(root: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                collect(&entry.path(), paths)?;
            } else if entry.file_name() == "rules.ron" {
                paths.push(entry.path());
            }
        }
        Ok(())
    }
    let mut paths = Vec::new();
    collect(root, &mut paths)?;
    paths.sort();
    let mut changed = Vec::new();
    for path in paths {
        let mut rules: Rules = read_ron(&path)?;
        if crate::campaign_units::repair_building_links(&mut rules) {
            changed.push((path, ron::ser::to_string(&rules)?));
        }
    }
    if changed.is_empty() {
        return Ok(());
    }
    progress(&format!(
        "Restoring StarCraft building links in {} packages",
        changed.len()
    ));
    let archive = root.join(".updated-rules.sras");
    let mut writer = asset_archive::Writer::create(&archive)?;
    writer.extend(&asset_archive::Reader::open(root)?)?;
    for (path, rules) in changed {
        let reference = SharedDocument {
            shared: writer.insert(rules.as_bytes())?,
        };
        fs::write(path, ron::ser::to_string(&reference)?)?;
    }
    writer.finish()?;
    fs::rename(archive, root.join("shared-assets.sras"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use straterust_engine::{
        content::Package,
        game::GameInfo,
        session::{SavedGame, ServerSession},
        sim::*,
    };

    #[test]
    fn armory_upgrade_keeps_shared_storage_and_older_saves_and_is_idempotent() {
        let original =
            Package::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"))
                .unwrap()
                .world(42)
                .unwrap();
        let mut rules = original.rules().clone();
        for id in [32, 78] {
            rules.units.push(UnitType {
                id: UnitTypeId(id),
                structure: true,
                speed: 0,
                max_hp: 100,
                ..Default::default()
            });
        }
        let older = World::new(rules.clone(), original.map().clone(), 42).unwrap();
        let session = ServerSession::new(older.clone(), 42, vec![PlayerId(0)]).unwrap();
        let save = SavedGame::capture(&session).unwrap().encode().unwrap();
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("game.ron"),
            ron::to_string(&GameInfo {
                schema_version: 1,
                version: "1.0".into(),
                importer: None,
                icon: None,
                asset_archive: Some("shared-assets.sras".into()),
            })
            .unwrap(),
        )
        .unwrap();
        let archive = root.path().join("shared-assets.sras");
        let mut writer = asset_archive::Writer::create(&archive).unwrap();
        let reference = SharedDocument {
            shared: writer
                .insert(ron::to_string(&rules).unwrap().as_bytes())
                .unwrap(),
        };
        writer.finish().unwrap();
        fs::write(
            root.path().join("rules.ron"),
            ron::to_string(&reference).unwrap(),
        )
        .unwrap();
        upgrade_installed_rules(root.path(), &|_| {}).unwrap();
        let fixed: Rules = read_ron(&root.path().join("rules.ron")).unwrap();
        let worker = fixed.units.iter().find(|u| u.id == UnitTypeId(2)).unwrap();
        assert!(worker.builds.contains(&UnitTypeId(78)));
        assert_eq!(
            fixed
                .units
                .iter()
                .find(|u| u.id == UnitTypeId(78))
                .unwrap()
                .prerequisites,
            [UnitTypeId(32)]
        );
        let _: SharedDocument =
            ron::from_str(&fs::read_to_string(root.path().join("rules.ron")).unwrap()).unwrap();
        let before = fs::read(&archive).unwrap();
        upgrade_installed_rules(root.path(), &|_| {}).unwrap();
        assert_eq!(fs::read(&archive).unwrap(), before);
        let current = World::new(fixed, original.map().clone(), 42).unwrap();
        let restored =
            ServerSession::restore_saved(&current, SavedGame::decode(&save).unwrap()).unwrap();
        assert_eq!(restored.world().state().entities, older.state().entities);
        assert_eq!(
            restored.world().resource_balance(PlayerId(0), "minerals"),
            older.resource_balance(PlayerId(0), "minerals")
        );
    }
}
