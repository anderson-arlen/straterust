#!/usr/bin/env python3
"""Keep authored source modules within the AGENTS.md size budget."""
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent.parent
EXTENSIONS = {".rs", ".py", ".sh", ".c", ".cpp", ".h"}
oversized = []
review = []
largest = (0, "")
for directory in (ROOT / "crates", ROOT / "tool"):
    for path in sorted(directory.rglob("*")):
        if not path.is_file() or path.suffix not in EXTENSIONS:
            continue
        count = len(path.read_text().splitlines())
        name = str(path.relative_to(ROOT))
        largest = max(largest, (count, name))
        if count >= 1500:
            oversized.append((count, name))
        elif count >= 1000:
            review.append((count, name))
for count, name in review:
    print(f"Consider a cohesive split: {name} ({count} lines)")
for count, name in oversized:
    print(f"Hard limit reached: {name} ({count} lines)", file=sys.stderr)
print(f"Largest source file: {largest[1]} ({largest[0]} lines)")
sys.exit(bool(oversized))
