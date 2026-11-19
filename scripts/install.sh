#!/usr/bin/env bash
# cc-switch-web installer: downloads a release, verifies it, installs it under $HOME and
# runs it as a systemd user service.
#   curl -fsSL https://github.com/alxwlw/cc-switch-web/releases/latest/download/install.sh | bash
#   ... | bash -s -- --tailscale --port 18800
#
# Usage: install.sh [--tailscale | --loopback] [--port N] [--self-update | --no-self-update]
#                   [--version vX.Y.Z-web.N] [--uninstall]
#   --tailscale   listen on this machine's tailnet address (sets TRUSTED_TAILNET=1)
#   --loopback    listen on 127.0.0.1 only (the default on a first install)
# A re-run without flags keeps the address, port and self-update setting of the installed unit.
#
# Env: CC_SWITCH_WEB_REPO (alxwlw/cc-switch-web)  CC_SWITCH_WEB_UPDATE_API (https://api.github.com)
#      CC_SWITCH_WEB_DOWNLOAD_BASE (https://github.com/$REPO/releases/download)
#      CC_SWITCH_WEB_HEALTH_TIMEOUT (30 seconds)

# The release workflow replaces the placeholder with the tag this file was published with.
RELEASE_TAG="__RELEASE_TAG__"

TAG_RE='^v[0-9]+\.[0-9]+\.[0-9]+-web\.[0-9]+$'
REPO_RE='^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$'
APT_PACKAGES="libssl3 libgtk-3-0 libayatana-appindicator3-1 librsvg2-2 libwebkit2gtk-4.1-0"

die() {
  echo "install.sh: $*" >&2
  exit 1
}

# Prints x86_64 or aarch64; anything else is an error.
detect_arch() {
  local m
  m="$(uname -m)"
  case "$m" in
    x86_64 | amd64) echo x86_64 ;;
    aarch64 | arm64) echo aarch64 ;;
    *)
      echo "install.sh: unsupported architecture $m (only x86_64 and aarch64 have release archives); use the Docker image instead: docker pull ghcr.io/alxwlw/cc-switch-web" >&2
      return 1
      ;;
  esac
}

# Prints the ldd lines that say "not found" for <binary>; an ldd that fails prints nothing.
missing_libs() {
  local out
  out="$(ldd "$1" 2>/dev/null)" || return 0
  printf '%s\n' "$out" | grep 'not found' || true
}

# Prints the value of Environment=<KEY>=... from a unit file (first match), or nothing.
read_unit_env() {
  sed -n "s/^Environment=$2=//;T;p;q" "$1" 2>/dev/null
}

# render_unit <host> <port> <trusted 0|1> <self_update 0|1> <repo>
render_unit() {
  local host="$1" port="$2" trusted="$3" self_update="$4" repo="$5"
  cat <<UNIT
[Unit]
Description=cc-switch-web console (no in-app login; listens on HOST only)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=%h/.local/bin/cc-switch-web
Environment=HOST=$host
Environment=PORT=$port
UNIT
  if [ "$trusted" = 1 ]; then
    echo "Environment=TRUSTED_TAILNET=1"
  fi
  cat <<UNIT
Environment=CC_SWITCH_CONFIG_DIR=%h/.cc-switch-web
Environment=WEB_DIST=%h/.local/share/cc-switch-web/dist-web
Environment=CC_SWITCH_WEB_INSTALL_DIR=%h/.local/share/cc-switch-web
Environment=CC_SWITCH_WEB_UPDATE_REPO=$repo
Environment=CC_SWITCH_WEB_SELF_UPDATE=$self_update
Environment=RUST_LOG=info
Restart=always
RestartSec=3

[Install]
WantedBy=default.target
UNIT
}

# `source install.sh` only defines the functions above. Piped into bash (curl | bash)
# BASH_SOURCE is empty and the script must run.
if [ -n "${BASH_SOURCE[0]:-}" ] && [ "${BASH_SOURCE[0]}" != "$0" ]; then
  return 0
fi

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: install.sh [--tailscale | --loopback] [--port N] [--self-update | --no-self-update]
                  [--version vX.Y.Z-web.N] [--uninstall]
  --tailscale        listen on this machine's tailnet address (sets TRUSTED_TAILNET=1)
  --loopback         listen on 127.0.0.1 only (the default on a first install)
  --port N           port to listen on (default 18766)
  --self-update      let the console update this install from a release (default)
  --no-self-update   turn that off
  --version TAG      install this release (vX.Y.Z-web.N) instead of the latest
  --uninstall        remove the install; the data in ~/.cc-switch-web is kept
