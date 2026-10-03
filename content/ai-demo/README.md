# Original opponent AI demonstration

Run `cargo run --release --locked -p straterust-client -- --package content/ai-demo`.
Player 0 gathers, builds and trains with the same public Terran rules as the original demonstration. Player 1 runs a deterministic town AI: six workers, supply construction, two barracks and repeated groups of six rifle units. All production costs real resources; no source assets are needed. Destroy the opposing army and structures to win.

Map AI programs set build counts, priorities, waits and attack sizes. Campaign imports instead translate their selected original mission scripts; these timings are not a claim of exact original AI compatibility. Human commands are replay inputs; AI decisions are regenerated and their state is part of the authoritative hash.
