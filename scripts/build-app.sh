#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

# Prints only the selected identity, so the caller can capture it without
# mixing diagnostics into codesign arguments.
hex_codesign_identity() {
  team_id=$1
  identity=${HEX_CODESIGN_IDENTITY:-}
  if [ -z "$identity" ]; then
    identity=$(security find-identity -v -p codesigning | sed -n "s/.*\"\(Developer ID Application:.*($team_id)\)\"/\1/p" | head -1)
  fi
  if [ -z "$identity" ]; then
    echo "No Developer ID signing identity found for team $team_id. Set HEX_CODESIGN_IDENTITY." >&2
    return 1
  fi
  case "$identity" in
    "Developer ID Application:"*"($team_id)") ;;
    *)
      echo "Signing identity is not a Developer ID Application identity for team $team_id." >&2
      return 1
      ;;
  esac
  printf '%s\n' "$identity"
}

hex_version() {
  printf '%s\n' "${HEX_VERSION:-$(cargo metadata --no-deps --format-version 1 --manifest-path "$1/Cargo.toml" | jq -r '.packages[] | select(.name == "hex") | .version')}"
}

# CFBundleVersion: the run number of a dev build (0.0.0-dev-N), else from
# X.Y.Z (a prerelease shares its release's number).
hex_build_number() {
  if [ -n "${HEX_BUILD_NUMBER:-}" ]; then
    printf '%s\n' "$HEX_BUILD_NUMBER"
    return
  fi
  case "$1" in
    0.0.0-dev-[0-9]*) printf '%s\n' "${1#0.0.0-dev-}" ;;
    *) printf '%s\n' "${1%%-*}" | awk -F. '{ print ($1 * 10000) + ($2 * 100) + $3 }' ;;
  esac
}

# HEX_TEST_BUILD=1 builds the test app: ad-hoc signed, under its own name
# and bundle ID so it installs beside Hex and never touches its permissions;
# it never checks for updates.
# HEX_LOCAL_BUILD=1 marks a build made from a checkout for its owner's own
# machine (Developer ID signed as usual): it shows "Local build · updates off"
# with its commit and never checks for or offers updates.
# HEX_RELEASE_UNSIGNED=1 builds Hex (anomaly.hex) ad-hoc signed, for a
# machine that holds no Developer ID.
if [ "${HEX_RELEASE_UNSIGNED:-}" = 1 ]; then
  team_id=
  identity=-
  app_name=Hex
  bundle_id=anomaly.hex
elif [ "${HEX_TEST_BUILD:-}" = 1 ]; then
  team_id=
  identity=-
  app_name="Hex Test"
  bundle_id=anomaly.hex.test
else
  team_id=${HEX_TEAM_ID:?Set HEX_TEAM_ID to the Apple Developer signing team}
  identity=$(hex_codesign_identity "$team_id")
  app_name=Hex
  bundle_id=anomaly.hex
fi
# Every build carries the `capture` experiment (dormant until enabled), with
# Scrawl's `scrawl-helper` beside `hex`, where the engine looks for it.
# crates/capture/build.rs compiles the helper from crates/capture/helper into
# target/release.
icon_output="$root/target/AppIcon.assets"
icon_info="$root/target/AppIcon-info.plist"
version=$(hex_version "$root")
build_number=$(hex_build_number "$version")
bundle_name="$app_name.app"
executable_name=hex

bundle="$root/target/app/$bundle_name"
executable="$bundle/Contents/MacOS/$executable_name"

sign() {
  if [ "$identity" = - ]; then
    codesign --force --sign - "$@"
  else
    codesign --force --timestamp --options runtime --sign "$identity" "$@"
  fi
}

export MACOSX_DEPLOYMENT_TARGET=15.0
# Cargo may run through a wrapper that does not forward the environment, so
# build-time variables also travel as --config.
env_args=()
if [ -z "${HEX_COMMIT:-}" ] && { [ "${HEX_LOCAL_BUILD:-}" = 1 ] || [ "${HEX_TEST_BUILD:-}" = 1 ]; }; then
  HEX_COMMIT=$(git -C "$root" rev-parse --short=7 HEAD 2>/dev/null || true)
fi
for name in HEX_LOCAL_BUILD HEX_TEST_BUILD HEX_COMMIT HEX_BUILD_CHANNEL; do
  value=${!name:-}
  if [ -n "$value" ]; then
    case "$value" in
      *\'*) echo "$name must not contain a single quote" >&2; exit 1 ;;
    esac
    env_args+=(--config "env.$name='$value'")
  fi
done
# The helper's embedded Info.plist names Hex, not Scrawl.
SCRAWL_HELPER_PLIST="$root/packaging/macos/scrawl-helper-Info.plist" \
  cargo build --locked --release ${env_args[@]+"${env_args[@]}"} --manifest-path "$root/Cargo.toml" >&2
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources" "$bundle/Contents/Library"
cp "$root/target/release/hex" "$executable"
cp "$root/target/release/scrawl-helper" "$bundle/Contents/MacOS/scrawl-helper"
cp "$root/app/Info.plist" "$bundle/Contents/Info.plist"
cp "$root/LICENSE" "$root/THIRD_PARTY_NOTICES.md" "$bundle/Contents/Resources/"
HEX_VERSION="$version" HEX_BUILD_NUMBER="$build_number" HEX_TEAM_ID="$team_id" HEX_CODESIGN_IDENTITY="${identity#-}" HEX_SKIP_INSTALL_SCRIPT=1 \
  "$root/scripts/build-hex-microphone-driver.sh" "$bundle/Contents/Library" >&2
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $build_number" "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $bundle_id" "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleExecutable $executable_name" "$bundle/Contents/Info.plist"
plutil -replace CFBundleName -string "$app_name" "$bundle/Contents/Info.plist"
plutil -replace CFBundleDisplayName -string "$app_name" "$bundle/Contents/Info.plist"
# Links into the playground (src/rune_ui/story_link.rs); a test build's own
# scheme never opens the installed Hex.
url_scheme=hex
[ "${HEX_TEST_BUILD:-}" = 1 ] && url_scheme=hex-test
/usr/libexec/PlistBuddy -c "Set :CFBundleURLTypes:0:CFBundleURLName $bundle_id" "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleURLTypes:0:CFBundleURLSchemes:0 $url_scheme" "$bundle/Contents/Info.plist"
# The helper mirrors a cabled iPhone as a camera; macOS asks on Hex's behalf.
plutil -replace NSCameraUsageDescription -string "Hex mirrors and records a cabled iPhone's screen for a capture." "$bundle/Contents/Info.plist"
rm -rf "$icon_output"
mkdir -p "$icon_output"
xcrun actool "$root/app/AppIcon.icon" \
  --compile "$icon_output" \
  --platform macosx \
  --minimum-deployment-target 15.0 \
  --app-icon AppIcon \
  --standalone-icon-behavior all \
  --output-partial-info-plist "$icon_info" \
  --output-format human-readable-text \
  --warnings \
  --notices >&2
cp "$icon_output/AppIcon.icns" "$icon_output/Assets.car" "$bundle/Contents/Resources/"

sign "$bundle/Contents/Library/HexMicrophone.driver"
# Hex runs it only to decode the capture's movie, which needs no
# entitlement; anything it touches is attributed to Hex, its responsible
# process.
sign --identifier "$bundle_id.scrawl-helper" "$bundle/Contents/MacOS/scrawl-helper"
sign --entitlements "$root/packaging/macos/hex.entitlements" "$bundle"
codesign --verify --deep --strict --verbose=2 "$bundle"
echo "$bundle"
