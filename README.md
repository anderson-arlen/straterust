# StrateRust

StrateRust is an open-source real-time strategy engine written in Rust, with
support for content imported from **StarCraft** and **Warcraft II: Battle.net Edition**.

You can play the included original-content demos without importing game assets, or
import your own games to play their campaigns and original multiplayer maps. StarCraft imports include ten
missions for each race; Warcraft II imports include both original campaigns and
both Beyond the Dark Portal campaigns (52 missions).
The client supports modern window sizes, widescreen, high-DPI displays and classic
RTS controls. Imported graphics, voices, music and maps stay on your computer;
they are not included in this repository.

The project is in active development on **Linux**. Game compatibility is
partial, multiplayer supports up to eight players through direct/LAN lobbies, and Windows/macOS
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

Development and test builds use limited debug information and disable incremental
caches to limit disk usage. `cargo clean --profile dev` removes generated debug
builds while preserving the release build and game data.

Start the included economy-and-combat demo:

```sh
cargo run --release --locked -p straterust-client -- --package content/terran-demo
```

Gather minerals, construct a base and train an army to defeat the opponent.
For a demo with an opponent that builds and launches attack waves, use
`--package content/ai-demo` instead.

Build and run the launcher to choose an installed game or **Import a game**:

```sh
make run
```

The client's `--package-dir /path/to/games` option adds another search directory. Each game can
supply its own `menus.ron` with screen layouts, navigation, animated artwork, music and
campaign entries; packages without one get basic menus. Options include audio
levels, display size, fullscreen, scrolling, frame rate and local game speed.
Settings persist in `local/client-settings.ron`, or the file passed with `--config`.
During play, running debug graphs show FPS and network upload/download rates
over the last 24 seconds. Network rates count application bytes actually read
and written after compression, including message framing. The host excludes
its own local connection. The launcher also displays a small frame-rate counter.
Press **~ / backquote** to slide down the console over the top half of the screen.
Type `debug off` or `debug on` to hide or show the graphs; `fps on/off` and
`net on/off` control them separately. **Tab** completes fuzzy command matches
and cycles suggestions; **Up/Down** recalls commands, **PgUp/PgDn** scrolls output,
and **Esc** or **~** closes the console. `help`, `status` and `clear` are available.
Opening the console captures input without pausing the game.

LAN guests run the engine's gameplay tick on their last disclosed state between
compressed, field-level server updates sent up to ten times per second. Fresh
updates replace speculative state and replay pending movement commands. Worker
transitions, collision and training timers use the same engine code as the host.
The host receives its own view at the full simulation rate. Its authoritative
updates use sequence ordering without a remote-correction age filter. Catch-up
work yields to update delivery after one tick interval. The server retains
authority over results and visibility; announcements use confirmed state only.
Fog retains the last confirmed resource appearance. Unit animations advance
locally through connection interruptions. Timed ping/pong exchanges estimate
clock offset; live corrections older than 250 ms are discarded without an ACK,
so their changes remain pending for a fresh server update. `max_server_frame_age_ms` in
the client settings adjusts that limit (100–1000 ms). Movement prediction is
bounded to two seconds without a fresh correction. Live state, commands, ACKs
and clock probes use UDP with session tokens. Commands retain their IDs through
retries; newer replacement orders cancel earlier orders for that unit. Queued
orders keep their intended sequence. Individual command ACKs travel in snapshots
and take effect only when the client accepts that snapshot. Missing fragments of
an old snapshot do not block a newer one. Map transfer, lobbies and chat use TCP.
The sender finishes handing a partially sent snapshot to UDP before sending the
newest pending state; newer ticks cannot endlessly restart its unsent fragments.
Connections allow 30 seconds of silence before timing out. Multiplayer peers must use
matching builds; these transport changes do not invalidate saved games.

Normal launches automatically write timestamped diagnostics and panic backtraces
to a rotating log. On Linux the path is `/var/tmp/straterust-UID/client.log`, where
`UID` is the player's numeric user ID. The directory and logs are readable by
other local users; only that player can modify them. Three older logs are retained,
each limited to 5 MiB. Concurrent launches use a separate log named with their
process ID. Logging also continues to stderr; `RUST_LOG` controls verbosity.
Multiplayer logs include periodic tick rates, worst simulation and view-generation
times, snapshot/ACK progress and compressed frame sizes; rejected remote corrections
report their age at a bounded frequency.

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
Installed games store shared artwork and audio once by content hash; maps
reference that common storage instead of creating a file for every frame in
every map. Older installations can be upgraded without reconverting maps:

