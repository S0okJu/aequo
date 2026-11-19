"""Write a release-only Tauri overlay. Never ship an updater without its key."""

import base64
import json
import os
from pathlib import Path
import sys


def public_key():
    value = os.environ.get("TAURI_UPDATER_PUBLIC_KEY", "").strip()
    try:
        lines = base64.b64decode(value, validate=True).decode().splitlines()
        raw = base64.b64decode(lines[1], validate=True)
        if len(raw) != 42 or raw[:2] != b"Ed":
            raise ValueError("invalid key")
    except (ValueError, IndexError, UnicodeError) as error:
        raise SystemExit("Set TAURI_UPDATER_PUBLIC_KEY to the Tauri public key contents") from error
    return value


if __name__ == "__main__":
    Path(sys.argv[1]).write_text(json.dumps({
        "bundle": {"createUpdaterArtifacts": True},
        "plugins": {"updater": {"pubkey": public_key(), "requireSignedVersion": True}},
    }) + "\n")
