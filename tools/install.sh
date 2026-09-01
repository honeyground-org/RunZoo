#!/bin/bash
# Install RunZoo from source on a Mac that has nothing installed yet.
#
#   ./tools/install.sh                 build, install to /Applications, launch
#   ./tools/install.sh --login-item    also start it at login
#
# From a bare machine, with no checkout:
#   curl -fsSL https://raw.githubusercontent.com/wangjacsi/RunZoo/main/tools/install.sh | bash
#
# It installs what is missing (Command Line Tools, Rust) and touches nothing
# else. If you would rather not build at all, download the .app from
# https://github.com/wangjacsi/RunZoo/releases instead.
set -euo pipefail

REPO="https://github.com/wangjacsi/RunZoo.git"
LOGIN_ITEM=0
[[ "${1:-}" == "--login-item" ]] && LOGIN_ITEM=1

say() { printf '\033[1m▸ %s\033[0m\n' "$*"; }
die() { printf '\033[31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

[[ "$(uname)" == "Darwin" ]] || die "RunZoo is macOS only."

# --- 1. Command Line Tools (the compiler and linker RunZoo builds against)
if ! xcode-select -p > /dev/null 2>&1; then
  say "installing Xcode Command Line Tools (a system dialog will open)"
  xcode-select --install > /dev/null 2>&1 || true
  until xcode-select -p > /dev/null 2>&1; do
    sleep 5
  done
fi

# --- 2. Rust
if ! command -v cargo > /dev/null 2>&1; then
  # shellcheck disable=SC1091
  [[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"
fi
if ! command -v cargo > /dev/null 2>&1; then
  say "installing Rust (rustup, ~1 minute)"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable > /dev/null
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi
say "rust $(rustc --version | cut -d' ' -f2)"

# --- 3. Sources: this checkout if we are in one, a fresh clone if we are not
SRC="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." 2>/dev/null && pwd || true)"
if [[ ! -f "$SRC/Cargo.toml" ]]; then
  SRC="${TMPDIR:-/tmp}/RunZoo-install"
  say "fetching source into $SRC"
  rm -rf "$SRC"
  git clone --depth 1 "$REPO" "$SRC" > /dev/null 2>&1 \
    || die "could not clone $REPO"
fi
cd "$SRC"

# --- 4. Build and install
say "building"
./tools/bundle.sh

say "installing to /Applications"
# Quit a running copy first, or the replaced bundle keeps running the old code.
osascript -e 'tell application "RunZoo" to quit' > /dev/null 2>&1 || true
pkill -x RunZoo > /dev/null 2>&1 || true
rm -rf /Applications/RunZoo.app
cp -R dist/RunZoo.app /Applications/

if [[ $LOGIN_ITEM == 1 ]]; then
  say "adding to login items"
  osascript > /dev/null <<'APPLESCRIPT'
tell application "System Events"
    if not (exists login item "RunZoo") then
        make login item at end with properties ¬
            {path:"/Applications/RunZoo.app", hidden:true, name:"RunZoo"}
    end if
end tell
APPLESCRIPT
fi

open /Applications/RunZoo.app
say "running. Look for the animal in your menu bar."
[[ $LOGIN_ITEM == 1 ]] || echo "  start it at login:  ./tools/install.sh --login-item"
