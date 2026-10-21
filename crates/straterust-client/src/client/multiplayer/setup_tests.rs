use super::*;
use straterust_engine::multiplayer::*;
use straterust_engine::sim::ResourceAmount;
pub(super) fn catalog(game: &str) -> Catalog {
    Catalog {
        colors: Vec::new(),
        schema_version: 1,
        game: game.into(),
        maps: (0..2)
            .map(|i| MapEntry {
                title: format!("{game} map {i}"),
                package: format!("map{i}"),
                players: if i == 0 { 2 } else { 4 },
                size: if i == 0 { [64, 64] } else { [128, 128] },
                tileset: 0,
            })
            .collect(),
        factions: vec![Faction {
            ai_difficulties: Default::default(),
            ai_research: Vec::new(),
            name: "Human".into(),
            presentation: "map0".into(),
            tileset_presentations: Vec::new(),
            worker: UnitTypeId(2),
            starting_units: Vec::new(),
            equivalents: Default::default(),
            ai: Vec::new(),
        }],
        resources: vec![ResourcePreset {
            name: "High".into(),
            amounts: vec![ResourceAmount {
                kind: "gold".into(),
                amount: 10000,
            }],
        }],
        speeds: vec![Speed {
            name: "Fast".into(),
            tick_ms: 50,
        }],
        modes: vec![GameType::Melee, GameType::FreeForAll],
        tilesets: Vec::new(),
        terrain_mappings: Vec::new(),
        allow_cheats: false,
        random_faction: false,
        standard_start: false,
        defeat_without_structures: false,
        flag_art: None,
    }
}
#[test]
fn map_picker_and_setup_follow_selected_game_and_filters() {
    let root = tempfile::tempdir().unwrap();
    let mut games = Vec::new();
    for name in ["First", "Second"] {
        let directory = root.path().join(name);
        std::fs::create_dir(&directory).unwrap();
        for map in ["map0", "map1"] {
            std::fs::create_dir(directory.join(map)).unwrap();
        }
        std::fs::write(
            directory.join("manifest.ron"),
            format!("(schema_version:1,id:{name:?})"),
        )
        .unwrap();
        std::fs::write(
            directory.join("multiplayer.ron"),
            ron::ser::to_string(&catalog(name)).unwrap(),
        )
        .unwrap();
        games.push(catalog::GameEntry::read(&directory).unwrap());
    }
    let mut ui = menus::MenuUi::new(games.clone());
    ui.choose(games[0].clone()).unwrap();
    ui.select_map(0).unwrap();
    ui.page = Page::LanMaps;
    let labels = ui
        .choices(&Config::default())
        .into_iter()
        .filter(|c| matches!(c.pick, Pick::LanMap(_)))
        .map(|c| c.button.label)
        .collect::<Vec<_>>();
    assert_eq!(labels.len(), 2);
    assert!(labels.iter().all(|s| s.starts_with("First")));
    ui.map_players = Some(4);
    assert_eq!(ui.map_indices(), vec![1]);
    ui.map_size = Some([64, 64]);
    assert!(ui.map_indices().is_empty());
    ui.change_match_option(1).unwrap();
    assert_eq!(ui.match_settings.as_ref().unwrap().resources, Some(0));
    ui.choose(games[1].clone()).unwrap();
    assert!(ui.network_map.is_none());
    assert!(ui.match_settings.is_none());
    ui.select_map(0).unwrap();
    ui.page = Page::LanMaps;
    assert!(
        ui.choices(&Config::default())
            .into_iter()
            .filter(|c| matches!(c.pick, Pick::LanMap(_)))
            .all(|c| c.button.label.starts_with("Second"))
    );
}

