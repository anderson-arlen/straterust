use super::*;

#[test]
#[cfg(unix)]
fn shared_payloads_are_copied_once_without_linking_back_to_the_installation() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    let target = temporary.path().join("snapshot");
    fs::create_dir_all(source.join("maps/one")).unwrap();
    fs::create_dir_all(source.join("maps/two")).unwrap();
    let original = source.join("maps/one/audio.wav");
    fs::write(&original, "shared audio").unwrap();
    fs::hard_link(&original, source.join("maps/two/audio.wav")).unwrap();
    let messages = std::cell::RefCell::new(Vec::new());
    copy_game(&source, &target, &|m| {
        messages.borrow_mut().push(m.to_owned())
    })
    .unwrap();
    let digest = blake3::hash(b"shared audio").to_hex().to_string();
    let one = asset_archive::Reader::open(&target.join("maps/one")).unwrap();
    let two = one.for_directory(&target.join("maps/two")).unwrap();
    assert!(!target.join("maps/one/audio.wav").exists());
    assert!(!target.join("maps/two/audio.wav").exists());
    assert_eq!(
        one.location("audio.wav", &digest).unwrap().offset,
        two.location("audio.wav", &digest).unwrap().offset
    );
    fs::write(&original, "updated import").unwrap();
    assert_eq!(
        one.read("audio.wav", &digest, 100).unwrap(),
        b"shared audio"
    );
    assert!(messages.borrow().iter().any(|m| m.contains("2 / 2 (100%)")));
    assert!(
        !messages
            .borrow()
            .iter()
            .any(|m| m.starts_with("Validating"))
    );
}

#[test]
fn validation_progress_counts_packages_and_disambiguates_matching_folder_names() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo");
    copy_game(&fixture, &source.join("human/mission01"), &|_| {}).unwrap();
    copy_game(&fixture, &source.join("orc/mission01"), &|_| {}).unwrap();
    let messages = std::cell::RefCell::new(Vec::new());
    copy_game(&source, &temporary.path().join("snapshot"), &|m| {
        messages.borrow_mut().push(m.to_owned())
    })
    .unwrap();
    let messages = messages.borrow();
    let validation: Vec<_> = messages
        .iter()
        .filter(|m| m.starts_with("Validating"))
        .collect();
    assert_eq!(validation.len(), 2);
    assert!(validation[0].contains("1 / 2: human/mission01"));
    assert!(validation[1].contains("2 / 2: orc/mission01"));
}

#[test]
fn staged_updates_repair_old_definitions_before_validation_and_reject_invalid_results() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo");
    copy_game(&fixture, &source, &|_| {}).unwrap();
    let manifest = fs::read(source.join("manifest.ron")).unwrap();
    fs::write(
        source.join("manifest.ron"),
        "(schema_version:999,id:\"old\")",
    )
    .unwrap();
    let original = fs::read(source.join("manifest.ron")).unwrap();
    let target = temporary.path().join("updated");
    copy_game_with_update(&source, &target, &|_| {}, |staged| {
        fs::write(staged.join("manifest.ron"), &manifest)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(fs::read(target.join("manifest.ron")).unwrap(), manifest);
    assert_eq!(fs::read(source.join("manifest.ron")).unwrap(), original);
    assert!(
        copy_game_with_update(
            &source,
            &temporary.path().join("invalid"),
            &|_| {},
            |staged| {
                fs::write(
                    staged.join("manifest.ron"),
                    "(schema_version:999,id:\"broken\")",
                )?;
                Ok(())
            }
        )
        .is_err()
    );
}
