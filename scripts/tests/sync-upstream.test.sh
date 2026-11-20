#!/usr/bin/env bash
# Tests scripts/sync-upstream.sh against temporary git repositories: a local "upstream" and a
# fork cloned from it (remote `upstream` is a path, no network). The fork changes fork-owned files
# and code; then upstream changes both. The real repo and its remotes are never touched.
# Run: bash scripts/tests/sync-upstream.test.sh
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$HERE/../sync-upstream.sh"
LOCK="$HERE/../../docs/upstream.lock"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t

fails=0
passes=0
case_bad=0

# One result per case: the checks below only report what is wrong; case_done counts the case.
check() { # check <what> <got> <want>
  if [ "$2" != "$3" ]; then
    echo "  wrong: $1: got '$2', want '$3'"
    case_bad=1
  fi
}

check_has() { # check_has <what> <haystack> <needle>
  if [[ "$2" != *"$3"* ]]; then
    echo "  wrong: $1: '$2' lacks '$3'"
    case_bad=1
  fi
}

case_done() { # case_done <name>
  if [ "$case_bad" -eq 0 ]; then
    echo "ok   $1"
    passes=$((passes + 1))
  else
    echo "FAIL $1"
    fails=$((fails + 1))
  fi
  case_bad=0
}

numbered() { # numbered <n>: lines "line 1" .. "line n"
  local i
  for ((i = 1; i <= $1; i++)); do echo "line $i"; done
}

# setup_case <name>: builds $TMP/<name>/{upstream,fork}. UP_LINE1=<text> makes upstream change
# line 1 of src/a.rs too (a real conflict with the fork's change).
setup_case() {
  local up="$TMP/$1/upstream" fork="$TMP/$1/fork"
  mkdir -p "$up/.github/workflows" "$up/src"
  git init -q -b main "$up"
  (
    cd "$up" || exit 1
    echo "upstream readme" >README.md
    echo "release v1" >.github/workflows/release.yml
    numbered 20 >src/a.rs
    git add -A && git commit -q -m base
  )
  git clone -q "$up" "$fork"
  git -C "$fork" remote rename origin upstream
  (
    cd "$fork" || exit 1
    printf 'fork readme\nsecond line\n' >README.md
    git rm -q .github/workflows/release.yml
    { echo "fork line 1"; numbered 20 | tail -n +2; } >src/a.rs
    mkdir -p docs && cp "$LOCK" docs/upstream.lock
    git add -A && git commit -q -m fork
  )
  (
    cd "$up" || exit 1
    echo "upstream readme v2" >README.md
    echo "release v2" >.github/workflows/release.yml
    echo "lisez-moi" >README_FR.md
    echo "new workflow" >.github/workflows/new.yml
    { [ -n "${UP_LINE1:-}" ] && echo "$UP_LINE1" || echo "line 1"; numbered 19 | tail -n +2; echo "upstream line 20"; } >src/a.rs
    git add -A && git commit -q -m upstream
  )
}

run_sync() { # run_sync <case> [args]: sets OUT and RC
  local c="$1"
  shift
  OUT="$(cd "$TMP/$c/fork" && bash "$SCRIPT" "$@" 2>&1)"
  RC=$?
}

present() { # present <case> <path>: "yes" when the file is in the tree or the index
  local f="$TMP/$1/fork"
  if [ -e "$f/$2" ] || [ -n "$(git -C "$f" ls-files -- "$2")" ]; then echo yes; else echo no; fi
}

# 1. fork-owned files keep the fork version, new upstream ones are dropped
setup_case c1
run_sync c1
check "fork-owned files keep the fork version and new upstream ones are dropped: exit code" "$RC" "0"
check "README.md is the fork's byte for byte" "$(cmp -s "$TMP/c1/fork/README.md" <(git -C "$TMP/c1/fork" show main:README.md) && echo same || echo differs)" "same"
check "release.yml is absent" "$(present c1 .github/workflows/release.yml)" "no"
check "README_FR.md is absent" "$(present c1 README_FR.md)" "no"
check "new.yml is absent" "$(present c1 .github/workflows/new.yml)" "no"
case_done "fork-owned files keep the fork version and new upstream ones are dropped"

# 2. code changes from both sides merge
a="$(cat "$TMP/c1/fork/src/a.rs")"
check_has "code changes from both sides merge: fork line" "$a" "fork line 1"
check_has "code changes from both sides merge: upstream line" "$a" "upstream line 20"
case_done "code changes from both sides merge"

# 4. the report names what upstream changed in fork-owned paths
check_has "report names README.md" "$OUT" "kept fork version (upstream changed): README.md"
check_has "report names release.yml" "$OUT" "kept fork version (upstream changed): .github/workflows/release.yml"
case_done "the report names what upstream changed in fork-owned paths"

# 3. a real code conflict is reported and the fork-owned paths are still restored
UP_LINE1="upstream line 1" setup_case c3
run_sync c3
check "a real code conflict: exit code" "$RC" "2"
check_has "a real code conflict: src/a.rs listed" "$OUT" "  src/a.rs"
check "a real code conflict: README.md is the fork's" "$(cat "$TMP/c3/fork/README.md")" "$(git -C "$TMP/c3/fork" show main:README.md)"
case_done "a real code conflict is reported and the fork-owned paths are still restored"

# 5. a dirty tree or a merge in progress is refused
setup_case c5
echo dirty >>"$TMP/c5/fork/README.md"
run_sync c5
check "a dirty tree is refused: exit code" "$RC" "1"
check "a dirty tree: no sync branch" "$(git -C "$TMP/c5/fork" branch --list 'sync/*')" ""
git -C "$TMP/c5/fork" checkout -q -- README.md
git -C "$TMP/c5/fork" rev-parse HEAD >"$TMP/c5/fork/.git/MERGE_HEAD"
run_sync c5
check "a merge in progress is refused: exit code" "$RC" "1"
check_has "a merge in progress is refused by the script itself" "$OUT" "MERGE_HEAD in progress"
check "a merge in progress: no sync branch" "$(git -C "$TMP/c5/fork" branch --list 'sync/*')" ""
case_done "a dirty tree or a merge in progress is refused"

# 6. upstream-main only moves fast-forward
setup_case c6
git -C "$TMP/c6/fork" update-ref refs/heads/upstream-main "$(git -C "$TMP/c6/fork" commit-tree "$(git -C "$TMP/c6/fork" mktree </dev/null)" -m unrelated)"
before="$(git -C "$TMP/c6/fork" rev-parse upstream-main)"
run_sync c6
check "upstream-main only moves fast-forward: exit code" "$RC" "1"
check "upstream-main is left where it was" "$(git -C "$TMP/c6/fork" rev-parse upstream-main)" "$before"
check "upstream-main refused: no sync branch" "$(git -C "$TMP/c6/fork" branch --list 'sync/*')" ""
case_done "upstream-main only moves fast-forward"

# 7. a missing upstream-main is created at the resolved ref and the script says so
setup_case c7
run_sync c7
check "a missing upstream-main is created at the ref" "$(git -C "$TMP/c7/fork" rev-parse upstream-main)" "$(git -C "$TMP/c7/fork" rev-parse upstream/main)"
check_has "a missing upstream-main: the script says it created it" "$OUT" "created upstream-main at"
case_done "a missing upstream-main is created at the ref and the script says so"

echo "$passes passed, $fails failed"
[ "$fails" -eq 0 ]
