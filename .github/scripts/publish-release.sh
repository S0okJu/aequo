#!/usr/bin/env bash
set -euo pipefail

: "${RELEASE_TAG:?Missing release tag}"
: "${RELEASE_SHA:?Missing release commit}"
: "${GH_REPO:?Missing repository}"

test "$(git rev-parse HEAD)" = "$RELEASE_SHA"
script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
release_data=$(mktemp -d)
trap 'rm -rf "$release_data"' EXIT
shopt -s nullglob
dmgs=(dist/*.dmg)
if [[ ${#dmgs[@]} -ne 1 ]]; then
  echo "Expected exactly one Apple Silicon DMG, found ${#dmgs[@]}" >&2
  exit 1
fi

# A rerun may reuse its own tag, but must never move an existing release tag.
remote_tag=$(git ls-remote --tags origin "refs/tags/$RELEASE_TAG")
if [[ -n "$remote_tag" ]]; then
  git fetch --no-tags origin "refs/tags/$RELEASE_TAG:refs/tags/$RELEASE_TAG"
  if [[ "$(git rev-parse "$RELEASE_TAG^{commit}")" != "$RELEASE_SHA" ]]; then
    echo "Tag $RELEASE_TAG already points to a different commit" >&2
    exit 1
  fi
else
  git tag "$RELEASE_TAG" "$RELEASE_SHA"
  git push origin "refs/tags/$RELEASE_TAG"
fi

# Published releases are immutable here; a rerun only resumes an unfinished draft.
if existing=$(gh release view "$RELEASE_TAG" --json isDraft --jq '.isDraft'); then
  if [[ "$existing" == false ]]; then
    echo "Release $RELEASE_TAG is already published; nothing to change"
    exit 0
  fi
else
  notes=$(mktemp)
  cat > "$notes" <<'NOTES'
Apple Silicon (M-series) Macs only.

This app is ad-hoc signed, without an Apple Developer ID or Apple notarization.
macOS may block the first launch. If you trust this download, attempt to open
the app, then use System Settings → Privacy & Security → Open Anyway.

SHA256SUMS contains the SHA-256 checksum of the DMG installer.
NOTES
  gh release create "$RELEASE_TAG" --verify-tag --draft \
    --title "Aequo $RELEASE_TAG" --generate-notes --notes-file "$notes"
  rm "$notes"
fi

gh release list --limit 1000 --json tagName,isDraft,isPrerelease > "$release_data/releases.json"
python3 "$script_dir/update-manifest.py" guard "$release_data/releases.json"
gh release view "$RELEASE_TAG" --json tagName,isDraft,isPrerelease,body,createdAt > "$release_data/release.json"
python3 "$script_dir/update-manifest.py" prepare "$release_data/release.json"
archives=(dist/*.app.tar.gz)
signatures=(dist/*.app.tar.gz.sig)
test "${#signatures[@]}" -eq 1
gh release upload "$RELEASE_TAG" "${dmgs[@]}" "${archives[@]}" "${signatures[@]}" dist/SHA256SUMS dist/latest.json --clobber
# Confirm every asset reached the draft before making the release discoverable.
gh release view "$RELEASE_TAG" --json assets > "$release_data/assets.json"
python3 - "$release_data/assets.json" <<'PY'
import json, pathlib, sys
expected = {path.name for path in pathlib.Path('dist').iterdir()
            if path.name.endswith(('.dmg', '.app.tar.gz', '.app.tar.gz.sig'))
            or path.name in ('latest.json', 'SHA256SUMS')}
actual = {asset['name'] for asset in json.load(open(sys.argv[1]))['assets']}
if not expected <= actual:
    raise SystemExit(f'Missing uploaded assets: {expected - actual}')
PY
gh release list --limit 1000 --json tagName,isDraft,isPrerelease > "$release_data/releases.json"
python3 "$script_dir/update-manifest.py" guard "$release_data/releases.json"
gh release edit "$RELEASE_TAG" --draft=false --latest
