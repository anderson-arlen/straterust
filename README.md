# StrateRust

StrateRust is an open-source real-time strategy engine written in Rust, with
support for content imported from **StarCraft** and **Warcraft II: Battle.net Edition**.

You can play the included original-content demos without importing game assets, or
import your own games to play their campaigns. StarCraft imports include ten
missions for each race; Warcraft II imports include both original campaigns and
both Beyond the Dark Portal campaigns (52 missions).
The client supports modern window sizes, widescreen, high-DPI displays and classic
RTS controls. Imported graphics, voices, music and maps stay on your computer;
they are not included in this repository.

The project is in active development on **Linux**. Game compatibility is
partial, multiplayer supports two-player direct/LAN sessions, and Windows/macOS
support is not yet verified. Expect gameplay differences and unfinished unit mechanics.

## Build and run

You'll need a current stable Rust toolchain, a C compiler, and an X11 or Wayland
desktop with a Vulkan or OpenGL graphics driver. The client also needs the desktop
and ALSA development libraries. On Debian/Ubuntu:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libxkbcommon-x11-0 libwayland-dev libx11-dev libasound2-dev
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

Run without arguments to choose an installed game or **Import a game**:

```sh
cargo run --release --locked -p straterust-client
```

Add `--package-dir /path/to/games` to search another directory. Each game can
supply its own `menus.ron` with screen layouts, navigation, animated artwork, music and
campaign entries; packages without one get basic menus. Options include audio
levels, display size, fullscreen, scrolling, frame rate and local game speed.
Settings persist in `local/client-settings.ron`, or the file passed with `--config`.
An on-screen counter shows actual FPS, average frame time and the slowest frame
over roughly the last half-second, including during play.

## Import a game

From the first menu choose **Import a game**, then **StarCraft** or **Warcraft II**.
Choose an ISO or EXE file, or a directory containing the game files. Conversion
runs in the background and reports progress. Selected installers are read as
archives; they are never executed.

Install the source chooser and conversion utilities first. On Debian/Ubuntu:

```sh
sudo apt install zenity ffmpeg innoextract 7zip
```

KDE's `kdialog` can replace Zenity. StarCraft uses FFmpeg for portraits and
animated menus. Warcraft II uses innoextract for GOG installers and 7-Zip for ISOs.
Importers are bundled with the client; no separate importer download is needed.

Each importer owns one installation, named **StarCraft** or **Warcraft II**.
Reimporting replaces that installation only after the complete conversion passes
validation; failed imports preserve the previous installation.

| Platform | Installed games |
| --- | --- |
| Linux | `${XDG_DATA_HOME:-~/.local/share}/straterust/games/` |
| Windows | `%LOCALAPPDATA%/straterust/games/` |
| macOS | `~/Library/Application Support/straterust/games/` |

These paths do not depend on where you run the client. Older development imports
remain in `local/packages/`; the launcher no longer scans that directory by
default. Delete unwanted old exports there yourself, or use `--package-dir` to
keep displaying them. The importer does not remove them.

The same fixed-name installation is available from the command line:

```sh
cargo run --release --locked -p straterust-importers -- \
  starcraft --source /path/to/STARCRAFT.iso
cargo run --release --locked -p straterust-importers -- \
  warcraft2 --source /path/to/setup_warcraft_ii.exe
```

StarCraft supports the original English **Windows retail v1.00** disc, its
`INSTALL.EXE`, or a directory containing that installer. An installed game
containing only `stardat.mpq` is not supported. Choose the imported game, then
**Single Player** and a race. Press **Enter** to start after briefing and advance
after victory. Direct launches on Linux can use:

```sh
cargo run --release --locked -p straterust-client -- \
  --campaign "$HOME/.local/share/straterust/games/StarCraft" --mission 5
```

Add `/zerg` or `/protoss` to that campaign path for the other races. If you set
`XDG_DATA_HOME`, substitute that directory for `$HOME/.local/share`.

Warcraft II supports English **Battle.net Edition**, including the GOG 2.02
installer, a Battle.net Edition disc image, or a directory containing
`War2Dat.mpq` and `Support/TOMES/TOME.1` and `TOME.2`. Retain `Install.mpq` in an
installed directory for music and narrated briefings. Its menu offers Human and
Orc campaigns from **Tides of Darkness** and **Beyond the Dark Portal**.

