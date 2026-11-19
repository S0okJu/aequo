#!/usr/bin/env python3
"""Sample explicit macOS PIDs as JSONL (RSS, cumulative CPU, interval CPU).

This does not discover XPC helpers: supply every verified app-owned PID.
RSS is not Activity Monitor's memory footprint and cannot be compared to it.
"""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import subprocess
import time


def cpu_seconds(value):
    days, clock = value.split("-", 1) if "-" in value else ("0", value)
    parts = [float(part) for part in clock.split(":")]
    return int(days) * 86400 + sum(part * 60 ** index for index, part in enumerate(reversed(parts)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", type=int, action="append", required=True)
    parser.add_argument("--seconds", type=float, required=True)
    parser.add_argument("--interval", type=float, default=1)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.seconds <= 0 or args.interval <= 0 or any(pid <= 0 for pid in args.pid):
        parser.error("PIDs, duration and interval must be positive")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    previous = {}
    start = time.monotonic()
    with args.output.open("x") as output:
        sample = 0
        while True:
            now = time.monotonic()
            result = subprocess.run(
                ["ps", "-p", ",".join(str(pid) for pid in args.pid),
                 "-o", "pid=,rss=,time=,comm="], text=True, capture_output=True,
            )
            if result.returncode not in (0, 1) or result.stderr.strip():
                raise RuntimeError(result.stderr or f"ps failed: {result.returncode}")
            processes = []
            for line in result.stdout.splitlines():
                pid_text, rss, cpu, name = line.strip().split(None, 3)
                pid = int(pid_text)
                cumulative = cpu_seconds(cpu)
                last = previous.get(pid)
                usage = None if last is None else max(0, cumulative - last[1]) / (now - last[0]) * 100
                processes.append({"pid": pid, "name": name, "rssBytes": int(rss) * 1024,
                                  "cpuSeconds": cumulative, "intervalCpuPercent": usage})
                previous[pid] = (now, cumulative)
            output.write(json.dumps({
                "utc": datetime.now(timezone.utc).isoformat(), "elapsedSeconds": now - start,
                "processes": processes, "missingPids": sorted(set(args.pid) - {p['pid'] for p in processes}),
                "metric": "RSS (not physical footprint); 100% CPU = one logical core",
            }) + "\n")
            output.flush()
            if not processes:
                raise RuntimeError("All target PIDs exited; this run is incomplete")
            sample += 1
            deadline = start + sample * args.interval
            if deadline - start > args.seconds:
                break
            time.sleep(max(0, deadline - time.monotonic()))
    print(f"Saved samples to {args.output}")


if __name__ == "__main__":
    main()
