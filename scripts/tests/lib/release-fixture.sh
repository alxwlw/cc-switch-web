#!/usr/bin/env bash
# Shared by the release-install tests: builds a fake GitHub release on disk.
# Source it, then:
#   make_release <root> <version> <arch>...   archives + SHA256SUMS in <root>/v<version>/
#   set_latest <root> <version>               <root>/api/repos/o/r/releases/latest
# Point the scripts at it with CC_SWITCH_WEB_REPO=o/r,
# CC_SWITCH_WEB_DOWNLOAD_BASE=file://<root> and CC_SWITCH_WEB_UPDATE_API=file://<root>/api.
# <version> has no leading "v" (4.0.35-web.1); the tag directory is v<version>.
# The "binary" is a shell script whose second line is "# fake cc-switch-web <version>".

make_release() {
  local root="$1" version="$2" arch name work dir
  shift 2
  dir="$root/v$version"
  mkdir -p "$dir"
  : >"$dir/SHA256SUMS"
  work="$(mktemp -d)"
  for arch in "$@"; do
    name="cc-switch-web-$version-linux-$arch"
    mkdir -p "$work/$name/dist-web"
    printf '#!/bin/sh\n# fake cc-switch-web %s\n# arch %s\n' "$version" "$arch" >"$work/$name/cc-switch-web"
    chmod +x "$work/$name/cc-switch-web"
    printf 'web-%s\n' "$version" >"$work/$name/dist-web/index.html"
    printf '#!/bin/sh\n# fake install.sh %s\n' "$version" >"$work/$name/install.sh"
    printf '#!/bin/sh\n# fake self-update.sh %s\n' "$version" >"$work/$name/self-update.sh"
    chmod +x "$work/$name/install.sh" "$work/$name/self-update.sh"
    printf 'license %s\n' "$version" >"$work/$name/LICENSE"
    tar -czf "$dir/$name.tar.gz" -C "$work" "$name"
    (cd "$dir" && sha256sum "$name.tar.gz") >>"$dir/SHA256SUMS"
  done
  rm -rf "$work"
}

set_latest() {
  local root="$1" version="$2"
  mkdir -p "$root/api/repos/o/r/releases"
  printf '{\n  "url": "x",\n  "tag_name": "v%s",\n  "draft": false\n}\n' "$version" >"$root/api/repos/o/r/releases/latest"
}
