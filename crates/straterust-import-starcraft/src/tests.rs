use super::*;

fn fixture_files() -> Files {
    let tile = Image {
        width: 1,
        height: 1,
        rgba: vec![40, 90, 70, 255],
    };
    native_files(
        &tile,
        &[
            tile.clone(),
            Image {
                rgba: vec![255, 0, 0, 0],
                ..tile.clone()
            },
        ],
    )
    .unwrap()
}
#[test]
#[ignore = "requires the owner's private retail source ISO"]
fn original_first_five_campaign_packages_validate_and_run() -> Result<()> {
    let source = std::env::var_os("STRATERUST_SOURCE").context("set STRATERUST_SOURCE")?;
    let path = Path::new(&source);
    let payload = inspect(path)?;
    let root = tempfile::tempdir()?;
    for number in 1..=5 {
        let files = campaign::convert(&payload, path, number)
            .with_context(|| format!("convert mission {number}"))?;
        let output = root.path().join(format!("terran{number:02}"));
        publish(&output, &files).with_context(|| format!("publish mission {number}"))?;
        let mut world = Package::load(&output)?.world(42)?;
        for _ in 0..1600 {
            world.step(&[])?;
        }
        println!(
            "mission={number} entities={} triggers={} ai={:?} hash={}",
            world.state().entities.len(),
            world.state().mission.as_ref().unwrap().triggers.len(),
            world
                .state()
                .ai
                .iter()
                .map(|a| (a.active, a.instruction, a.accepted_orders, a.deployed.len()))
                .collect::<Vec<_>>(),
            world.state_hash()
        );
    }
    Ok(())
}

#[test]
fn imports_are_repeatable_and_failures_preserve_existing_package() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("package");
    let files = fixture_files();
    assert!(publish(&output, &files).unwrap());
    assert!(!publish(&output, &files).unwrap());
    let original = fs::read(output.join("terrain.srim")).unwrap();
    let mut invalid = files.clone();
    invalid.insert("terrain.srim".into(), b"broken".to_vec());
    assert!(
        publish(&output, &invalid)
            .unwrap_err()
            .to_string()
            .contains("staged presentation")
    );
    assert_eq!(fs::read(output.join("terrain.srim")).unwrap(), original);
    let missing = root.path().join("failed");
    assert!(publish(&missing, &invalid).is_err());
    assert!(!missing.exists());
    let mut different = files.clone();
    different.insert("extra.txt".into(), b"valid but different".to_vec());
    assert!(
        publish(&output, &different)
            .unwrap_err()
            .to_string()
            .contains("existing output differs")
    );
    assert_eq!(
        fs::read_dir(root.path()).unwrap().count(),
        1,
        "no abandoned staging directories"
    );
    assert_eq!(fs::read(output.join("terrain.srim")).unwrap(), original);
}

#[test]
fn native_package_is_independent_of_source_and_cosmetics_do_not_change_gameplay() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("package");
    publish(&output, &fixture_files()).unwrap();
    let native = Package::load(&output).unwrap().world(42).unwrap();
    let fixture =
        Package::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures"))
            .unwrap()
            .world(42)
            .unwrap();
    assert_eq!(native.state_hash(), fixture.state_hash());
    assert_eq!(AssetPack::load(&output).unwrap().unwrap().frames.len(), 2);
    fs::write(output.join("assets.ron"), "broken cosmetics").unwrap();
    assert!(AssetPack::load(&output).is_err());
    assert_eq!(
        Package::load(&output)
            .unwrap()
            .world(42)
            .unwrap()
            .state_hash(),
        fixture.state_hash()
    );
}
