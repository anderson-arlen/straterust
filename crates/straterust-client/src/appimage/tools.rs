//! Pinned build tools, cached once per user. No external downloader or FUSE needed.
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};

pub(super) struct Toolchain {
    pub deploy: PathBuf,
    pub runtime: Option<PathBuf>,
}

pub(super) struct Release {
    repository: &'static str,
    tag: &'static str,
    filename: &'static str,
    sha256: &'static str,
    bytes: u64,
}

type Fetch<'a> = dyn Fn(&Release, &mut File, &dyn Fn(&str)) -> Result<()> + 'a;

impl Release {
    fn url(&self) -> String {
        format!(
            "https://github.com/{}/releases/download/{}/{}",
            self.repository, self.tag, self.filename
        )
    }

    fn cached_path(&self, cache: &Path) -> PathBuf {
        cache.join(self.tag).join(self.filename)
    }
}

pub(super) fn prepare(progress: &dyn Fn(&str)) -> Result<Toolchain> {
    prepare_in(
        &cache_directory()?,
        std::env::var_os("STRATERUST_LINUXDEPLOY").map(PathBuf::from),
        std::env::var_os("LDAI_RUNTIME_FILE").map(PathBuf::from),
        progress,
        &download,
    )
}

fn cache_directory() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".cache")))
        .context("cannot locate user cache directory for AppImage tools")?;
    Ok(base.join("straterust/appimage"))
}

pub(super) fn prepare_in(
    cache: &Path,
    deploy: Option<PathBuf>,
    runtime: Option<PathBuf>,
    progress: &dyn Fn(&str),
    fetch: &Fetch<'_>,
) -> Result<Toolchain> {
    let deploy = deploy.map(|p| super::resolve_tool(&p)).transpose()?;
    let runtime = runtime
        .map(|p| {
            p.canonicalize()
                .context("AppImage runtime override does not exist")
        })
        .transpose()?;
    if let (Some(deploy), Some(runtime)) = (&deploy, &runtime) {
        return Ok(Toolchain {
            deploy: deploy.clone(),
            runtime: Some(runtime.clone()),
        });
    }
    let (deploy_release, runtime_release) = releases()?;
    fs::create_dir_all(cache).context("create AppImage tool cache")?;
    // A second client must not use a partially downloaded executable or download
    // the same files again. The OS releases this lock even if the process exits.
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(cache.join("download.lock"))?;
    progress("Preparing AppImage build tools...");
    fs2::FileExt::lock_exclusive(&lock)?;
    let deploy = match deploy {
        Some(path) => path,
        None => cached_release(&deploy_release, cache, fetch, progress)?,
    };
    let runtime = match runtime {
        Some(path) => path,
        None => cached_release(&runtime_release, cache, fetch, progress)?,
    };
    Ok(Toolchain {
        deploy,
        runtime: Some(runtime),
    })
}

fn cached_release(
    release: &Release,
    cache: &Path,
    fetch: &Fetch<'_>,
    progress: &dyn Fn(&str),
) -> Result<PathBuf> {
    let path = release.cached_path(cache);
    if verified(&path, release)? {
        super::executable_mode(&path)?;
        return Ok(path);
    }
    progress(&format!("Downloading {}...", release.filename));
    fs::create_dir_all(path.parent().unwrap())?;
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    fetch(release, temporary.as_file_mut(), progress).with_context(|| {
        format!(
            "download AppImage build tools; internet access is needed on the first export (retry to continue): {}",
            release.url()
        )
    })?;
    ensure!(
        verified(temporary.path(), release)?,
        "AppImage tool checksum mismatch for {}; retry the export",
        release.filename
    );
    super::executable_mode(temporary.path())?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(&path)
        .context("cache verified AppImage tool")?;
    Ok(path)
}

fn verified(path: &Path, release: &Release) -> Result<bool> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).context("read cached AppImage tool"),
    };
    if file.metadata()?.len() != release.bytes {
        return Ok(false);
    }
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()) == release.sha256)
}

pub(super) fn download(release: &Release, file: &mut File, progress: &dyn Fn(&str)) -> Result<()> {
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(300)))
        .timeout_connect(Some(Duration::from_secs(20)))
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent.get(&release.url()).call()?;
    let mut reader = response.body_mut().as_reader();
    let mut buffer = [0; 64 * 1024];
    let mut received = 0;
    let mut reported = 0;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        received += count as u64;
        ensure!(
            received <= release.bytes,
            "AppImage tool download is too large"
        );
        file.write_all(&buffer[..count])?;
        let percent = received * 100 / release.bytes;
        if percent >= reported + 10 {
            progress(&format!("Downloading {}: {percent}%", release.filename));
            reported = percent;
        }
    }
    ensure!(
        received == release.bytes,
        "incomplete AppImage tool download"
    );
    Ok(())
}

fn releases() -> Result<(Release, Release)> {
    // Official GitHub release SHA-256 digests. Updating versions requires
    // updating both digest and length; never execute an unverified download.
    let (deploy, deploy_hash, deploy_size, runtime, runtime_hash, runtime_size) =
        match std::env::consts::ARCH {
            "x86_64" => (
                "linuxdeploy-x86_64.AppImage",
                "c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d",
                19_421_688,
                "runtime-x86_64",
                "2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d",
                944_632,
            ),
            "aarch64" => (
                "linuxdeploy-aarch64.AppImage",
                "620095110d693282b8ebeb244a95b5e911cf8f65f76c88b4b47d16ae6346fcff",
                18_627_080,
                "runtime-aarch64",
                "00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444",
                936_456,
            ),
            other => bail!(
                "automatic AppImage tools are unavailable for {other}; set STRATERUST_LINUXDEPLOY and LDAI_RUNTIME_FILE to matching tools"
            ),
        };
    Ok((
        Release {
            repository: "linuxdeploy/linuxdeploy",
            tag: "1-alpha-20251107-1",
            filename: deploy,
            sha256: deploy_hash,
            bytes: deploy_size,
        },
        Release {
            repository: "AppImage/type2-runtime",
            tag: "20251108",
            filename: runtime,
            sha256: runtime_hash,
            bytes: runtime_size,
        },
    ))
}

#[cfg(test)]
mod tests;
