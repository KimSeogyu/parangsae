from __future__ import annotations

from pathlib import Path
import sys

text = Path("docs/migration-manifest.md").read_text()
required = ["parangsae", "parangsae-engine", "parangsae-quant", ".secrets", "node_modules", "target"]
missing = [item for item in required if item not in text]
if missing:
    print(f"missing migration manifest entries: {missing}")
    sys.exit(1)
print("migration manifest covers source repos and exclusions")
