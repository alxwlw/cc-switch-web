#!/usr/bin/env bash
# Works out what a Release (web) run builds: prints `tag=`, `version=` and `publish=` lines.
# Run from the repository root by the `meta` job of .github/workflows/release-web.yml.
#
#   EVENT_NAME   github.event_name
#   REF_TYPE     github.ref_type (branch | tag)
#   REF_NAME     github.ref_name
#
# A tag run must be v<upstream version>-web.<n> (no leading zeros), the upstream version must
# be the one in src-tauri/Cargo.toml and CHANGELOG.md must have its section. Only a push of such
# a tag publishes; a manual run on a tag checks it the same way and never publishes. Any other
# ref builds as <cargo version>-web.0 and does not publish.
set -euo pipefail

cargo_version="$(sed -nE 's/^version *= *"([^"]+)".*/\1/p' src-tauri/Cargo.toml | head -n 1)"
[ -n "$cargo_version" ] || { echo "no version in src-tauri/Cargo.toml" >&2; exit 1; }

publish=false
if [ "${REF_TYPE:-}" = tag ]; then
  tag="${REF_NAME:-}"
  num='(0|[1-9][0-9]*)'
  if ! [[ "$tag" =~ ^v$num\.$num\.$num-web\.$num$ ]]; then
    if [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+-web\.[0-9]+$ ]]; then
      echo "the tag '$tag' has a number with a leading zero; write v<upstream version>-web.<n> without leading zeros, for example v$cargo_version-web.1" >&2
    else
      echo "the tag '$tag' is not a release tag: expected v<upstream version>-web.<n>, for example v$cargo_version-web.1" >&2
    fi
    exit 1
  fi
  version="${tag#v}"
  upstream="${version%-web.*}"
  if [ "$upstream" != "$cargo_version" ]; then
    echo "the tag '$tag' is for upstream $upstream but src-tauri/Cargo.toml says $cargo_version" >&2
    exit 1
  fi
  if ! grep -qE "^## v${version//./\\.}([^0-9]|\$)" CHANGELOG.md; then
    echo "CHANGELOG.md has no line starting with '## v$version'; add the release section before tagging" >&2
    exit 1
  fi
  if [ "${EVENT_NAME:-}" = push ]; then publish=true; fi
else
  version="$cargo_version-web.0"
  tag="v$version"
fi
echo "tag=$tag"
echo "version=$version"
echo "publish=$publish"
