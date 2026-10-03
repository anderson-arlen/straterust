#!/usr/bin/env python3
"""Measure debug/release native frame cadence at two window sizes (needs a desktop)."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", default="content/terran-demo")
    args = parser.parse_args()
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    if not target.is_absolute():
        target = ROOT / target
    for profile in ("debug", "release"):
        for width, height in ((1280, 800), (1920, 1080)):
            config = Path(tempfile.gettempdir()) / f"straterust-client-{width}.ron"
            config.write_text(
                f"(width:{width},height:{height},audio:false,frames_per_second:60)\n"
            )
            binary = target / profile / ("straterust-client.exe" if os.name == "nt" else "straterust-client")
            result = subprocess.run(
                [str(binary), "--package", args.package, "--config", str(config),
                 "--benchmark-frames", "180"],
                cwd=ROOT, capture_output=True, text=True, timeout=30,
            )
            print(f"{profile} {width}x{height}", flush=True)
            if result.returncode:
                raise RuntimeError(result.stderr)
            for line in result.stderr.splitlines():
                if any(marker in line for marker in ("GPU renderer:", "client run passed:", "native frame timing:")):
                    print(line, flush=True)


if __name__ == "__main__":
    main()
