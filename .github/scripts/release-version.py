"""Validate app versions and, for release PRs, the release branch version."""

import json
import os
from pathlib import Path
import re
import tomllib


root = Path(__file__).resolve().parents[2]
package = json.loads((root / "package.json").read_text())
tauri = json.loads((root / "src-tauri/tauri.conf.json").read_text())
cargo = tomllib.loads((root / "src-tauri/Cargo.toml").read_text())
lock = tomllib.loads((root / "src-tauri/Cargo.lock").read_text())
version = package["version"]
versions = {
    "package.json": version,
    "src-tauri/tauri.conf.json": tauri["version"],
    "src-tauri/Cargo.toml": cargo["package"]["version"],
    "src-tauri/Cargo.lock": next(
        item["version"] for item in lock["package"]
        if item["name"] == cargo["package"]["name"]
    ),
}
if any(value != version for value in versions.values()):
    raise SystemExit(f"App versions must match: {versions}")

branch = os.environ.get("RELEASE_BRANCH", "")
if branch.startswith("release/"):
    branch_version = branch.removeprefix("release/").removeprefix("v")
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", branch_version):
        raise SystemExit("Use a stable release branch: release/vX.Y.Z")
    if branch_version != version:
        raise SystemExit(f"Branch {branch} requires app version {branch_version}; found {version}")

print(f"Validated app version: {version}")
if output := os.environ.get("GITHUB_OUTPUT"):
    with open(output, "a") as stream:
        stream.write(f"version={version}\ntag=v{version}\n")
