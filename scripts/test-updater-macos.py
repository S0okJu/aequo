"""Build and exercise real, ad-hoc signed Mac bundles in a private test directory.

No production releases, keys, installed apps or user library are changed. The
test-only driver is excluded from production builds. Artifacts/logs are retained.
"""

import hashlib
import http.server
import json
import os
import re
import sys
import plistlib
from pathlib import Path
import shutil
import signal
import sqlite3
import subprocess
import tempfile
import tarfile
import threading
import time

REPO = Path(__file__).resolve().parents[1]
REUSE = len(sys.argv) == 3 and sys.argv[1] == "--reuse-root"
ROOT = Path(sys.argv[2]).resolve() if REUSE else Path(tempfile.mkdtemp(prefix="aequo-updater-test-")).resolve()
if REUSE:
    assert ROOT.name.startswith("aequo-updater-test-")
    with open(ROOT / "base.app/Contents/Info.plist", "rb") as source:
        assert plistlib.load(source)["CFBundleIdentifier"] == "io.github.s0okju.aequo.updater-test"
    if (ROOT / "library").exists():
        (ROOT / "library").rename(ROOT / f"library-previous-{int(time.time())}")
    if (ROOT / "Applications/Aequo.app").exists():
        shutil.rmtree(ROOT / "Applications/Aequo.app")
    shutil.copytree(ROOT / "base.app", ROOT / "Applications/Aequo.app", symlinks=True)
ROOT.chmod(0o700)
CONTROL = ROOT / "control"
CONTROL.mkdir(exist_ok=True)
DATA = ROOT / "library"
PUBLIC = ROOT / "public"
PUBLIC.mkdir(exist_ok=True)
APP = ROOT / "Applications/Aequo.app"
APP.parent.mkdir(exist_ok=True)
REPORT = {"root": str(ROOT), "checks": {}}
counter = 0
process = None
mounted = False
recovery_mounted = False
server_mode = "latest"
package_mode = "valid"
requests = []


def run(args, env=None, log=None, check=True):
    if log:
        with open(ROOT / log, "a") as output:
            return subprocess.run(args, cwd=REPO, env=env, stdout=output, stderr=output, check=check)
    return subprocess.run(args, cwd=REPO, env=env, capture_output=True, text=True, check=check)


def wait_for(predicate, timeout=40):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        try:
            value = predicate()
            if value:
                return value
        except (FileNotFoundError, json.JSONDecodeError):
            pass
        time.sleep(0.1)
    raise AssertionError(f"Timed out: {predicate}")


def state():
    return json.loads((CONTROL / "state.json").read_text())


def command(action, **args):
    global counter
    counter += 1
    temp = CONTROL / "request.tmp"
    temp.write_text(json.dumps({"id": counter, "action": action, **args}))
    temp.replace(CONTROL / "request.json")
    if action in ("restart", "quit"):
        return None
    result = wait_for(lambda: (response if (response := json.loads((CONTROL / "response.json").read_text()))["id"] == counter else None))
    snapshot = result.get("result")
    if isinstance(snapshot, dict) and "revision" in snapshot:
        wait_for(lambda: state()["revision"] >= snapshot["revision"])
    return result


def phase(name):
    return wait_for(lambda: (value if (value := state())["phase"] == name else None))


def launch(env, expected_version="1.0.1"):
    global process
    for name in ("request.json", "response.json", "state.json", "pid"):
        (CONTROL / name).unlink(missing_ok=True)
    log = open(ROOT / "runtime.log", "a")
    process = subprocess.Popen([str(APP / "Contents/MacOS/aequo")], env=env, stdout=log, stderr=log)
    wait_for(lambda: state()["currentVersion"] == expected_version)


def stop():
    command("quit")
    if process:
        process.wait(timeout=10)
    time.sleep(0.5)


def digest():
    return hashlib.sha256((APP / "Contents/MacOS/aequo").read_bytes()).hexdigest()