```sh
target/release/straterust-importers starcraft --compact
target/release/straterust-importers warcraft2 --compact
```

If an update needs original artwork that older imports omitted, add
`--source /path/to/original-installer` to the compact command, or reimport the game.
The upgrade validates a replacement before switching installations, refreshes
importer-owned rules and presentation, and preserves saved progress. Multiplayer
players need matching updated game data and engine builds.

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
`Warcraft II BNE.exe`, `War2Dat.mpq` and `Support/TOMES/TOME.1` and `TOME.2`. Retain `Install.mpq` in an
installed directory for music and narrated briefings. Its menu offers Human and
Orc campaigns from **Tides of Darkness** and **Beyond the Dark Portal**.

Imports record the original game executable's product version and application
icon in `game.ron`. The displayed package version adds a monotonically increasing
importer build, for example `1.00+importer.42` for StarCraft or
`2.02b+importer.3` for Warcraft II. The engine version is recorded separately;
version and icon metadata do not change saved-game or multiplayer identities.
The game chooser shows the original icon and package version.

To add this metadata to an older installation without reconverting its maps,
rerun its importer with `--metadata-only`:

```sh
cargo run --release --locked -p straterust-importers -- \
  starcraft --source /path/to/STARCRAFT.iso --metadata-only
cargo run --release --locked -p straterust-importers -- \
  warcraft2 --source /path/to/setup_warcraft_ii.exe --metadata-only
```

Imports include original maps, graphics, command icons, cursors, effects, voices
and music. Units and buildings use the original player colors in both games.
Compatibility remains under development; loading every mission is
not evidence of a verified complete playthrough. StarCraft still has differences
in caster spells, ammunition, some upgrades, blending and AI timing. Warcraft II includes the original Human/Orc side consoles, naval economy,
transports, research, spells and campaign objectives. Exact original animation
cadence and AI schedules still differ in places.

## Export a game as an AppImage

On Linux, choose **Package a game** from the first menu, select an installed
game, and choose where to save its `.AppImage`. Export runs in the background and
includes the current client, shared libraries and the selected game's complete
native data (campaigns, multiplayer maps, graphics and sounds). Existing exports
are replaced only after a successful build. Your personal settings and saves
are not included.

The suggested filename includes the game version, importer build and CPU
architecture, such as `StarCraft-1.00+importer.42-x86_64.AppImage`.
Exports use the original application icon for the AppImage and its desktop
launcher, including standard icon files for package-manager extraction.
Authored games can supply their own version and icon in `game.ron`; older games
without metadata use the engine version and a default icon.

Export progress reports file counts, package counts and elapsed build time.
Artwork and audio are stored once per content hash in a shared game asset file.
Maps retain their original references, resolved against that shared store. The
export avoids millions of per-map file entries; shared payloads are checked once
while every reference and package limit is verified.

