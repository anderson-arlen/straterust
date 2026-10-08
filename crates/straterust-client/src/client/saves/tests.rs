use super::*;

fn client() -> Client {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures");
    let mut client = Client::new(
        Config {
            audio: false,
            ..Default::default()
        },
        PathBuf::from("/tmp/stratarust-save-menu-tests/settings.ron"),
        vec![root.clone()],
    );
    client
        .menus
        .choose(GameEntry::read(&root).unwrap())
        .unwrap();
    client
}
fn finish_save(client: &mut Client) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while client.save_pending.is_some() {
        client.poll_saves();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(
        client.menus.message.contains("Game saved"),
        "{}",
        client.menus.message
    );
}

#[test]
fn save_menu_loads_from_frontend_confirms_overwrite_and_retains_game_on_failure() {
    let mut client = client();
    let path = crate::saves::slot_path(&client.save_directory(), 0).unwrap();
    if path.exists() {
        std::fs::remove_file(&path).unwrap();
    }
    client.pick(Pick::Action(MenuAction::Play)).unwrap();
    client.open_pause();
    client.pick(Pick::Action(MenuAction::SaveGame)).unwrap();
    assert_eq!(client.menus.page, Page::Saves(true));
    client.pick(Pick::SaveSlot(0)).unwrap();
    finish_save(&mut client);
    assert!(client.session.as_ref().unwrap().menu_open);
    client.pick(Pick::Action(MenuAction::SaveGame)).unwrap();
    client.pick(Pick::SaveSlot(0)).unwrap();
    assert_eq!(client.menus.page, Page::Overwrite(0));
    client.key(KeyCode::Escape).unwrap();
    assert_eq!(client.menus.page, Page::Saves(true));
    client.end_session();
    client.pick(Pick::Action(MenuAction::LoadGame)).unwrap();
    assert_eq!(client.menus.page, Page::Saves(false));
    client.pick(Pick::LoadSlot(0)).unwrap();
    assert_eq!(client.menus.page, Page::Closed);
    assert!(client.session.as_ref().unwrap().world.is_player_view());
    client.open_pause();
    let before = client.session.as_ref().unwrap().world.state_hash();
    std::fs::write(&path, b"broken").unwrap();
    assert!(client.pick(Pick::LoadSlot(0)).is_err());
    assert_eq!(client.session.as_ref().unwrap().world.state_hash(), before);
    client.menus.multiplayer = true;
    assert!(client.open_saves(false).is_err());
    assert!(client.open_saves(true).is_err());
    assert!(
        !client
            .menus
            .choices(&client.config)
            .iter()
            .any(|c| matches!(
                c.pick,
                Pick::Action(MenuAction::SaveGame | MenuAction::LoadGame)
            ))
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires STRATERUST_CAMPAIGNS pointing to a published retail bundle"]
fn race_campaign_saves_load_from_the_combined_game_menu() -> Result<()> {
    let root = PathBuf::from(
        std::env::var_os("STRATERUST_CAMPAIGNS").context("set STRATERUST_CAMPAIGNS")?,
    )
    .canonicalize()?;
    let mut client = client();
    let path = crate::saves::slot_path(&client.save_directory(), 6)?;
    for directory in [root.clone(), root.join("zerg"), root.join("protoss")] {
        let manifest = Campaign::load(&directory)?;
        let mut app = App::load(
            &directory.join(&manifest.missions[5].package),
            client.config.clone(),
            None,
        )?;
        let before = app.world.state_hash();
        app.campaign = Some(CampaignSession {
            root: directory.clone(),
            manifest,
            index: 5,
        });
        client.direct(app, GameEntry::read(&directory)?)?;
        client.open_pause();
        client.open_saves(true)?;
        client.save_slot(6)?;
        if client.menus.page == Page::Overwrite(6) {
            client.save_slot(6)?;
        }
        finish_save(&mut client);
        client.end_session();
        client.menus.choose(GameEntry::read(&root)?)?;
        client.open_saves(false)?;
        assert!(!client.menus.save_labels[6].contains("another package"));
        client.load_slot(6)?;
        let restored = client.session.as_ref().unwrap();
        assert!(restored.world.is_player_view());
        assert_eq!(restored.world.state_hash(), before);
        assert_eq!(restored.campaign.as_ref().unwrap().index, 5);
        assert_eq!(restored.campaign.as_ref().unwrap().root, directory);
        println!(
            "{}: mission 6 save restored from bundle menu",
            directory.display()
        );
    }
    std::fs::remove_file(path)?;
    Ok(())
}
