fn main() -> anyhow::Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() == 3 && args[0] == "inspect" && args[1] == "--source" {
        return straterust_import_warcraft2::inspect(std::path::Path::new(&args[2]));
    }
    anyhow::ensure!(
        args.len() == 4 && args[0] == "convert" && args[1] == "--source",
        "developer export: straterust-import-warcraft2 convert --source PATH OUTPUT (use launcher for installation)"
    );
    straterust_import_warcraft2::import_game(
        std::path::Path::new(&args[2]),
        std::path::Path::new(&args[3]),
        &|s| println!("{s}"),
    )
}