A re-run without flags keeps the address, port and self-update setting of the installed unit.

Environment:
  CC_SWITCH_WEB_REPO              owner/name of the repository (alxwlw/cc-switch-web)
  CC_SWITCH_WEB_UPDATE_API        API base for the latest release (https://api.github.com)
  CC_SWITCH_WEB_DOWNLOAD_BASE     where release files are downloaded from
                                  (https://github.com/$REPO/releases/download)
  CC_SWITCH_WEB_HEALTH_TIMEOUT    seconds to wait for the service to come up (30)
USAGE
}

has_systemd() {
  command -v systemctl >/dev/null 2>&1 && systemctl --user show-environment >/dev/null 2>&1
}

uninstall() {
  local unit="$HOME/.config/systemd/user/cc-switch-web.service"
  if command -v systemctl >/dev/null 2>&1; then
    systemctl --user disable --now cc-switch-web 2>/dev/null || true
  fi
  rm -f "$unit" "$HOME/.local/bin/cc-switch-web"
  rm -rf "$HOME/.local/share/cc-switch-web"
  if command -v systemctl >/dev/null 2>&1; then
    systemctl --user daemon-reload 2>/dev/null || true
  fi
  echo "cc-switch-web removed. Your data is kept in ~/.cc-switch-web (delete it by hand if you do not need it)."
}

main() {
  local mode="" port_flag="" self_flag="" version_flag="" uninstall=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --tailscale | --loopback)
        if [ -n "$mode" ] && [ "$mode" != "${1#--}" ]; then die "--tailscale and --loopback exclude each other"; fi
        mode="${1#--}"
        ;;
      --port)
        [ $# -ge 2 ] || die "--port needs a number"
        port_flag="$2"
        shift
        ;;
      --self-update) self_flag=1 ;;
      --no-self-update) self_flag=0 ;;
      --version)
        [ $# -ge 2 ] || die "--version needs a tag like v4.0.35-web.1"
        version_flag="$2"
        shift
        ;;
      --uninstall) uninstall=1 ;;
      -h | --help)
        usage
        return 0
        ;;
      *) die "unknown argument: $1 (see --help)" ;;
    esac
    shift
  done

  if [ "$uninstall" = 1 ]; then
    uninstall
    return 0
  fi

  if [ -n "$port_flag" ]; then
    [[ "$port_flag" =~ ^[0-9]+$ ]] && [ "$port_flag" -ge 1 ] && [ "$port_flag" -le 65535 ] || die "--port must be a number from 1 to 65535"
  fi
  if [ -n "$version_flag" ]; then
    [[ "$version_flag" =~ $TAG_RE ]] || die "--version must look like v4.0.35-web.1, got: $version_flag"
  fi

  # 1. tools
  local t missing_tools=""
  for t in curl tar sha256sum; do
    command -v "$t" >/dev/null 2>&1 || missing_tools="$missing_tools $t"
  done
  [ -z "$missing_tools" ] || die "missing required tools:$missing_tools"
  local uid
  uid="$(id -u)"
  if [ "$uid" = 0 ]; then
    echo "install.sh: warning: running as root; agent configs will be read from root's home directory ($HOME)" >&2
  fi

  # settings (step 5, read-only): the installed unit, then flags
  local unit="$HOME/.config/systemd/user/cc-switch-web.service"
  local host=127.0.0.1 port=18766 trusted=0 self_update=1 repo="${CC_SWITCH_WEB_REPO:-}" v
  if [ -f "$unit" ]; then
    v="$(read_unit_env "$unit" HOST)"
    if [ -n "$v" ]; then host="$v"; fi
    v="$(read_unit_env "$unit" PORT)"
    if [ -n "$v" ]; then port="$v"; fi
    v="$(read_unit_env "$unit" TRUSTED_TAILNET)"
    if [ "$v" = 1 ]; then trusted=1; fi
    v="$(read_unit_env "$unit" CC_SWITCH_WEB_SELF_UPDATE)"
    if [ -n "$v" ]; then self_update="$v"; fi
    if [ -z "$repo" ]; then repo="$(read_unit_env "$unit" CC_SWITCH_WEB_UPDATE_REPO)"; fi
  fi
  if [ -z "$repo" ]; then repo="alxwlw/cc-switch-web"; fi
  [[ "$repo" =~ $REPO_RE ]] || die "CC_SWITCH_WEB_REPO is not a usable owner/name: $repo"
  if [ -n "$port_flag" ]; then port="$port_flag"; fi
  if [ -n "$self_flag" ]; then self_update="$self_flag"; fi
  if [ "$mode" = tailscale ]; then
    command -v tailscale >/dev/null 2>&1 || die "--tailscale needs the tailscale command; install Tailscale or use --loopback"
    host="$(tailscale ip -4)" || host=""
    host="${host%%$'\n'*}"
    [ -n "$host" ] || die "tailscale ip -4 printed no address; is Tailscale up?"
    trusted=1
  elif [ "$mode" = loopback ]; then
    host=127.0.0.1
    trusted=0
  fi

  # 2. tag, architecture, download
  local api="${CC_SWITCH_WEB_UPDATE_API:-https://api.github.com}"
  local base="${CC_SWITCH_WEB_DOWNLOAD_BASE:-https://github.com/$repo/releases/download}"
  local tag="$RELEASE_TAG"
  if [ -n "$version_flag" ]; then
    tag="$version_flag"
  elif ! [[ "$tag" =~ $TAG_RE ]]; then
    tag="$(curl -fsSL "$api/repos/$repo/releases/latest" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/;T;p;q')" || tag=""
    [ -n "$tag" ] || die "could not read the latest release of $repo from $api"
  fi
  [[ "$tag" =~ $TAG_RE ]] || die "the release tag '$tag' is not a cc-switch-web release (expected v<x.y.z>-web.<n>)"
  local version="${tag#v}" arch
  arch="$(detect_arch)" || exit 1
  local name="cc-switch-web-$version-linux-$arch"

  local tmp
  tmp="$(mktemp -d)"
  # shellcheck disable=SC2064 # $tmp is fixed now, on purpose
  trap "rm -rf '$tmp'" EXIT

  echo "Installing cc-switch-web $version ($arch)"
  curl -fsSL -o "$tmp/SHA256SUMS" "$base/$tag/SHA256SUMS" || die "could not download $base/$tag/SHA256SUMS"
  local want
  want="$(awk -v n="$name.tar.gz" '$2 == n || $2 == "*" n { print $1; exit }' "$tmp/SHA256SUMS")"
  [ -n "$want" ] || die "no archive for $arch in $tag; there are no release archives for this architecture, use the Docker image instead: docker pull ghcr.io/alxwlw/cc-switch-web"
  curl -fsSL -o "$tmp/$name.tar.gz" "$base/$tag/$name.tar.gz" || die "could not download $base/$tag/$name.tar.gz"
  local got
  got="$(sha256sum "$tmp/$name.tar.gz" | awk '{ print $1 }')"
  [ "$got" = "$want" ] || die "checksum mismatch for $name.tar.gz (expected $want, got $got); nothing was installed"

  # 3. unpack, check libraries
  local listing
  listing="$(tar -tzf "$tmp/$name.tar.gz")" || die "could not read $name.tar.gz; nothing was installed"
  if grep -qE '^/|(^|/)\.\.(/|$)' <<<"$listing"; then
    die "the archive has unsafe paths; nothing was installed"
  fi
  mkdir "$tmp/x"
  tar -xzf "$tmp/$name.tar.gz" -C "$tmp/x" || die "could not unpack $name.tar.gz; nothing was installed"
  local ex="$tmp/x/$name" f
  for f in cc-switch-web install.sh self-update.sh; do
    [ -f "$ex/$f" ] || die "the archive is incomplete (no $f); nothing was installed"
  done
  [ -d "$ex/dist-web" ] || die "the archive is incomplete (no dist-web); nothing was installed"

  # 4. system libraries
  local libs
  libs="$(missing_libs "$ex/cc-switch-web")"
  if [ -n "$libs" ]; then
    if ! command -v apt-get >/dev/null 2>&1; then
      die "system libraries are missing:
