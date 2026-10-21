use super::*;
use crate::menus::map_browser::Dropdown;
use straterust_engine::multiplayer::{
    Catalog,
    browser::{self, Overview},
};

fn overview(layout: &str, settings: &str, players: u8) -> Result<Overview> {
    Ok(Overview {
        layout: layout.into(),
        settings: settings.into(),
        fields: vec![("Players".into(), players.to_string())],
        army: "same army".into(),
        resources: "same deposits".into(),
        terrain: "same artwork".into(),
    })
}

fn menu_fixture() -> (tempfile::TempDir, MenuUi) {
    let root = tempfile::tempdir().unwrap();
    let mut catalog = super::setup_tests::catalog("test");
    let mut copy = catalog.maps[0].clone();
    copy.package = "map2".into();
    catalog.maps.insert(1, copy);
    let mut version = catalog.maps[0].clone();
    version.package = "map3".into();
    version.players = 4;
    catalog.maps.insert(2, version);
    for map in &catalog.maps {
        std::fs::create_dir(root.path().join(&map.package)).unwrap();
    }
    std::fs::write(
        root.path().join("manifest.ron"),
        "(schema_version:1,id:\"test\")",
    )
    .unwrap();
    std::fs::write(
        root.path().join("multiplayer.ron"),
        ron::ser::to_string(&catalog).unwrap(),
    )
    .unwrap();
    let game = catalog::GameEntry::read(root.path()).unwrap();
    let mut menu = MenuUi::new(Vec::new());
    menu.choose(game).unwrap();
    menu.select_map(0).unwrap();
    menu.page = Page::HostSetup;
    menu.navigate(Page::LanMaps);
    menu.browse_map(1); // Equivalent alias must select its surviving version.
    menu.map_browser.index(
        &catalog,
        vec![
            overview("first", "base", 2),
            overview("first", "base", 2),
            overview("first", "changed", 4),
            overview("second", "other", 4),
        ],
    );
    (root, menu)
}

#[test]
fn variants_group_by_layout_preserve_settings_and_filter_individual_versions() {
    let (_root, mut menu) = menu_fixture();
    assert_eq!(menu.map_browser.groups, vec![vec![0, 2], vec![3]]);
    assert_eq!(menu.map_browser.selected, Some(0));
    assert_eq!(menu.map_indices(), vec![0, 3]);
    assert_eq!(menu.map_versions(0), vec![0, 2]);
    assert!(menu.version_label(2).contains("Players: 4"));
    menu.map_players = Some(4);
    menu.map_filter_changed();
    assert_eq!(menu.map_browser.selected, Some(2));
    assert_eq!(menu.map_indices(), vec![2, 3]);
    assert_eq!(menu.map_versions(2), vec![2]);
    menu.map_size = Some([64, 64]);
    menu.map_filter_changed();
    assert_eq!(menu.map_indices(), vec![2]);
    menu.map_players = Some(8);
    menu.map_filter_changed();
    assert!(menu.map_indices().is_empty());
    assert!(menu.map_browser.selected.is_none());
    // Inspection failure keeps the original source map available separately.
    let catalog = menu.match_catalog.clone().unwrap();
    menu.map_browser.index(
        &catalog,
        vec![
            overview("first", "base", 2),
            Err(anyhow::anyhow!("broken data")),
            overview("first", "changed", 4),
            overview("second", "other", 4),
        ],
    );
    assert_eq!(menu.map_browser.groups, vec![vec![0, 2], vec![1], vec![3]]);
    menu.map_browser.index(&catalog, Vec::new());
    assert_eq!(
        menu.map_browser.groups,
        vec![vec![0], vec![1], vec![2], vec![3]],
        "failed comparison must leave every original map available"
    );
}

#[test]
fn preview_selection_does_not_change_match_and_dropdowns_use_direct_choices() {
    let (root, menus) = menu_fixture();
    let mut client = Client::new(
        Config {
            audio: false,
            ..Default::default()
        },
        root.path().join("config.ron"),
        Vec::new(),
    );
    client.menus = menus;
    client.pick(Pick::MapFilter(true)).unwrap();
    assert_eq!(client.menus.map_players, None);
    assert_eq!(client.menus.map_browser.dropdown, Some(Dropdown::Players));
    assert!(
        client
            .menus
            .choices(&client.config)
            .iter()
            .any(|c| c.pick == Pick::MapPlayers(Some(4)))
    );
    client.pick(Pick::MapPlayers(Some(4))).unwrap();
    assert_eq!(client.menus.map_players, Some(4));
    assert!(client.menus.map_browser.dropdown.is_none());
    client.pick(Pick::LanMap(2)).unwrap();
    assert_eq!(client.menus.page, Page::LanMaps);
    assert_eq!(client.menus.match_map, 0);
    assert_eq!(client.menus.map_browser.selected, Some(2));
    client.pick(Pick::MapVersions).unwrap();
    client.key(KeyCode::Escape).unwrap();
    assert!(client.menus.map_browser.dropdown.is_none());
    assert_eq!(client.menus.page, Page::LanMaps);
    client.pick(Pick::MapFilter(false)).unwrap();
    client.pick(Pick::MapSize(Some([64, 64]))).unwrap();
    assert_eq!(client.menus.map_size, Some([64, 64]));
    client.pick(Pick::UseLanMap).unwrap();
    assert_eq!(client.menus.page, Page::HostSetup);
    assert_eq!(client.menus.match_map, 2);
    assert!(client.menus.network_map.as_ref().unwrap().ends_with("map3"));
    assert_eq!(client.menus.match_settings.as_ref().unwrap().slots.len(), 4);
}

