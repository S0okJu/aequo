# CPU / memory measurement

This tooling supports `docs/MEMORY_OPTIMIZATION_GOAL.md`. A preserved build or
passing test is not a measured performance result.

## Preserve before changing application code

```sh
python3 scripts/perf/snapshot.py .perf-results/before
python3 scripts/perf/fixture.py .perf-results/fixtures/representative.db
python3 scripts/perf/fixture.py .perf-results/fixtures/empty.db --items 0 --feeds 1
pnpm tauri build --no-bundle --config '{"identifier":"io.github.s0okju.aequo.perf","build":{"beforeBuildCommand":"pnpm build"}}'
```

Copy the resulting release binary and built frontend into the before directory,
and record their hashes and the exact command in its manifest. The archive includes
tracked and untracked source but excludes ignored paths. Keep this ignored artifact
directory with the final report; source hashes identify all dirty files used.

The benchmark identifier is distinct from the user's app. Never copy fixtures over
the real `io.github.s0okju.aequo` data directory. Before launching the benchmark,
install a **copy** in its isolated data directory. The fixture must be reset between
runs. The app performs schema migration and builds its search index on first open:
report this cold start separately from prepared/warm DB runs.

## Collect explicit process samples

```sh
python3 scripts/perf/collect.py --pid 12345 --pid 12346 --seconds 360 --output .perf-results/before/s0-run-1.jsonl
```

Use verified PIDs for the app and each associated WebKit/XPC process. Helpers are
not necessarily children of the app. Do not attribute all system WebKit processes
to this app. Record PID roles and newly created or exited processes; the current
collector deliberately reports missing PIDs and does not guess their replacements.
Restart a measurement with a documented segment if the PID set changes.

RSS bytes and cumulative user+system CPU seconds come from macOS `ps`. RSS differs
from the Activity Monitor memory column and physical footprint; never compare
those metrics as if they were the same. The first CPU percentage is null because
there is no preceding sample. Missing samples are not zero usage. Use the raw
cumulative CPU differences and actual elapsed interval for CPU utilization.

Final runs must use the same metric, fixture, effective settings, window size,
sampling interval, stabilization and action sequence before and after. Native
WebView metrics require the native release app; browser or jsdom tests cannot
substitute for them. No forced GC or connected inspector in final measurements.

The representative fixture contains text only and deliberately has no due polling
or icon jobs. It covers S0/S1 input, not S2 image workloads, S4 AI or S5 polling.
Those require additional fixed fixtures before those scenarios can be signed off.

## DB / JSON sub-benchmark

```sh
cargo build --manifest-path src-tauri/Cargo.toml --release --example list_workload
python3 scripts/perf/run_list.py --binary .perf-results/before/list_workload --fixture .perf-results/fixtures/representative.db --output .perf-results/before/list-baseline
```

Preserve the example binary before source changes. Use the same harness before and
after. Each of three independent processes receives a fresh fixture copy, performs
schema/search setup once, and traverses all pages three times. Each JSON row reports
one traversal. `time.txt` CPU seconds and maximum RSS cover the entire process,
including setup and all three traversals. They are not per-traversal metrics.
Report setup separately, and do not label the within-process repetitions as three
independent processes. The runner verifies all traversals returned every input row.


## Initial collection policy comparison

For `docs/FEED_COLLECTION_RESOURCE_GOAL.md`, build the debug lib tests with
`cargo test --manifest-path src-tauri/Cargo.toml --lib --no-run`, then pass the
reported executable to `python3 scripts/perf/collection.py TEST_BINARY OUTPUT.json`.
The macOS runner uses isolated processes, temporary databases and a local mock
server to compare four policies over five samples for 1 and 20 subscriptions.
Results include all samples, source/binary hashes, process CPU/peak RSS, request
counts and database size. This is a backend workload, not a whole-app WebView
measurement. See `docs/FEED_COLLECTION_RESOURCE_RESULTS.md` for results and limits.