def library():
    with sqlite3.connect(DATA / "aequo.db") as conn:
        return {table: conn.execute(f"SELECT * FROM {table} ORDER BY 1").fetchall() for table in
                ("feed_folders", "feeds", "items", "annotations", "ai_sessions", "ai_session_items", "article_cache")}


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        requests.append(self.path)
        if self.path == "/latest.json":
            if server_mode == "404":
                self.send_error(404)
                return
            version = "1.0.1" if server_mode == "same" else "1.0.2"
            signature = (ROOT / ("invalid.gz.sig" if package_mode == "invalid" else "Aequo.app.tar.gz.sig")).read_text().strip()
            manifest = {"version": version, "notes": "Isolated native integration test", "pub_date": "2026-11-19T00:00:00Z",
                        "platforms": {"darwin-aarch64": {"url": f"{origin}/S0okJu/aequo/releases/download/v{version}/Aequo.app.tar.gz", "signature": signature}}}
            if server_mode == "missing-platform":
                manifest["platforms"] = {}
            body = json.dumps(manifest).encode()
        else:
            body = (ROOT / ("invalid.gz" if package_mode == "invalid" else "Aequo.app.tar.gz")).read_bytes()
            if package_mode == "tampered":
                body = b"tampered archive"
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if package_mode == "truncated" and self.path != "/latest.json":
            self.wfile.write(body[:1024])
            self.close_connection = True
        elif self.path != "/latest.json":
            for offset in range(0, len(body), 64 * 1024):
                self.wfile.write(body[offset:offset + 64 * 1024])
                time.sleep(0.002)
        else:
            self.wfile.write(body)


port = 0
if REUSE:
    binary = (ROOT / "base.app/Contents/MacOS/aequo").read_bytes()
    match = re.search(rb"http://127\.0\.0\.1:(\d+)/latest\.json", binary)
    assert match, "Test endpoint missing in existing bundle"
    port = int(match[1])
server = http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler)
origin = f"http://127.0.0.1:{server.server_port}"
threading.Thread(target=server.serve_forever, daemon=True).start()
env = os.environ.copy()
env.update(APPLE_SIGNING_IDENTITY="-", AEQUO_UPDATE_ENDPOINT=f"{origin}/latest.json",
           AEQUO_UPDATE_TEST_CONTROL_DIR=str(CONTROL), AEQUO_UPDATE_TEST_DATA_DIR=str(DATA),
           TAURI_SIGNING_PRIVATE_KEY_PASSWORD="")

