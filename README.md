# StrateRust

StrateRust is an open-source real-time strategy engine written in Rust, with
support for content imported from the original **StarCraft**.

You can play the included original-content demos without importing game assets, or
import your own StarCraft disc to play the first five Terran campaign missions.
The client supports modern window sizes, widescreen, high-DPI displays and classic
RTS controls. Imported graphics, voices, music and maps stay on your computer;
they are not included in this repository.

The project is in active development on **Linux**. StarCraft compatibility is
partial, multiplayer is not implemented, and Windows/macOS support is not yet
verified. Expect gameplay differences and unfinished unit mechanics.

## Build and run

You'll need a current stable Rust toolchain, a C compiler, and an X11 or Wayland
desktop with a Vulkan or OpenGL graphics driver. The client also needs the desktop
and ALSA development libraries. On Debian/Ubuntu:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev libx11-dev libasound2-dev
```

Run the following commands from the repository root.

The client executable is `target/release/straterust-client`. Cargo can leave an
older `stratarust-client` behind after the project rename; use the current spelling.

Start the included economy-and-combat demo:

```sh
cargo run --release --locked -p straterust-client -- --package content/terran-demo
```

Gather minerals, construct a base and train an army to defeat the opponent.
For a demo with an opponent that builds and launches attack waves, use
`--package content/ai-demo` instead.

Run without arguments to choose a game from detected packages in `content/` and
`local/packages/`:

```sh
cargo run --release --locked -p straterust-client
```

Add `--package-dir /path/to/games` to search another directory. Each game can
supply its own `menus.ron` with screen layouts, navigation, animated artwork and
campaign entries; packages without one get basic menus. Options include audio
levels, display size, fullscreen, scrolling, frame rate and local game speed.
Settings persist in `local/client-settings.ron`, or the file passed with `--config`.
An on-screen counter shows actual FPS, average frame time and the slowest frame
over roughly the last half-second, including during play.

## Play StarCraft

The importer currently targets the original English **Windows retail v1.00**
disc. You'll need your own disc image or installer and **FFmpeg** for portrait
and menu animation conversion. Replace the source path below with your ISO:

```sh
cargo run --release --locked -p straterust-import-starcraft -- import-campaign \
  --source /path/to/STARCRAFT.iso --output local/packages/terran-campaign-v4
cargo run --release --locked -p straterust-client -- \
  --campaign local/packages/terran-campaign-v4
```

The campaign includes **Wasteland**, **Backwater Station**, **Desperate Alliance**,
**The Jacobs Installation**, and **Revolution**. Press **Enter** to start after
briefing and to advance after victory. Add `--mission 3` to the client command to
start at Desperate Alliance; mission numbers run from 1 to 5.

The source can also be `INSTALL.EXE` or a directory containing that installer;
an installed game directory containing only `stardat.mpq` is not supported.
Keep imported assets under the ignored `local/` directory. To update an existing
campaign's assets and rules, use `update-campaign` in place of `import-campaign`.
Use `update-menus` to add the original animated StarCraft frontend to an existing
campaign without reimporting its missions. Select the campaign from the launcher,
choose Single Player, then Terran and a mission.

## Controls

| Input | Action |
| --- | --- |
| Left click / drag | Inspect a unit, building or resource / select a group of units |
| Right click | Move, attack, gather, repair, load a transport or set a rally point |
| Shift while ordering | Queue orders |
| A, then left click | Attack-move |
| B / V with a worker selected | Open basic / advanced building menus |
| Ctrl+0–9 / 0–9 | Save / recall a control group |
| Arrow keys / mouse wheel / Home | Pan / zoom / center on your start |
| F5 / Space / F11 | Restart the mission / pause / toggle fullscreen |
| Esc / F10 | Open the game menu; press again to return or go back |

The command panel shows available actions, hotkeys, costs and requirements.
Click a passenger in a Bunker or transport to unload it. Use **U** to unload all:
transports ask for a destination; Bunkers unload nearby immediately.
The client, importer and headless runner accept `--help` for
command-line options.

## Development

Public fixtures, builds and checks do not require StarCraft assets:

```sh
cargo run --locked -p straterust-tools --bin straterust-headless
make check
```

`make check` runs formatting, linting, tests and deterministic replay checks.
Use `make headless` for engine/tools tests without building the graphical client.

## License

StrateRust code and original demo content use the [MIT license](LICENSE).
Imported game content remains subject to its original owners' rights.
