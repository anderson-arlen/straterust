use std::{path::Path, process::Command};

fn runner() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_straterust-headless"));
    command
        .arg("--package")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures"));
    command.env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
    command
}

#[test]
fn independent_headless_processes_produce_identical_hashes() {
    let a = runner().output().unwrap();
    let b = runner().output().unwrap();
    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stderr));
    assert!(b.status.success());
    assert_eq!(a.stdout, b.stdout);
    let changed = runner().args(["--seed", "43"]).output().unwrap();
    assert!(changed.status.success());
    assert_ne!(a.stdout, changed.stdout);
}

#[test]
fn bad_arguments_and_hash_mismatches_have_actionable_errors() {
    let missing = runner()
        .args(["--scenario", "/does-not-exist.ron"])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("cannot open"));
    let invalid = runner().args(["--hash-every", "0"]).output().unwrap();
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("positive"));
    let mismatch = runner()
        .args(["--expect-hash", "incorrect"])
        .output()
        .unwrap();
    assert!(!mismatch.status.success());
    let error = String::from_utf8_lossy(&mismatch.stderr);
    assert!(
        error.contains("tick 240")
            && error.contains("expected incorrect")
            && error.contains("--dump-state")
    );
}
