# macOS releases

Aequo builds only for Apple Silicon (`aarch64-apple-darwin`). GitHub Actions
uses ad-hoc signing (`APPLE_SIGNING_IDENTITY=-`); no Apple account, certificate,
notarization credentials, or custom repository secrets are required.

## Continuous integration

`.github/workflows/ci.yml` runs on pushes to `release/**` and pull requests into
`main` or `release/**`. It checks app version consistency, Svelte/TypeScript,
Vitest, Rust formatting, ESLint/Clippy, Rust tests, and release-script regression
tests. It then builds a DMG,
verifies that the app is ARM64 and ad-hoc signed, and uploads the installer as
the `macos-aarch64-dmg` Actions artifact (retained for seven days).

Node 24, pnpm 12.15.0, and stable Rust are installed on a macOS 15 ARM64 runner.
Dependency installation uses the committed pnpm and Cargo lockfiles.

App builds regenerate the bundled icons from `img/logo.png` before building
the frontend. Replace that image to update the app icon, or run `pnpm icons`
to regenerate the icon files manually.

## Publish a release

1. Work on a branch named `release/vX.Y.Z` (for example `release/v1.0.0`).
2. Before opening the release PR, set `package.json`,
   `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml` to the same version
   without the `v` prefix. Refresh `src-tauri/Cargo.lock` with Cargo and commit
   it too. Release PRs into `main` must match their branch version.
3. Open the release branch's PR into `main` and wait for CI to pass.
4. Merge the PR. `.github/workflows/release.yml` validates and builds the exact
   merged commit using the reusable CI workflow. Only same-repository
   `release/*` PRs that were actually merged into `main` can publish.
5. After verification succeeds, the publish job creates `vX.Y.Z` on that commit,
   creates a draft GitHub Release, uploads the verified DMG and `SHA256SUMS`,
   and publishes the release. It does not rebuild the installer.

Closing an unmerged PR, pushing to a release branch, merging a non-release PR,
or pushing directly to `main` does not publish a release. A failed verification
does not create a tag or release. A publishing failure may leave a tag or draft;
use **Re-run all jobs** on that release workflow to resume. Existing tags pointing
to another commit are rejected. Already published releases are left unchanged.

## Repository configuration

- Enable GitHub Actions. The publish job requests `contents: write` for the
  automatic `GITHUB_TOKEN`; repository/organization policies must allow it.
- Protect `main`: require pull requests and the CI job
  **Test and build Apple Silicon**, and block direct pushes.
- Tag rules for `v*`, if configured, must allow this workflow to create tags.
- Keep the workflow files in the release PR. For predictable first-release
  operation, land the workflow infrastructure on `main` before the release PR.
- Automated publication begins after a release PR is merged; there is no
  additional approval step in these workflows.

## Installing an ad-hoc signed build

Download the DMG, open it, and copy Aequo to Applications. Ad-hoc signing does
not establish an Apple-verified developer identity or provide notarization.
macOS can block the first launch. If you trust the download, attempt to open
Aequo, then allow it through **System Settings → Privacy & Security → Open
Anyway**. Do not disable Gatekeeper globally.

References: [Tauri ad-hoc signing](https://tauri.app/distribute/sign/macos/#ad-hoc-signing),
[GitHub merged PR triggers](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#running-your-pull_request-workflow-when-a-pull-request-merges).
