use super::*;

fn release() -> Release {
    Release {
        repository: "example/tools",
        tag: "1",
        filename: "tool",
        sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        bytes: 5,
    }
}

#[test]
fn verified_cache_is_reusable_offline_and_corruption_is_replaced() {
    let temporary = tempfile::tempdir().unwrap();
    let release = release();
    let fetch = |_: &Release, file: &mut File, _: &dyn Fn(&str)| {
        file.write_all(b"hello")?;
        Ok(())
    };
    let cached = cached_release(&release, temporary.path(), &fetch, &|_| {}).unwrap();
    assert_eq!(fs::read(&cached).unwrap(), b"hello");
    let offline = |_: &Release, _: &mut File, _: &dyn Fn(&str)| {
        panic!("verified cached tool must not need internet")
    };
    assert_eq!(
        cached_release(&release, temporary.path(), &offline, &|_| {}).unwrap(),
        cached
    );
    // Corruption of the same length still fails digest verification.
    fs::write(&cached, b"world").unwrap();
    cached_release(&release, temporary.path(), &fetch, &|_| {}).unwrap();
    assert_eq!(fs::read(&cached).unwrap(), b"hello");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&cached).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
}

#[test]
fn failed_download_or_digest_never_publishes_or_leaves_partial_tools() {
    let temporary = tempfile::tempdir().unwrap();
    let release = release();
    let cached = release.cached_path(temporary.path());
    fs::create_dir_all(cached.parent().unwrap()).unwrap();
    fs::write(&cached, b"prior").unwrap();
    let interrupted = |_: &Release, file: &mut File, _: &dyn Fn(&str)| {
        file.write_all(b"hel")?;
        bail!("connection lost")
    };
    let error = cached_release(&release, temporary.path(), &interrupted, &|_| {}).unwrap_err();
    assert!(format!("{error:#}").contains("connection lost"));
    let bad_digest = |_: &Release, file: &mut File, _: &dyn Fn(&str)| {
        file.write_all(b"world")?;
        Ok(())
    };
    let error = cached_release(&release, temporary.path(), &bad_digest, &|_| {}).unwrap_err();
    assert!(error.to_string().contains("checksum mismatch"));
    assert_eq!(fs::read(&cached).unwrap(), b"prior");
    assert_eq!(fs::read_dir(cached.parent().unwrap()).unwrap().count(), 1);
}

#[test]
fn explicit_offline_tools_skip_cache_and_downloads() {
    let temporary = tempfile::tempdir().unwrap();
    let deploy = temporary.path().join("deploy");
    let runtime = temporary.path().join("runtime");
    fs::write(&deploy, "custom tool").unwrap();
    fs::write(&runtime, "custom runtime").unwrap();
    let cache = temporary.path().join("unused cache");
    let tools = prepare_in(
        &cache,
        Some(deploy.clone()),
        Some(runtime.clone()),
        &|_| {},
        &|_, _, _| panic!("explicit tools must not download"),
    )
    .unwrap();
    assert_eq!(tools.deploy, deploy);
    assert_eq!(tools.runtime, Some(runtime));
    assert!(!cache.exists());
}
