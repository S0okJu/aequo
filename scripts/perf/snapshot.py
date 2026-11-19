#!/usr/bin/env python3
"""Preserve the exact (including dirty/untracked) source used for a perf build."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile
from datetime import datetime, timezone


def command(*args):
    return subprocess.check_output(args, text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    names = subprocess.check_output(
        ["git", "-C", str(root), "ls-files", "-z", "--cached", "--others", "--exclude-standard"]
    ).decode().split("\0")
    files = sorted({name for name in names if name and (root / name).is_file()})
    hashes = {name: hashlib.sha256((root / name).read_bytes()).hexdigest() for name in files}
    with tarfile.open(output / "source.tar.gz", "w:gz") as archive:
        for name in files:
            archive.add(root / name, arcname=name, recursive=False)
    (output / "worktree.patch").write_bytes(subprocess.check_output(
        ["git", "-C", str(root), "diff", "--binary", "HEAD"]
    ))
    metadata = {
        "createdAt": datetime.now(timezone.utc).isoformat(),
        "commit": command("git", "-C", str(root), "rev-parse", "HEAD"),
        "branch": command("git", "-C", str(root), "branch", "--show-current"),
        "status": command("git", "-C", str(root), "status", "--short"),
        "os": command("sw_vers"),
        "architecture": command("uname", "-m"),
        "files": hashes,
        "sourceArchiveSha256": hashlib.sha256((output / "source.tar.gz").read_bytes()).hexdigest(),
    }
    (output / "manifest.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(f"Preserved {len(files)} source files in {output}")


if __name__ == "__main__":
    main()
