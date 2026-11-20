#!/usr/bin/env bash
# Checks that the fork-owned files (docs/upstream.lock `fork_owned`) carry no trace of the
# upstream author's identity: no site, sponsor, affiliate or contact address.
# Run: bash scripts/tests/fork-identity.test.sh
# Allowed: links to https://github.com/farion1231/cc-switch... and the upstream copyright line
# of LICENSE.
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
# shellcheck source=../sync-upstream.sh
source "$ROOT/scripts/sync-upstream.sh"

IDENTITY_RE='ccswitch\.io|trendshift|star-history|assets/partners|farion1231@|@farion1231|github: farion1231|aff=cc-switch|invitecode='
EMAIL_RE='[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}'

# scan_file <path>: prints "path:line" for every identity match in the file's text.
scan_file() {
  local path="$1"
  {
    grep -n -i -E -- "$IDENTITY_RE" "$path"
    grep -n -E -- "$EMAIL_RE" "$path" | grep -v 'noreply\.github\.com'
  } 2>/dev/null | cut -d: -f1 | sort -un | sed "s|^|$path:|"
}

# scan_tree <root>: the same over every tracked fork-owned file of the repository at <root>.
scan_tree() {
  local path
  while IFS= read -r path; do
    is_fork_owned "$path" || continue
    [ -f "$1/$path" ] || continue
    (cd "$1" && scan_file "$path")
  done < <(git -C "$1" ls-files)
}

fails=0
passes=0
check() { # check <name> <condition exit code>
  if [ "$2" -eq 0 ]; then
    echo "ok   $1"
    passes=$((passes + 1))
  else
    echo "FAIL $1"
    fails=$((fails + 1))
  fi
}

# Positive control on a scratch repository: every kind of trace is found, allowed text is not.
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
git init -q "$TMP/r"
mkdir -p "$TMP/r/docs" "$TMP/r/src"
cp "$ROOT/docs/upstream.lock" "$TMP/r/docs/upstream.lock"
printf 'clean\nsee https://ccswitch.io now\nmail a.b@example.org\nnoreply: 1+x@users.noreply.github.com\nowner @farion1231 here\n[x](https://github.com/farion1231/cc-switch/issues)\n' >"$TMP/r/README.md"
printf 'Copyright (c) 2025 Jason Young\nCopyright (c) 2026 alxwlw\n' >"$TMP/r/LICENSE"
printf 'star-history\n' >"$TMP/r/src/not-owned.txt"
git -C "$TMP/r" add -A

got="$(scan_tree "$TMP/r" | tr '\n' ' ')"
ok=1
[ "$got" = "README.md:2 README.md:3 README.md:5 " ] && ok=0
check "scan finds site, address and handle; skips noreply, upstream links, other files" "$ok"

# The real tree.
real="$(scan_tree "$ROOT")"
if [ -n "$real" ]; then
  echo "$real"
fi
ok=1
[ -z "$real" ] && ok=0
check "fork-owned files of this repository carry no upstream identity" "$ok"

echo "$passes passed, $fails failed"
[ "$fails" -eq 0 ]
