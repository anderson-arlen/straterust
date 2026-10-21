//! Diagnostics for ordinary desktop launches; no privileged launcher required.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use fs2::FileExt;

const LIMIT: u64 = 5 * 1024 * 1024;
const HISTORY: usize = 3;

struct Output {
    file: Option<RollingFile>,
}
struct RollingFile {
    path: PathBuf,
    file: File,
    bytes: u64,
    // Hold the separate lock through renames of the actual log.
    _lock: File,
}

impl RollingFile {
    fn open(directory: &Path) -> io::Result<Self> {
        prune_old_processes(directory);
        let primary = directory.join("client.log");
        let (path, lock) = match lock(&directory.join("client.lock")) {
            Ok(lock) => (primary, lock),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let path = directory.join(format!("client-{}.log", std::process::id()));
                let lock = lock(&path.with_extension("lock"))?;
                (path, lock)
            }
            Err(error) => return Err(error),
        };
        rotate(&path)?;
        let file = public_file(&path)?;
        Ok(Self {
            path,
            file,
            bytes: 0,
            _lock: lock,
        })
    }
    fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.bytes > 0 && self.bytes + bytes.len() as u64 > LIMIT {
            self.file.flush()?;
            rotate(&self.path)?;
            self.file = public_file(&self.path)?;
            self.bytes = 0;
        }
        self.file.write_all(bytes)?;
        self.bytes += bytes.len() as u64;
        self.file.flush()
    }
}

