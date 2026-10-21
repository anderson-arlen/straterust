fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let name = args.next().unwrap_or_default();
    if name.is_empty() || name == "--help" {
        println!(
            "usage: straterust-importers starcraft|warcraft2 --source PATH [--metadata-only]\n       straterust-importers starcraft|warcraft2 --compact [--source PATH]\nInstalls into the platform application data directory; game names are fixed.\n--metadata-only refreshes version/icon metadata without reconverting installed maps.\n--compact upgrades existing installations to shared asset storage without reconversion."
        );
        return Ok(());
    }
    let importer = straterust_importers::IMPORTERS
        .into_iter()
        .find(|i| name == i.id())
        .ok_or_else(|| {
            anyhow::anyhow!("usage: straterust-importers starcraft|warcraft2 --source PATH")
        })?;
    let action = args.next().unwrap_or_default();
    if action == "--compact" {
        let original = if let Some(option) = args.next() {
            anyhow::ensure!(option == "--source", "expected --source PATH");
            Some(std::path::PathBuf::from(
                args.next()
                    .ok_or_else(|| anyhow::anyhow!("expected source path"))?,
            ))
        } else {
            None
        };
        anyhow::ensure!(args.next().is_none(), "unexpected argument");
        let root = straterust_importers::games_directory()?;
        let installed =
            importer.compact_installation_with_source(&root, original.as_deref(), &|s| {
                println!("{s}")
            })?;
        println!("Compacted {} at {}", importer.name(), installed.display());
        return Ok(());
    }
    anyhow::ensure!(action == "--source", "expected --source PATH or --compact");
    let source = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("expected source path"))?;
    let metadata_only = args
        .next()
        .map(|a| {
            anyhow::ensure!(a == "--metadata-only", "unexpected argument");
            Ok::<_, anyhow::Error>(true)
        })
        .transpose()?
        .unwrap_or(false);
    anyhow::ensure!(
        args.next().is_none(),
        "unexpected argument; install names are fixed"
    );
    let root = straterust_importers::games_directory()?;
    let installed = if metadata_only {
        importer.update_metadata(std::path::Path::new(&source), &root, &|s| println!("{s}"))?
    } else {
        importer.install(std::path::Path::new(&source), &root, &|s| println!("{s}"))?
    };
    println!("Installed {} at {}", importer.name(), installed.display());
    Ok(())
}
