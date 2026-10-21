use super::*;
use crate::chat::Chat;
use straterust_engine::{
    multiplayer::*,
    net::{
        chat::{Channel, Message},
        lobby,
    },
    sim::PlayerId,
};

fn lobby_menu() -> MenuUi {
    let mut menu = MenuUi::new(Vec::new());
    let catalog: Catalog = ron::from_str(
        r#"(
        schema_version:1,game:"demo",
        maps:[(title:"Eight players",package:"map",players:8,size:(64,64),tileset:0)],
        factions:[(name:"Human",presentation:"map",worker:2,starting_units:[])],
        resources:[],speeds:[(name:"Fast",tick_ms:50)],modes:[TeamMelee]
    )"#,
    )
    .unwrap();
    let settings = catalog.defaults(0);
    menu.match_settings = Some(settings.clone());
    menu.match_catalog = Some(catalog);
    menu.map_browser.preview_map = Some(0);
    menu.map_browser.preview = Some(straterust_engine::multiplayer::browser::Preview {
        image: Image {
            width: 64,
            height: 64,
            rgba: (0..64 * 64)
                .flat_map(|i| {
                    if (i / 8 + i / 64 / 8) % 2 == 0 {
                        [42, 78, 42, 255]
                    } else {
                        [36, 67, 36, 255]
                    }
                })
                .collect(),
        },
        size: [640, 640],
        starts: (0..8)
            .map(|i| straterust_engine::sim::StartLocation {
                player: PlayerId(i),
                position: straterust_engine::sim::Position {
                    x: 80 + i32::from(i % 4) * 160,
                    y: 80 + i32::from(i / 4) * 480,
                },
            })
            .collect(),
    });
    menu.page = Page::Lobby;
    menu.message = "Connected. Choose your race, then mark Ready.".into();
    menu.lobby = Some((
        0,
        lobby::View {
            host: 0,
            running: false,
            map: 0,
            players: (0..8)
                .map(|i| {
                    Some(lobby::Player {
                        ready: i % 2 == 0,
                        name: if i == 0 {
                            "Commander".into()
                        } else {
                            format!("Player {}", i + 1)
                        },
                    })
                })
                .collect(),
            settings,
        },
    ));
    menu
}
fn conversation() -> Chat {
    let mut chat = Chat::default();
    for (channel, name, text) in [
        (Channel::All, "Commander", "Hello everyone!"),
        (Channel::All, "Opponent", "Ready when you are."),
        (
            Channel::Team,
            "Teammate",
            "Let's take the northern passage and build near the gold mine.",
        ),
    ] {
        chat.receive(Message {
            player: PlayerId(0),
            name: name.into(),
            channel,
            text: text.into(),
        })
        .unwrap();
    }
    chat.begin();
    chat.channel = Channel::Team;
    chat.draft = "I'll defend our base.".into();
    chat
}
#[test]
fn eight_named_slots_chat_and_controls_have_separate_areas_at_each_scale() {
    let menu = lobby_menu();
    assert_eq!(menu.slot_markers().len(), 8);
    for marker in menu.slot_markers() {
        assert!(
            menu.choices(&Config::default())
                .iter()
                .any(|c| c.pick == Pick::SelectSlot(marker.slot))
        );
        for other in menu
            .slot_markers()
            .into_iter()
            .filter(|m| m.slot != marker.slot)
        {
            let [x, y, w, h] = marker.rect;
            let [ox, oy, ow, oh] = other.rect;
            assert!(x + w <= ox || x >= ox + ow || y + h <= oy || y >= oy + oh);
        }
    }
    assert!(menu.slot_ready(0));
    assert!(!menu.slot_ready(1));
    for size in [[640., 480.], [1100., 760.], [1920., 1296.]] {
        let [cx, cy, cw, ch] = menu_rect([24, 350, 592, 80], size);
        for c in menu.choices(&Config::default()) {
            let [x, y, w, h] = menu_rect(c.button.rect, size);
            assert!(
                x + w <= cx || x >= cx + cw || y + h <= cy || y >= cy + ch,
                "chat overlaps {:?}",
                c.pick
            );
            assert!(x >= 0. && y >= 0. && x + w <= size[0] && y + h <= size[1]);
        }
    }
    let chat = conversation();
    for dpi in [1., 2.] {
        let mut scene = crate::gpu::Scene {
            width: (640. * dpi) as u32,
            height: (480. * dpi) as u32,
            clear: 0,
            commands: Vec::new(),
        };
        draw_menu(
            &mut scene,
            &menu,
            &Config::default(),
            [-1000.; 2],
            0,
            dpi,
            true,
            Some(&chat),
            false,
        );
        assert!(!scene.commands.is_empty());
    }
}

