use super::*;
use std::collections::BTreeMap;

fn client() -> Client {
    Client::new(
        Config {
            audio: false,
            ..Config::default()
        },
        PathBuf::from("/tmp/straterust-client-settings.ron"),
        vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")],
    )
}
fn game() -> GameEntry {
    GameEntry::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"))
        .unwrap()
}

#[test]
fn menus_start_without_world_then_launch_and_return_to_game_home() {
    let mut client = client();
    assert!(client.session.is_none());
    assert_eq!(client.menus.page, Page::Packages);
    assert!(client.menus.games.len() >= 3);
    client.menus.choose(game()).unwrap();
    client.pick(Pick::Action(MenuAction::Play)).unwrap();
    assert_eq!(client.menus.page, Page::Closed);
    assert!(client.session.is_some());
    client.open_pause();
    assert!(client.session.as_ref().unwrap().menu_open);
    client.pick(Pick::Action(MenuAction::EndMission)).unwrap();
    assert!(matches!(client.menus.page, Page::Confirm(_)));
    client.pick(Pick::Confirm).unwrap();
    assert!(client.session.is_none());
    assert!(matches!(client.menus.page, Page::Authored(_)));
}

#[test]
fn menus_escape_preserves_existing_pause_and_captures_orders() {
    for paused in [false, true] {
        let mut client = client();
        client.menus.choose(game()).unwrap();
        client.pick(Pick::Action(MenuAction::Play)).unwrap();
        let app = client.session.as_mut().unwrap();
        app.paused = paused;
        app.keys.insert(KeyCode::ArrowLeft);
        let before = app.world.state_hash();
        client.open_pause();
        let app = client.session.as_mut().unwrap();
        assert!(app.keys.is_empty());
        app.issue(Order::Move {
            entity: EntityId(1),
            target: Position { x: 500, y: 400 },
        })
        .unwrap();
        assert!(app.recorded.is_empty());
        client.pick(Pick::Action(MenuAction::Settings)).unwrap();
        client.key(KeyCode::Escape).unwrap();
        assert!(client.session.as_ref().unwrap().menu_open);
        client.key(KeyCode::Escape).unwrap();
        let app = client.session.as_ref().unwrap();
        assert!(!app.menu_open);
        assert_eq!(app.paused, paused);
        assert_eq!(app.world.state_hash(), before);
    }
}

#[test]
fn hotkeys_escape_cancels_active_commands_before_opening_the_game_menu() {
    let mut client = client();
    client.menus.choose(game()).unwrap();
    client.pick(Pick::Action(MenuAction::Play)).unwrap();
    let app = client.session.as_mut().unwrap();
    app.presentation.command_keys = BTreeMap::from([
        ("cancel".into(), "Esc".into()),
        ("back".into(), "Esc".into()),
    ]);
    app.target_mode = Some(TargetMode::Move);
    client.key(KeyCode::Escape).unwrap();
    assert_eq!(client.menus.page, Page::Closed);
    assert!(client.session.as_ref().unwrap().target_mode.is_none());
    client.session.as_mut().unwrap().build_menu = true;
    client.key(KeyCode::Escape).unwrap();
    assert_eq!(client.menus.page, Page::Closed);
    assert!(!client.session.as_ref().unwrap().build_menu);
    client.key(KeyCode::Escape).unwrap();
    assert!(client.session.as_ref().unwrap().menu_open);
    client.key(KeyCode::Escape).unwrap();
    client.session.as_mut().unwrap().target_mode = Some(TargetMode::Move);
    client.key(KeyCode::F10).unwrap();
    assert!(client.session.as_ref().unwrap().menu_open);
}

#[test]
fn menus_failed_launch_retains_the_live_session() {
    let mut client = client();
    client.menus.choose(game()).unwrap();
    client.pick(Pick::Action(MenuAction::Play)).unwrap();
    let before = client.session.as_ref().unwrap().world.state_hash();
    client.open_pause();
    assert!(
        client
            .play(Path::new("/tmp/straterust-missing-package"), None)
            .is_err()
    );
    assert_eq!(client.session.as_ref().unwrap().world.state_hash(), before);
    assert!(client.session.as_ref().unwrap().menu_open);
}

