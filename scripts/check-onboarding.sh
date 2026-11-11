#!/usr/bin/env bash
# The onboarding check. Linux only, like every golden: the macOS and Windows
# scenes are the same window drawn from fixtures, so they render here too.
#
#   1. Renders every onboarding scene (SCENES in
#      src/rune_ui/tests/onboarding_check.rs) in both appearances and
#      compares each with its approved golden in tests/golden/. Any
#      difference fails and names the changed scenes; an intentional change
#      refreshes the goldens in the same PR
#      (HEX_UPDATE_GOLDENS=1 cargo test --bin hex onboarding_goldens). A GPU
#      adapter is required (Mesa's lavapipe will do), so the check never
#      passes by skipping.
#   2. Checks Skip: it shows only on the optional pages (the demo and the
#      practice) and lands on the next step setup still needs, never closing
#      onboarding.
#
#   scripts/check-onboarding.sh [<out-dir>]   (default target/onboarding-check)
#
# <out-dir>/scenes/ holds every render; <out-dir>/summary.md says what passed.
set -euo pipefail

[ "$(uname -s)" = Linux ] || { echo "The onboarding check renders on Linux only." >&2; exit 1; }
root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-$root/target/onboarding-check}
mkdir -p "$out"
out=$(cd "$out" && pwd)
rm -rf "$out/scenes" "$out/summary.md"
mkdir -p "$out/scenes"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$root"
# A golden refresh must never happen here.
unset HEX_UPDATE_GOLDENS

# run_one <log> <test> [env...]: runs exactly one test; anything but
# "1 passed" (a failure, or a filter that matched nothing) fails.
run_one() {
  local log=$1 test=$2
  shift 2
  env "$@" cargo test --locked --bin hex -- --exact --include-ignored --nocapture "$test" 2>&1 | tee "$log"
  local result=${PIPESTATUS[0]}
  [ "$result" = 0 ] && grep -q 'test result: ok. 1 passed' "$log"
}

scenes=$(awk '/^pub\(super\) const SCENES/,/^\];/' src/rune_ui/tests/onboarding_check.rs | grep -c '^    "')
expected=$((scenes * 2))
status=0 goldens=pass skip=pass

echo "== Onboarding scenes against their goldens"
if ! run_one "$work/goldens.log" rune_ui::tests::onboarding_check::onboarding_goldens \
  HEX_REQUIRE_GPU=1 HEX_GOLDEN_RENDERS="$out/scenes"; then
  goldens=fail status=1
fi
rendered=$(find "$out/scenes" -name '*.png' | wc -l | tr -d ' ')
if [ "$rendered" != "$expected" ]; then
  echo "Rendered $rendered scenes, expected $expected." >&2
  goldens=fail status=1
fi
changed=$(grep -oE '^[a-z0-9-]+: [0-9]+ pixels differ|rune-[a-z0-9-]+\.png is missing' "$work/goldens.log" || true)

echo "== Skip only on the optional pages"
if ! run_one "$work/skip.log" rune_ui::tests::onboarding_check::onboarding_skip; then
  skip=fail status=1
fi

{
  echo "### Onboarding check: $([ "$status" = 0 ] && echo pass || echo FAIL)"
  echo
  echo "- Commit: $(git rev-parse HEAD)"
  echo "- Scenes against goldens: $goldens ($rendered of $expected renders)"
  if [ -n "$changed" ]; then
    echo "- Changed scenes:"
    printf '%s\n' "$changed" | sed 's/^/  - /'
  fi
  echo "- Skip only on the demo and practice, to the next required step: $skip"
} > "$out/summary.md"
cat "$out/summary.md"
exit "$status"
