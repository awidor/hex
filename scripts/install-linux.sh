#!/bin/sh
set -eu

# Builds Hex from this checkout and installs it for the current user:
#
#   <support>/versions/<version>/hex   the executable, beside a `source-build`
#                                      marker: Hex never replaces a build
#                                      from a checkout (src/updater/linux.rs)
#   <support>/current                  link to that version
#   <install dir>/hex                  link to current/hex
#   the launcher and autostart entries, and the hex.service user unit
#
#   scripts/install-linux.sh [install|uninstall]
#
# <support> is HEX_APPLICATION_SUPPORT_DIR (default $XDG_DATA_HOME/hex) and
# <install dir> is HEX_INSTALL_DIR (default ~/.local/bin). Run it again to
# install a newer build. Uninstalling keeps logs and settings.
root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
install_dir=${HEX_INSTALL_DIR:-"$HOME/.local/bin"}
data_home=${XDG_DATA_HOME:-$HOME/.local/share}
config_home=${XDG_CONFIG_HOME:-$HOME/.config}
support_dir=${HEX_APPLICATION_SUPPORT_DIR:-"$data_home/hex"}
applications="$data_home/applications"
autostart="$config_home/autostart"
units="$config_home/systemd/user"
unit="$units/hex.service"

require() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Hex installation requires $1." >&2
    exit 1
  fi
}

fail() {
  echo "$1" >&2
  exit 1
}

safe_user_directory() {
  path=$1
  if [ -L "$path" ]; then
    fail "Refusing directory symlink: $path"
  fi
  mkdir -p "$path"
  if [ "$(stat -c %u "$path")" != "$(id -u)" ]; then
    fail "Hex requires a user-owned directory: $path"
  fi
  if find "$path" -maxdepth 0 -perm /022 -print -quit | grep -q .; then
    fail "Hex refuses a group- or world-writable directory: $path"
  fi
}

managed_directory() {
  path=$1
  if [ -L "$path" ]; then
    fail "Refusing managed symlink: $path"
  fi
  mkdir -p "$path"
  if [ "$(stat -c %u "$path")" != "$(id -u)" ]; then
    fail "Hex requires a user-owned directory: $path"
  fi
  chmod 700 "$path"
}

# Hex's desktop entries: its own, or the autostart entry the desktop's
# Background portal writes for it.
hex_entry() {
  grep -Fxq 'X-Hex-Managed=true' "$1" || grep -Fxq 'X-XDP-Autostart=anomaly.hex' "$1"
}

