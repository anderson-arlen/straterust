# Original playable demonstration

This fixture exercises gathering, construction, production and combat with five
roles: worker, rifle unit, base, supply depot and barracks. Its map, ridge,
resource placements, scenario, geometric graphics and interface tones are
original StrateRust content under the repository's MIT license. No proprietary
images, sound or source archive bytes are included.

Selected numeric costs, HP, armor, supply, dimensions, build frames and weapon
fields match the owner's Windows retail v1.00 reference data. Their values do
not establish behavioral compatibility. Gathering and movement timings,
construction HP, refunds, prerequisites, spawn positions, combat scheduling and
victory rules are authored approximations rather than verified StarCraft behavior.

Run `cargo run --release --locked -p straterust-client -- --package content/terran-demo`.
The map is fully revealed. Player 0 starts with a base, worker and 50 minerals;
player 1 has one stationary defender. Collect minerals, build a depot and
barracks, train fighters, and attack across the ridge. Destroying all opposing
entities wins. F5 restarts.

Builders now move between reachable work points around their structure and
pause to work, while construction continues. Stop or a replacement order pauses
the job; right-click the unfinished structure to resume. The 30–93 tick work
pauses use a timer found in the reference executable, but collision-safe exterior
routes and native tick timing remain approximations. Placement previews snap
to 32-unit build tiles; the map's walkability cells are eight units wide.

SCVs can repair damaged friendly SCVs and completed buildings by right-clicking
them, or with **R** then left-click. Repair consumes resources, pauses when funds
run out, and supports queued orders and Stop. Marines cannot be repaired.
Newly trained units approach occupied rally points and settle in nearby clear
positions. These behaviors have dedicated integration tests.

`scenario.ron` is a complete successful command recording. Regenerate it with
`cargo run --locked -p straterust-tools --bin record_terran` after inspecting an
intentional behavior change. `expected-hashes.txt` guards the checked scenario;
`smoke.ron` is a short gathering/rendering check, not the full victory trace.

Simulation revision `straterust-sim-34` completes the nine-command recording in
5893 ticks with player 0 victorious using the restored per-unit navigator.
Debug and release traces agree with `expected-hashes.txt`.

`terrain.srtm` is original native data: 192×120 cells, eight world units per
cell, open walk/build flags (3), with a sight-blocking ridge (16) at cell
x=94..97 and y=18..102, excluding y=48..71 for its opening. It can be recreated
with the native SRTM header and those row-major flags.
