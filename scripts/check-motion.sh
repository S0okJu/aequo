#!/usr/bin/env sh
# Fails when a component drives CSS transitions/animations from literal
# durations or easings, duplicates the shared spin keyframe, or animates a
# layout/paint-heavy property without an explicit, justified exception
# (docs/UNIFIED_UX_MOTION_PLAN.md §3.3). Token definitions themselves
# (+layout.svelte's :root block) are allowed.
fail=0

# Rule 1: transition/animation duration must come from var(--motion-*) or
# var(--transition*), never a bare ms/s literal.
literal_durations=$(grep -rnE '(transition|animation)(-duration)?[[:space:]]*:[^;]*[0-9]+(ms|s)\b' src --include='*.svelte' |
  grep -v 'src/routes/+layout.svelte' |
  grep -vE 'var\(--(motion|transition)[a-z-]*\)')
if [ -n "$literal_durations" ]; then
  echo "Use var(--motion-*) / var(--transition*) instead of a literal duration:"
  echo "$literal_durations"
  fail=1
fi

# Rule 2: cubic-bezier(...) literals only live in the token definitions.
literal_easing=$(grep -rnE 'cubic-bezier\(' src --include='*.svelte' | grep -v 'src/routes/+layout.svelte')
if [ -n "$literal_easing" ]; then
  echo "cubic-bezier(...) literals are only allowed in +layout.svelte's token definitions:"
  echo "$literal_easing"
  fail=1
fi

# Rule 3: the spinner's rotation keyframe is defined once, not copy-pasted.
spin_dupes=$(grep -rl '@keyframes spin' src --include='*.svelte' | grep -v 'src/lib/components/Spinner.svelte')
if [ -n "$spin_dupes" ]; then
  echo "@keyframes spin must live only in Spinner.svelte, found also in:"
  echo "$spin_dupes"
  fail=1
fi

# Rule 4: don't animate properties that force layout/paint instead of just
# compositing (transform/opacity/background-color are fine). Allowlist is a
# short, explicit list of files where the property genuinely has to move -
# add a line + one-sentence reason above it rather than loosening the regex.
#
# A plain line-based grep misses this: prettier wraps a multi-value
# `transition:` across several lines (property name on one line, each
# comma-separated value on the next), so "transition" and e.g. "height"
# are often never on the same physical line. `perl -0777` reads each whole
# file at once so the regex can span those line breaks.
#
# Allowed:
#   - src/routes/+page.svelte: grid-template-columns (panel open/close is a
#     real grid-track resize, not a position change - UNIFIED_UX_MOTION_PLAN §5.1)
#   - src/lib/components/SegmentedControl.svelte: width (the thumb has to
#     resize with the selected label's width, which varies per locale/value)
#   - src/routes/settings/+page.svelte: height (the nav indicator's height
#     matches a nav link's; links are min-height 36px today, but a long
#     localized section name could still wrap to two lines at 200% zoom)
forbidden=$(find src -name '*.svelte' -print0 | xargs -0 perl -0777 -ne '
  while (/(transition|animation)\s*:\s*((?:[^;]|\n)*?);/gs) {
    my ($prop, $value) = ($1, $2);
    my $start = $-[0];
    next
      unless $value =~ /\b(?:width|height|grid-template-columns|clip-path|box-shadow|filter|top|left|outline)\b/;
    my $before = substr($_, 0, $start);
    my $line = 1 + ($before =~ tr/\n//);
    (my $flat = $value) =~ s/\s+/ /g;
    print "$ARGV:$line: $prop: $flat;\n";
  }
' |
  grep -v 'src/routes/+page.svelte.*grid-template-columns' |
  grep -v 'src/lib/components/SegmentedControl.svelte.*width' |
  grep -v 'src/routes/settings/+page.svelte.*height')
if [ -n "$forbidden" ]; then
  echo "Animating this property triggers layout/paint - use transform/opacity, or add a justified allowlist line in scripts/check-motion.sh:"
  echo "$forbidden"
  fail=1
fi

exit $fail
