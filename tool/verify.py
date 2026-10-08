#!/usr/bin/env python3
"""Cross-process/profile fixture regression and optional native window smoke check."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
if not TARGET.is_absolute():
    TARGET = ROOT / TARGET
SUFFIX = ".exe" if os.name == "nt" else ""
HEADLESS = TARGET / "debug" / f"straterust-headless{SUFFIX}"


def run(binary, *args, display=False, succeeds=True, timeout=45):
    environment = os.environ.copy()
    if not display:
        environment.pop("DISPLAY", None)
        environment.pop("WAYLAND_DISPLAY", None)
    try:
        result = subprocess.run(
            [str(binary), *map(str, args)], cwd=ROOT, env=environment,
            text=True, capture_output=True, timeout=timeout,
        )
    except subprocess.TimeoutExpired as error:
        diagnostics = (error.stderr or b"").decode(errors="replace")
        raise AssertionError(f"{binary.name} timed out\n{diagnostics}") from error
    if (result.returncode == 0) != succeeds:
        raise AssertionError(f"{binary.name} returned {result.returncode}\n{result.stderr}")
    return result


def verify_content(baseline):
    with tempfile.TemporaryDirectory(prefix="straterust-verify-") as directory:
        package = Path(directory) / "fixture"
        shutil.copytree(ROOT / "content/fixtures", package)
        (package / "presentation.ron").unlink()
        assert run(HEADLESS, "--package", package).stdout == baseline
        (package / "presentation.ron").write_text("deliberately invalid cosmetics")
        assert run(HEADLESS, "--package", package).stdout == baseline

        scenario = package / "scenario.ron"
        original = scenario.read_text()
        scenario.write_text(original.replace("x: 800", "x: 804", 1))
        assert run(HEADLESS, "--package", package).stdout != baseline
        scenario.write_text(original)

        manifest = package / "manifest.ron"
        manifest.write_text(manifest.read_text().replace("schema_version: 1", "schema_version: 99"))
        assert "unsupported package schema" in run(HEADLESS, "--package", package, succeeds=False).stderr
        shutil.copyfile(ROOT / "content/fixtures/manifest.ron", manifest)
        rules = package / "rules.ron"
        rules.write_text("(" + " " * (4 * 1024 * 1024))
        assert "content limit" in run(HEADLESS, "--package", package, succeeds=False).stderr
        rules.write_text("(malformed")
        assert "invalid RON" in run(HEADLESS, "--package", package, succeeds=False).stderr


def verify_client(last_line):
    client = TARGET / "debug" / f"straterust-client{SUFFIX}"
    screenshot = Path(tempfile.gettempdir()) / "straterust-client.ppm"
    result = run(client, "--smoke-test", "--screenshot", screenshot, display=True)
    assert result.stdout.strip() == last_line, "native client and headless simulation diverged"
    assert screenshot.stat().st_size > 1000, "empty screenshot"
    print(result.stderr.strip())
    print(f"Native render, resize, clean exit and headless equivalence passed. Screenshot: {screenshot}")
    screenshot = Path(tempfile.gettempdir()) / "straterust-terran-original.ppm"
    demo = ROOT / "content/terran-demo"
    expected = run(HEADLESS, "--package", demo, "--scenario", demo / "smoke.ron").stdout.strip().splitlines()[-1]
    result = run(client, "--package", demo, "--scenario", demo / "smoke.ron", "--smoke-test",
                 "--screenshot", screenshot, display=True)
    assert result.stdout.strip() == expected, "playable demo client diverged"
    assert screenshot.stat().st_size > 1000
    print(result.stderr.strip())
    print(f"Original playable demo render and gathering check passed. Screenshot: {screenshot}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client", action="store_true", help="verify a real window instead of release profile")
    args = parser.parse_args()
    baseline = run(HEADLESS).stdout
    expected = (ROOT / "content/fixtures/expected-hashes.txt").read_text()
    assert baseline == expected, "fixture hashes changed; inspect behavior before updating the golden file"
    last_line = baseline.strip().splitlines()[-1]
    if args.client:
        verify_client(last_line)
        return
    assert baseline == run(HEADLESS).stdout, "separate processes diverged"
    verify_content(baseline)
    demo = ROOT / "content/terran-demo"
    demo_baseline = run(HEADLESS, "--package", demo).stdout
    assert demo_baseline == run(HEADLESS, "--package", demo).stdout, "demo processes diverged"
    assert demo_baseline == (demo / "expected-hashes.txt").read_text(), "demo gameplay changed"
    ai_demo = ROOT / "content/ai-demo"
    # The 12,000-tick AI workload is slower in debug; keep window checks at 45s.
    ai_baseline = run(HEADLESS, "--package", ai_demo, "--hash-every", 600, timeout=120).stdout
    assert ai_baseline == run(HEADLESS, "--package", ai_demo, "--hash-every", 600, timeout=120).stdout, "AI processes diverged"
    assert ai_baseline == (ai_demo / "expected-hashes.txt").read_text(), "AI gameplay changed"
    release = TARGET / "release" / f"straterust-headless{SUFFIX}"
    assert run(release).stdout == baseline, "debug/release divergence"
    assert run(release, "--package", demo).stdout == demo_baseline, "demo debug/release divergence"
    assert run(release, "--package", ai_demo, "--hash-every", 600, timeout=120).stdout == ai_baseline, "AI debug/release divergence"
    with tempfile.TemporaryDirectory(prefix="straterust-demo-verify-") as directory:
        package = Path(directory) / "demo"
        shutil.copytree(demo, package)
        (package / "presentation.ron").write_text("intentionally invalid cosmetics")
        assert run(HEADLESS, "--package", package).stdout == demo_baseline
    print("Process, debug/release, golden hashes, input sensitivity and content isolation checks passed.")
    print(demo_baseline.strip().splitlines()[-1])
    print(last_line)


if __name__ == "__main__":
    main()
