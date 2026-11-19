"""Exercise release guards with isolated repositories and a fake GitHub CLI."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.env = os.environ.copy()
        self.env.pop("GITHUB_OUTPUT", None)
        self.env.pop("RELEASE_BRANCH", None)

    def run_command(self, *args, check=True):
        return subprocess.run(
            args, cwd=self.repo, env=self.env, text=True,
            capture_output=True, check=check,
        )

    def version_fixture(self):
        scripts = self.repo / ".github/scripts"
        scripts.mkdir(parents=True)
        shutil.copy(SCRIPTS / "release-version.py", scripts)
        (self.repo / "src-tauri").mkdir()
        (self.repo / "package.json").write_text('{"version":"1.2.3"}')
        (self.repo / "src-tauri/tauri.conf.json").write_text('{"version":"1.2.3"}')
        (self.repo / "src-tauri/Cargo.toml").write_text(
            '[package]\nname="aequo"\nversion="1.2.3"\n'
        )
        (self.repo / "src-tauri/Cargo.lock").write_text(
            '[[package]]\nname="aequo"\nversion="1.2.3"\n'
        )

    def test_release_version_outputs(self):
        self.version_fixture()
        self.env["RELEASE_BRANCH"] = "release/v1.2.3"
        output = self.root / "output"
        self.env["GITHUB_OUTPUT"] = str(output)
        self.run_command("python3", ".github/scripts/release-version.py")
        self.assertEqual(output.read_text(), "version=1.2.3\ntag=v1.2.3\n")

    def test_branch_version_mismatch_is_rejected(self):
        self.version_fixture()
        self.env["RELEASE_BRANCH"] = "release/v2.0.0"
        result = self.run_command("python3", ".github/scripts/release-version.py", check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires app version 2.0.0", result.stderr)

    def test_lockfile_version_mismatch_is_rejected(self):
        self.version_fixture()
        (self.repo / "src-tauri/Cargo.lock").write_text(
            '[[package]]\nname="aequo"\nversion="0.1.0"\n'
        )
        result = self.run_command("python3", ".github/scripts/release-version.py", check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("App versions must match", result.stderr)

    def test_non_version_release_branch_is_rejected(self):
        self.version_fixture()
        self.env["RELEASE_BRANCH"] = "release/preview"
        result = self.run_command("python3", ".github/scripts/release-version.py", check=False)
        self.assertNotEqual(result.returncode, 0)

    def publish_fixture(self, state="missing"):
        self.run_command("git", "init", "--bare", str(self.root / "remote.git"))
        self.run_command("git", "init")
        self.run_command("git", "config", "user.name", "Release Test")
        self.run_command("git", "config", "user.email", "test@example.com")
        self.run_command("git", "config", "commit.gpgsign", "false")
        self.run_command("git", "config", "tag.gpgsign", "false")
        (self.repo / "app.txt").write_text("app")
        self.run_command("git", "add", "app.txt")
        self.run_command("git", "commit", "-m", "Initial")
        self.run_command("git", "remote", "add", "origin", str(self.root / "remote.git"))
        self.run_command("git", "push", "origin", "HEAD:refs/heads/main")
        self.env.update(
            RELEASE_TAG="v1.2.3",
            RELEASE_SHA=self.run_command("git", "rev-parse", "HEAD").stdout.strip(),
            GH_REPO="test/aequo",
            GH_LOG=str(self.root / "gh.log"),
            GH_STATE=str(self.root / "gh.state"),
        )
        Path(self.env["GH_STATE"]).write_text(state)
        (self.repo / "dist").mkdir()
        (self.repo / "dist/Aequo_1.2.3_aarch64.dmg").write_bytes(b"test installer")
        (self.repo / "dist/Aequo.app.tar.gz").write_bytes(b"test update")
        key = self.run_command("node", "--input-type=module", "-e", r"""
