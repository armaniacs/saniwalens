#!/usr/bin/env bash
# Flaky-test detector (PBI 25): run the suite N times, report any test that
# failed at least once with its count. Deterministic in itself: the same
# outputs always parse to the same report; only the suite's own flakiness
# varies. No CI job by default (run on demand); needs no tokens.
# Portable POSIX-ish bash (no associative arrays: macOS ships bash 3.2).
#
# Usage: scripts/flaky_repeat.sh [N]  (default N=5)
set -u
N="${1:-5}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
: > "$TMP/fails.txt"

i=0
while [ "$i" -lt "$N" ]; do
    i=$((i + 1))
    # One full run per iteration; a red run still leaves its log behind.
    cargo test --locked >"$TMP/run-$i.log" 2>&1 || true
    # `test <name> ... FAILED` lines, plus the bare-name `failures:` list.
    sed -E -n 's/^test ([^ ]+) \.\.\. FAILED$/\1/p' "$TMP/run-$i.log" >>"$TMP/fails.txt"
    awk '/^failures:$/{f=1; next} /^$/{f=0} f && /^    [A-Za-z0-9_:]+$/ {gsub(/^    /,""); print}' \
        "$TMP/run-$i.log" >>"$TMP/fails.txt"
done

if [ ! -s "$TMP/fails.txt" ]; then
    echo "flaky check: $N runs, no failures"
    exit 0
fi
COUNT="$(LC_ALL=C sort -u "$TMP/fails.txt" | wc -l | tr -d ' ')"
echo "flaky check: $N runs, $COUNT test(s) failed at least once:"
LC_ALL=C sort "$TMP/fails.txt" | uniq -c | LC_ALL=C sort -rn | while read -r n name; do
    echo "  ${n}x  $name"
done
echo "Suspect non-determinism cuts: wall-clock reads, RNG without a fixed"
echo "seed, and parallel-test interference (shared files, env vars, ports)."
exit 1
