#!/usr/bin/env bash
# Runs every scripts/tests/*.test.sh, prints each suite, and exits 1 if any suite fails.
# Used by .github/workflows/scripts.yml and the `scripts` job of release-web.yml.
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
shopt -s nullglob
tests=("$HERE"/*.test.sh)
if [ "${#tests[@]}" -eq 0 ]; then
  echo "no scripts/tests/*.test.sh found" >&2
  exit 1
fi
failed=()
for t in "${tests[@]}"; do
  echo "::group::${t#"$HERE"/}"
  if ! bash "$t"; then
    failed+=("${t#"$HERE"/}")
  fi
  echo "::endgroup::"
done
if [ "${#failed[@]}" -gt 0 ]; then
  echo "failed: ${failed[*]}" >&2
  exit 1
fi
echo "all ${#tests[@]} suites passed"