Imports include original maps, graphics, command icons, cursors, effects, voices
and music. Units and buildings use the original player colors in both games.
Compatibility remains under development; loading every mission is
not evidence of a verified complete playthrough. StarCraft still has differences
in caster spells, ammunition, some upgrades, blending and AI timing. Warcraft II includes the original Human/Orc side consoles, naval economy,
transports, research, spells and campaign objectives. Exact original animation
cadence and AI schedules still differ in places.

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
| F5 / Space / F11 | Restart a local mission / pause / toggle fullscreen |
| Esc / F10 | Open the game menu; press again to return or go back |

The command panel shows available actions, hotkeys, costs and requirements.
Group commands issue ordinary orders to each selected unit.
Click a passenger in a Bunker or transport to unload it. Use **U** to unload all:
transports ask for a destination; Bunkers unload nearby immediately.
Imported StarCraft controls use **U** for burrow/unburrow.
Imported command keys match the original game's English controls, with **C / D**
kept as the cloak/decloak split. **Esc** cancels targeting, closes a build menu,
or cancels the selected building's active job before opening the game menu;
**F10** opens the menu directly.
The client, importer and headless runner accept `--help` for
command-line options.

Local games can be saved through **Esc / F10 → Save Game**. Choose one of seven
slots and confirm before overwriting an existing save. Use **Load Game** from
the selected game's main menu or in-game menu to resume. Saves keep mission
progress, orders, resources, fog, research, selections and control groups.
They are stored in `local/saves/`, or a `saves/` directory beside a custom
`--config` file. Keep the same game package installed. Local saves can migrate
across engine and gameplay fixes, preserving progress while using updated rules.
Older saves remain untouched when loaded. Incompatible changes, such as replacing
the map or removing a saved unit type, receive a specific load error.
Save/load is unavailable in multiplayer and scripted scenario playback.

## LAN multiplayer

Choose **Multiplayer** from a game's menu, select a package, then host or join an
IP address. **Find LAN games** discovers hosts on the local network. To launch
directly, run these on the host and the other player's computer:

```sh
cargo run --release --locked -p straterust-client -- \
  --package content/lan-demo --host 0.0.0.0:6112
cargo run --release --locked -p straterust-client -- \
  --package content/lan-demo --join 192.168.1.10:6112
```

Replace `192.168.1.10` with the host's address. Allow TCP port 6112 for play and
UDP port 6113 for discovery. Both players need the same engine version and game
rules; their graphics and sound can differ. The host supplies the map, so the
joining player does not need that map installed. Transfers contain bounded native
terrain, tile layouts and images only. Downloaded maps cannot install or execute
client programs, libraries or scripts; mission logic runs on the server.

Clicking a discovered host automatically chooses compatible installed rules,
including rules bundled with campaigns. The selected local map does not need to
match the host's map. Hosts with no compatible installed rules are labeled
**rules unavailable**. Waiting for a guest and pausing keep the connection alive.

The original `lan-demo` map supports gathering, building and fighting with fog
of war. Clients receive their own units and visible enemy appearances, without
enemy queues, orders or hidden passenger contents. Single-player uses the same
server session and filtered player updates.

The host can pause/resume the match with **Space**. Opening a LAN game menu does
not pause the other player. A disconnect ends the match with a diagnostic.
Completed matches show victory/defeat and each player's unit, structure and
gathered-resource statistics. Dismiss the results with **Enter**, **Esc**, or the
button to return to the multiplayer lobby. **F5** restarts local missions only.
Automatic NAT traversal, reconnect and host migration are unavailable.

Add `--record-replay local/match.ron` to the host command to save an authoritative
replay. Verify it against the host's map package with:

```sh
cargo run --locked -p straterust-tools --bin straterust-session -- \
  replay --package content/lan-demo --replay local/match.ron
```

Replay files contain the full server state and are for sharing after a match.
The same tool provides headless `host`, `join` and `discover` commands; use `--help`.

## Development

Public fixtures, builds and checks do not require proprietary game assets:

```sh
cargo run --locked -p straterust-tools --bin straterust-headless
make check
```

`make check` runs formatting, linting, tests and deterministic replay checks.
Use `make headless` for engine/tools tests without building the graphical client.

## License

StrateRust code and original demo content use the [MIT license](LICENSE).
Imported game content remains subject to its original owners' rights.
