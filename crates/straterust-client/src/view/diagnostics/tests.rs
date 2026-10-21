use super::*;
use std::time::Duration;

#[test]
#[cfg(target_os = "linux")]
#[ignore = "opens native GPU windows and captures the console, graphs and startup modal"]
fn native_debug_ui_review() {
    use crate::*;
    use winit::platform::wayland::EventLoopBuilderExtWayland;
    let Some(mode) = std::env::var_os("STRATERUST_DEBUG_REVIEW") else {
        for mode in ["console", "graphs", "starting"] {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "view::diagnostics::tests::native_debug_ui_review",
                    "--nocapture",
                ])
                .env("STRATERUST_DEBUG_REVIEW", mode)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            while child.try_wait().unwrap().is_none() {
                if Instant::now() >= deadline {
                    child.kill().unwrap();
                    let _ = child.wait();
                    panic!("native review window did not redraw; keep the desktop active");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let result = child.wait_with_output().unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            eprintln!("{}", String::from_utf8_lossy(&result.stdout));
        }
        return;
    };
    let mode = mode.to_string_lossy();
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-demo");
    let config = Config {
        width: 1280,
        height: 720,
        audio: false,
        ..Config::default()
    };
    let mut app = if mode == "starting" {
        App::load_network(&directory, config)
    } else {
        App::load(&directory, config, None)
    }
    .unwrap();
    if mode == "console" {
        app.diagnostics.edit(KeyCode::Backquote, None, false);
        app.diagnostics.edit(KeyCode::KeyD, Some("dbg of"), false);
    }
    if mode == "starting" {
        app.start_network(&directory, "127.0.0.1:0".parse().unwrap(), true, None)
            .unwrap();
    }
    app.benchmark_frames = Some(60);
    app.screenshot = Some(PathBuf::from(format!("/tmp/stratarust-debug-{mode}.ppm")));
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder.build().unwrap().run_app(&mut app).unwrap();
    assert!(app.failure.is_none(), "{:?}", app.failure);
    assert!(app.screenshot.as_ref().unwrap().metadata().unwrap().len() > 1000);
}
#[test]
fn debug_console_covers_only_the_top_half_and_graphs_can_be_disabled() {
    let mut debug = Diagnostics::default();
    debug.open = true;
    debug.sample(Duration::from_millis(200), [1000, 20000], 24);
    debug.input = "dbg of".into();
    debug.caret = debug.input.len();
    for [width, height] in [[640, 480], [1280, 720]] {
        let mut pixels = vec![0x334455; width * height];
        let mut canvas = Canvas {
            width,
            height,
            pixels: &mut pixels,
            scene: None,
            scale: 1.,
        };
        paint(&mut canvas, &debug, [width as f64, height as f64], 0);
        assert_eq!(pixels[width * (height / 2 - 3) + width / 2], 0x0b111a);
        assert_eq!(pixels[width * (height / 2 + 2) + width / 2], 0x334455);
        if width == 1280 {
            use std::io::Write;
            let mut file = std::fs::File::create("/tmp/stratarust-diagnostics.ppm").unwrap();
            write!(file, "P6\n{width} {height}\n255\n").unwrap();
            for pixel in &pixels {
                file.write_all(&[(*pixel >> 16) as u8, (*pixel >> 8) as u8, *pixel as u8])
                    .unwrap();
            }
        }
    }
    debug.open = false;
    debug.height = 0.;
    debug.fps = false;
    debug.network = false;
    let mut pixels = vec![0x334455; 640 * 480];
    paint(
        &mut Canvas {
            width: 640,
            height: 480,
            pixels: &mut pixels,
            scene: None,
            scale: 1.,
        },
        &debug,
        [640., 480.],
        0,
    );
    assert!(pixels.iter().all(|p| *p == 0x334455));
}

#[test]
fn download_grid_does_not_erase_a_constant_upload_curve() {
    let mut debug = Diagnostics::default();
    debug.current = crate::diagnostics::Sample {
        fps: 50.,
        up: 10.,
        down: 20.,
    };
    debug.samples = std::collections::VecDeque::from(vec![debug.current; 120]);
    let mut pixels = vec![0; 1280 * 720];
    paint(
        &mut Canvas {
            width: 1280,
            height: 720,
            pixels: &mut pixels,
            scene: None,
            scale: 1.,
        },
        &debug,
        [1280., 720.],
        0,
    );
    assert!(pixels.iter().filter(|color| **color == 0xffae69).count() > 100);
    assert!(pixels.iter().filter(|color| **color == 0x54d8c7).count() > 100);
}
