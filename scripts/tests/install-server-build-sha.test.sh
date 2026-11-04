#!/usr/bin/env bash
# Tests resolve_build_sha from scripts/install-server.sh in temporary repositories.
# Run: bash scripts/tests/install-server-build-sha.test.sh   (local gate; CI does not run it)
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$HERE/../install-server.sh"
# shellcheck source=../install-server.sh
source "$SCRIPT"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t

fails=0
passes=0

# new_repo <name>: a repository with one commit that tracks file `f`; prints its path.
new_repo() {
  local dir="$TMP/$1"
  mkdir -p "$dir"
  git -C "$dir" init -q
  echo one >"$dir/f"
  git -C "$dir" add f
  git -C "$dir" commit -q -m one
  printf '%s\n' "$dir"
}

# expect <name> <expected stdout> <expected stderr substring or ""> <root>
expect() {
  local name="$1" want="$2" want_err="$3" root="$4" out err errfile
  errfile="$TMP/stderr"
  out="$(resolve_build_sha "$root" 2>"$errfile")"
  err="$(cat "$errfile")"
  if [ "$out" != "$want" ]; then
    echo "FAIL $name: stdout '$out', want '$want'"
    fails=$((fails + 1))
    return
  fi
  if [ -n "$want_err" ] && [[ "$err" != *"$want_err"* ]]; then
    echo "FAIL $name: stderr '$err' lacks '$want_err'"
    fails=$((fails + 1))
    return
  fi
  if [ -z "$want_err" ] && [ -n "$err" ]; then
    echo "FAIL $name: unexpected stderr '$err'"
    fails=$((fails + 1))
    return
  fi
  echo "ok   $name"
  passes=$((passes + 1))
}

repo="$(new_repo clean)"
head_sha="$(git -C "$repo" rev-parse HEAD)"
expect "clean tree prints HEAD" "$head_sha" "" "$repo"

repo="$(new_repo untracked)"
head_sha="$(git -C "$repo" rev-parse HEAD)"
echo x >"$repo/untracked"
expect "untracked file does not count" "$head_sha" "" "$repo"

repo="$(new_repo unstaged)"
echo two >>"$repo/f"
expect "unstaged tracked change prints nothing and warns" "" "tracked files differ" "$repo"

repo="$(new_repo staged)"
echo two >>"$repo/f"
git -C "$repo" add f
expect "staged tracked change prints nothing and warns" "" "tracked files differ" "$repo"

plain="$TMP/plain"
mkdir -p "$plain"
echo x >"$plain/f"
expect "plain directory prints nothing" "" "" "$plain"

repo="$(new_repo outer)"
mkdir -p "$repo/sub"
expect "subdirectory of a repository prints nothing" "" "" "$repo/sub"

echo "$passes passed, $fails failed"
[ "$fails" -eq 0 ]
