# Original development fixture

This package was authored for StrateRust. Its gameplay data, command scenario, colours, and geometric unit art use the project's [MIT license](../../LICENSE). No StarCraft or Warcraft assets, maps, mechanics data, or extracted files are included. The client uses the separate MIT-licensed `font8x8` dependency for text.

- `manifest.ron` declares the package schema and identifier.
- `rules.ron` defines the provisional 50 ms tick and two movement speeds.
- `map.ron` defines an empty rectangular field and five starting entities. Spawn order assigns stable entity IDs beginning at 1.
- `scenario.ron` is an authored command schedule for deterministic testing, not a stable replay file format.
- `presentation.ron` supplies optional client colours and geometric unit size. The headless loader never reads it.
- `client.ron` is an example local configuration; load it with `--config content/fixtures/client.ron`.
- `expected-hashes.txt` records the reference scenario's canonical hashes every 20 ticks, including the initial state. Investigate differences before updating this file.

The fixture is fully revealed and has no terrain obstacles, collision, resource gathering, construction, combat, or victory condition. `Wander` is a test command that consumes simulation randomness, not a compatibility mechanic.