#[test]
fn ready_toggle_and_all_position_pickers_fit_without_overlapping_controls() {
    let mut menu = lobby_menu();
    menu.match_catalog.as_mut().unwrap().modes = vec![GameType::Teams];
    menu.selected_slot = Some(0);
    for ready in [false, true] {
        menu.lobby.as_mut().unwrap().1.players[0]
            .as_mut()
            .unwrap()
            .ready = ready;
        let button = menu
            .choices(&Config::default())
            .into_iter()
            .find(|c| c.pick == Pick::Ready)
            .unwrap();
        assert_eq!(
            button.button.label,
            if ready { "Ready" } else { "Not ready" }
        );
    }
    for field in [None, Some(0), Some(1), Some(2)] {
        menu.slot_field = field;
        let choices = menu.choices(&Config::default());
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
fn old_messages_fade_from_play_but_remain_in_open_history() {
    let mut chat = conversation();
    chat.close();
    for (time, _) in &mut chat.history {
        *time -= std::time::Duration::from_secs(20);
    }
    let mut pixels = vec![0x243020; 640 * 480];
    let paint = |pixels: &mut [u32], chat: &Chat| {
        super::super::chat::paint(
            &mut Canvas {
                width: 640,
                height: 480,
                pixels,
                scene: None,
                scale: 1.,
            },
            chat,
            [16., 56., 560., 220.],
            1.,
            false,
        );
    };
    paint(&mut pixels, &chat);
    assert!(pixels.iter().all(|p| *p == 0x243020));
    chat.begin();
    paint(&mut pixels, &chat);
    assert!(pixels.contains(&0x89dfb0));
    assert_eq!(chat.history.len(), 3);
}

#[test]
#[ignore = "writes public chat UI review images"]
fn chat_interface_review() {
    let config = Config::default();
    let chat = conversation();
    for (label, width, height) in [("small", 640usize, 480usize), ("large", 1920, 1296)] {
        for page in [Page::Lobby, Page::PlayerName] {
            let mut menu = lobby_menu();
            menu.page = page.clone();
            menu.name_input = "Commander".into();
            menu.message.clear();
            let mut pixels = vec![0; width * height];
            paint(
                &mut Canvas {
                    width,
                    height,
                    pixels: &mut pixels,
                    scene: None,
                    scale: 1.,
                },
                &menu,
                &config,
                [-1000.; 2],
                0,
                [width as f64, height as f64],
                false,
                Some(&chat),
                false,
            );
            let screen = if page == Page::Lobby { "lobby" } else { "name" };
            write_ppm(
                &format!("/tmp/straterust-chat-{screen}-{label}.ppm"),
                width,
                height,
                &pixels,
            );
        }
        let mut pixels = vec![0x243020; width * height];
        super::super::chat::paint(
            &mut Canvas {
                width,
                height,
                pixels: &mut pixels,
                scene: None,
                scale: 1.,
            },
            &chat,
            [16., 56., (width as f64 - 32.).min(560.), 220.],
            1.,
            false,
        );
        write_ppm(
            &format!("/tmp/straterust-chat-game-{label}.ppm"),
            width,
            height,
            &pixels,
        );
    }
}
fn write_ppm(path: &str, width: usize, height: usize, pixels: &[u32]) {
    use std::io::Write;
    let mut file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    writeln!(file, "P6\n{width} {height}\n255").unwrap();
    for p in pixels {
        file.write_all(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8])
            .unwrap();
    }
}
