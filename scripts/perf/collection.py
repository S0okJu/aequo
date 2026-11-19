#!/usr/bin/env python3
"""Compare initial collection policies in isolated Rust test processes on macOS.
Build first: cargo test --manifest-path src-tauri/Cargo.toml --lib --no-run
Usage: python3 scripts/perf/collection.py TEST_BINARY OUTPUT.json
"""
import hashlib
import json
import os
import platform
from pathlib import Path
import re
import statistics
import subprocess
import sys

binary, output = map(Path, sys.argv[1:])
binary = binary.resolve()
records = []
for feeds in (1, 20):
    for repeat in range(5):
        # Rotate order so a policy is not always measured first or last.
        variants = ["legacy", "limit", "prefetch_off", "both"]
        variants = variants[repeat % 4:] + variants[:repeat % 4]
        for variant in variants:
            env = {**os.environ, "AEQUO_COLLECTION_BENCH_VARIANT": variant,
                   "AEQUO_COLLECTION_BENCH_FEEDS": str(feeds)}
            command = ["/usr/bin/time", "-l", str(binary), "--exact",
                       "commands::add_feed::collection_benchmark::resource_collection_benchmark",
                       "--ignored", "--nocapture"]
            run = subprocess.run(command, env=env, text=True, capture_output=True, check=True)
            line = next(s.removeprefix("COLLECTION_BENCH ") for s in run.stdout.splitlines()
                        if s.startswith("COLLECTION_BENCH "))
            row = json.loads(line)
            row["repeat"] = repeat + 1
            match = re.search(r"([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys", run.stderr)
            assert match, run.stderr
            row["process_cpu_seconds"] = float(match[2]) + float(match[3])
            row["process_peak_rss_bytes"] = int(re.search(r"(\d+)\s+maximum resident set size", run.stderr)[1])
            row["time_stderr"] = run.stderr
            records.append(row)
            print(f"{feeds} feeds {variant} sample {repeat+1}: {row['elapsed_ms']:.2f} ms", flush=True)
summary = []
for feeds in (1, 20):
    for variant in ("legacy", "limit", "prefetch_off", "both"):
        samples = [r for r in records if r["feeds"] == feeds and r["variant"] == variant]
        entry = {"feeds": feeds, "variant": variant}
        for field in ("elapsed_ms", "process_cpu_seconds", "process_peak_rss_bytes", "db_bytes",
                      "rows", "body_bytes", "exclusions", "identity_bytes", "article_requests",
                      "feed_requests", "feed_response_bytes", "article_response_bytes"):
            values = [r[field] for r in samples]
            entry[field] = {"median": statistics.median(values), "min": min(values), "max": max(values)}
        summary.append(entry)
output.parent.mkdir(parents=True, exist_ok=True)
source_paths = ["src-tauri/src/commands/add_feed.rs", "src-tauri/src/db/collection.rs",
                "src-tauri/src/db/repository.rs", "src-tauri/src/db/schema.rs",
                "src-tauri/src/db/resources.rs", "src-tauri/src/fetch/parser.rs",
                "src-tauri/src/fetch/enrichment.rs", "src-tauri/src/resource_policy.rs"]
manifest = {"source_sha256": {name: hashlib.sha256(Path(name).read_bytes()).hexdigest() for name in source_paths},
            "platform": [platform.system(), platform.release(), platform.machine()]}
output.write_text(json.dumps({**manifest, "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                              "command": sys.argv, "build": "debug lib test, isolated processes",
                              "summary": summary, "samples": records}, indent=2) + "\n")
