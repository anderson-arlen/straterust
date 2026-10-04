mod audio;
mod controls;
mod gpu;
mod mission;
#[cfg(test)]
mod presentation_tests;
mod timing;
mod view;
mod visual;

use std::{
    collections::BTreeSet,
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use straterust_engine::{
    assets::AssetPack,
    content::{Campaign, Package, read_ron},
    media::MediaPack,
    scenario::{CommandQueue, Scenario},
    sim::{Command, EntityId, Order, PlayerId, Position, ResourceId, UnitOrder, UnitTypeId, World},
};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition},
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Fullscreen, Window, WindowId},
};

use audio::{Audio, Cue};
use controls::{Bindings, TargetMode, button_at};
use timing::TickClock;
use view::{Camera, Presentation, View, unit_half_size};
use visual::Visuals;

mod config;
use config::Config;
mod client;
mod menus;

const SELECTION_LIMIT: usize = 12;

#[derive(Clone)]
struct CampaignSession {
    root: PathBuf,
    manifest: Campaign,
    index: usize,
}

struct App {
    campaign: Option<CampaignSession>,
    world: World,
    simulation: Option<simulation::SimulationWorker>,
    visuals: Visuals,
    initial_world: World,
    initial_scenario: Option<Scenario>,
    presentation: Presentation,
    assets: Option<AssetPack>,
    media: Option<MediaPack>,
    mission_ui: Option<mission::MissionUi>,
    animation_elapsed: Duration,
    portrait_elapsed: Duration,
    config: Config,
    queue: CommandQueue,
    recorded: Vec<Command>,
    playback_end: Option<u64>,
    selected: BTreeSet<EntityId>,
    selected_resource: Option<ResourceId>,
    drag_start: Option<[f64; 2]>,
    last_selection_click: Option<(Instant, EntityId, [f64; 2])>,
    target_mode: Option<TargetMode>,
    build_menu: bool,
    advanced_build_menu: bool,
    minimap_drag: bool,
    groups: [BTreeSet<EntityId>; 10],
    audio: Audio,
    sequence: u64,
    camera: Camera,
    cursor: PhysicalPosition<f64>,
    keys: BTreeSet<KeyCode>,
    paused: bool,
    menu_open: bool,
    status: String,
    clock: TickClock,
    last_frame: Instant,
    next_frame: Instant,
    window: Option<Arc<Window>>,
    surface: Option<gpu::Renderer>,
    smoke: bool,
    benchmark_frames: Option<u32>,
    frames: u32,
    frame_times: Vec<f64>,
    frame_intervals: Vec<f64>,
    frame_stats: Option<timing::FrameStats>,
    resize_events: u32,
    observed_sizes: BTreeSet<(u32, u32)>,
    screenshot: Option<PathBuf>,
    failure: Option<anyhow::Error>,
}

mod runtime;
mod selection;
mod session;
mod simulation;
impl App {}

fn home_position(world: &World) -> Position {
    world
        .map()
        .start_locations
        .iter()
        .find(|start| start.player == PlayerId(0))
        .map(|start| start.position)
        .or_else(|| {
            world
                .state()
                .entities
                .iter()
                .find(|entity| entity.owner == PlayerId(0))
                .map(|entity| entity.position)
        })
        .unwrap_or(Position {
            x: world.map().width / 2,
            y: world.map().height / 2,
        })
}

fn queued_order(order: Order) -> Order {
    let (entity, order) = match order {
        Order::Move { entity, target } => (entity, UnitOrder::Move { target }),
        Order::Attack { entity, target } => (entity, UnitOrder::Attack { target }),
        Order::AttackMove { entity, target } => (entity, UnitOrder::AttackMove { target }),
        Order::Patrol { entity, target } => (entity, UnitOrder::Patrol { target }),
        Order::Gather { entity, resource } => (entity, UnitOrder::Gather { resource }),
        Order::Repair { entity, target } => (entity, UnitOrder::Repair { target }),
        Order::Load { entity, target } => (entity, UnitOrder::Load { target }),
        Order::Land { entity, target } => (entity, UnitOrder::Land { target }),
        Order::UnloadAt { entity, target } => (entity, UnitOrder::UnloadAt { target }),
        Order::PlaceMine { entity, target } => (entity, UnitOrder::PlaceMine { target }),
        Order::Hold { entity } => (entity, UnitOrder::Hold),
        other => return other,
    };
    Order::Queue { entity, order }
}

