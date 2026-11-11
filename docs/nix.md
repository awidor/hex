# Nix

The flake packages this fork of Hex for Linux only, `x86_64-linux` and
`aarch64-linux`; there is no macOS package. Both targets build from source.
This fork transcribes only through the ElevenLabs Scribe cloud API: Hex needs
a network connection and an ElevenLabs API key (see [API Key](#api-key)), and
audio is sent to ElevenLabs on every transcription. What each desktop needs
for shortcuts, paste, and the recording overlay is in the
[Linux guide](linux.md).

Not verified: since this fork changed the Nix files, the package has not been
rebuilt, `nix flake check` has not been run, and neither module has been used
on a system.

## Run Or Install

Use the flake from a checkout of this fork. With Nix flakes enabled:

```sh
nix build
nix run . -- key set    # paste your ElevenLabs API key
nix run . -- app
```

Nix builds only the files Git tracks in the checkout, so `git add` new files
before building.

Or point a flake input of your configuration at the checkout and install
`hex.packages.${system}.default` (`x86_64-linux` or `aarch64-linux`):

```nix
{
  inputs.hex = {
    url = "git+file:///path/to/hex";
    # Use the name of your own nixpkgs input here.
    inputs.nixpkgs.follows = "nixpkgs";
  };
}
```

The package supplies the `hex` command and desktop launcher, native libraries
(including PipeWire and libva for Annotate), and the `curl`, `wl-copy`,
`wtype`, and `ffmpeg`/`ffprobe` runtime dependencies. Annotate's hardware
encoder uses whatever VA-API driver your system provides (NixOS:
`hardware.graphics`, through `/run/opengl-driver`); without one it falls back
to ffmpeg's libx264. Your system still needs a working audio session, a
network connection, and a Vulkan driver for the window. On non-NixOS hosts,
Nix GUI applications may require the host's usual Nix graphics integration.

The `follows` line matters on NixOS. Hex built from its own pinned nixpkgs can
use an older glibc than your system's graphics drivers need; Hex then finds no
suitable Vulkan adapter and its window does not open. Following your system's
nixpkgs makes both use the same glibc.

Launch Hex as your normal desktop user. Do not run Hex as root.

Hex has no updater in this fork: it never checks for, downloads, or installs
an update. Pull the checkout, update the flake input, and rebuild or switch
your Nix/Home Manager configuration instead. Remove any old source-install XDG
autostart entry before enabling a second autostart mechanism; do not run two
Hex listeners.

## API Key

Hex needs an ElevenLabs API key. Without one the listener shows "API key
required" and does not start; it starts by itself once a key is set.

Neither module has an option for the key, and a key written into a Nix
configuration would end up in the world-readable Nix store. Save it as your
desktop user instead:

```sh
hex key set       # paste the key at the prompt, or: hex key set < key.txt
hex key check     # ask ElevenLabs whether it accepts the key; sends no audio
```

`hex key set` writes the owner-only file `elevenlabs.key` in Hex's data folder
(`~/.local/share/hex` by default), which Hex reads for each request. Settings
› General › Transcription saves the same file.

Hex also reads the key from the environment variable `ELEVENLABS_API_KEY`,
but the systemd user unit does not inherit your shell's environment: a
variable exported in a shell reaches the `hex` commands run in that shell
(`hex model transcribe`, `hex key check`), not the background service. The
same holds for `HEX_ELEVENLABS_URL`, which replaces ElevenLabs' base URL. Use
the saved key file; the [Linux guide](linux.md#api-key) has the details.

## NixOS

Import `hex.nixosModules.hex` and enable the package:

```nix
{
  imports = [ hex.nixosModules.hex ];
  programs.hex.enable = true;
}
```

This installs Hex system-wide. It does not start a root service or enable
autostart for every account. Use the
Home Manager module below for per-user autostart. Both modules accept an
explicit `programs.hex.package` override. The flake also exports
`hex.overlays.default` for configurations that prefer `pkgs.hex`.

## Home Manager

```nix
{
  imports = [ hex.homeManagerModules.hex ];
  programs.hex = {
    enable = true;
    autostart = true;
    # Use your compositor's session target when appropriate:
    # systemdTarget = "sway-session.target";
  };
}
```

The module always installs the systemd **user** service. Autostart defaults to
`false`; when enabled, the unit is wanted by `graphical-session.target` by default.
It waits for a Wayland socket or an X11 window manager before launching `hex service`,
which reports readiness to systemd (`Type=notify`) once its socket answers.
Hex is one process: `hex` opens its window, starting the unit when it is not
running, and `hex start` starts it without the window. Closing the window never
stops Hex; a tray icon shows when the desktop has a tray host. The unit stops with the configured target
and retries failed starts without a tight restart loop; a crash restart opens no
window. `hex quit` (or **Quit Hex** in the window) leaves it stopped until the
next session or the next launch.

Your session manager must import its current `DISPLAY`, `XAUTHORITY`,
`WAYLAND_DISPLAY`, and `XDG_RUNTIME_DIR` values as applicable into the systemd
user environment **before** starting the target, and stop the target at
logout. Use your compositor's systemd integration, such as Home Manager's
Sway integration, rather than starting the target from a shell login.
The module does not create a graphical session or grant permission to use
one. A bare `startx`/i3 session without systemd session integration will not
start this service automatically.

`hex start` records the current display/session environment in an owner-only
file read by the unit; it clears stale variables when switching between X11 and
Wayland. There is one Hex per user. Launching it from a different desktop
explicitly restarts the unit for that session. The unit names Hex's default
data folder, so Hex still treats it as its own supervised service: a stale
`XAUTHORITY` from an earlier login (GNOME's XWayland cookie) is replaced with
the user manager's current one, keeping the HUD working after a re-login (#1140).

Home Manager's normal service-switch policy applies: newly enabled services
can start during a switch if the session target is already active, and
package changes can restart a running service. Do not switch during an
active recording. Inspect failures with:

```sh
systemctl --user status hex.service
journalctl --user -u hex.service
```

### Declarative Settings

```nix
{
  programs.hex = {
    enable = true;
    shortcut = "control+option+space"; # dictation_hotkey
    language = "de";                   # transcription.language
    settings = {
      shortcut_mode = "toggle";
      sound_effect_volume = 0.4;
      paste_with_shift = true;
      "scribe.numbers_format" = "digits";
      "meetings.enabled" = true;
    };
  };
}
```

`settings` takes the dotted keys `hex settings list` shows, with JSON values:
booleans, numbers, text such as shortcuts and choices, `null` to clear an
optional setting, and attribute sets or lists for its `json` settings. The
`shortcut` and `language` options are shorthands for `dictation_hotkey` and
`transcription.language` (an ISO 639 code, or `auto` to let Scribe detect the
language). There is one model, `scribe_v3`, which runs at ElevenLabs, so the
modules have no model option and nothing to download.

Every activation merges them into Hex with `hex settings apply`, after
systemd units reload. Declared keys overwrite Hex's values; anything you
change in the window that isn't declared keeps its value until you declare
it. A running Hex saves and adopts the settings at once, so it never writes
older values back from memory; with Hex stopped they go straight into
`settings.json`. `settings.json` stays an ordinary writable file. An unknown
key or invalid value applies nothing and prints a warning, without failing
the switch.

## Wayland Shortcuts

Hex reads no input devices, so neither module adds users to the `input`
group, installs udev rules, or grants capabilities. With a GlobalShortcuts
portal, the desktop grants the shortcuts Settings records; the portal
identifies Hex by the package's `anomaly.hex.desktop` entry, so keep the package's
`share/applications` on `XDG_DATA_DIRS` (NixOS and Home Manager profiles do).
Without the portal, bind the commands in your compositor, for example in Sway:

```sh
bindsym F9 exec hex dictation start
bindsym --release F9 exec hex dictation stop
```

`hex dictation toggle` and `hex paste-last`
are also available; see the
[Linux guide](linux.md#shortcuts). The package ships `wl-copy` and `wtype` for
paste; where the compositor lacks the virtual keyboard protocol (KDE, GNOME),
Hex pastes through the desktop's RemoteDesktop portal instead. No privileged
input-injection fallback is installed.

## Development

```sh
nix develop
cargo build --locked
cargo test --locked
cargo run --locked -- app
```

The package and shell share the native build environment and runtime
library/plugin paths. The shell also supplies `curl`, `wl-copy`, `wtype`, and
`ffmpeg`, with rust-analyzer, rustfmt, and clippy, so it does not rely on the
installed app's wrapper. Desktop commands still require a real graphical/audio
session and the permissions described above.

`nix flake check` evaluates the module, shell-environment, and
session-readiness regressions and builds the package with its normal
display-free Rust tests. Tests explicitly marked ignored because they require
a desktop remain ignored. The tests read no API key, so the build sends
nothing to ElevenLabs, and it exercises no live input.

The packaging comes from upstream Hex: the initial design builds on
[PR #29](https://github.com/anomalyco/hex/pull/29), and declarative settings
follow
[@SymphonySimper's request in #588](https://github.com/anomalyco/hex/issues/588).
