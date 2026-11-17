#!/usr/bin/env sh
# Fails when a component sets a font-size or font-weight outside the role
# tokens defined in src/routes/+layout.svelte (docs/UI_SIMPLIFICATION_PLAN.md
# §3.13). Token definitions themselves (`--text-*:` / `--weight-*:`) are allowed.
violations=$(grep -rnE 'font-(size|weight):' src --include='*.svelte' |
  grep -vE 'font-size: var\(--text-[a-z]+\)|font-weight: var\(--weight-[a-z]+\)')

if [ -n "$violations" ]; then
  echo "Use var(--text-*) / var(--weight-*) for font-size and font-weight:"
  echo "$violations"
  exit 1
fi