#[test]
fn multiplayer_home_only_offers_joining_and_a_separate_create_flow() {
    for native in [false, true] {
        let mut ui = MenuUi::new(Vec::new());
        if native {
            ui.match_catalog = Some(catalog("demo"));
        }
        ui.page = Page::Multiplayer;
        let choices = ui.choices(&Config::default());
        for pick in [
            Pick::PlayerName,
            Pick::Address,
            Pick::Join,
            Pick::DiscoverLan,
            Pick::CreateGame,
        ] {
            assert!(choices.iter().any(|c| c.pick == pick));
        }
        assert!(choices.iter().all(|c| !matches!(
            c.pick,
            Pick::Host
                | Pick::HostPort
                | Pick::ChooseLanMap
                | Pick::MatchSlots
                | Pick::MatchOptions
        )));
        ui.page = Page::HostSetup;
        let choices = ui.choices(&Config::default());
        assert!(choices.iter().any(|c| c.pick == Pick::Host));
        assert!(choices.iter().any(|c| c.pick == Pick::ChooseLanMap));
        assert!(choices.iter().all(|c| !matches!(
            c.pick,
            Pick::Join | Pick::JoinLan(_) | Pick::DiscoverLan | Pick::Address
        )));
        for (i, a) in choices.iter().enumerate() {
            for b in choices.iter().skip(i + 1) {
                let [x, y, w, h] = a.button.rect;
                let [bx, by, bw, bh] = b.button.rect;
                assert!(
                    x + w <= bx || bx + bw <= x || y + h <= by || by + bh <= y,
                    "{:?} overlaps {:?}",
                    a.pick,
                    b.pick
                );
            }
        }
    }
}

#[test]
fn create_game_preserves_setup_navigation_and_join_address() {
    let content = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-demo");
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("manifest.ron"),
        "(schema_version:1,id:\"demo\")",
    )
    .unwrap();
    for map in ["map0", "map1"] {
        std::fs::create_dir(root.path().join(map)).unwrap();
    }
    for native in [false, true] {
        let mut client = Client::new(
            Config {
                audio: false,
                ..Default::default()
            },
            PathBuf::from("/tmp/stratarust-client-settings.ron"),
            Vec::new(),
        );
        client
            .menus
            .choose(catalog::GameEntry::read(if native { root.path() } else { &content }).unwrap())
            .unwrap();
        if native {
            client.menus.match_catalog = Some(catalog("demo"));
        }
        client.pick(Pick::Action(MenuAction::Multiplayer)).unwrap();
        assert!(client.menus.network_map.is_none());
        assert!(client.menus.match_settings.is_none());
        client.menus.address = "192.168.1.10:6123".into();
        client.pick(Pick::CreateGame).unwrap();
        assert_eq!(client.menus.page, Page::HostSetup);
        assert!(client.menus.network_map.is_some());
        client.pick(Pick::PlayerName).unwrap();
        client.key(KeyCode::Escape).unwrap();
        assert_eq!(client.menus.page, Page::HostSetup);
        if native {
            client.pick(Pick::MatchOptions).unwrap();
            client.pick(Pick::MatchOption(1)).unwrap();
            client.key(KeyCode::Escape).unwrap();
            assert_eq!(client.menus.page, Page::HostSetup);
            assert_eq!(
                client.menus.match_settings.as_ref().unwrap().resources,
                Some(0)
            );
        }
        client.pick(Pick::ChooseLanMap).unwrap();
        client.key(KeyCode::Escape).unwrap();
        assert_eq!(client.menus.page, Page::HostSetup);
        client.menus.host_port = "6222".into();
        assert_eq!(client.host_address().unwrap().port(), 6222);
        assert!(client.host_address().unwrap().ip().is_unspecified());
        client.key(KeyCode::Escape).unwrap();
        assert_eq!(client.menus.page, Page::Multiplayer);
        assert_eq!(client.menus.address, "192.168.1.10:6123");
        for bad in ["", "0", "65536", "no port"] {
            client.menus.host_port = bad.into();
            assert!(client.host_address().is_err());
        }
    }
}

