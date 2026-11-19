"""Generate a complete, verified static updater manifest from a draft release."""

import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys


def stable(tag):
    if not re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", tag):
        raise ValueError("Expected a stable vX.Y.Z release tag")
    return tuple(map(int, tag[1:].split(".")))


def guard_latest(tag, releases):
    version = stable(tag)
    for release in releases:
        if release.get("isDraft") or release.get("isPrerelease"):
            continue
        try:
            other = stable(release["tagName"])
        except (ValueError, KeyError):
            continue
        if other > version:
            raise ValueError("A newer stable release is already public")


def prepare(dist, release, repo, tag):
    stable(tag)
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo):
        raise ValueError("Invalid repository")
    if release.get("tagName") != tag or not release.get("isDraft") or release.get("isPrerelease"):
        raise ValueError("Expected the matching stable draft")
    dmgs = list(dist.glob("*.dmg"))
    archives = list(dist.glob("*.app.tar.gz"))
    if len(dmgs) != 1 or len(archives) != 1:
        raise ValueError("Expected one DMG and one updater archive")
    archive = archives[0]
    if not re.fullmatch(r"[A-Za-z0-9_.-]+\.app\.tar\.gz", archive.name):
        raise ValueError("Invalid updater filename")
    signature = Path(str(archive) + ".sig")
    subprocess.run([
        "node", str(Path(__file__).with_name("verify-update.mjs")),
        str(archive), str(signature), tag[1:],
    ], check=True)
    # Keep this timestamp in a resumed draft so retries generate identical metadata.
    date = release.get("createdAt")
    if not date:
        raise ValueError("Draft creation time is missing")
    datetime.datetime.fromisoformat(date.replace("Z", "+00:00"))
    manifest = {
        "version": tag[1:], "notes": release.get("body") or "", "pub_date": date,
        "platforms": {"darwin-aarch64": {
            "url": f"https://github.com/{repo}/releases/download/{tag}/{archive.name}",
            "signature": signature.read_text().strip(),
        }},
    }
    (dist / "latest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    (dist / "SHA256SUMS").write_text("".join(
        f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n"
        for path in [dmgs[0], archive]
    ))


if __name__ == "__main__":
    try:
        if sys.argv[1] == "guard":
            guard_latest(os.environ["RELEASE_TAG"], json.loads(Path(sys.argv[2]).read_text()))
        else:
            prepare(Path("dist"), json.loads(Path(sys.argv[2]).read_text()),
                    os.environ["GH_REPO"], os.environ["RELEASE_TAG"])
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error)) from error
