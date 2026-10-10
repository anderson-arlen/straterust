fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let name = args.next().unwrap_or_default();
    if name.is_empty() || name == "--help" {
        println!(
            "usage: straterust-importers starcraft|warcraft2 --source PATH\nInstalls into the platform application data directory; game names are fixed."
        );
        return Ok(());
    }
    let importer = straterust_importers::IMPORTERS
        .into_iter()
        .find(|i| name == i.id())
        .ok_or_else(|| {
            anyhow::anyhow!("usage: straterust-importers starcraft|warcraft2 --source PATH")
        })?;
    anyhow::ensure!(
        args.next().is_some_and(|a| a == "--source"),
        "expected --source PATH"
    );
    let source = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("expected source path"))?;
    anyhow::ensure!(
        args.next().is_none(),
        "unexpected argument; install names are fixed"
    );
    let installed = importer.install(
        std::path::Path::new(&source),
        &straterust_importers::games_directory()?,
        &|s| println!("{s}"),
    )?;
    println!("Installed {} at {}", importer.name(), installed.display());
    Ok(())
}