fn prune_old_processes(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut families = std::collections::BTreeMap::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(stem) = name.to_str().and_then(|name| name.strip_suffix(".log")) else {
            continue;
        };
        if stem
            .strip_prefix("client-")
            .is_some_and(|pid| pid.parse::<u32>().is_ok())
        {
            families.insert(
                stem.to_owned(),
                entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH),
            );
        }
    }
    let mut families: Vec<_> = families.into_iter().collect();
    families.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    for (stem, _) in families.into_iter().skip(2) {
        let lock_path = directory.join(format!("{stem}.lock"));
        let Ok(_lock) = lock(&lock_path) else {
            continue;
        }; // Never touch a live process.
        let _ = fs::remove_file(directory.join(format!("{stem}.log")));
        for index in 1..=HISTORY {
            let _ = fs::remove_file(directory.join(format!("{stem}.log.{index}")));
        }
        // Keep its lock inode, so another launch cannot race a replacement lock.
    }
}
fn lock(path: &Path) -> io::Result<File> {
    let file = public_file(path)?;
    file.try_lock_exclusive()?;
    Ok(file)
}
fn public_file(path: &Path) -> io::Result<File> {
    if fs::symlink_metadata(path).is_ok_and(|m| !m.is_file()) {
        return Err(io::Error::other("log path is not a regular file"));
    }
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o644))?;
    }
    Ok(file)
}
fn rotate(path: &Path) -> io::Result<()> {
    for index in (1..=HISTORY).rev() {
        let source = if index == 1 {
            path.to_path_buf()
        } else {
            PathBuf::from(format!("{}.{}", path.display(), index - 1))
        };
        match fs::rename(source, format!("{}.{index}", path.display())) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
fn directory() -> io::Result<PathBuf> {
    #[cfg(target_os = "linux")]
    let path = {
        use std::os::unix::fs::MetadataExt;
        let uid = fs::metadata("/proc/self")?.uid();
        let path = PathBuf::from(format!("/var/tmp/straterust-{uid}"));
        if !path.exists() {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new().mode(0o755).create(&path)?;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o022 != 0 {
            return Err(io::Error::other(
                "log directory must be owned and writable only by this user",
            ));
        }
        path
    };
    #[cfg(not(target_os = "linux"))]
    let path = {
        let root = std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("HOME"))
            .ok_or_else(|| io::Error::other("user data directory unavailable"))?;
        let path = PathBuf::from(root).join("straterust/logs");
        fs::create_dir_all(&path)?;
        path
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(path)
}
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let _ = io::stderr().write_all(bytes);
        if let Some(file) = &mut self.file
            && let Err(error) = file.append(bytes)
        {
            eprintln!("StrateRust file logging unavailable: {error}");
            self.file = None;
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }
}
pub fn init() {
    install(directory().and_then(|path| RollingFile::open(&path)));
}

fn install(file: io::Result<RollingFile>) {
    let path = file.as_ref().ok().map(|file| file.path.clone());
    if let Err(error) = &file {
        eprintln!("StrateRust file logging unavailable: {error}");
    }
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn,naga=warn"),
    )
    .format(|buffer, record| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        writeln!(
            buffer,
            "[{}.{:03} {} {}] {}",
            now.as_secs(),
            now.subsec_millis(),
            record.level(),
            record.target(),
            record.args()
        )
    })
    .target(env_logger::Target::Pipe(Box::new(Output {
        file: file.ok(),
    })))
    .init();
    std::panic::set_hook(Box::new(|panic| {
        log::error!(
            "panic in thread {:?}: {panic}\n{}",
            std::thread::current().name(),
            std::backtrace::Backtrace::force_capture()
        );
    }));
    let build = (|| -> io::Result<_> {
        #[cfg(target_os = "linux")]
        let source = PathBuf::from("/proc/self/exe");
        #[cfg(not(target_os = "linux"))]
        let source = std::env::current_exe()?;
        let mut hash = blake3::Hasher::new();
        hash.update_reader(File::open(source)?)?;
        Ok(hash.finalize().to_hex().to_string())
    })()
    .ok();
    log::info!(
        "StrateRust {} started; pid={}, protocol={}, simulation={}, executable={:?}, build_blake3={build:?}, log={path:?}",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        straterust_engine::session::PROTOCOL_VERSION,
        straterust_engine::sim::SIMULATION_REVISION,
        std::env::current_exe().ok()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_panics_write_a_backtrace_to_the_file_and_stderr() {
        const CHILD: &str = "STRATERUST_LOG_TEST_DIRECTORY";
        if let Some(path) = std::env::var_os(CHILD) {
            install(RollingFile::open(Path::new(&path)));
            panic!("intentional logging regression");
        }
        let temp = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "logging::tests::ordinary_panics_write_a_backtrace_to_the_file_and_stderr",
                "--nocapture",
            ])
            .env(CHILD, temp.path())
            .env("RUST_LOG", "error")
            .output()
            .unwrap();
        assert!(!output.status.success());
        let log = fs::read_to_string(temp.path().join("client.log")).unwrap();
        assert!(log.contains("intentional logging regression"));
        assert!(log.contains("panic in thread"));
        assert!(
            log.contains("logging::tests::ordinary_panics"),
            "backtrace missing: {log}"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("intentional logging regression"));
    }
    #[test]
    fn logs_rotate_are_public_and_concurrent_processes_do_not_replace_live_logs() {
        let temp = tempfile::tempdir().unwrap();
        let mut first = RollingFile::open(temp.path()).unwrap();
        first.append(b"first process\n").unwrap();
        let mut second = RollingFile::open(temp.path()).unwrap();
        assert_ne!(first.path, second.path);
        second.append(b"second process\n").unwrap();
        assert_eq!(fs::read(&first.path).unwrap(), b"first process\n");
        first.bytes = LIMIT;
        first.append(b"after rotation\n").unwrap();
        assert_eq!(fs::read(&first.path).unwrap(), b"after rotation\n");
        assert_eq!(
            fs::read(temp.path().join("client.log.1")).unwrap(),
            b"first process\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&first.path).unwrap().permissions().mode() & 0o777,
                0o644
            );
        }
        let primary = first.path.clone();
        drop((first, second));
        // Parallel tests spawn children. Between fork and exec a child can
        // briefly retain our close-on-exec lock descriptors after we drop them.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut next = loop {
            if let Ok(file) = RollingFile::open(temp.path())
                && file.path == primary
            {
                break file;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "primary log lock was not released"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        next.append(b"next launch\n").unwrap();
        assert_eq!(
            fs::read(temp.path().join("client.log.2")).unwrap(),
            b"first process\n"
        );
    }
}
