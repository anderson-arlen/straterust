use super::*;

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