#[test]
fn direct_and_discovered_guests_join_the_hosts_lobby_without_local_setup() {
    let root = tempfile::tempdir().unwrap();
    let content = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-demo");
    let mut catalog = catalog("demo");
    catalog.colors = (0..8)
        .map(|i| PlayerColor {
            name: format!("Color {}", i + 1),
            shades: vec![[i * 30, 40, 60]],
        })
        .collect();
    catalog.resources[0].amounts[0].kind = "minerals".into();
    for map in &catalog.maps {
        let directory = root.path().join(&map.package);
        std::fs::create_dir(&directory).unwrap();
        for file in ["manifest.ron", "rules.ron", "presentation.ron"] {
            std::fs::copy(content.join(file), directory.join(file)).unwrap();
        }
        // Joining must not read a local scenario, even an invalid one.
        std::fs::write(directory.join("map.ron"), "invalid local map").unwrap();
    }
    std::fs::write(
        root.path().join("manifest.ron"),
        "(schema_version:1,id:\"demo\")",
    )
    .unwrap();
    std::fs::write(
        root.path().join("multiplayer.ron"),
        ron::ser::to_string(&catalog).unwrap(),
    )
    .unwrap();
    let game = catalog::GameEntry::read(root.path()).unwrap();
    let make_client = || {
        let mut client = Client::new(
            Config {
                audio: false,
                ..Default::default()
            },
            root.path().join("config.ron"),
            Vec::new(),
        );
        client.menus.choose(game.clone()).unwrap();
        client.pick(Pick::Action(MenuAction::Multiplayer)).unwrap();
        client
    };
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut host = make_client();
    host.pick(Pick::CreateGame).unwrap();
    host.menus.select_map(1).unwrap();
    host.menus.match_settings.as_mut().unwrap().resources = Some(0);
    host.menus.host_port = port.to_string();
    host.menus.address = "invalid join address".into();
    host.pick(Pick::Host).unwrap();
    let poll = |client: &mut Client| {
        client.session.as_mut().unwrap().advance_network().unwrap();
        client.poll_lobby();
        assert!(
            !client
                .session
                .as_ref()
                .unwrap()
                .network
                .as_ref()
                .unwrap()
                .ended,
            "{}",
            client.session.as_ref().unwrap().status
        );
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while host.menus.lobby.is_none() {
        poll(&mut host);
        assert!(Instant::now() < deadline, "host lobby did not open");
        std::thread::sleep(Duration::from_millis(5));
    }
    host.pick(Pick::Ready).unwrap();
    for discovered in [false, true] {
        let mut guest = make_client();
        assert!(guest.menus.network_map.is_none());
        assert!(guest.menus.match_settings.is_none());
        if discovered {
            guest.lan_rules = catalog::installed_rules(std::slice::from_ref(&game));
            guest.menus.lan_games = vec![straterust_engine::net::lan::LanGame {
                started: false,
                open_slots: 2,
                name: "Host's map".into(),
                address,
                compatible: true,
                identity: guest.lan_rules[0].identity.clone(),
            }];
            guest.pick(Pick::JoinLan(0)).unwrap();
        } else {
            guest.menus.address = address.to_string();
            guest.pick(Pick::Join).unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while guest.menus.lobby.is_none() {
            poll(&mut host);
            poll(&mut guest);
            assert!(Instant::now() < deadline, "guest did not enter lobby");
            std::thread::sleep(Duration::from_millis(5));
        }
        let (slot, view) = guest.menus.lobby.as_ref().unwrap();
        assert_eq!(*slot, 1);
        assert_eq!(view.map, 1);
        assert_eq!(view.settings.slots.len(), 4);
        assert_eq!(view.settings.resources, Some(0));
        assert!(guest.menus.network_map.is_none());
        assert!(guest.menus.choices(&guest.config).iter().all(|c| !matches!(
            c.pick,
            Pick::MatchOptions | Pick::MatchSlots | Pick::ChooseLanMap
        )));
        guest.pick(Pick::SelectSlot(1)).unwrap();
        assert!(
            guest
                .menus
                .choices(&guest.config)
                .iter()
                .any(|c| matches!(c.pick, Pick::SlotField(3)))
        );
        guest.pick(Pick::SlotField(3)).unwrap();
        let choices = guest.menus.choices(&guest.config);
        assert!(
            !choices
                .iter()
                .any(|c| matches!(c.pick, Pick::SetSlot(1, 3, 0))),
            "host color cannot be taken"
        );
        assert!(
            choices
                .iter()
                .any(|c| matches!(c.pick, Pick::SetSlot(1, 3, 7)))
        );
        guest.pick(Pick::Ready).unwrap();
        let color = if discovered { 6 } else { 7 };
        guest.pick(Pick::SetSlot(1, 3, color)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            poll(&mut host);
            poll(&mut guest);
            let view = &guest.menus.lobby.as_ref().unwrap().1;
            if view.settings.slots[1].color == Some(color)
                && view.players.iter().flatten().all(|p| p.ready)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "color and readiness did not reach the lobby"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            guest
                .menus
                .choices(&guest.config)
                .iter()
                .any(|c| matches!(c.pick, Pick::Ready) && c.button.label == "Ready")
        );
        guest.pick(Pick::SelectSlot(0)).unwrap();
        assert!(
            !guest
                .menus
                .choices(&guest.config)
                .iter()
                .any(|c| matches!(c.pick, Pick::SlotField(3))),
            "guests cannot edit someone else's color"
        );
        guest.pick(Pick::Back).unwrap();
        assert_eq!(guest.menus.page, Page::Multiplayer);
        assert!(guest.session.is_none());
        // Wait for the freed slot before joining again.
        while host.menus.lobby.as_ref().unwrap().1.players[1].is_some() {
            poll(&mut host);
            assert!(Instant::now() < deadline, "guest did not leave lobby");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    host.pick(Pick::Back).unwrap();
}

#[test]
#[ignore = "requires STRATERUST_GAMES pointing to refreshed retail imports"]
fn retail_catalogs_prepare_and_menu_settings_stay_separate() -> Result<()> {
    let root = PathBuf::from(std::env::var_os("STRATERUST_GAMES").context("set games directory")?);
    for name in ["StarCraft", "Warcraft II"] {
        let directory = root.join(name);
        let game = catalog::GameEntry::read(&directory)?;
        let mut menu = menus::MenuUi::new(catalog::discover(std::slice::from_ref(&root)));
        menu.choose(game)?;
        menu.select_map(0)?;
        menu.page = Page::LanMaps;
        let catalog = menu.match_catalog.as_ref().unwrap();
        ensure!(
            catalog.maps.len() >= 28,
            "missing original multiplayer maps"
        );
        println!("{name}: {} multiplayer maps", catalog.maps.len());
        for (i, map) in catalog.maps.iter().enumerate() {
            let mut settings = catalog.defaults(i);
            settings.random_locations = false;
            for slot in &mut settings.slots {
                slot.faction = Some(0);
            }
            let occupied = (0..usize::from(map.players)).collect::<Vec<_>>();
            let prepared = catalog
                .prepare(&directory, i, &settings, &occupied, 42)
                .with_context(|| map.title.clone())?;
            prepared.map_transfer()?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires STRATERUST_GAMES containing existing StarCraft and Warcraft II imports"]
fn retail_player_color_choices_reach_transferred_art_without_reimporting() -> Result<()> {
    let root = PathBuf::from(std::env::var_os("STRATERUST_GAMES").context("set games directory")?);
    for name in ["StarCraft", "Warcraft II"] {
        let directory = root.join(name);
        let original = std::fs::read(directory.join("multiplayer.ron"))?;
        let catalog = straterust_importers::multiplayer_catalog(&directory)?;
        ensure!(
            catalog.colors.len() >= 8,
            "missing native colors for {name}"
        );
        let index = catalog
            .maps
            .iter()
            .position(|m| m.players == 2)
            .unwrap_or(0);
        for era in 0..catalog.tilesets.len().max(1) {
            let mut settings = catalog.defaults(index);
            for slot in &mut settings.slots[2..] {
                slot.kind = SlotKind::Closed;
            }
            if !catalog.tilesets.is_empty() {
                settings.tileset = Some(era);
            }
            catalog.choose_color(&mut settings, 0, 7)?;
            catalog.choose_color(&mut settings, 1, 0)?;
            let prepared = catalog.prepare(&directory, index, &settings, &[0, 1], 42)?;
            let art = prepared
                .map_transfer()?
                .artwork
                .context("missing native map art")?;
            for (player, color) in [(0, 7), (1, 0)] {
                let shades: Vec<_> = art.player_colors[&PlayerId(player)]
                    .colors
                    .iter()
                    .map(|p| p[1])
                    .collect();
                assert_eq!(
                    shades, catalog.colors[color].shades,
                    "{name} tileset {era}, player {player}"
                );
            }
        }
        assert_eq!(std::fs::read(directory.join("multiplayer.ron"))?, original);
    }
    Ok(())
}

#[test]
#[ignore = "requires STRATERUST_GAMES containing refreshed retail menus"]
fn retail_multiplayer_menus_render_for_review() -> Result<()> {
    use std::io::Write;
    let root = PathBuf::from(std::env::var_os("STRATERUST_GAMES").context("set games directory")?);
    for (name, label) in [("StarCraft", "starcraft"), ("Warcraft II", "warcraft2")] {
        let mut menu = menus::MenuUi::new(Vec::new());
        menu.choose(catalog::GameEntry::read(&root.join(name))?)?;
        let index = menu
            .match_catalog
            .as_ref()
            .unwrap()
            .maps
            .iter()
            .position(|m| m.players == 8)
            .context("eight-player map")?;
        menu.select_map(index)?;
        menu.match_settings.as_mut().unwrap().mode = menu
            .match_catalog
            .as_ref()
            .unwrap()
            .modes
            .iter()
            .position(|m| *m == GameType::Teams)
            .unwrap();
        menu.map_browser.preview = Some(straterust_engine::multiplayer::browser::preview(
            menu.network_map.as_ref().unwrap(),
        )?);
        menu.map_browser.preview_map = Some(index);
        let identity = straterust_engine::sim::GameplayIdentity::of(&Package::client_definitions(
            menu.network_map.as_ref().unwrap(),
        )?);
        menu.lan_games = (0..3)
            .map(|i| straterust_engine::net::lan::LanGame {
                started: false,
                open_slots: 2,
                name: format!("LAN game {}", i + 1),
                address: std::net::SocketAddr::from(([192, 168, 1, 10 + i], 6112)),
                compatible: true,
                identity: identity.clone(),
            })
            .collect();
        for (page, suffix) in [
            (Page::Multiplayer, "join"),
            (Page::HostSetup, "create"),
            (Page::LanMaps, "maps"),
            (Page::MatchOptions, "settings"),
            (Page::MatchSlots, "slots"),
            (Page::Lobby, "lobby"),
            (Page::Lobby, "position"),
            (Page::Lobby, "player-picker"),
            (Page::Lobby, "race-picker"),
            (Page::Lobby, "team-picker"),
            (Page::Lobby, "color-picker"),
        ] {
            menu.page = page.clone();
            menu.selected_slot = (suffix != "lobby").then_some(0);
            menu.slot_field = match suffix {
                "player-picker" => Some(0),
                "race-picker" => Some(1),
                "team-picker" => Some(2),
                "color-picker" => Some(3),
                _ => None,
            };
            let settings = menu.match_settings.clone().unwrap();
            menu.lobby = Some((
                0,
                straterust_engine::net::lobby::View {
                    host: 0,
                    running: false,
                    map: index,
                    players: settings
                        .slots
                        .iter()
                        .enumerate()
                        .map(|(i, _)| {
                            Some(straterust_engine::net::lobby::Player {
                                ready: i % 2 == 0,
                                name: format!("Player {}", i + 1),
                            })
                        })
                        .collect(),
                    settings,
                },
            ));
            menu.message = if page == Page::Lobby {
                "Waiting for players to mark ready.".into()
            } else {
                String::new()
            };
            for (width, height, size) in [(640, 480, "small"), (1920, 1296, "large")] {
                let mut pixels = vec![0; (width * height) as usize];
                crate::view::draw_menu_pixels(
                    &menu,
                    &Config::default(),
                    &mut pixels,
                    width,
                    height,
                );
                let mut output = std::io::BufWriter::new(std::fs::File::create(format!(
                    "/tmp/straterust-multiplayer-{label}-{suffix}-{size}.ppm"
                ))?);
                writeln!(output, "P6\n{width} {height}\n255")?;
                for p in pixels {
                    output.write_all(&[(p >> 16) as u8, (p >> 8) as u8, p as u8])?;
                }
            }
        }
    }
    Ok(())
}
