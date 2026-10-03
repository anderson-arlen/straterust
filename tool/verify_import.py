#!/usr/bin/env python3
"""Opt-in private-disc verification; never downloads or publishes original data."""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from verify import ROOT, TARGET, SUFFIX, HEADLESS


def run(command, *, display=False, succeeds=True, cwd=ROOT, env_updates=None, timeout=90):
    env = os.environ.copy()
    env.update(env_updates or {})
    if not display:
        env.pop("DISPLAY", None)
        env.pop("WAYLAND_DISPLAY", None)
    result = subprocess.run(list(map(str, command)), cwd=cwd, env=env,
                            capture_output=True, text=True, timeout=timeout)
    assert (result.returncode == 0) == succeeds, result.stderr
    return result


def snapshot(package):
    return {str(path.relative_to(package)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(package.rglob("*")) if path.is_file()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--client", action="store_true")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--map", help="archive member for explicit terrain/placement inspection")
    mode.add_argument("--terran", action="store_true", help="verify the playable five-role demo")
    mode.add_argument("--backwater", action="store_true", help="verify the original Mission 2 import")
    mode.add_argument("--campaign", action="store_true", help="verify the first five Terran missions")
    parser.add_argument("--isolate", action="store_true",
                        help="Linux/bwrap: hide the source and local/source from runtime")
    args = parser.parse_args()
    source = args.source.resolve(strict=True)
    debug = TARGET / "debug" / f"straterust-import-starcraft{SUFFIX}"
    release = TARGET / "release" / debug.name
    conversion = (["import-campaign"] if args.campaign else
                  ["import-backwater"] if args.backwater else
                  ["import-terran"] if args.terran else
                  ["import-map", "--map", args.map, "--terrain-only"] if args.map else ["import"])
    with tempfile.TemporaryDirectory(prefix="straterust-import-verify-") as temporary:
        directory = Path(temporary)
        package = directory / "native"
        second = directory / "release"
        inventory = run([debug, "inventory", "--source", source]).stdout
        for binary, output in [(debug, package), (release, second)]:
            print(f"Importing with {binary.parent.name} profile...", flush=True)
            run([binary, *conversion, "--source", source, "--output", output], timeout=300)
        expected = snapshot(package)
        print("Comparing imports and checking publication safety...", flush=True)
        assert expected == snapshot(second), "debug/release imports differ"
        assert "Already identical" in run([debug, *conversion, "--source", source,
                                            "--output", package], timeout=300).stdout
        assert expected == snapshot(package)
        bad = directory / "truncated.iso"
        bad.write_bytes(b"bad source")
        result = run([debug, *conversion, "--source", bad, "--output", package], succeeds=False)
        assert "malformed ISO" in result.stderr
        assert expected == snapshot(package), "failed import changed existing package"
        failed = directory / "failed"
        run([debug, *conversion, "--source", bad, "--output", failed], succeeds=False)
        assert not failed.exists(), "failed import left a package"
        assert inventory == run([debug, "inventory", "--source", source]).stdout

        if args.backwater:
            run(["cargo", "test", "--locked", "-p", "straterust-import-starcraft",
                 "private_backwater_source_inventory", "--", "--ignored"],
                env_updates={"STRATERUST_BACKWATER_SOURCE": str(source),
                             "STRATERUST_BACKWATER_PACKAGE": str(package)}, timeout=300)

        isolation = []
        if args.isolate:
            assert shutil.which("bwrap"), "--isolate requires Linux bubblewrap"
            isolation = ["bwrap", "--ro-bind", "/", "/", "--bind", "/tmp", "/tmp",
                         "--dev-bind", "/dev", "/dev"]
            if source.is_dir():
                isolation += ["--tmpfs", source]
            else:
                isolation += ["--ro-bind", "/dev/null", source]
            private = ROOT / "local/source"
            if private.exists() and private != source:
                isolation += ["--tmpfs", private]
            isolation += ["--chdir", directory]
            # Confirm the same mount configuration actually hides the input.
            probe = """import pathlib, sys
p = pathlib.Path(sys.argv[1])
try:
    if p.is_dir():
        assert not list(p.iterdir())
    else:
        with p.open('rb') as stream:
            assert not stream.read(1)
except (PermissionError, FileNotFoundError):
    pass
"""
            run([*isolation, "python3", "-c", probe, source])
        if args.campaign:
            for number in range(1, 6):
                print(f"Checking mission {number} native simulation and presentation...", flush=True)
                native = package / f"terran{number:02}"
                baseline = run([HEADLESS, "--package", native], cwd=directory).stdout
                assert run([TARGET / "release" / HEADLESS.name, "--package", native],
                           cwd=directory).stdout == baseline, "campaign debug/release divergence"
                assert run([*isolation, HEADLESS, "--package", native], cwd=directory).stdout == baseline
                if args.client:
                    screenshot = Path(tempfile.gettempdir()) / f"straterust-terran{number:02}-preview.ppm"
                    result = run([*isolation, TARGET / "debug" / f"straterust-client{SUFFIX}",
                                  "--campaign", package, "--mission", number, "--smoke-test",
                                  "--screenshot", screenshot], display=True, cwd=directory, timeout=300)
                    assert result.stdout.strip() == baseline.strip().splitlines()[-1]
                    assert screenshot.stat().st_size > 1000
                    print(result.stderr.strip())
                    print(f"Mission {number} native rendering and resize passed: {screenshot}")
            print("Five-mission imports, identical retry, safe failures and native debug/release checks passed.")
            return
        fixture = ROOT / "content" / ("terran-demo" if args.terran else "fixtures")
        baseline = (run([HEADLESS, "--package", package], cwd=directory).stdout if (args.map or args.backwater)
                    else (fixture / "expected-hashes.txt").read_text())
        if args.map or args.terran or args.backwater:
            release_headless = TARGET / "release" / HEADLESS.name
            assert run([release_headless, "--package", package], cwd=directory).stdout == baseline, "native simulation differs across build profiles"
        result = run([*isolation, HEADLESS, "--package", package], cwd=directory)
        assert result.stdout == baseline, "imported cosmetics changed simulation"
        if args.client:
            screenshot = Path(tempfile.gettempdir()) / ("straterust-backwater-preview.ppm" if args.backwater else "straterust-terran-preview.ppm" if args.terran else "straterust-map-preview.ppm" if args.map else "straterust-import-preview.ppm")
            client = TARGET / "debug" / f"straterust-client{SUFFIX}"
            scenario_args = ["--scenario", fixture / "smoke.ron"] if args.terran else []
            if args.terran:
                baseline = run([HEADLESS, "--package", package, *scenario_args], cwd=directory).stdout
            result = run([*isolation, client, "--package", package, "--smoke-test",
                          "--screenshot", screenshot, *scenario_args], display=True, cwd=directory)
            assert result.stdout.strip() == baseline.strip().splitlines()[-1]
            assert screenshot.stat().st_size > 1000
            print(result.stderr.strip())
            print(f"Imported native rendering, resize and clean exit passed: {screenshot}")
        qualifier = " with source media hidden" if args.isolate else ""
        print(f"Inventory, debug/release reproducibility, safe retries, failure preservation and native runtime checks passed{qualifier}.")


if __name__ == "__main__":
    main()
