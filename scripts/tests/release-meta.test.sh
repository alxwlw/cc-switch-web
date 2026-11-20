#!/usr/bin/env bash
# Tests scripts/release-meta.sh: what a Release (web) run builds, and when it publishes.
# Run: bash scripts/tests/release-meta.test.sh   (local gate; CI runs every scripts/tests/*.test.sh)
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$HERE/../release-meta.sh"

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

check_has() { # check_has <name> <haystack> <needle>
  if [[ "$2" == *"$3"* ]]; then
    echo "ok   $1"
    passes=$((passes + 1))
  else
    echo "FAIL $1: output lacks '$3'"
    fails=$((fails + 1))
  fi
}

# A repository root with upstream 4.0.35 and changelog sections for two releases.
ROOT="$TMP/repo"
mkdir -p "$ROOT/src-tauri"
printf '[package]\nname = "cc-switch"\nversion = "4.0.35"\n' >"$ROOT/src-tauri/Cargo.toml"
printf '# Changelog\n\n## v4.0.35-web.1 - 2026-11-20\n\n## v4.0.35-web.10 - 2026-11-21\n' >"$ROOT/CHANGELOG.md"

# run_meta <event> <ref type> <ref name>: sets RC OUT ERR
run_meta() {
  OUT="$(cd "$ROOT" && EVENT_NAME="$1" REF_TYPE="$2" REF_NAME="$3" bash "$SCRIPT" 2>"$TMP/err")"
  RC=$?
  ERR="$(cat "$TMP/err")"
}

run_meta push tag v4.0.35-web.1
check "a pushed release tag: exit code" "$RC" "0"
check "a pushed release tag: output" "$OUT" $'tag=v4.0.35-web.1\nversion=4.0.35-web.1\npublish=true'

run_meta push tag v4.0.35-web.10
check "a two-digit counter is fine: exit code" "$RC" "0"
check_has "a two-digit counter is fine: version" "$OUT" "version=4.0.35-web.10"

run_meta workflow_dispatch tag v4.0.35-web.1
check "a manual run on a tag never publishes: exit code" "$RC" "0"
check_has "a manual run on a tag: tag" "$OUT" "tag=v4.0.35-web.1"
check_has "a manual run on a tag: publish" "$OUT" "publish=false"

run_meta pull_request branch 43/merge
check "a pull request builds web.0: exit code" "$RC" "0"
check "a pull request builds web.0: output" "$OUT" $'tag=v4.0.35-web.0\nversion=4.0.35-web.0\npublish=false'

run_meta workflow_dispatch branch feat-cld/x
check_has "a manual run on a branch does not publish" "$OUT" "publish=false"

# the tag regex: no leading zero in any number
run_meta push tag v4.0.35-web.01
check "a counter with a leading zero: exit code" "$RC" "1"
check_has "a counter with a leading zero: stderr" "$ERR" "leading zero"
check "a counter with a leading zero: nothing printed" "$OUT" ""

run_meta push tag v04.0.35-web.1
check "an upstream number with a leading zero: exit code" "$RC" "1"
check_has "an upstream number with a leading zero: stderr" "$ERR" "leading zero"

run_meta push tag v4.00.35-web.1
check "a minor with a leading zero: exit code" "$RC" "1"
check_has "a minor with a leading zero: stderr" "$ERR" "leading zero"

# the other refusals
run_meta push tag v4.0.35
check "a tag without -web.<n>: exit code" "$RC" "1"
check_has "a tag without -web.<n>: stderr" "$ERR" "not a release tag"

run_meta push tag v4.0.36-web.1
check "a tag of another upstream version: exit code" "$RC" "1"
check_has "a tag of another upstream version: stderr" "$ERR" "src-tauri/Cargo.toml says 4.0.35"

run_meta push tag v4.0.35-web.2
check "a tag without a changelog section: exit code" "$RC" "1"
check_has "a tag without a changelog section: stderr" "$ERR" "CHANGELOG.md has no line"

echo
echo "$passes passed, $fails failed"
[ "$fails" -eq 0 ]