fn main() -> std::process::ExitCode {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn,naga=warn"),
    )
    .format_timestamp(None)
    .init();
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            log::error!("{error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut package_path = PathBuf::from("content/fixtures");
    let mut campaign_path = None;
    let mut package_specified = false;
    let mut package_roots = vec![PathBuf::from("content"), PathBuf::from("local/packages")];
    let mut first_mission = 1_usize;
    let mut config_path = None;
    let mut scenario_path = None;
    let mut smoke = false;
    let mut benchmark_frames = None;
    let mut screenshot = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "straterust-client [--package DIR | --campaign DIR [--mission N]] [--config FILE] [--scenario FILE]\n  [--smoke-test | --benchmark-frames N] [--screenshot FILE.ppm]\n--scenario plays a fixture command schedule. --smoke-test resizes and exits automatically.\n--benchmark-frames measures native frame cadence at the configured window size and exits.\nWithout a package/campaign, choose a detected game from the launcher.\n--package-dir DIR adds a package search root. Esc/F10 opens the in-game menu.\nScreenshot output requires a smoke or benchmark run. Logs go to stderr; use RUST_LOG for verbosity."
                );
                return Ok(());
            }
            "--mission" => {
                first_mission = args.next().context("missing mission number")?.parse()?;
            }
            "--campaign" => {
                campaign_path = Some(PathBuf::from(
                    args.next().context("missing campaign directory")?,
                ));
            }
            "--smoke-test" => smoke = true,
            "--benchmark-frames" => {
                let count: u32 = args
                    .next()
                    .context("missing benchmark frame count")?
                    .parse()?;
                ensure!(
                    (30..=100_000).contains(&count),
                    "benchmark frame count must be 30..=100000"
                );
                benchmark_frames = Some(count);
            }
            "--package" | "--package-dir" | "--config" | "--scenario" | "--screenshot" => {
                let value = PathBuf::from(
                    args.next()
                        .with_context(|| format!("missing value for {arg}"))?,
                );
                match arg.as_str() {
                    "--package" => {
                        package_path = value;
                        package_specified = true;
                    }
                    "--package-dir" => package_roots.push(value),
                    "--config" => config_path = Some(value),
                    "--scenario" => scenario_path = Some(value),
                    _ => screenshot = Some(value),
                }
            }
            _ => bail!("unknown argument {arg}; use --help"),
        }
    }
    ensure!(
        smoke || benchmark_frames.is_some() || screenshot.is_none(),
        "--screenshot requires --smoke-test or --benchmark-frames"
    );
    ensure!(
        !smoke || benchmark_frames.is_none(),
        "choose smoke or benchmark, not both"
    );
    ensure!(
        campaign_path.is_some() || first_mission == 1,
        "--mission requires --campaign"
    );
    ensure!(
        !package_specified || campaign_path.is_none(),
        "choose --package or --campaign"
    );
    let frontend = !package_specified
        && campaign_path.is_none()
        && !smoke
        && benchmark_frames.is_none()
        && scenario_path.is_none();
    let campaign = if let Some(root) = campaign_path {
        let manifest = Campaign::load(&root)?;
        ensure!(
            (1..=manifest.missions.len()).contains(&first_mission),
            "mission number outside campaign"
        );
        package_path = root.join(&manifest.missions[first_mission - 1].package);
        Some(CampaignSession {
            root,
            manifest,
            index: first_mission - 1,
        })
    } else {
        None
    };
    let settings_path = config_path
        .clone()
        .unwrap_or_else(|| PathBuf::from("local/client-settings.ron"));
    let config: Config = if config_path.is_some() || settings_path.is_file() {
        read_ron(&settings_path)?
    } else {
        Config::default()
    };
    config.validate()?;
    let mut client = client::Client::new(config.clone(), settings_path, package_roots);
    if frontend {
        EventLoop::new()?.run_app(&mut client)?;
        return client.finish();
    }
    if smoke && scenario_path.is_none() {
        scenario_path = Some(package_path.join("scenario.ron"));
    }
    let scenario: Option<Scenario> = scenario_path.map(|path| read_ron(&path)).transpose()?;
    let mut app = App::load(&package_path, config, scenario)?;
    let game_dir = campaign
        .as_ref()
        .map(|c| c.root.clone())
        .unwrap_or_else(|| {
            package_path
                .parent()
                .filter(|p| p.join("campaign.ron").is_file())
                .unwrap_or(&package_path)
                .to_path_buf()
        });
    let game = menus::catalog::GameEntry::read(&game_dir)?;
    app.campaign = campaign;
    app.smoke = smoke;
    if (smoke || benchmark_frames.is_some())
        && let Some(mission) = &mut app.mission_ui
    {
        mission.start(&mut app.audio);
    }
    if smoke {
        // Exercise group status graphics and imported portraits on the GPU too.
        for entity in app
            .world
            .state()
            .entities
            .iter()
            .filter(|entity| {
                entity.owner == PlayerId(0)
                    && app
                        .world
                        .unit_type(entity.unit_type)
                        .is_some_and(|unit| !unit.structure)
            })
            .take(SELECTION_LIMIT)
        {
            app.selected.insert(entity.id);
        }
    }
    app.benchmark_frames = benchmark_frames;
    app.screenshot = screenshot;
    client.direct(app, game)?;
    EventLoop::new()?.run_app(&mut client)?;
    client.finish()
}

#[cfg(test)]
mod tests;

mod window;