try:
    print(f"Test artifacts: {ROOT}", flush=True)
    if not REUSE:
        run(["pnpm", "tauri", "signer", "generate", "--ci", "-p", "", "-w", str(ROOT / "test.key")], log="key-generation.log")
    env["TAURI_SIGNING_PRIVATE_KEY"] = (ROOT / "test.key").read_text().strip()
    public_key = (ROOT / "test.key.pub").read_text().strip()
    if not REUSE:
        for version in ["1.0.2", "1.0.1"]:
            config = {"version": version, "identifier": "io.github.s0okju.aequo.updater-test",
                      "plugins": {"updater": {"pubkey": public_key, "requireSignedVersion": True}},
                      "bundle": {"createUpdaterArtifacts": version == "1.0.2"}}
            config_path = ROOT / f"{version}.json"
            config_path.write_text(json.dumps(config))
            print(f"Building native {version} test bundle…", flush=True)
            run(["pnpm", "tauri", "build", "--config", str(config_path), "--features", "updater-test",
                 "--target", "aarch64-apple-darwin", "--bundles", "app", "--ci", "--", "--locked"], env, f"build-{version}.log")
            output = REPO / "src-tauri/target/aarch64-apple-darwin/release/bundle/macos"
            if version == "1.0.2":
                for name in ["Aequo.app.tar.gz", "Aequo.app.tar.gz.sig"]:
                    shutil.copy2(output / name, ROOT / name)
            else:
                shutil.copytree(output / "Aequo.app", ROOT / "base.app", symlinks=True)
                shutil.copytree(output / "Aequo.app", APP, symlinks=True)
    verify_env = {**env, "TAURI_UPDATER_PUBLIC_KEY": public_key}
    run(["node", ".github/scripts/verify-update.mjs", str(ROOT / "Aequo.app.tar.gz"), str(ROOT / "Aequo.app.tar.gz.sig"), "1.0.2"], verify_env)
    REPORT["checks"]["tauri_signature_cross_check"] = True
    (ROOT / "invalid.gz").write_bytes(b"Signed but invalid gzip archive")
    run(["pnpm", "tauri", "signer", "sign", "--app-version", "1.0.2", str(ROOT / "invalid.gz")], env, "invalid-sign.log")
    launch(env)
    command("auto", enabled=False)
    stop()
    with sqlite3.connect(DATA / "aequo.db") as conn:
        conn.create_function("strip_html", 1, lambda value: value or "")
        stamp = int(time.time())
        conn.execute("INSERT INTO feed_folders(id,name,created_at) VALUES(1,'Update fixture',?)", [stamp])
        conn.execute("INSERT INTO feeds(id,url,title,format,created_at,next_fetch_at,folder_id) VALUES(1,'https://example.invalid/feed','Fixture','rss',?,?,1)", [stamp,stamp + 86400])
        conn.execute("INSERT INTO feed_icons(feed_id,mime,data,fetched_at) VALUES(1,'',X'',?)",[stamp + 86400])
        conn.execute("INSERT INTO items(id,feed_id,guid_hash,title,published_at,fetched_at,is_read,read_at,is_saved) VALUES(1,1,'fixture','Fixture',?,?,1,?,1)", [stamp,stamp,stamp])
        for row in [(1,"highlight","yellow",None),(2,"memo",None,"Saved memo")]:
            conn.execute("INSERT INTO annotations(id,item_id,kind,content_hash,start_offset,end_offset,quoted_text,color,note,created_at,updated_at) VALUES(?,1,?,'hash',0,4,'text',?,?,?,?)",[*row,stamp,stamp])
        conn.execute("INSERT INTO ai_sessions(id,provider,title,created_at,updated_at) VALUES(1,'codex','Conversation',?,?)",[stamp,stamp])
        conn.execute("INSERT INTO ai_session_items(session_id,item_id,item_title,position,added_at) VALUES(1,1,'Fixture',0,?)",[stamp])
        conn.execute("INSERT INTO article_cache(item_id,original_url,text_content,fetched_at) VALUES(1,'https://example.invalid/article','Saved article',?)",[stamp])
    expected = library()
    memory_samples = []
    sampling = threading.Event()
    def sample_memory():
        while not sampling.is_set():
            try:
                pid = (CONTROL / "pid").read_text().strip()
                rss = run(["ps", "-o", "rss=", "-p", pid], check=False).stdout.strip()
                if rss:
                    memory_samples.append(int(rss))
            except (OSError, ValueError):
                pass
            sampling.wait(0.2)
    threading.Thread(target=sample_memory, daemon=True).start()
    original_digest = digest()
    launch(env)
    server_mode = "same"
    command("check")
    phase("up_to_date")
    assert command("check")["error"] == "rate_limited"
    REPORT["checks"]["same_version_and_click_throttle"] = True
    for mode, code in [("404","http"),("missing-platform","metadata")]:
        server_mode = mode
        command("reset_cooldown")
        command("check")
        assert phase("error")["error"]["code"] == code
    REPORT["checks"]["invalid_metadata_and_404"] = True
    server_mode = "latest"
    for mode, code in [("tampered","signature"),("invalid","install_failed"),("truncated","network")]:
        package_mode = mode
        command("reset_cooldown")
        command("check")
        phase("available")
        command("install")
        assert phase("error")["error"]["code"] == code
        assert digest() == original_digest
        assert library() == expected
        if mode == "tampered":
            stop()
            launch(env)
            assert state()["error"]["code"] == "signature"
            count = len([path for path in requests if path != "/latest.json"])
            command("auto", enabled=True)
            wait_for(lambda: state()["operationId"] >= 1 and state()["phase"] == "error")
            assert len([path for path in requests if path != "/latest.json"]) == count
            command("auto", enabled=False)
            REPORT["checks"]["signature_failure_blocks_automatic_retry_across_restart"] = True
    REPORT["checks"]["tampered_signature_and_failed_install_preserve_app_and_data"] = True
    package_mode = "valid"
    command("reset_cooldown")
    command("check")
    phase("available")
    APP.parent.chmod(0o500)
    try:
        command("install")
        assert phase("error")["error"]["code"] == "permission"
        assert digest() == original_digest
    finally:
        APP.parent.chmod(0o700)
    command("reset_cooldown")
    command("check")
    phase("available")
    REPORT["checks"]["permission_failure_preserves_app"] = True
    # OFF must leave a valid update available without installing it.
    command("due")
    time.sleep(1)
    assert state()["phase"] == "available"
    command("work")
    command("install")
    duplicate = command("install")
    assert duplicate["result"]["phase"] == "waiting_for_idle"
    phase("waiting_for_idle")
    assert digest() == original_digest
    command("idle")
    phase("restart_required")
    assert digest() != original_digest
    command("draft", dirty=True)
    # Use a request with a response for rejected restart, unlike a successful restart.
    counter += 1
    (CONTROL / "request.json").write_text(json.dumps({"id":counter,"action":"restart"}))
    rejected = wait_for(lambda: r if (r := json.loads((CONTROL / "response.json").read_text()))["id"] == counter else None)
    assert rejected["error"] == "draft_active"
    command("draft", dirty=False)
    command("work")
    counter += 1
    (CONTROL / "request.json").write_text(json.dumps({"id":counter,"action":"restart"}))
    rejected = wait_for(lambda: r if (r := json.loads((CONTROL / "response.json").read_text()))["id"] == counter else None)
    assert rejected["error"] == "work_active"
    command("idle")
    command("restart")
    applied = wait_for(lambda: s if (s := state())["currentVersion"] == "1.0.2" else None)
    assert applied["appliedVersion"] == "1.0.2"
    assert library() == expected
    REPORT["checks"]["manual_update_restart_and_work_draft_gates"] = True
    stop()
    shutil.rmtree(APP)
    shutil.copytree(ROOT / "base.app", APP, symlinks=True)
    launch(env)
    command("work")
    command("auto", enabled=True)
    phase("waiting_for_idle")
    assert digest() == original_digest
    command("idle")
    phase("restart_required")
    command("restart")
    wait_for(lambda: state()["currentVersion"] == "1.0.2")
    assert library() == expected
    REPORT["checks"]["automatic_install_and_restart_with_library_preserved"] = True
    signed = run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", str(APP)])
    REPORT["checks"]["updated_bundle_codesign"] = signed.returncode == 0
    assessment = run(["spctl", "--assess", "--type", "execute", "--verbose=2", str(APP)], check=False)
    REPORT["gatekeeper"] = {"exitCode": assessment.returncode, "output": assessment.stderr.strip(),
                            "note": "Ad-hoc bundles are not notarized. Updates relaunched through Tauri app.restart / macOS LaunchServices. Download quarantine and first-launch Open Anyway remain distribution constraints."}
    stop()
    # Force a real ENOSPC during extraction on a disposable, separately mounted volume.
    image = ROOT / f"disk-test-{int(time.time())}.sparseimage"
    volume = ROOT / "disk-volume"
    volume.mkdir(exist_ok=True)
    run(["hdiutil", "create", "-size", "64m", "-fs", "HFS+", "-type", "SPARSE", "-volname", "AequoUpdateTest", str(image)])
    run(["hdiutil", "attach", "-nobrowse", "-mountpoint", str(volume), str(image)])
    mounted = True
    APP = volume / "Aequo.app"
    shutil.copytree(ROOT / "base.app", APP, symlinks=True)
    remaining = max(0, shutil.disk_usage(volume).free - 1024 * 1024)
    with open(volume / "fill", "wb") as fill:
        block = b"x" * (1024 * 1024)
        while remaining:
            chunk = block[:min(len(block),remaining)]
            fill.write(chunk)
            remaining -= len(chunk)
    launch(env)
    command("auto", enabled=False)
    command("reset_cooldown")
    command("check")
    phase("available")
    command("install")
    assert phase("error")["error"]["code"] == "install_failed"
    assert digest() == original_digest
    assert library() == expected
    REPORT["checks"]["disk_full_during_real_extraction_preserves_app_and_data"] = True
    stop()
    # Recover with a DMG copy, using the signed test release and the same library.
    recovery = ROOT / f"manual-recovery-{int(time.time())}"
    recovery.mkdir()
    with tarfile.open(ROOT / "Aequo.app.tar.gz") as archive:
        archive.extractall(recovery, filter="data")
    recovery_image = recovery.with_suffix(".dmg")
    recovery_mount = ROOT / "recovery-mount"
    recovery_mount.mkdir(exist_ok=True)
    run(["hdiutil", "create", "-srcfolder", str(recovery), "-volname", "AequoRecoveryTest", "-format", "UDZO", str(recovery_image)])
    run(["hdiutil", "attach", "-nobrowse", "-mountpoint", str(recovery_mount), str(recovery_image)])
    recovery_mounted = True
    (volume / "fill").unlink()
    APP.rename(volume / "Previous.app")
    shutil.copytree(recovery_mount / "Aequo.app", APP, symlinks=True)
    launch(env, "1.0.2")
    assert library() == expected
    run(["codesign", "--verify", "--deep", "--strict", str(APP)])
    REPORT["checks"]["manual_dmg_recovery_preserves_library"] = True
    stop()
    run(["hdiutil", "detach", str(recovery_mount)])
    recovery_mounted = False
    run(["hdiutil", "detach", str(volume)])
    mounted = False
    sampling.set()
    REPORT["memory"] = {"maxRssKiB": max(memory_samples) if memory_samples else None,
                        "packageBytes": (ROOT / "Aequo.app.tar.gz").stat().st_size,
                        "downloadLimitBytes": 256 * 1024 * 1024}
    REPORT["passed"] = True
    print(json.dumps(REPORT, indent=2), flush=True)
finally:
    (ROOT / "report.json").write_text(json.dumps(REPORT, indent=2) + "\n")
    try:
        if (CONTROL / "pid").exists():
            pid = int((CONTROL / "pid").read_text())
            os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    if recovery_mounted:
        run(["hdiutil", "detach", "-force", str(ROOT / "recovery-mount")], check=False)
    if mounted:
        run(["hdiutil", "detach", "-force", str(ROOT / "disk-volume")], check=False)
    server.shutdown()
