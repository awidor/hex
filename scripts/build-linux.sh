#!/bin/sh
# Build the release executable for this machine and copy it to
# dist/hex-<version>-linux-<x64|arm64>, named for the version the build
# reports. HEX_RELEASE_DIST names another folder. Prints only the
# executable's path.
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
dist=${HEX_RELEASE_DIST:-"$root/dist"}
target_dir=${CARGO_TARGET_DIR:-"$root/target"}

case "$(uname -s) $(uname -m)" in
  "Linux x86_64") label=x64 ;;
  "Linux aarch64") label=arm64 ;;
  *)
    echo "Hex builds for Linux on x86_64 or aarch64 Linux." >&2
    exit 1
    ;;
esac

# Only the executable's path goes to stdout. Cargo reads .cargo/config.toml
# from the working directory, not from --manifest-path.
(cd "$root" && cargo build --locked --release) >&2
version=$("$target_dir/release/hex" --version | awk 'NF { value=$NF } END { print value }')
test -n "$version"
mkdir -p "$dist"
executable="$dist/hex-$version-linux-$label"
install -m755 "$target_dir/release/hex" "$executable"
echo "$executable"
