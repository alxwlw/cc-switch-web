#!/usr/bin/env bash
# Prepares a merge of upstream into the fork: fetches upstream, fast-forwards `upstream-main`,
# starts sync/upstream-<sha> from main, merges without committing and puts the fork-owned paths
# (docs/upstream.lock, `fork_owned`) back to the fork's version. Nothing is committed.
# Usage: scripts/sync-upstream.sh [--ref <upstream-ref>]   (default upstream/main)
# Exit: 0 merge prepared without conflicts, 2 conflicts left in code, 1 refused or failed.
# Sourced: defines fork_owned_patterns and is_fork_owned only.

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

upstream_lock_path() {
  local top
  if top="$(git rev-parse --show-toplevel 2>/dev/null)" && [ -f "$top/docs/upstream.lock" ]; then
    echo "$top/docs/upstream.lock"
  else
    echo "$HERE/../docs/upstream.lock"
  fi
}

# Prints the patterns of the `fork_owned = [ ... ]` array of docs/upstream.lock, one per line.
fork_owned_patterns() {
  sed -n '/^fork_owned[[:space:]]*=[[:space:]]*\[/,/^\]/p' "$(upstream_lock_path)" |
    sed -n 's/^[[:space:]]*"\([^"]*\)".*/\1/p'
}

is_fork_owned() { # is_fork_owned <path>
  local pattern
  while IFS= read -r pattern; do
    # shellcheck disable=SC2053 # the pattern is a glob on purpose
    [[ $1 == $pattern ]] && return 0
  done < <(fork_owned_patterns)
  return 1
}

sync_upstream_main() {
  local ref="upstream/main" sha branch gitdir
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --ref)
        [ "$#" -ge 2 ] || { echo "sync-upstream: --ref needs a value" >&2; return 1; }
        ref="$2"
        shift 2
        ;;
      *) echo "sync-upstream: unknown argument $1" >&2; return 1 ;;
    esac
  done

  # 1. refuse a dirty tree or a merge/rebase in progress
  gitdir="$(git rev-parse --git-dir)" || return 1
  local f
  for f in MERGE_HEAD CHERRY_PICK_HEAD REVERT_HEAD rebase-merge rebase-apply; do
    if [ -e "$gitdir/$f" ]; then
      echo "sync-upstream: refused, $f in progress" >&2
      return 1
    fi
  done
  if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
    echo "sync-upstream: refused, tracked files have changes" >&2
    return 1
  fi

  # 2. fetch, then move upstream-main fast-forward only
  git fetch upstream || { echo "sync-upstream: git fetch upstream failed" >&2; return 1; }
  sha="$(git rev-parse --verify --quiet "$ref^{commit}")" || {
    echo "sync-upstream: refused, $ref does not resolve to a commit" >&2
    return 1
  }
  if git rev-parse --verify --quiet refs/heads/upstream-main >/dev/null; then
    if ! git merge-base --is-ancestor refs/heads/upstream-main "$sha"; then
      echo "sync-upstream: refused, upstream-main is not an ancestor of $ref (fast-forward only)" >&2
      return 1
    fi
    git update-ref refs/heads/upstream-main "$sha" || return 1
  else
    git update-ref refs/heads/upstream-main "$sha" || return 1
    echo "created upstream-main at ${sha:0:7}"
  fi
  if git merge-base --is-ancestor "$sha" main; then
    echo "sync-upstream: main already contains $ref, nothing to merge"
    return 0
  fi

  # 3. branch from main, merge without committing (conflicts are expected)
  branch="sync/upstream-${sha:0:7}"
  if git rev-parse --verify --quiet "refs/heads/$branch" >/dev/null; then
    echo "sync-upstream: refused, branch $branch exists" >&2
    return 1
  fi
  git switch -q -c "$branch" main || return 1
  git merge --no-ff --no-commit upstream-main >/dev/null
  if [ ! -e "$gitdir/MERGE_HEAD" ]; then
    echo "sync-upstream: git merge did not start" >&2
    return 1
  fi

  # 4. fork-owned paths from HEAD, MERGE_HEAD and the index go back to the fork's version
  local path base
  while IFS= read -r -d '' path; do
    is_fork_owned "$path" || continue
    if git cat-file -e "HEAD:$path" 2>/dev/null; then
      git checkout -q HEAD -- "$path" || return 1
    else
      git rm -q -f --cached -- "$path" 2>/dev/null
      rm -f -- "$path"
    fi
  done < <({ git ls-tree -r -z --name-only HEAD; git ls-tree -r -z --name-only MERGE_HEAD; git ls-files -z; } | sort -zu)

  # 5. report
  base="$(git merge-base HEAD MERGE_HEAD)"
  local changed=0
  while IFS= read -r -d '' path; do
    is_fork_owned "$path" || continue
    echo "kept fork version (upstream changed): $path"
    changed=1
  done < <(git diff -z --name-only "$base" MERGE_HEAD)
  [ "$changed" -eq 1 ] || echo "no fork-owned path changed upstream"
  local conflicts
  conflicts="$(git diff --name-only --diff-filter=U)"
  if [ -n "$conflicts" ]; then
    echo "conflicts left in code:"
    git diff --name-only --diff-filter=U | sed 's/^/  /'
    echo "resolve them, then review the kept lines above and run: git commit"
    return 2
  fi
  echo "merge prepared on $branch without conflicts; review the kept lines above, then run: git commit"
  return 0
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  sync_upstream_main "$@"
  exit $?
fi
