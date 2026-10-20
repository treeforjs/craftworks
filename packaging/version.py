"""Read the launcher's package version without a Python TOML dependency."""

from pathlib import Path
import re

root = Path(__file__).resolve().parent.parent
manifest = (root / "apps/craftworks/Cargo.toml").read_text(encoding="utf-8")
package = manifest.split("[package]", 1)[1].split("\n[", 1)[0]
match = re.search(r'^version\s*=\s*"([^"]+)"\s*$', package, re.MULTILINE)
if not match or not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", match.group(1)):
    raise SystemExit("The Craftworks manifest must contain a literal package version.")
print(match.group(1))