$libs
Install them with your package manager (Debian/Ubuntu names: $APT_PACKAGES) or use the Docker image: docker pull ghcr.io/alxwlw/cc-switch-web"
    fi
    local sudo_cmd=""
    if [ "$uid" != 0 ]; then
      command -v sudo >/dev/null 2>&1 || die "system libraries are missing and sudo is not available; run as root: apt-get update && apt-get install -y --no-install-recommends $APT_PACKAGES"
      sudo_cmd=sudo
    fi
    echo "Missing system libraries; installing with apt ($APT_PACKAGES), a few hundred MB:"
    printf '%s\n' "$libs"
    $sudo_cmd apt-get update
    # shellcheck disable=SC2086 # the package list is words on purpose
    $sudo_cmd apt-get install -y --no-install-recommends $APT_PACKAGES
    libs="$(missing_libs "$ex/cc-switch-web")"
    [ -z "$libs" ] || die "system libraries are still missing after apt:
$libs"
  fi

  # 6. layout: temporary name next to the target, then mv (never write over a running file)
  local bin_dir="$HOME/.local/bin" inst="$HOME/.local/share/cc-switch-web" data="$HOME/.cc-switch-web"
  mkdir -p "$bin_dir" "$inst"
  for f in install.sh self-update.sh; do
    cp "$ex/$f" "$inst/.$f.new"
    chmod 755 "$inst/.$f.new"
    mv -f "$inst/.$f.new" "$inst/$f"
  done
  rm -rf "$inst/dist-web.new" "$inst/dist-web.old"
  cp -R "$ex/dist-web" "$inst/dist-web.new"
  if [ -e "$inst/dist-web" ]; then mv "$inst/dist-web" "$inst/dist-web.old"; fi
  mv "$inst/dist-web.new" "$inst/dist-web"
  rm -rf "$inst/dist-web.old"
  cp "$ex/LICENSE" "$inst/LICENSE" 2>/dev/null || true
  cp "$ex/cc-switch-web" "$bin_dir/.cc-switch-web.new"
  chmod 755 "$bin_dir/.cc-switch-web.new"
  mv -f "$bin_dir/.cc-switch-web.new" "$bin_dir/cc-switch-web"
  if [ ! -d "$data" ]; then
    mkdir -p "$data"
    chmod 700 "$data"
  fi

  local me="${USER:-$(id -un)}"
  if ! has_systemd; then
    # 8 (no systemd user session): files are in place, nothing to enable
    local tn=""
    if [ "$trusted" = 1 ]; then tn="TRUSTED_TAILNET=1 "; fi
    echo "cc-switch-web $version is installed, but there is no systemd user session here (a container?)."
    echo "Run it in the foreground with:"
    echo "  HOST=$host PORT=$port ${tn}CC_SWITCH_CONFIG_DIR=~/.cc-switch-web WEB_DIST=~/.local/share/cc-switch-web/dist-web CC_SWITCH_WEB_INSTALL_DIR=~/.local/share/cc-switch-web CC_SWITCH_WEB_UPDATE_REPO=$repo CC_SWITCH_WEB_SELF_UPDATE=$self_update ~/.local/bin/cc-switch-web"
    echo "Data: ~/.cc-switch-web"
    return 0
  fi

  # 7 + 8. unit, enable, linger
  local udir="$HOME/.config/systemd/user"
  mkdir -p "$udir"
  render_unit "$host" "$port" "$trusted" "$self_update" "$repo" >"$udir/.cc-switch-web.service.new"
  mv -f "$udir/.cc-switch-web.service.new" "$unit"
  systemctl --user daemon-reload
  systemctl --user enable cc-switch-web
  systemctl --user restart cc-switch-web
  if command -v loginctl >/dev/null 2>&1; then
    local linger
    linger="$(loginctl show-user "$me" -p Linger 2>/dev/null)" || linger=""
    if [ "$linger" != "Linger=yes" ]; then
      loginctl enable-linger "$me" 2>/dev/null || echo "To keep the service running after you log out, run: sudo loginctl enable-linger $me"
    fi
  fi

  # 9. wait for the new version
  local probe_host="$host" timeout="${CC_SWITCH_WEB_HEALTH_TIMEOUT:-30}" waited=0 want_re ok=0 caps
  if [ "$probe_host" = 0.0.0.0 ]; then probe_host=127.0.0.1; fi
  want_re="\"webVersion\"[[:space:]]*:[[:space:]]*\"${version//./\\.}\""
  while :; do
    caps="$(curl -fsS --max-time 2 "http://$probe_host:$port/api/capabilities" 2>/dev/null)" || caps=""
    if grep -qE "$want_re" <<<"$caps"; then
      ok=1
      break
    fi
    [ "$waited" -lt "$timeout" ] || break
    sleep 1
    waited=$((waited + 1))
  done
  [ "$ok" = 1 ] || die "the service did not report version $version on http://$probe_host:$port within ${timeout}s; see: journalctl --user -u cc-switch-web -n 50"

  # 10. summary
  echo
  echo "cc-switch-web $version is running."
  if [[ "$host" == 127.* ]]; then
    echo "It listens on this machine only. From your computer:"
    echo "  ssh -L $port:localhost:$port $me@$(hostname -f 2>/dev/null || hostname 2>/dev/null || echo localhost)"
    echo "then open http://localhost:$port"
  else
    echo "Open http://$host:$port (reachable from your tailnet only; there is no in-app login)."
  fi
  echo "Data: ~/.cc-switch-web"
  echo "Remove: curl -fsSL https://github.com/$repo/releases/latest/download/install.sh | bash -s -- --uninstall"
}

main "$@" </dev/null
