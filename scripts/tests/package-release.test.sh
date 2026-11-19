#!/usr/bin/env bash
# Tests scripts/package-release.sh with a fake binary and a fake dist-web directory.
# Run: bash scripts/tests/package-release.test.sh   (local gate; CI runs every scripts/tests/*.test.sh)
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$HERE/../package-release.sh"
TAG="v4.0.35-web.1"
NAME="cc-switch-web-4.0.35-web.1-linux-x86_64"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fails=0
passes=0

check() { # check <name> <got> <want>
  if [ "$2" = "$3" ]; then
    echo "ok   $1"
    passes=$((passes + 1))
  else
    echo "FAIL $1: got '$2', want '$3'"
    fails=$((fails + 1))
  fi
}

check_has() { # check_has <name> <haystack> <needle>
  if [[ "$2" == *"$3"* ]]; then
    echo "ok   $1"
    passes=$((passes + 1))
  else
    echo "FAIL $1: output lacks '$3'"
    fails=$((fails + 1))
  fi
}

check_lacks() { # check_lacks <name> <haystack> <needle>
  if [[ "$2" != *"$3"* ]]; then
    echo "ok   $1"
    passes=$((passes + 1))
  else
    echo "FAIL $1: output has '$3'"
    fails=$((fails + 1))
  fi
}

printf '#!/bin/sh\necho fake\n' >"$TMP/bin"
chmod 644 "$TMP/bin" # the script must set 0755 itself
mkdir -p "$TMP/dist/assets"
echo '<div id="root"></div>' >"$TMP/dist/index.html"
echo 'console.log(1)' >"$TMP/dist/assets/app.js"

OUT="$TMP/out"
bash "$SCRIPT" "$TAG" x86_64 "$TMP/bin" "$TMP/dist" "$OUT" >"$TMP/pack.log" 2>&1
check "packaging exits 0" "$?" "0"

# 1. layout
listing="$(tar -tzf "$OUT/$NAME.tar.gz" | sed 's:/$::' | sort | tr '\n' ' ')"
want="$(printf '%s\n' \
  "$NAME" "$NAME/LICENSE" "$NAME/cc-switch-web" "$NAME/dist-web" "$NAME/dist-web/assets" \
  "$NAME/dist-web/assets/app.js" "$NAME/dist-web/index.html" "$NAME/install.sh" \
  "$NAME/self-update.sh" | sort | tr '\n' ' ')"
check "the archive has exactly the release layout" "$listing" "$want"
mkdir "$TMP/x"
tar -xzf "$OUT/$NAME.tar.gz" -C "$TMP/x"
check "the binary in the archive is 0755" "$(stat -c %a "$TMP/x/$NAME/cc-switch-web")" "755"
check "install.sh and self-update.sh in the archive are 0755" \
  "$(stat -c %a "$TMP/x/$NAME/install.sh") $(stat -c %a "$TMP/x/$NAME/self-update.sh")" "755 755"
check "the archive carries the repository LICENSE" \
  "$(cmp -s "$TMP/x/$NAME/LICENSE" "$HERE/../../LICENSE" && echo same || echo differs)" "same"
check "the archive carries the real self-update.sh" \
  "$(cmp -s "$TMP/x/$NAME/self-update.sh" "$HERE/../self-update.sh" && echo same || echo differs)" "same"

# 2. baked installer
inst="$(cat "$TMP/x/$NAME/install.sh")"
check_has "the installer inside the archive has the tag baked in" "$inst" "RELEASE_TAG=\"$TAG\""
check_lacks "the installer inside the archive has no placeholder left" "$inst" "__RELEASE_TAG__"
check "the baked installer still parses" "$(bash -n "$TMP/x/$NAME/install.sh" 2>&1 && echo ok)" "ok"

# 3. --installer
bash "$SCRIPT" --installer "$TAG" "$TMP/install.sh" >"$TMP/inst.log" 2>&1
check "--installer exits 0" "$?" "0"
check "--installer writes the same baked copy" \
  "$(cmp -s "$TMP/install.sh" "$TMP/x/$NAME/install.sh" && echo same || echo differs)" "same"
check "--installer output is executable" "$(stat -c %a "$TMP/install.sh")" "755"

# 4. refusals
bash "$SCRIPT" v4.0.35 x86_64 "$TMP/bin" "$TMP/dist" "$TMP/out2" >"$TMP/bad.log" 2>&1
rc=$?
check "a tag that is not a fork version is refused (exit code)" "$([ "$rc" != 0 ] && echo nonzero || echo zero)" "nonzero"
check "a refused tag writes no archive" "$(find "$TMP/out2" -type f 2>/dev/null | wc -l | tr -d ' ')" "0"
check_has "a refused tag names the reason" "$(cat "$TMP/bad.log")" "not a cc-switch-web release tag"
bash "$SCRIPT" --installer v4.0.35 "$TMP/bad-install.sh" >/dev/null 2>&1
rc=$?
check "--installer refuses a tag that is not a fork version" "$([ "$rc" != 0 ] && echo nonzero || echo zero)" "nonzero"
check "--installer writes nothing for a refused tag" "$([ -e "$TMP/bad-install.sh" ] && echo exists || echo none)" "none"
bash "$SCRIPT" "$TAG" armv7 "$TMP/bin" "$TMP/dist" "$TMP/out3" >/dev/null 2>&1
rc=$?
check "an unknown architecture is refused" "$([ "$rc" != 0 ] && echo nonzero || echo zero)" "nonzero"
bash "$SCRIPT" "$TAG" x86_64 "$TMP/missing" "$TMP/dist" "$TMP/out4" >/dev/null 2>&1
rc=$?
check "a missing binary is refused" "$([ "$rc" != 0 ] && echo nonzero || echo zero)" "nonzero"

echo
echo "$passes passed, $fails failed"
[ "$fails" = 0 ]
