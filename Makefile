.PHONY: check headless verify client smoke import-check import-smoke map-smoke terran-smoke backwater-smoke campaign-check campaign-smoke

check:
	python3 tool/check_source_size.py
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --locked -- -D warnings
	cargo test --workspace --locked
	$(MAKE) verify

headless:
	cargo test --locked -p straterust-engine -p straterust-tools

verify:
	cargo build --locked -p straterust-tools
	cargo build --release --locked -p straterust-tools
	python3 tool/verify.py

client:
	cargo run --release --locked -p straterust-client

smoke:
	cargo build --workspace --locked
	python3 tool/verify.py --client

# Explicit opt-in: SOURCE is a private reference disc/installer, never CI data.
import-check:
	test -n "$(SOURCE)"
	cargo build --workspace --locked
	cargo build --release --locked -p straterust-import-starcraft -p straterust-tools
	python3 tool/verify_import.py --source "$(SOURCE)"

import-smoke:
	test -n "$(SOURCE)"
	cargo build --workspace --locked
	cargo build --release --locked -p straterust-import-starcraft -p straterust-tools
	python3 tool/verify_import.py --source "$(SOURCE)" --client --isolate

map-smoke:
	test -n "$(SOURCE)"
	cargo build --workspace --locked
	cargo build --release --locked -p straterust-import-starcraft -p straterust-tools
	python3 tool/verify_import.py --source "$(SOURCE)" --map 'multimaps\(2)Challenger.scm' --client --isolate

terran-smoke:
	test -n "$(SOURCE)"
	cargo build --workspace --locked
	cargo build --release --locked -p straterust-import-starcraft -p straterust-tools
	python3 tool/verify_import.py --source "$(SOURCE)" --terran --client --isolate

backwater-smoke:
	test -n "$(SOURCE)"
	cargo build --workspace --locked
	cargo build --release --locked -p straterust-import-starcraft -p straterust-tools
	python3 tool/verify_import.py --source "$(SOURCE)" --backwater --client --isolate

campaign-check:
	test -n "$(SOURCE)"
	cargo build --workspace --locked
	cargo build --release --locked -p straterust-import-starcraft -p straterust-tools
	python3 tool/verify_import.py --source "$(SOURCE)" --campaign

campaign-smoke:
	test -n "$(SOURCE)"
	cargo build --workspace --locked
	cargo build --release --locked -p straterust-import-starcraft -p straterust-tools
	python3 tool/verify_import.py --source "$(SOURCE)" --campaign --client --isolate
