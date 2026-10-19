#!/usr/bin/env bash
# The full gate, on Windows, Linux and macOS. verify.ps1 is a shim that runs
# this file through Git for Windows' Bash. The window's acceptance differs
# by platform: the WebdriverIO suite on Windows (the fake machine, registry
# and all), e2e/linux/smoke.py on Linux (the real package managers, read
# only, in a throwaway home folder), and none on macOS, where WKWebView has
# no WebDriver for apps: there the window is checked by hand.
#
# Cheapest checks first, so a formatting slip fails in seconds rather than
# after a WebDriver run. Nothing here packages an installer: that is the
# release step (`--package`, gated on AGENT_RELEASE=1).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

package=0
[ "${1:-}" = "--package" ] && package=1

say() { printf '\n== %s ==\n' "$1"; }

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) os=windows ;;
  Darwin) os=macos ;;
  *) os=linux ;;
esac
echo "platform: $os"

say "rust fmt"
cargo fmt --all -- --check

say "clippy"
# The fake machine is compiled here so its code and the tests behind it are
# linted too; the release build never enables it (checked below).
cargo clippy --locked --workspace --all-targets --features wattclean/fake-system -- -D warnings

say "rust tests"
cargo test --locked --workspace --all-targets --features wattclean/fake-system

say "test-only code stays out of release builds"
grep -qE '^default = \[\]$' src-tauri/Cargo.toml || { echo "src-tauri default features must be empty"; exit 1; }
for engine in wcc-core wrc-core wu-core; do
  grep -qE '^fake = \[\]$' "crates/$engine/Cargo.toml" || { echo "$engine's fake feature must not be default"; exit 1; }
done
if grep -q 'fake-system' src-tauri/tauri.conf.json; then echo "tauri.conf.json must not enable fake-system"; exit 1; fi
if grep -q 'allow-close' src-tauri/capabilities/main.json; then echo "only the e2e build may close the window from the page"; exit 1; fi
echo "release features OK"

say "frontend types and build"
npm run --silent build

say "e2e types"
# The specs run through tsx, which strips types, so a wrong wdio call would
# otherwise surface only as a runtime failure after the debug build below.
npx --no-install tsc --noEmit -p e2e

say "frontend formatting"
npx --no-install prettier --check "ui/src/**/*.ts" "e2e/**/*.ts"

say "frontend assets"
dist="ui/dist"
index="$dist/index.html"
[ -f "$index" ] || { echo "no built frontend at $index"; exit 1; }
referenced=$(grep -oE '(src|href)="[^"]+"' "$index" | sed -E 's/.*="([^"]+)".*/\1/' | grep -E '^/?assets/' || true)
[ -n "$referenced" ] || { echo "index.html references no bundled assets"; exit 1; }
count=0
while IFS= read -r asset; do
  [ -n "$asset" ] || continue
  path="$dist/${asset#/}"
  [ -f "$path" ] || { echo "index.html references $asset, which is not on disk"; exit 1; }
  count=$((count + 1))
done <<< "$referenced"
echo "frontend assets OK ($count referenced, all present)"

say "frontend tests"
npm test --silent

say "third-party notices"
# The licences of every package built into the programs, shipped with them;
# regenerate after a dependency change.
node scripts/third-party-notices.mjs --check

say "version agreement"
cargo_version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
node scripts/check-version-agreement.mjs "$cargo_version"

say "file sizes"
# 400 lines is the cap for Rust and TypeScript sources. awk's NR counts an
# unterminated final line too, which `wc -l` does not.
hard=400
over=0
while IFS= read -r file; do
  case "$file" in
    ./target/*|./node_modules/*|./ui/dist/*|./.git/*|./src-tauri/gen/*) continue ;;
  esac
  lines=$(awk 'END { print NR }' "$file")
  # A file may stay larger when its first 30 lines say why (engineering skill).
  if [ "$lines" -gt "$hard" ] && ! head -30 "$file" | grep -q 'size-exception:'; then
    echo "  OVER CAP ($hard): $file — $lines lines"
    over=$((over + 1))
  fi
done < <(find . -type f \( -name '*.rs' -o -name '*.ts' \))
[ "$over" -eq 0 ] || { echo "Split these by responsibility before committing."; exit 1; }
echo "file sizes OK (none over $hard lines)"

if [ "$os" = "windows" ]; then
  say "debug application (fake machine, no bundle)"
  npx --no-install tauri build --debug --no-bundle --features fake-system --config e2e/tauri.conf.json

  say "native WebDriver acceptance"
  export PATH="$root/.webdriver:$PATH"
  npm run --silent e2e:run
else
  say "debug application (this machine, no bundle)"
  npx --no-install tauri build --debug --no-bundle
fi

if [ "$os" = "linux" ]; then
  say "Linux WebDriver acceptance"
  export PATH="$root/.webdriver:$PATH"
  if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
    python3 e2e/linux/smoke.py target/debug/wattclean
  else
    xvfb-run -a python3 e2e/linux/smoke.py target/debug/wattclean
  fi
fi

if [ "$os" = "macos" ]; then
  echo "macOS: no WebDriver for WKWebView; open target/debug/wattclean and check the window by hand."
fi

if [ "$package" = "1" ]; then
  [ "${AGENT_RELEASE:-}" = "1" ] || {
    echo "Packaging is a separate release/handoff step; set AGENT_RELEASE=1 after debug verification."
    exit 1
  }
  say "release packaging"
  npx --no-install tauri build
fi

printf '\nALL CHECKS PASSED\n'
