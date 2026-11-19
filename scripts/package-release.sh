#!/usr/bin/env bash
# Builds the files of a cc-switch-web release.
#   package-release.sh <tag> <arch> <binary> <dist-web-dir> <out-dir>
#       -> <out-dir>/cc-switch-web-<version>-linux-<arch>.tar.gz
#   package-release.sh --installer <tag> <out-file>
#       -> a copy of scripts/install.sh with <tag> baked in (the release asset install.sh)
# <tag> is v<x.y.z>-web.<n>; <version> is the tag without the "v"; <arch> is x86_64 or aarch64.
# The archive holds one top directory cc-switch-web-<version>-linux-<arch>/ with cc-switch-web,
# dist-web/, install.sh (tag baked in), self-update.sh and LICENSE: the layout install.sh unpacks.
set -euo pipefail

TAG_RE='^v[0-9]+\.[0-9]+\.[0-9]+-web\.[0-9]+$'
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"

die() {
  echo "package-release.sh: $*" >&2
  exit 1
}

usage() {
  echo "usage: package-release.sh <tag> <arch> <binary> <dist-web-dir> <out-dir>" >&2
  echo "       package-release.sh --installer <tag> <out-file>" >&2
  exit 2
}

check_tag() {
  [[ "$1" =~ $TAG_RE ]] || die "'$1' is not a cc-switch-web release tag (expected v<x.y.z>-web.<n>)"
}

# bake_installer <tag> <out-file>: scripts/install.sh with the placeholder replaced by <tag>.
bake_installer() {
  local tag="$1" out="$2" src="$HERE/install.sh" n
  n="$(grep -c '^RELEASE_TAG="__RELEASE_TAG__"$' "$src" || true)"
  [ "$n" = 1 ] || die "$src must hold exactly one RELEASE_TAG=\"__RELEASE_TAG__\" line, found $n"
  sed "s/^RELEASE_TAG=\"__RELEASE_TAG__\"\$/RELEASE_TAG=\"$tag\"/" "$src" >"$out"
  chmod 755 "$out"
  grep -q "^RELEASE_TAG=\"$tag\"\$" "$out" || die "baking the tag into $out failed"
  if grep -q '__RELEASE_TAG__' "$out"; then
    # the placeholder is also named in a comment or a check: that is a bug in install.sh
    die "$out still mentions __RELEASE_TAG__ after baking"
  fi
}

if [ "${1:-}" = "--installer" ]; then
  [ $# -eq 3 ] || usage
  check_tag "$2"
  bake_installer "$2" "$3"
  exit 0
fi

[ $# -eq 5 ] || usage
tag="$1" arch="$2" binary="$3" dist="$4" out="$5"
check_tag "$tag"
case "$arch" in
  x86_64 | aarch64) ;;
  *) die "unsupported architecture '$arch' (x86_64 or aarch64)" ;;
esac
[ -f "$binary" ] || die "binary not found: $binary"
[ -d "$dist" ] || die "dist-web directory not found: $dist"
[ -f "$dist/index.html" ] || die "$dist has no index.html: is it the built console?"
[ -f "$ROOT/LICENSE" ] || die "no LICENSE at the repository root"
[ -f "$HERE/self-update.sh" ] || die "no scripts/self-update.sh"

version="${tag#v}"
name="cc-switch-web-$version-linux-$arch"
mkdir -p "$out"

work="$(mktemp -d)"
# shellcheck disable=SC2064 # $work is fixed now, on purpose
trap "rm -rf '$work'" EXIT
stage="$work/$name"
mkdir -p "$stage"
cp "$binary" "$stage/cc-switch-web"
chmod 755 "$stage/cc-switch-web"
cp -R "$dist" "$stage/dist-web"
bake_installer "$tag" "$stage/install.sh"
cp "$HERE/self-update.sh" "$stage/self-update.sh"
chmod 755 "$stage/self-update.sh"
cp "$ROOT/LICENSE" "$stage/LICENSE"
chmod 644 "$stage/LICENSE"

tar --owner=0 --group=0 --numeric-owner --sort=name -czf "$out/$name.tar.gz" -C "$work" "$name"
echo "$out/$name.tar.gz"
