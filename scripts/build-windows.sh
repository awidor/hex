#!/bin/sh
# Build Hex for Windows from Git Bash on Windows and copy it to
# dist/hex-<version>-windows-<x64|arm64>.exe (the GUI app) and .com
# (crates/console, the console entry point that belongs beside it as
# hex.com). HEX_WINDOWS_ARCH=x86_64 (default) or aarch64; HEX_RELEASE_DIST
# names another folder. Prints only the executable's path.
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
dist=${HEX_RELEASE_DIST:-"$root/dist"}
target_dir=${CARGO_TARGET_DIR:-"$root/target"}
arch=${HEX_WINDOWS_ARCH:-x86_64}
case "$arch" in
  x86_64) label=x64 ;;
  aarch64) label=arm64 ;;
  *)
    echo "HEX_WINDOWS_ARCH must be x86_64 or aarch64." >&2
    exit 1
    ;;
esac
triple="$arch-pc-windows-msvc"

case "$(uname -s)" in
  MINGW64* | MSYS*) ;;
  *)
    echo "Windows builds run from Git Bash on Windows." >&2
    exit 1
    ;;
esac

# Cargo reads .cargo/config.toml (the static Windows runtime) from the
# working directory, not from --manifest-path.
(cd "$root" && cargo build --locked --release --target "$triple") >&2
(cd "$root" && cargo build --locked --release --target "$triple" -p hex-console) >&2
built="$target_dir/$triple/release/hex.exe"
built_console="$target_dir/$triple/release/hex-console.exe"
test "$(head -c 2 "$built")" = MZ
test "$(head -c 2 "$built_console")" = MZ

if [ "$(uname -m)" = "$arch" ]; then
  # Named for the version the build reports. hex.com runs the hex.exe beside
  # it, as installed.
  pair=$(mktemp -d)
  cp "$built" "$pair/hex.exe"
  cp "$built_console" "$pair/hex.com"
  version=$("$pair/hex.com" --version | awk 'NF { value=$NF } END { print value }')
  rm -rf "$pair"
else
  # An executable for the other architecture cannot run here, so it is named
  # for HEX_VERSION (which the build also reports) or the crate version.
  version=${HEX_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)}
fi
test -n "$version"

mkdir -p "$dist"
executable="$dist/hex-$version-windows-$label.exe"
cp "$built" "$executable"
cp "$built_console" "$dist/hex-$version-windows-$label.com"
echo "$executable"