The first export automatically downloads pinned
[linuxdeploy](https://github.com/linuxdeploy/linuxdeploy/releases) and
[AppImage runtime](https://github.com/AppImage/type2-runtime/releases) releases
and verifies their checksums. They are cached in
`${XDG_CACHE_HOME:-~/.cache}/straterust/appimage/`; later exports can run offline.
No manual build-tool installation or environment setup is needed on x86-64 or
ARM64 Linux. Zenity or KDialog supplies the save dialog. Use a release client to
avoid distributing a large debug build. For custom offline tool installations,
`STRATERUST_LINUXDEPLOY` and `LDAI_RUNTIME_FILE` can override the downloaded tools.

The same export is available without a graphical session:

```sh
target/release/straterust-client --list-games
target/release/straterust-client --export-game /path/to/game --output /path/to/Game.AppImage
```

The AppImage opens directly into that game's menu. It can be relocated or
imported into a package manager such as PacSmith. Settings and saves live under
`${XDG_DATA_HOME:-~/.local/share}/straterust/profiles/<game-id>/`, independently
of the bundle's location and version. Run with `--appimage-extract-and-run` if
FUSE is unavailable. The destination needs the same CPU architecture, a
compatible glibc and working graphics drivers; export uses the running client
and host libraries, so an export built on a recent distribution is not guaranteed
to run on older distributions. No Rust toolchain or source installation is
needed to play the exported game.

## Controls

| Input | Action |
| --- | --- |
| Left click / drag | Inspect a unit, building or resource / select a group of units |
| Right click | Move, attack, gather, repair, load a transport or set a rally point |
| Minimap | Left click/drag pans; right click issues orders; active commands accept left click |
| Shift while ordering | Queue orders |
| A, then left click | Attack-move |
| B / V with a worker selected | Open basic / advanced building menus |
| Ctrl+0–9 / 0–9 | Save / recall a control group |
| Arrow keys / mouse wheel / Home | Pan / zoom / center on your start |
| F5 / Space / F11 | Restart a local mission / pause / toggle fullscreen |
| Esc / F10 | Open the game menu; press again to return or go back |
| ~ / backquote | Open or close the debug console |

The command panel shows available actions, hotkeys, costs and requirements.
Hover over a command to read its role or effect alongside those costs.
Completed research keeps its full-color icon with a green checkmark and a
**Researched** tooltip, distinct from an unavailable upgrade.
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

Choose **Multiplayer** from a game's menu to join an IP address or find a LAN
game. Guests enter the host's lobby without choosing a map or match settings.
To host, choose **Create game**, select a map, configure match settings and
player slots, then choose **Host match**. The listening port defaults to 6112.
Matching terrain layouts share one map entry, with their original
settings versions selectable beside a terrain preview. Numbered markers show
starting locations and yellow markers show resource deposits. Player counts and
dimensions have dropdown filters. Choose **Use this map** to confirm a version;
**Version settings** shows its original defaults.
Click a starting position on the lobby map to choose its player, race, team and
an unused player color. Color swatches match units and the minimap in the match.
The host can move or swap connected players; guests can join vacant positions
and change their own race, team and color. The **Ready / Not ready** toggle shows a
green checkmark or waiting hourglass, also shown beside each position.
Readiness stays set through settings changes and seat moves until you toggle it
off. Newly joined players start not ready.
Everyone marks themselves ready before the host chooses **Start match**.
Unoccupied open positions run AI when the match starts and remain joinable;
computer positions also accept live replacements, while closed positions stay empty.

Click **Name** before hosting/joining, or **Change name** in the lobby, to choose
your player name. It is saved with your client settings. Lobby and in-game chat
use **Enter** to open/send, **Tab** to switch between **All** and **Team** when
teams are enabled, and **Esc** to cancel. **Page Up / Page Down** browse recent
messages while chat is open. Team messages go only to teammates; All reaches
everyone in the lobby or match. Typing captures gameplay hotkeys without pausing
the match. Chat is not recorded in saved games or replays.

Match settings follow the selected game. Warcraft II offers map-default/low/
medium/high resources, map-default/one-worker starts, fixed/random locations,
fog, speed and all four terrain sets. StarCraft offers its three races and Random,
standard starting armies. Choose **Free for all** or **Teams** and assign each
position to one of up to eight teams. Teammates keep separate armies, resources
and research. Changing match settings or assignments resets readiness.
Reimport an older installation to add its multiplayer catalog.

Warcraft II stores one layout per multiplayer map. The host applies the selected
shared tileset in memory and sends its terrain artwork through the map transfer.
Rules, graphics definitions and audio definitions use shared game storage.

Imports include 69 StarCraft multiplayer maps and 181 Warcraft II maps from the
disc/installer. Some special **Use Map Settings** scenarios require unsupported
script actions, placements or custom unit statistics; the lobby reports the
specific limitation. Their terrain remains available for ordinary matches.
Warcraft II cheat-code entry is currently unavailable.

**Find LAN games** discovers hosts for the selected game on the local network. To launch
directly, run these on the host and the other player's computer:

```sh
cargo run --release --locked -p straterust-client -- \
  --package content/lan-demo --host 0.0.0.0:6112
cargo run --release --locked -p straterust-client -- \
  --package content/lan-demo --join 192.168.1.10:6112
```

Replace `192.168.1.10` with the host's address. Allow TCP and UDP port 6112 for play
and UDP port 6113 for discovery. All players need the same engine version and game
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
not pause other players. Broken connections reconnect automatically for up to
30 seconds, retaining command IDs and snapshot acknowledgements. Leaving or
exhausting that grace hands the slot to the computer while the match continues.
LAN browsing distinguishes lobbies from running matches and shows available slots;
join a running match to choose a computer-controlled slot. Players can choose open
lobby slots, including the host, whose Start and pause controls follow its identity.
The host must keep the match open; host migration is not supported.
Computer difficulty is selectable in match settings. Warcraft II's native plans
build an economy and military, launch early attacks, research upgrades and field
ground and air forces. Disconnected starts also build a naval economy, warships
and transports for landings. Standing army limits include units already deployed,
and missing recruits do not hold back ready groups.
Completed matches show victory/defeat and each player's unit, structure and
gathered-resource statistics. Dismiss the results with **Enter**, **Esc**, or the
button to return to the multiplayer lobby. **F5** restarts local missions only.
Automatic NAT traversal and host migration are unavailable.

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