uninstall() {
  if [ -f "$unit" ] && grep -Fxq '# X-Hex-Managed=true' "$unit"; then
    if command -v systemctl >/dev/null 2>&1; then
      active_unit=$(systemctl --user show hex.service --property=FragmentPath --value 2>/dev/null || true)
      if [ "$active_unit" = "$unit" ]; then
        systemctl --user disable --now hex.service || fail "Could not stop hex.service; refusing to uninstall a running service."
      fi
    fi
    rm -f "$unit"
    if command -v systemctl >/dev/null 2>&1; then
      systemctl --user daemon-reload >/dev/null 2>&1 || true
    fi
  fi
  if [ -L "$support_dir" ]; then
    fail "Refusing managed symlink: $support_dir"
  fi
  if [ -e "$support_dir" ]; then
    managed_directory "$support_dir"
    managed_directory "$support_dir/updates"
    exec 9> "$support_dir/updates/update.lock"
    if ! flock -n 9; then
      fail "A Hex install or update is already in progress."
    fi
  fi
  binary="$install_dir/hex"
  expected="$support_dir/current/hex"
  if [ -L "$binary" ] && [ "$(readlink "$binary")" = "$expected" ]; then
    rm -f "$binary"
  fi
  for desktop in \
    "$applications/anomaly.hex.desktop" \
    "$autostart/anomaly.hex.desktop" \
    "$applications/hex.desktop" \
    "$autostart/hex.desktop"
  do
    if [ -f "$desktop" ] && [ ! -L "$desktop" ] && hex_entry "$desktop"; then
      rm -f "$desktop"
    fi
  done
  # The icons Hex writes into the user's theme when no package ships them,
  # marked by a PNG text chunk.
  for icon in "$data_home"/icons/hicolor/*/apps/anomaly.hex.png; do
    if [ -f "$icon" ] && [ ! -L "$icon" ] && LC_ALL=C grep -Faq 'X-Hex-Managed=true' "$icon"; then
      rm -f "$icon"
    fi
  done
  if [ -d "$data_home/icons/hicolor" ] && [ ! -L "$data_home/icons/hicolor" ]; then
    touch "$data_home/icons/hicolor" 2>/dev/null || true
  fi
  rm -f "$support_dir/current" "$support_dir/.current.next" "$support_dir/install.json"
  rm -rf "$support_dir/versions"
  echo "Removed Hex. Logs and settings remain in $support_dir."
}

case "$(uname -s) $(uname -m)" in
  "Linux x86_64" | "Linux aarch64") ;;
  *)
    echo "Hex supports x86_64 and aarch64 Linux." >&2
    exit 1
    ;;
esac
if [ "$(id -u)" -eq 0 ]; then
  echo "Install Hex as your desktop user, not root." >&2
  exit 1
fi
umask 022
for command in find flock grep readlink stat; do
  require "$command"
done

case ${1:-install} in
  install) ;;
  uninstall)
    uninstall
    exit 0
    ;;
  *)
    echo "Usage: $0 [install|uninstall]" >&2
    exit 1
    ;;
esac

for command in cargo sync; do
  require "$command"
done

# Cargo reads .cargo/config.toml from the working directory, not from
# --manifest-path.
target_dir=${CARGO_TARGET_DIR:-"$root/target"}
(cd "$root" && cargo build --locked --release)
artifact_path="$target_dir/release/hex"
test -x "$artifact_path" || fail "Hex executable is missing: $artifact_path"
version=$("$artifact_path" --version 2>/dev/null \
  | awk 'NF { value=$NF } END { print value }') || true
# The version is display text that names a directory, never compared.
printf '%s\n' "$version" | grep -Eq '^[0-9A-Za-z][0-9A-Za-z.+_-]{0,127}$' \
  || fail "The built executable reports no usable version: $artifact_path"

application_staged=
autostart_staged=
unit_staged=
binary_staged=
current_staged=
version_staged=
cleanup() {
  for path in \
    "$application_staged" \
    "$autostart_staged" \
    "$unit_staged" \
    "$binary_staged" \
    "$current_staged" \
    "$version_staged"
  do
    if [ -n "$path" ]; then
      rm -f "$path"
    fi
  done
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

managed_directory "$support_dir"
managed_directory "$support_dir/versions"
managed_directory "$support_dir/updates"
exec 9> "$support_dir/updates/update.lock"
if ! flock -n 9; then
  echo "A Hex install or update is already in progress." >&2
  exit 1
fi
current="$support_dir/current"
# Installs this build, whatever version is installed now: versions are never
# compared.
if [ -e "$current" ] || [ -L "$current" ]; then
  if [ ! -L "$current" ]; then
    fail "Refusing unmanaged current version path: $current"
  fi
  readlink "$current" | grep -Eq '^versions/[0-9A-Za-z][0-9A-Za-z.+_-]{0,127}$' \
    || fail "Installed Hex has an invalid current version link."
fi
version_dir="$support_dir/versions/$version"
managed_directory "$version_dir"

safe_user_directory "$install_dir"
safe_user_directory "$applications"
safe_user_directory "$autostart"
safe_user_directory "$units"
binary="$install_dir/hex"
if printf '%s' "$binary" | LC_ALL=C grep -Eq '["\\[:cntrl:]]'; then
  fail "Hex install path contains unsupported characters."
fi
expected_binary="$support_dir/current/hex"
if printf '%s' "$support_dir" | LC_ALL=C grep -Eq '["\\[:cntrl:]]'; then
  fail "Hex service path contains unsupported characters."
fi
if [ -L "$unit" ] || { [ -e "$unit" ] && ! grep -Fxq '# X-Hex-Managed=true' "$unit"; }; then
  fail "Refusing to replace an unmanaged systemd unit: $unit"
fi
if { [ -e "$binary" ] || [ -L "$binary" ]; } \
  && { [ ! -L "$binary" ] || [ "$(readlink "$binary")" != "$expected_binary" ]; }
then
  fail "Refusing to replace an unmanaged file: $binary"
fi
# Named for Hex's app id, which its windows and the desktop portal use.
application_entry="$applications/anomaly.hex.desktop"
autostart_entry="$autostart/anomaly.hex.desktop"
for desktop in "$application_entry" "$autostart_entry"; do
  if [ -L "$desktop" ]; then
    fail "Refusing to replace a desktop-entry symlink: $desktop"
  fi
  if [ -e "$desktop" ] && ! hex_entry "$desktop"; then
    fail "Refusing to replace an unmanaged desktop entry: $desktop"
  fi
done

application_staged=$(mktemp "$applications/.anomaly.hex.desktop.XXXXXX")
autostart_staged=$(mktemp "$autostart/.anomaly.hex.desktop.XXXXXX")
unit_staged=$(mktemp "$units/.hex.service.XXXXXX")
umask 077
cat > "$application_staged" <<EOF
[Desktop Entry]
Type=Application
Name=Hex
Comment=Voice dictation
Exec="$binary"
Icon=anomaly.hex
Terminal=false
Categories=Utility;Accessibility;
StartupNotify=true
StartupWMClass=anomaly.hex
Actions=quit;
X-Hex-Managed=true

[Desktop Action quit]
Name=Quit Hex
Exec="$binary" quit
EOF
cat > "$autostart_staged" <<EOF
[Desktop Entry]
Type=Application
Name=Hex
Comment=Voice dictation
Exec="$binary" start
Icon=anomaly.hex
Terminal=false
X-GNOME-Autostart-enabled=true
X-Hex-Managed=true
EOF
chmod 644 "$application_staged" "$autostart_staged"

# systemd expands percent specifiers, and ExecStart also expands dollar signs.
unit_binary=$(printf '%s' "$binary" | sed -e 's/%/%%/g' -e 's/\$/$$/g')
unit_support=$(printf '%s' "$support_dir" | sed 's/%/%%/g')
cat > "$unit_staged" <<EOF
# X-Hex-Managed=true
[Unit]
Description=Hex background dictation
PartOf=graphical-session.target
After=graphical-session.target
StartLimitIntervalSec=60
StartLimitBurst=5

[Service]
Type=notify
NotifyAccess=main
ExecStart="$unit_binary" service
Environment="HEX_APPLICATION_SUPPORT_DIR=$unit_support"
EnvironmentFile=-$unit_support/session.env
Restart=on-failure
RestartSec=3
TimeoutStopSec=30
UMask=0077

[Install]
WantedBy=graphical-session.target
EOF
chmod 644 "$unit_staged"

version_staged="$version_dir/.hex.install.$$"
current_staged="$support_dir/.current.next"
binary_staged="$install_dir/.hex.next"
install -m755 "$artifact_path" "$version_staged"
sync -f "$version_staged"
mv -Tf "$version_staged" "$version_dir/hex"
version_staged=
# A build from a checkout updates from that checkout: the marker and the
# missing receipt keep Hex from replacing it with a published release.
: > "$version_dir/source-build"
sync -f "$version_dir"
rm -f "$current_staged" "$binary_staged"
ln -s "versions/$version" "$current_staged"
mv -Tf "$current_staged" "$support_dir/current"
current_staged=
rm -f "$support_dir/install.json"
sync -f "$support_dir"
ln -s "$support_dir/current/hex" "$binary_staged"
mv -Tf "$binary_staged" "$binary"
binary_staged=
sync -f "$install_dir"
mv -Tf "$application_staged" "$application_entry"
application_staged=
mv -Tf "$autostart_staged" "$autostart_entry"
autostart_staged=
# Earlier installs named both entries hex.desktop.
for desktop in "$applications/hex.desktop" "$autostart/hex.desktop"; do
  if [ -f "$desktop" ] && [ ! -L "$desktop" ] && grep -Fxq 'X-Hex-Managed=true' "$desktop"; then
    rm -f "$desktop"
  fi
done
mv -Tf "$unit_staged" "$unit"
unit_staged=
sync -f "$applications"
sync -f "$autostart"
sync -f "$units"
if command -v systemctl >/dev/null 2>&1; then
  systemctl --user daemon-reload >/dev/null 2>&1 || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$applications" >/dev/null 2>&1 || true
fi

printf 'Installed Hex %s to %s\n' "$version" "$binary"
# Wayland paste runs wl-copy, from the wl-clipboard package this installer
# does not bring. Say so now, not after the first dictation fails.
if ! command -v wl-copy >/dev/null 2>&1; then
  printf '\nWayland paste needs wl-copy from the wl-clipboard package, and wtype on wlroots compositors and niri:\n'
  printf '  Debian/Ubuntu: sudo apt install wl-clipboard wtype\n'
  printf '  Fedora: sudo dnf install wl-clipboard wtype\n'
  printf '  Arch: sudo pacman -S --needed wl-clipboard wtype\n'
  printf '\n'
fi
printf 'Launch Hex: %s\n' "$binary"
printf 'Start in the background: %s start (quit with %s quit)\n' "$binary" "$binary"
printf 'Uninstall: scripts/install-linux.sh uninstall (from the Hex checkout)\n'
