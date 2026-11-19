#!/usr/bin/env python3
"""Summarize independent DB benchmark processes; preserve scope and variability."""
import argparse
import json
from pathlib import Path
import re
from statistics import median


def summarize(directory):
    manifest = json.loads((directory / "manifest.json").read_text())
    if len(manifest["runs"]) < 3 or not all(run["valid"] for run in manifest["runs"]):
        raise ValueError("At least three valid independent processes required")
    runs = []
    for index in range(1, len(manifest["runs"]) + 1):
        path = directory / f"run-{index}"
        rows = [json.loads(line) for line in (path / "workload.jsonl").read_text().splitlines()]
        timing = (path / "time.txt").read_text()
        real, user, system = re.search(r"([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys", timing).groups()
        rss = int(re.search(r"(\d+)\s+maximum resident set size", timing).group(1))
        runs.append({"run": index, "pageTraversalMs": median(row["elapsedMs"] for row in rows),
                     "pageP95Ms": median(row["pageP95Ms"] for row in rows),
                     "payloadBytes": rows[0]["payloadBytes"], "setupMs": rows[0]["setupMs"],
                     "processCpuSeconds": float(user) + float(system),
                     "processRealSeconds": float(real), "maxRssBytes": rss})
    metrics = {key: {"median": median(run[key] for run in runs),
                     "min": min(run[key] for run in runs), "max": max(run[key] for run in runs)}
               for key in runs[0] if key != "run"}
    return {"scope": "DB/JSON only; CPU and max RSS include setup + three traversals per process",
            "binarySha256": manifest["binarySha256"], "fixtureSha256": manifest["fixtureSha256"],
            "expectedItems": manifest["expectedItems"], "runs": runs, "metrics": metrics}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    before, after = summarize(args.before), summarize(args.after)
    if (before["fixtureSha256"], before["expectedItems"]) != (after["fixtureSha256"], after["expectedItems"]):
        raise ValueError("Fixture mismatch: refusing to calculate improvement")
    comparison = {}
    for key, metric in before["metrics"].items():
        previous, current = metric["median"], after["metrics"][key]["median"]
        comparison[key] = {"before": metric, "after": after["metrics"][key],
                           "absoluteImprovement": previous - current,
                           "improvementPercent": None if previous == 0 else (previous - current) / previous * 100}
    result = {"before": before, "after": after, "comparison": comparison,
              "limitation": "Backend sub-benchmark only; no app-wide CPU/memory claim"}
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(comparison, indent=2))


if __name__ == "__main__":
    main()