import {generateKeyPairSync, sign, createHash} from 'node:crypto';
import {writeFileSync} from 'node:fs';
const {publicKey, privateKey} = generateKeyPairSync('ed25519');
const id = Buffer.alloc(8, 1);
const key = Buffer.concat([Buffer.from('Ed'), id, publicKey.export({format:'der',type:'spki'}).subarray(-32)]);
const message = createHash('blake2b512').update(Buffer.from('test update')).digest();
const signature = sign(null, message, privateKey);
const sig = Buffer.concat([Buffer.from('ED'), id, signature]);
const comment = 'timestamp:1\tversion:1.2.3';
const globalSig = sign(null, Buffer.concat([signature,Buffer.from(comment)]), privateKey);
const wrap = s => Buffer.from(s).toString('base64');
writeFileSync('dist/Aequo.app.tar.gz.sig', wrap(`untrusted comment: test\n${sig.toString('base64')}\ntrusted comment: ${comment}\n${globalSig.toString('base64')}\n`));
console.log(wrap(`untrusted comment: test\n${key.toString('base64')}\n`));
""").stdout.strip()
        self.env["TAURI_UPDATER_PUBLIC_KEY"] = key
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        gh = bin_dir / "gh"
        gh.write_text('''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
state = Path(os.environ["GH_STATE"])
with open(os.environ["GH_LOG"], "a") as log:
    log.write(json.dumps(args) + "\\n")
if args[:2] == ["release", "view"]:
    value = state.read_text()
    if value == "missing":
        sys.exit(1)
    fields = args[args.index("--json") + 1]
    if fields == "isDraft":
        print("true" if value == "draft" else "false")
    elif fields == "assets":
        assets = [] if os.environ.get("GH_MISSING_ASSETS") else [
            {"name":p.name} for p in Path("dist").iterdir()]
        print(json.dumps({"assets":assets}))
    else:
        print(json.dumps({"tagName":"v1.2.3", "isDraft":value=="draft",
            "isPrerelease":False,"body":"Release notes", "createdAt":"2026-11-19T00:00:00Z"}))
elif args[:2] == ["release", "list"]:
    print(os.environ.get("GH_RELEASES", "[]"))
elif args[:2] == ["release", "create"]:
    state.write_text("draft")
elif args[:2] == ["release", "edit"]:
    state.write_text("published")
''')
        gh.chmod(0o755)
        self.env["PATH"] = str(bin_dir) + os.pathsep + self.env["PATH"]

    def publish(self, check=True):
        return self.run_command("bash", str(SCRIPTS / "publish-release.sh"), check=check)

    def gh_calls(self):
        log = Path(self.env["GH_LOG"])
        return [json.loads(line)[:2] for line in log.read_text().splitlines()]

    def test_publish_then_rerun_does_not_modify_published_release(self):
        self.publish_fixture()
        self.publish()
        calls = self.gh_calls()
        self.assertEqual(calls.count(["release", "upload"]), 1)
        self.assertEqual(calls[-1], ["release", "edit"])
        manifest = json.loads((self.repo / "dist/latest.json").read_text())
        self.assertEqual(manifest["version"], "1.2.3")
        self.assertEqual(manifest["notes"], "Release notes")
        self.assertIn("Aequo.app.tar.gz", (self.repo / "dist/SHA256SUMS").read_text())
        count = len(calls)
        self.assertTrue((self.repo / "dist/SHA256SUMS").is_file())
        self.publish()
        self.assertEqual(self.gh_calls()[-1], ["release", "view"])
        self.assertEqual(len(self.gh_calls()), count + 1)

    def test_existing_draft_is_resumed(self):
        self.publish_fixture("draft")
        self.publish()
        self.assertNotIn(["release", "create"], self.gh_calls())
        self.assertEqual(self.gh_calls()[-1], ["release", "edit"])

    def test_existing_tag_on_another_commit_is_rejected(self):
        self.publish_fixture()
        self.run_command("git", "tag", "v1.2.3")
        self.run_command("git", "push", "origin", "refs/tags/v1.2.3")
        (self.repo / "app.txt").write_text("changed")
        self.run_command("git", "commit", "-am", "Next")
        self.env["RELEASE_SHA"] = self.run_command("git", "rev-parse", "HEAD").stdout.strip()
        result = self.publish(check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("different commit", result.stderr)
        self.assertFalse(Path(self.env["GH_LOG"]).exists())

    def test_missing_or_tampered_archive_cannot_be_published(self):
        self.publish_fixture()
        (self.repo / "dist/Aequo.app.tar.gz").write_bytes(b"tampered")
        self.assertNotEqual(self.publish(check=False).returncode, 0)
        self.assertNotIn(["release", "upload"], self.gh_calls())
        self.assertNotIn(["release", "edit"], self.gh_calls())

    def test_incomplete_uploaded_assets_remain_draft(self):
        self.publish_fixture()
        self.env["GH_MISSING_ASSETS"] = "1"
        self.assertNotEqual(self.publish(check=False).returncode, 0)
        self.assertNotIn(["release", "edit"], self.gh_calls())
        self.assertEqual(Path(self.env["GH_STATE"]).read_text(), "draft")

    def test_stable_version_regression_is_rejected(self):
        self.publish_fixture()
        self.env["GH_RELEASES"] = json.dumps([{
            "tagName":"v1.10.0", "isDraft":False, "isPrerelease":False}])
        self.assertNotEqual(self.publish(check=False).returncode, 0)
        self.assertNotIn(["release", "upload"], self.gh_calls())

    def test_missing_signing_key_prevents_release_config(self):
        self.env.pop("TAURI_UPDATER_PUBLIC_KEY", None)
        result = self.run_command("python3", str(SCRIPTS / "configure-updater.py"),
                                  str(self.root / "config.json"), check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "config.json").exists())

    def test_release_overlay_enables_version_bound_signatures(self):
        self.publish_fixture()
        self.run_command("python3", str(SCRIPTS / "configure-updater.py"), str(self.root / "config.json"))
        config = json.loads((self.root / "config.json").read_text())
        self.assertTrue(config["bundle"]["createUpdaterArtifacts"])
        self.assertTrue(config["plugins"]["updater"]["requireSignedVersion"])


if __name__ == "__main__":
    unittest.main()
