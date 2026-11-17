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
    print("true" if value == "draft" else "false")
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
        self.assertEqual(self.gh_calls(), [
            ["release", "view"], ["release", "create"],
            ["release", "upload"], ["release", "edit"],
        ])
        self.assertTrue((self.repo / "dist/SHA256SUMS").is_file())
        self.publish()
        self.assertEqual(self.gh_calls()[-1], ["release", "view"])
        self.assertEqual(len(self.gh_calls()), 5)

    def test_existing_draft_is_resumed(self):
        self.publish_fixture("draft")
        self.publish()
        self.assertEqual(self.gh_calls(), [
            ["release", "view"], ["release", "upload"], ["release", "edit"],
        ])

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


if __name__ == "__main__":
    unittest.main()
