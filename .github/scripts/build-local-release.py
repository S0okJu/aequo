"""Build a production-signed local release using the provisioned macOS keychain.

Secrets are passed as environment values, never printed or included in arguments.
This builds artifacts only and does not tag, push, or publish anything.
"""
import getpass
import json
import os
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[2]
key_dir = Path.home() / ".tauri/aequo"
password = subprocess.run([
    "security", "find-generic-password", "-a", getpass.getuser(),
    "-s", "io.github.s0okju.aequo.updater-signing", "-w",
], capture_output=True, text=True)
if password.returncode:
    raise SystemExit("Could not read updater signing password from keychain")
env = os.environ.copy()
env.update(
    TAURI_SIGNING_PRIVATE_KEY=(key_dir / "updater.key").read_text().strip(),
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD=password.stdout.strip(),
    TAURI_UPDATER_PUBLIC_KEY=(key_dir / "updater.key.pub").read_text().strip(),
    APPLE_SIGNING_IDENTITY="-",
)
for name in tuple(env):
    if name.startswith("AEQUO_UPDATE_"):
        del env[name]
with tempfile.TemporaryDirectory(prefix="aequo-release-config-") as temp:
    config = str(Path(temp) / "updater.json")
    subprocess.run(["python3", ".github/scripts/configure-updater.py", config], cwd=root, env=env, check=True)
    subprocess.run(["pnpm", "tauri", "build", "--config", config, "--target", "aarch64-apple-darwin",
                    "--bundles", "app,dmg", "--ci", "--", "--locked"], cwd=root, env=env, check=True)
    version = json.loads((root / "package.json").read_text())["version"]
    archive = root / "src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Aequo.app.tar.gz"
    subprocess.run(["node", ".github/scripts/verify-update.mjs", str(archive), str(archive) + ".sig", version], cwd=root, env=env, check=True)
print("Production artifacts built and updater signature verified; nothing published.")
