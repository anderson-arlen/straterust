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

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Config {
    width: u32,
    height: u32,
    zoom: f64,
    seed: u64,
    frames_per_second: u32,
    movement_heading_debounce_ms: u32,
    audio: bool,
    bindings: Bindings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: 1100,
            height: 760,
            zoom: 1.0,
            seed: 42,
            frames_per_second: 60,
            movement_heading_debounce_ms: 500,
            audio: true,
            bindings: Bindings::default(),
        }
    }
}

impl Config {
    fn validate(&self) -> Result<()> {
        self.bindings.validate()?;
        ensure!(
            self.movement_heading_debounce_ms <= 5000,
            "movement_heading_debounce_ms must be 0..=5000"
        );
        ensure!(
            (640..=7680).contains(&self.width) && (480..=4320).contains(&self.height),
            "window size must be 640x480 through 7680x4320"
        );
        ensure!(
            self.zoom.is_finite() && (0.25..=4.0).contains(&self.zoom),
            "zoom must be 0.25..=4"
        );
        ensure!(
            (1..=240).contains(&self.frames_per_second),
            "frames_per_second must be 1..=240"
        );
        Ok(())
    }
}

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
                    "straterust-client [--package DIR | --campaign DIR [--mission N]] [--config FILE] [--scenario FILE]\n  [--smoke-test | --benchmark-frames N] [--screenshot FILE.ppm]\n--scenario plays a fixture command schedule. --smoke-test resizes and exits automatically.\n--benchmark-frames measures native frame cadence at the configured window size and exits.\nScreenshot output requires a smoke or benchmark run. Logs go to stderr; use RUST_LOG for verbosity."
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
            "--package" | "--config" | "--scenario" | "--screenshot" => {
                let value = PathBuf::from(
                    args.next()
                        .with_context(|| format!("missing value for {arg}"))?,
                );
                match arg.as_str() {
                    "--package" => package_path = value,
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
    let config: Config = match config_path {
        Some(path) => read_ron(&path)?,
        None => Config::default(),
    };
    config.validate()?;
    let presentation_path = package_path.join("presentation.ron");
    let presentation: Presentation = if presentation_path.try_exists()? {
        read_ron(&presentation_path)?
    } else {
        log::warn!("no presentation.ron; using geometric placeholders");
        Presentation::default()
    };
    presentation.validate()?;
    let package = Package::load(&package_path)?;
    let assets = AssetPack::load(&package_path)?;
    if let Some(assets) = &assets {
        log::info!(
            "native art loaded: {}, {} animation frames (fixture simulation)",
            assets.manifest.unit_name,
            assets.frames.len()
        );
    }
    if smoke && scenario_path.is_none() {
        scenario_path = Some(package_path.join("scenario.ron"));
    }
    let scenario: Option<Scenario> = scenario_path.map(|path| read_ron(&path)).transpose()?;
    let mut app = App::new(&package, config, presentation, assets, scenario)?;
    app.load_media(&package_path)?;
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
    EventLoop::new()?.run_app(&mut app)?;
    if let Some(error) = app.failure {
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests;

mod window;