#[test]
#[ignore = "requires STRATERUST_GAMES with native original multiplayer catalogs"]
fn retail_map_browser_groups_and_previews_for_review() -> Result<()> {
    use std::io::Write;
    let root = PathBuf::from(std::env::var_os("STRATERUST_GAMES").context("set games directory")?);
    for (name, label) in [("StarCraft", "starcraft"), ("Warcraft II", "warcraft2")] {
        let mut menu = MenuUi::new(Vec::new());
        menu.choose(catalog::GameEntry::read(&root.join(name))?)?;
        let catalog: Catalog = menu.match_catalog.clone().unwrap();
        let start = Instant::now();
        let values = catalog
            .maps
            .iter()
            .map(|entry| {
                browser::inspect(
                    &straterust_engine::multiplayer::package_path(
                        &root.join(name),
                        &entry.package,
                    )?,
                    entry,
                    &catalog,
                )
            })
            .collect::<Vec<_>>();
        for (i, value) in values.iter().enumerate() {
            if let Err(error) = value {
                anyhow::bail!("{}: {error:#}", catalog.maps[i].title);
            }
        }
        menu.map_browser.index(&catalog, values);
        println!(
            "{name}: {} files -> {} layouts, {} settings versions, inspection {:?}",
            catalog.maps.len(),
            menu.map_browser.groups.len(),
            menu.map_browser.groups.iter().map(Vec::len).sum::<usize>(),
            start.elapsed()
        );
        if name == "Warcraft II" {
            ensure!(
                menu.map_browser.groups.len() < catalog.maps.len(),
                "no duplicate layouts grouped"
            );
        }
        let group = menu
            .map_browser
            .groups
            .iter()
            .find(|g| g.len() > 1)
            .cloned()
            .unwrap_or_else(|| menu.map_browser.groups[0].clone());
        for i in &group {
            println!("  {}: {}", catalog.maps[*i].title, menu.version_label(*i));
        }
        if name == "Warcraft II" {
            let plains = catalog
                .maps
                .iter()
                .enumerate()
                .filter(|(_, m)| m.title == "Plains of snow")
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            let layout = &menu.map_browser.overviews[plains[0]]
                .as_ref()
                .unwrap()
                .layout;
            ensure!(
                plains
                    .iter()
                    .all(|i| menu.map_browser.overviews[*i].as_ref().unwrap().layout == *layout),
                "cosmetic copies of Plains of snow were not grouped"
            );
        }
        let index = group[0];
        menu.browse_map(index);
        menu.map_browser.preview = Some(browser::preview(
            &straterust_engine::multiplayer::package_path(
                &root.join(name),
                &catalog.maps[index].package,
            )?,
        )?);
        menu.map_browser.preview_map = Some(index);
        menu.page = Page::LanMaps;
        menu.offset = menu.map_indices().iter().position(|i| *i == index).unwrap() / 7 * 7;
        for (dropdown, suffix) in [
            (None, "preview"),
            (Some(Dropdown::Players), "players"),
            (Some(Dropdown::Size), "sizes"),
            (Some(Dropdown::Versions), "versions"),
        ] {
            menu.map_browser.dropdown = dropdown;
            for (width, height, size_label) in [(640, 480, "small"), (1920, 1296, "large")] {
                let mut pixels = vec![0; width as usize * height as usize];
                crate::view::draw_menu_pixels(
                    &menu,
                    &Config::default(),
                    &mut pixels,
                    width,
                    height,
                );
                let mut output = std::io::BufWriter::new(std::fs::File::create(format!(
                    "/tmp/straterust-map-browser-{label}-{suffix}-{size_label}.ppm"
                ))?);
                writeln!(output, "P6\n{width} {height}\n255")?;
                for p in pixels {
                    output.write_all(&[(p >> 16) as u8, (p >> 8) as u8, p as u8])?;
                }
            }
        }
        // Every version resolves a real thumbnail, including rectangular maps.
        for indices in &menu.map_browser.groups {
            for index in indices {
                let entry = &catalog.maps[*index];
                let directory =
                    straterust_engine::multiplayer::package_path(&root.join(name), &entry.package)?;
                let preview = browser::preview(&directory).with_context(|| entry.title.clone())?;
                ensure!(
                    preview.image.width <= 256 && preview.image.height <= 256,
                    "oversized thumbnail"
                );
                ensure!(
                    (i64::from(preview.image.width) * i64::from(preview.size[1])
                        - i64::from(preview.image.height) * i64::from(preview.size[0]))
                    .abs()
                        <= i64::from(preview.size[0].max(preview.size[1])),
                    "preview aspect changed"
                );
            }
        }
    }
    Ok(())
}
