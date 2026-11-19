#!/usr/bin/env python3
"""Run a preserved DB/JSON benchmark in fresh processes against fixture copies."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import sqlite3
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repetitions", type=int, default=3)
    args = parser.parse_args()
    if args.repetitions < 3:
        parser.error("at least three independent processes are required")
    args.output.mkdir(parents=True, exist_ok=False)
    with sqlite3.connect(f"file:{args.fixture.resolve()}?mode=ro", uri=True) as conn:
        expected = conn.execute("SELECT COUNT(*) FROM items").fetchone()[0]
    metadata = {"binarySha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
                "fixtureSha256": hashlib.sha256(args.fixture.read_bytes()).hexdigest(),
                "expectedItems": expected, "scope": "DB/JSON only; not WebView",
                "runs": []}
    for number in range(1, args.repetitions + 1):
        run_dir = args.output / f"run-{number}"
        run_dir.mkdir()
        fixture = run_dir / "fixture.db"
        shutil.copy2(args.fixture, fixture)
        command = ["/usr/bin/time", "-l", str(args.binary.resolve()), str(fixture.resolve())]
        with (run_dir / "workload.jsonl").open("x") as stdout, (run_dir / "time.txt").open("x") as stderr:
            result = subprocess.run(command, stdout=stdout, stderr=stderr)
        rows = [json.loads(line) for line in (run_dir / "workload.jsonl").read_text().splitlines()]
        valid = result.returncode == 0 and len(rows) == 3 and all(row["items"] == expected for row in rows)
        metadata["runs"].append({"command": command, "exitCode": result.returncode, "valid": valid})
        (args.output / "manifest.json").write_text(json.dumps(metadata, indent=2) + "\n")
        if not valid:
            raise RuntimeError(f"run {number} incomplete; inspect {run_dir}")
        print(f"Run {number}: verified {expected} items, {rows[0]['payloadBytes']} JSON bytes", flush=True)


if __name__ == "__main__":
    main()
