"""Provision or resume Aequo updater signing without printing secret material.

The user must authorize creation of the local key and GitHub configuration.
Private key: encrypted file, mode 0600. Password: macOS login keychain + CI.
Re-running reuses the key and never rotates an already distributed public key.
"""

import datetime
import getpass
import hashlib
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys

REPO = "S0okJu/aequo"
SERVICE = "io.github.s0okju.aequo.updater-signing"
ACCOUNT = getpass.getuser()
ROOT = Path.home() / ".tauri/aequo"
KEY = ROOT / "updater.key"
PROJECT = Path(__file__).resolve().parents[2]


def execute(command, stage, data=None):
    result = subprocess.run(command, input=data, capture_output=True, text=True, cwd=PROJECT)
    if result.returncode:
        # Do not echo the command, stdout or stderr: signers may include key material.
        raise RuntimeError(f"Could not complete signing setup: {stage}")
    return result.stdout.strip()


def setup():
    ROOT.mkdir(parents=True, exist_ok=True, mode=0o700)
    ROOT.chmod(0o700)
    if KEY.exists():
        if not KEY.with_suffix(".key.pub").exists():
            raise RuntimeError("Existing private key has no public key; refusing to overwrite it")
        password = execute(["security", "find-generic-password", "-a", ACCOUNT, "-s", SERVICE, "-w"], "read keychain password")
    else:
        existing = subprocess.run(["security", "find-generic-password", "-a", ACCOUNT, "-s", SERVICE], capture_output=True)
        if existing.returncode == 0:
            raise RuntimeError("Existing keychain entry has no local key; restore the backup before continuing")
        password = secrets.token_urlsafe(48)
        execute(["pnpm", "tauri", "signer", "generate", "--ci", "-p", password, "-w", str(KEY)], "generate encrypted key")
        KEY.chmod(0o600)
        execute(["security", "add-generic-password", "-a", ACCOUNT, "-s", SERVICE, "-w", password], "save password to keychain")
    public = KEY.with_suffix(".key.pub").read_text().strip()
    # Refuse accidental rotation if a public key has already been configured remotely.
    variables = json.loads(execute(["gh", "variable", "list", "--repo", REPO, "--json", "name,value"], "read repository variables"))
    for variable in variables:
        if variable["name"] == "TAURI_UPDATER_PUBLIC_KEY" and variable["value"].strip() != public:
            raise RuntimeError("Repository has a different updater key; refusing automatic rotation")
    execute(["gh", "secret", "set", "TAURI_SIGNING_PRIVATE_KEY", "--repo", REPO], "register signing key", KEY.read_text())
    execute(["gh", "secret", "set", "TAURI_SIGNING_PRIVATE_KEY_PASSWORD", "--repo", REPO], "register signing password", password)
    execute(["gh", "variable", "set", "TAURI_UPDATER_PUBLIC_KEY", "--repo", REPO, "--body", public], "register public key")
    record = {"repository": REPO, "owner": ACCOUNT, "privateKey": str(KEY), "passwordKeychainService": SERVICE,
              "publicKeySha256": hashlib.sha256(public.encode()).hexdigest(),
              "configuredAt": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    record_path = ROOT / "signing-setup.json"
    record_path.write_text(json.dumps(record, indent=2) + "\n")
    record_path.chmod(0o600)
    print(json.dumps(record, indent=2))


if __name__ == "__main__":
    os.umask(0o077)
    try:
        setup()
    except (RuntimeError, OSError, ValueError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
