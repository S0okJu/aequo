# Updater signing and release operations

Aequo supports stable updates on Apple Silicon macOS using `tauri-plugin-updater` 2.14.1. Update signatures bind the app version as well as the archive bytes. Apple ad-hoc code signing and Gatekeeper are separate from updater signatures.

## Key custody

The repository owner is responsible for keeping the same updater key available for every release. Existing clients trust the public key embedded in their installed app. Losing the key requires a manual migration; do not generate a replacement merely to fix a failed build.

The initial key was configured on 2026-11-19 by `damekz`:

- Encrypted private key: `~/.tauri/aequo/updater.key` (mode `0600`, directory `0700`).
- Public key: `~/.tauri/aequo/updater.key.pub`.
- Password: macOS login keychain, service `io.github.s0okju.aequo.updater-signing`, account `damekz`.
- Non-secret setup receipt: `~/.tauri/aequo/signing-setup.json`.
- Public-key SHA-256: `a1332e6413d48dc13becbf3b8e7d5be2f5ed59791765e06e464fe3fd0c3fc1a9`.

GitHub repository configuration:

| Kind | Name | Value |
| --- | --- | --- |
| Actions secret | `TAURI_SIGNING_PRIVATE_KEY` | Contents of the encrypted private key file |
| Actions secret | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Its password |
| Actions variable | `TAURI_UPDATER_PUBLIC_KEY` | Contents of the public key file |

Keep an independently recoverable encrypted backup of the private key and password in the owner's secure backup system. GitHub Actions secrets cannot be read back as a recovery mechanism. The local key and keychain are the configured recovery sources; an off-device backup is an owner operation, not performed by this implementation.

To resume registration using the same local key:

```sh
python3 .github/scripts/setup-updater-signing.py
```

This script never prints key material or passwords, refuses a different already-configured remote public key, and does not rotate an existing key. It requires macOS keychain access and authenticated `gh` access to the repository.

## Builds and publication

The committed `tauri.conf.json` has an empty public key and `createUpdaterArtifacts: false`. This is intentional: regular PRs and local development need no production signing key, and those builds display that updates are unavailable. The merged release workflow writes a separate Tauri configuration overlay with the public key and updater artifacts enabled.

For a local signed release build, load the three values above into environment variables using a secure local mechanism, then run:

```sh
python3 .github/scripts/configure-updater.py /tmp/aequo-updater.json
pnpm tauri build --config /tmp/aequo-updater.json --target aarch64-apple-darwin --bundles app,dmg --ci -- --locked
node .github/scripts/verify-update.mjs src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Aequo.app.tar.gz src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Aequo.app.tar.gz.sig 1.0.1
```

Use the actual app version in the last argument. Tauri CLI 2.12.4 or later signs the version automatically during the build. Manually signing an archive requires `tauri signer sign --app-version X.Y.Z`.

On the provisioned Mac, `python3 .github/scripts/build-local-release.py` performs these steps using the encrypted local key and keychain password without printing them. It builds and verifies files without publishing a release.

The release workflow serializes stable publication and rechecks version ordering immediately before publishing. The publisher validates the version-bound signature again, derives notes and timestamp from the matching stable draft, uploads the full asset set, verifies asset presence, and only then marks it public/latest. A rerun resumes a draft; published releases and existing tags pointing elsewhere are never rewritten. Correct published mistakes with a new patch version.

`pub_date` is the draft creation timestamp, fixed across retries, rather than an estimate of when the final GitHub publish call will finish. The UI labels it as the release date. The updater endpoint is:

```text
https://github.com/S0okJu/aequo/releases/latest/download/latest.json
```

## Verification

```sh
pnpm check
pnpm test
pnpm lint
cargo test --manifest-path src-tauri/Cargo.toml --locked
python3 -m unittest discover -s .github/tests -v
python3 scripts/test-updater-macos.py
```

The native integration script builds two real app versions using a fresh test key, serves an isolated localhost endpoint, and launches copies in a temporary `Applications` directory. It uses a separate bundle identifier and library. The `updater-test` Cargo feature contains a file-based test driver and allows compile-time test endpoints. Never enable that feature for production builds. The production workflow does not pass it.

The script retains private temporary artifacts and a `report.json` with the actual checks performed. No test result substitutes for assessing Gatekeeper on a downloaded release. Ad-hoc apps may require the documented first-launch Open Anyway flow. Stop publication if a previously approved installation cannot update and relaunch.

## Failure and recovery

HTTP errors, invalid metadata, missing platforms, and oversized responses are errors, never evidence that the app is up to date. Automatic transient retries use 1 minute, 5 minutes, 30 minutes, then 6 hours; longer `Retry-After` values take precedence. A bad signature does not get automatically retried for the same version. A manual recheck or new release allows reevaluation.

The native stream caps metadata at 256 KiB and the compressed package at 256 MiB; requests time out after 15 seconds and 15 minutes respectively. Downloading is bounded in Aequo because the plugin's download API buffers without a size cap. Tauri verifies signed bytes and performs app replacement and restoration. No custom shell installer is used.

If installation cannot finish, use the official DMG to replace the app bundle without deleting the Aequo data directory. If the updater reports that the previous app could not be restored, reinstall the official bundle before restarting. Libraries, annotations, and AI session references are not removed by the updater. Provider transcript files remain with their CLI provider.

Pending installation is recorded before replacement. On next launch, an exact version match records success. A mismatch shows that the update was not applied and clears the pending marker; the app never enters an automatic restart loop.