#[test]
fn menus_settings_roundtrip_preserves_bindings_and_valid_ranges() {
    let mut config = Config::default();
    for setting in menus::settings::ALL {
        setting.change(&mut config, 1);
        config.validate().unwrap();
    }
    config.bindings.pause = "F12".into();
    let path = PathBuf::from("/tmp/straterust-client-settings.ron");
    config.save(&path).unwrap();
    let loaded: Config = read_ron(&path).unwrap();
    assert_eq!(loaded.bindings.pause, "F12");
    assert_eq!(loaded.music_volume, config.music_volume);
    assert_eq!(loaded.game_speed, config.game_speed);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn menus_discovery_stops_at_campaigns_and_skips_broken_metadata() {
    let temp =
        std::env::temp_dir().join(format!("straterust-menu-discovery-{}", std::process::id()));
    std::fs::create_dir_all(temp.join("game/first")).unwrap();
    std::fs::create_dir_all(temp.join("broken")).unwrap();
    let manifest = Campaign {
        schema_version: 1,
        id: "Example".into(),
        missions: vec![straterust_engine::content::CampaignMission {
            title: "First".into(),
            package: "first".into(),
        }],
    };
    std::fs::write(
        temp.join("game/campaign.ron"),
        ron::ser::to_string(&manifest).unwrap(),
    )
    .unwrap();
    std::fs::write(temp.join("game/first/manifest.ron"), "invalid").unwrap();
    std::fs::write(temp.join("broken/manifest.ron"), "invalid").unwrap();
    let games = catalog::discover(&[temp.clone(), temp.clone()]);
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].title, "Example");
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn menus_distinguish_installed_copies_with_the_same_map_name() {
    let temp = std::env::temp_dir().join("straterust-menu-duplicate-packages");
    for name in ["older-import", "current-import"] {
        std::fs::create_dir_all(temp.join(name)).unwrap();
        std::fs::write(
            temp.join(name).join("manifest.ron"),
            "(schema_version:1,id:\"test.map\")",
        )
        .unwrap();
    }
    let games = catalog::discover(std::slice::from_ref(&temp));
    assert_eq!(games.len(), 2);
    assert_eq!(games[0].title, "current-import: test map");
    assert_eq!(games[1].title, "older-import: test map");
    let mut menu = menus::MenuUi::new(games);
    menu.page = Page::LanMaps;
    let choices = menu.choices(&Config::default());
    assert_eq!(choices[0].button.label, "current-import: test map");
    assert_eq!(choices[1].button.label, "older-import: test map");
    menu.network_map = Some(menu.games[0].directory.clone());
    menu.page = Page::Multiplayer;
    assert_eq!(
        menu.choices(&Config::default())[0].button.label,
        "Map: current-import: test map"
    );
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
#[ignore = "requires privately imported campaign menus; set STRATERUST_CAMPAIGN_PACKAGE"]
fn native_menus_render_and_launch_the_fifth_mission() {
    let root = PathBuf::from(std::env::var("STRATERUST_CAMPAIGN_PACKAGE").unwrap());
    let mut client = client();
    client
        .menus
        .choose(GameEntry::read(&root).unwrap())
        .unwrap();
    assert_eq!(client.menus.pack.manifest.title, "StarCraft");
    assert!(client.menus.pack.images.len() > 100);
    let mut pixels = vec![0; 1100 * 760];
    let mut chooser = MenuUi::new(client.menus.games.clone());
    view::draw_menu_pixels(&chooser, &client.config, &mut pixels, 1100, 760);
    fn capture(pixels: &[u32], name: &str) {
        let mut out = BufWriter::new(File::create(name).unwrap());
        writeln!(out, "P6\n1100 760\n255").unwrap();
        for p in pixels {
            out.write_all(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8])
                .unwrap();
        }
    }
    capture(&pixels, "/tmp/straterust-menu-packages.ppm");
    chooser.page = Page::Settings;
    view::draw_menu_pixels(&chooser, &client.config, &mut pixels, 1100, 760);
    capture(&pixels, "/tmp/straterust-menu-settings.ppm");
    view::draw_menu_pixels(&client.menus, &client.config, &mut pixels, 1100, 760);
    capture(&pixels, "/tmp/straterust-menu-main.ppm");
    client
        .pick(Pick::Action(MenuAction::Screen("campaigns".into())))
        .unwrap();
    view::draw_menu_pixels(&client.menus, &client.config, &mut pixels, 1100, 760);
    capture(&pixels, "/tmp/straterust-menu-campaigns.ppm");
    client
        .pick(Pick::Action(MenuAction::Campaign(".".into())))
        .unwrap();
    assert_eq!(client.menus.campaign.as_ref().unwrap().1.missions.len(), 5);
    view::draw_menu_pixels(&client.menus, &client.config, &mut pixels, 1100, 760);
    capture(&pixels, "/tmp/straterust-menu-missions.ppm");
    client.pick(Pick::Mission(4)).unwrap();
    assert_eq!(
        client
            .session
            .as_ref()
            .unwrap()
            .campaign
            .as_ref()
            .unwrap()
            .index,
        4
    );
    client.open_pause();
    view::draw_menu_pixels(&client.menus, &client.config, &mut pixels, 1100, 760);
    capture(&pixels, "/tmp/straterust-menu-pause.ppm");
    client.key(KeyCode::Escape).unwrap();
    assert!(!client.session.as_ref().unwrap().menu_open);
}
