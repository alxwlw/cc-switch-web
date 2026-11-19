#!/usr/bin/env bash
# Tests scripts/tests/run-all.sh against a temporary tests directory.
# Run: bash scripts/tests/run-all.test.sh   (local gate; CI runs every scripts/tests/*.test.sh)
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fails=0
passes=0
check() { # check <name> <got> <want>
  if [ "$2" = "$3" ]; then
    echo "ok   $1"
    passes=$((passes + 1))
  else
    echo "FAIL $1: got '$2', want '$3'"
    fails=$((fails + 1))
  fi
}

mkdir -p "$TMP/green/tests" "$TMP/red/tests" "$TMP/empty/tests"
for d in green red empty; do cp "$HERE/run-all.sh" "$TMP/$d/tests/run-all.sh"; done
printf 'echo suite-a\nexit 0\n' >"$TMP/green/tests/a.test.sh"
printf 'echo suite-b\nexit 0\n' >"$TMP/green/tests/b.test.sh"
printf 'echo suite-a\nexit 0\n' >"$TMP/red/tests/a.test.sh"
printf 'echo suite-fails\nexit 1\n' >"$TMP/red/tests/b.test.sh"
printf 'echo suite-c\nexit 0\n' >"$TMP/red/tests/c.test.sh"

out="$(bash "$TMP/green/tests/run-all.sh" 2>&1)"
check "all green suites exit 0" "$?" "0"
check "all green suites are listed" "$(grep -c 'suite-' <<<"$out")" "2"

out="$(bash "$TMP/red/tests/run-all.sh" 2>&1)"
check "one failing suite makes the runner exit 1" "$?" "1"
check "the failing suite is named" "$(grep -c 'failed: b.test.sh' <<<"$out")" "1"
check "a suite after the failing one still runs" "$(grep -c 'suite-' <<<"$out")" "3"

bash "$TMP/empty/tests/run-all.sh" >/dev/null 2>&1
check "no suites at all is an error" "$?" "1"

echo
echo "$passes passed, $fails failed"
[ "$fails" = 0 ]
