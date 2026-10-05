use super::*;

#[test]
#[ignore = "requires STRATERUST_SOURCE pointing to the authorized retail disc"]
fn inventory_first_five_race_campaigns() {
    let path = std::env::var_os("STRATERUST_SOURCE").expect("set STRATERUST_SOURCE");
    let source = Source::open(Path::new(&path)).unwrap();
    let mut installer = Archive::open_region(&source.path, source.offset, source.len).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/source/starcraft");
    std::fs::create_dir_all(&root).unwrap();
    for race in ["zerg", "protoss"] {
        for number in 1..=5 {
            let member = format!("campaign\\{race}\\{race}{number:02}\\staredit\\scenario.chk");
            let chk = installer.read_file(&member, 8 * 1024 * 1024).unwrap();
            std::fs::write(
                root.join(format!(
                    "campaign_{race}_{race}{number:02}_staredit_scenario.chk"
                )),
                &chk,
            )
            .unwrap();
        }
    }
}
