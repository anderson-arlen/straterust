# StrateRust

StrateRust is an open-source real-time strategy engine written in Rust, with
support for content imported from the original **StarCraft**.

You can play the included original-content demos without importing game assets, or
import your own StarCraft disc to play the first five missions of each Terran,
Zerg and Protoss campaign.
The client supports modern window sizes, widescreen, high-DPI displays and classic
RTS controls. Imported graphics, voices, music and maps stay on your computer;
they are not included in this repository.

The project is in active development on **Linux**. StarCraft compatibility is
partial, multiplayer supports two-player direct/LAN sessions, and Windows/macOS
support is not yet verified. Expect gameplay differences and unfinished unit mechanics.

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
supply its own `menus.ron` with screen layouts, navigation, animated artwork, music and
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
  --source /path/to/STARCRAFT.iso --output local/packages/starcraft-campaigns-v1
cargo run --release --locked -p straterust-client -- \
  --campaign local/packages/starcraft-campaigns-v1
```

The import includes fifteen missions: five each for Terran, Zerg and Protoss.
Run the client without arguments, select the imported game, then choose **Single
Player** and a race. Direct campaign launches use the root directory for Terran,
`local/packages/starcraft-campaigns-v1/zerg` for Zerg, or
`local/packages/starcraft-campaigns-v1/protoss` for Protoss. For example:

```sh
cargo run --release --locked -p straterust-client -- \
  --campaign local/packages/starcraft-campaigns-v1/zerg --mission 1
```

Press **Enter** to start after briefing and to advance after victory. Mission
numbers run from 1 to 5. Add `--race zerg`, `--race protoss`, or `--race terran`
to the importer to produce only that campaign at the specified output directory.
Imported units include source voices, portraits, movement, attack, building
activity, death effects, racial consoles and three music tracks per race.
Compatibility remains partial: caster spells, Reaver ammunition, some upgrades,
suicide units, exact warp-glow blending and animation/AI
timing are unfinished. Mission imports are validated for loading; complete
playthroughs of every mission are not verified.

The source can also be `INSTALL.EXE` or a directory containing that installer;
an installed game directory containing only `stardat.mpq` is not supported.
Keep imported assets under the ignored `local/` directory. To update an existing
campaign's assets and rules, use `update-campaign` in place of `import-campaign`.
Use `update-menus` to add the original animated StarCraft frontend and menu music to an existing
campaign without reimporting its missions. Select the campaign from the launcher,
choose Single Player, then an available race and a mission.
Use `update-hotkeys` to refresh an existing package or campaign's original
command keys without rebuilding artwork or changing gameplay data.

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
