use super::*;

#[test]
fn launcher_export_lists_all_games_and_does_not_start_a_session() {
    let mut c = client();
    assert!(
        c.menus
            .choices(&c.config)
            .iter()
            .any(|choice| choice.pick == Pick::Export)
    );
    c.pick(Pick::Export).unwrap();
    assert_eq!(c.menus.page, Page::ExportGames);
    let choices = c.menus.choices(&c.config);
    assert!(
        choices
            .iter()
            .any(|choice| matches!(choice.pick, Pick::ExportGame(_)))
    );
    assert!(
        !choices
            .iter()
            .any(|choice| matches!(choice.pick, Pick::Game(_)))
    );
    c.key(KeyCode::Escape).unwrap();
    assert_eq!(c.menus.page, Page::Packages);
    assert!(c.session.is_none());
    c.select_game(game()).unwrap();
    assert!(matches!(c.menus.page, Page::Authored(_)));
    assert!(c.session.is_none());
}

#[test]
fn cancelled_or_failed_export_restores_the_game_list_without_losing_games() {
    for result in [Ok(None), Err(anyhow::anyhow!("packager failed"))] {
        let mut c = client();
        let count = c.menus.games.len();
        let (sender, receiver) = std::sync::mpsc::channel();
        c.exporting = Some(receiver);
        c.menus.page = Page::Exporting;
        sender
            .send(crate::client::imports::Update::Done(result))
            .unwrap();
        c.poll_export();
        assert!(c.exporting.is_none());
        assert_eq!(c.menus.page, Page::ExportGames);
        assert_eq!(c.menus.games.len(), count);
        assert!(c.session.is_none());
    }
}

#[test]
#[ignore = "writes fixed export-menu review captures, including installed retail icons when available"]
fn export_menu_layout_review() {
    let mut c = client();
    if let Ok(root) = straterust_importers::games_directory() {
        let mut installed = catalog::discover(&[root]);
        installed.append(&mut c.menus.games);
        c.menus = MenuUi::new(installed);
    }
    for page in [Page::Packages, Page::ExportGames, Page::Exporting] {
        c.menus.page = page.clone();
        for (width, height) in [(640, 480), (1920, 1080)] {
            let mut pixels = vec![0; width * height];
            view::draw_menu_pixels(
                &c.menus,
                &c.config,
                &mut pixels,
                width as u32,
                height as u32,
            );
            let label = match page {
                Page::Packages => "games",
                Page::ExportGames => "select",
                _ => "progress",
            };
            let path = format!("/tmp/straterust-export-{label}-{width}.ppm");
            let mut out = BufWriter::new(File::create(path).unwrap());
            writeln!(out, "P6\n{width} {height}\n255").unwrap();
            for pixel in pixels {
                out.write_all(&[(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8])
                    .unwrap();
            }
        }
    }
}

#[test]
fn launcher_lists_bundled_importers_and_keeps_source_selection_outside_gameplay() {
    let mut c = client();
    c.pick(Pick::Import).unwrap();
    assert_eq!(c.menus.page, Page::Importers);
    let options = c.menus.choices(&c.config);
    assert!(options.iter().any(|c| c.button.label == "StarCraft"));
    assert!(options.iter().any(|c| c.button.label == "Warcraft II"));
    c.pick(Pick::Importer(straterust_importers::Importer::Warcraft2))
        .unwrap();
    assert_eq!(
        c.menus.page,
        Page::ImportSource(straterust_importers::Importer::Warcraft2)
    );
    assert!(c.session.is_none());
    c.key(KeyCode::Escape).unwrap();
    assert_eq!(c.menus.page, Page::Importers);
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a retail import"]
fn warcraft_campaigns_have_valid_client_controls_and_open_from_the_game_menu() {
    let root = PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2").unwrap());
    let mut c = client();
    c.menus.choose(GameEntry::read(&root).unwrap()).unwrap();
    assert_eq!(c.menus.pack.manifest.campaigns.len(), 4);
    let campaigns = c.menus.pack.manifest.campaigns.clone();
    for entry in campaigns {
        let directory = root.join(&entry.directory);
        let campaign = Campaign::load(&directory).unwrap();
        assert_eq!(
            campaign.missions.len(),
            if entry.directory.contains("expansion") {
                12
            } else {
                14
            }
        );
        for mission in &campaign.missions {
            let presentation: Presentation =
                read_ron(&directory.join(&mission.package).join("presentation.ron")).unwrap();
            presentation.validate().unwrap();
        }
        c.pick(Pick::Action(MenuAction::Campaign(entry.directory.clone())))
            .unwrap();
        c.pick(Pick::Mission(0)).unwrap();
        assert!(c.session.is_some());
        assert!(
            c.session
                .as_ref()
                .unwrap()
                .assets
                .as_ref()
                .unwrap()
                .indicators
                .is_some()
        );
        let app = c.session.as_mut().unwrap();
        let assets = app.assets.as_ref().unwrap();
        let media = app.media.as_ref().unwrap();
        for unit in app
            .world
            .rules()
            .units
            .iter()
            .filter(|u| u.weapon.is_some())
        {
            assert!(
                media.audio.iter().any(|a| a.unit_type == Some(unit.id)
                    && a.cue == straterust_engine::media::AudioCue::Attack),
                "missing attack recording for {:?}",
                unit.id
            );
        }
        let layout = assets.manifest.console_layout.expect("authored console");
        assert!(app.camera.viewport.is_some());
        for size in [[640.0, 480.0], [1280.0, 800.0], [1920.0, 1080.0]] {
            let [left, top, right, bottom] = app.camera.viewport_bounds(size);
            assert!(
                app.camera
                    .screen_to_world([left - 1.0, top + 20.0], size)
                    .is_none()
            );
            let center = [(left + right) / 2.0, (top + bottom) / 2.0];
            assert_eq!(
                app.camera.screen_to_world(center, size),
                Some(Position {
                    x: app.camera.x.round() as i32,
                    y: app.camera.y.round() as i32
                })
            );
            for button in app.buttons() {
                let [x, y, w, h] = layout.button_rect(button.slot, size);
                assert_eq!(
                    controls::button_at(
                        &app.buttons(),
                        [x + w / 2.0, y + h / 2.0],
                        size,
                        app.assets.as_ref()
                    ),
                    Some(button.action)
                );
            }
        }
        if let Some(ui) = &mut app.mission_ui {
            ui.start(&mut app.audio);
        }
        app.selected = app
            .world
            .state()
            .entities
            .iter()
            .filter(|e| {
                e.owner == PlayerId(0) && !app.world.unit_type(e.unit_type).unwrap().structure
            })
            .take(12)
            .map(|e| e.id)
            .collect();
        crate::presentation_tests::capture(
            app,
            &format!("warcraft2-{}", entry.directory),
            [1280, 800],
            None,
        );
        if let Some(building) =
            app.world.state().entities.iter().find(|e| {
                e.owner == PlayerId(0) && app.world.unit_type(e.unit_type).unwrap().structure
            })
        {
            app.selected = [building.id].into();
            crate::presentation_tests::capture(
                app,
                &format!("warcraft2-{}-building", entry.directory),
                [1280, 800],
                None,
            );
        }
        c.open_pause();
        c.pick(Pick::Action(MenuAction::EndMission)).unwrap();
        c.pick(Pick::Confirm).unwrap();
    }
}
