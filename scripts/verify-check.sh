#!/usr/bin/env bash
# One check of one part of the repo: the commands CI runs, with CI's environment where a test depends on it.
# The agent harness calls it from .claude/verify.json; contributors can run it before a PR.
# Usage: scripts/verify-check.sh <web|backend|scripts> <typecheck|lint|format|test>
# A check that a part does not have prints "n/a" and passes.
set -euo pipefail
cd "$(dirname "$0")/.."

part=${1:?part: web, backend or scripts}
kind=${2:?kind: typecheck, lint, format or test}
manifest=src-tauri/Cargo.toml

na() { echo "$part has no $kind check: n/a"; }

# A git worktree builds into the main checkout's target directory: a fresh worktree would otherwise compile
# everything from scratch. In a plain clone this is the default directory anyway.
common_dir=$(git rev-parse --path-format=absolute --git-common-dir)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$(dirname "$common_dir")/src-tauri/target}

# CI runners have umask 022 and no ~/.bun/bin on PATH, and some upstream tests assume both: the golden file-mode
# snapshots (a failure there also poisons the shared test mutex for the tests after it) and the Bun search paths.
ci_env() {
  umask 022
  PATH=$(printf %s "$PATH" | tr : '\n' | grep -v '/\.bun/bin$' | paste -sd:)
  export PATH
}

case "$part:$kind" in
web:typecheck) ./node_modules/.bin/tsc --noEmit ;;
web:format) ./node_modules/.bin/prettier --check "src/**/*.{js,jsx,ts,tsx,css,json}" ;;
web:test) ./node_modules/.bin/vitest run ;;
web:lint) na ;;
backend:format) cargo fmt --check --manifest-path "$manifest" ;;
# tauri's build script needs the frontend dist directory to exist; CI creates the same placeholder
backend:lint)
  mkdir -p dist
  cargo clippy --manifest-path "$manifest" -- -D warnings
  ;;
backend:typecheck) na ;; # clippy compiles the crate
backend:test)
  mkdir -p dist
  ci_env
  # run it, not just find it: a version manager's shim exists even when no version is selected
  if cargo nextest --version >/dev/null 2>&1; then
    cargo nextest run --manifest-path "$manifest" --profile ci
  else
    cargo test --no-fail-fast --manifest-path "$manifest"
  fi
  ;;
scripts:test) bash scripts/tests/run-all.sh ;;
scripts:lint) shellcheck -x -P SCRIPTDIR scripts/*.sh scripts/tests/*.sh ;;
scripts:typecheck | scripts:format) na ;;
*)
  echo "verify-check: unknown part or kind: $part $kind" >&2
  exit 2
  ;;
esac
