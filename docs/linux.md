# Hex On Linux

This fork of Hex transcribes only through the ElevenLabs Scribe cloud API
(`scribe_v3`). Every dictation's audio is sent to ElevenLabs and nothing is
transcribed on the computer, so dictation needs a network connection and an
ElevenLabs API key (see [API Key](#api-key)).

The build scripts accept x86_64 and ARM64 (aarch64) Linux, and the code covers
X11 and Wayland. This fork was developed and verified on KDE Plasma (Wayland)
only: what this guide says about other desktops and compositors (GNOME,
Hyprland, Sway, niri, i3/X11) describes code inherited from upstream Hex that
was not run here. Against the real ElevenLabs service only the command line
and the two request paths were run (see
[features/transcription.md](features/transcription.md#verified-against-elevenlabs));
the app itself was never launched after the change.

Audio uses ALSA, including PipeWire systems with ALSA compatibility. The
window draws with Vulkan (the Rune UI renders through wgpu), so it needs a
Vulkan driver; `vulkan-icd-loader` alone is not a driver. Install a driver for
your GPU or a software Vulkan driver. See the Wayland requirements below.

The window has Settings, Modes, and History. Meetings is a lab, off by default
([features/meetings.md](features/meetings.md#linux-and-windows)); recording a
meeting sends the other participants' audio to ElevenLabs too.
Paste on Wayland uses `wtype` where the compositor offers the virtual keyboard
protocol (wlroots compositors, niri) and the desktop's RemoteDesktop portal
elsewhere (KDE, GNOME). Shortcuts are general: the desktop's GlobalShortcuts portal when it has one, otherwise
your desktop's own keybindings running `hex` commands (see
[Shortcuts](#shortcuts)).

With **Labs › Live typing** on (`labs.live_typing`, off by default), a hotkey
dictation's provisional words are an underlined input-method preedit in the
focused text field and the final text is committed through
`zwp_input_method_v2`, so it lands without the clipboard and Ctrl-V.
`zwp_input_method_manager_v2` is a wlroots-family protocol (Sway, Hyprland,
and other wlroots compositors); KWin and Mutter do not offer it, so KDE and
GNOME use the paste above. Hex takes the input method when the capture
becomes intentional (the hold threshold, or at once in Toggle) and gives the
seat back at Finish or Cancel, including post-release processing; short taps
and chords create none. It falls back to the paste above when the compositor
does not offer the protocol, another input method (fcitx, ibus) holds the
seat, the focused field has no active text input, the final text is
multi-line, or the compositor does not answer in time. The provisional words
come from a Scribe realtime session (`scribe_v3_live`), which Hex opens for a
dictation whose words it shows and which ElevenLabs bills for all it hears. If
that session fails, no provisional words show, the whole clip is transcribed
by the batch model, and the final text still goes in through the input
method. Live typing has **not** been verified on a real compositor, upstream
or in this fork (KWin does not offer the protocol).

With **Labs › Transform typed text** on (`labs.typed_text`, off by default),
Settings › Output adds a **Typed text** section whose shortcut
(`typed_text_hotkey`) is unbound until you record one. Pressing it while
nothing records clears the clipboard, sends Ctrl+A and Ctrl+C to the focused
field, runs the focused app's Output steps on the copied text, clears and
copies again to check the field still holds that text, then sends Ctrl+V to
replace it. Spoken formatting, spoken symbols, and filler words are
dictation-only and do not run on typed text. It takes the clipboard for the
job, including any paste still waiting to restore, and puts the saved
clipboard back 500 ms after the paste unless something newer replaced it. If
Hex could not snapshot the clipboard, the result stays on it. Without
data-control on GNOME, text is read through `wl-paste`, the clipboard is
cleared through `wl-copy --clear`, and a result Hex could not snapshot stays
on the clipboard. Because it sends Ctrl+A and Ctrl+C, it is not meant for
terminals. With **Terminal paste** on, it refuses before sending any
key: its Ctrl+V would become Ctrl+Shift+V. Select-all replaces the whole
field, so mixed formatting is not preserved, and on GNOME `wl-paste` may
briefly take focus. With the lab off, Output and the listener are unchanged
and the shortcut is not registered; `hex typed-text` runs the same job from a
desktop keybinding, and refuses clearly when the lab is off, Terminal paste
shortcut is on, or Hex isn't listening.

For Nix/NixOS, use the [Nix guide](nix.md).

## One Hex Process

Hex on Linux is **one process per user** (and per data directory), not a
system-wide/root daemon. It owns the microphone, shortcuts, a dictation's
requests to ElevenLabs, sounds, paste, History, the recording pill, and the
window. The window is optional: closing it does not stop dictation, and
launching Hex again opens it. When the desktop has a tray (a
StatusNotifierItem host, such as Waybar, KDE, or Omarchy's bar), Hex shows a
tray icon: click it to open the window, or use its menu: **Show Hex**,
**Paste last dictation**, the meeting items while Meetings is on,
**Enable / Disable Hex**, and **Quit Hex**;
while Hex records or transcribes, the menu and tooltip say so. The mark is white,
and black when the desktop's color scheme prefers light; Hex reads that setting
when it creates the tray, so a later switch applies at the next launch. KDE's
portal takes the scheme from the window palette, so a mixed theme with light
windows and a dark panel (such as Breeze Twilight) gets a black mark on a dark
panel. Without a tray, quit with **Quit Hex** at the bottom of the window's
sidebar, the launcher's **Quit Hex** action, or `hex quit`.

The window opens at 980×760 logical pixels and floats on tiling window
managers with no window rule: it maps with its minimum size equal to its
maximum, which sway, Hyprland, niri, and X11 managers such as i3 float, then
drops the maximum so you can resize it down to 860×560 or up to any size.

Settings live in `settings.json` in Hex's data directory: `~/.local/share/hex`
(`$XDG_DATA_HOME/hex`), or the folder `HEX_APPLICATION_SUPPORT_DIR` names.

```sh
hex                             # open the window, starting Hex if needed
hex start                       # start Hex in the background; no window
hex status [--json]             # query the running Hex, not an old log entry
hex quit [--force]              # finish accepted work, release the microphone, exit
hex restart [--force]           # restart the systemd unit in this desktop's environment
                                # (over SSH: the one it last ran in)
hex settings set sound_effects off   # every control command (docs/cli.md)
journalctl --user -u hex.service # diagnostics of the supervised process
```

`hex app` is the same as `hex`, `hex start` runs `hex app --hidden` in the
background and waits until Hex answers, and `hex stop` is the same as
`hex quit`, so desktop and autostart entries written by older builds keep
working. `hex quit` and `hex restart` refuse while a
dictation records or transcribes unless `--force`. The control commands are
listed in [docs/cli.md](cli.md): `settings`, `model`, `key`,
`window`, `vocabulary`, `history clear`, `check-for-updates`, `onboarding
reset`, `annotate status`, `launch-at-login` (the XDG autostart entry),
`logs`, `commands init|check`, `history`, `meetings`, and `mcp`.

A launch first asks the running Hex over its socket. If one answers, it
forwards `Activate` with the launcher's `XDG_ACTIVATION_TOKEN` or
`DESKTOP_STARTUP_ID` and exits 0; the running process opens or focuses its
window. If none runs and the install script's (or Home Manager's) `hex.service` user
unit is loaded, the launch writes an owner-only `session.env` and starts the
unit, which reports readiness to systemd (`Type=notify`) once its socket
answers, then forwards `Activate`. Otherwise (a custom data directory,
`cargo run`, a desktop without systemd) the launching process becomes Hex
itself. Two racing launches produce one Hex; the loser forwards and exits 0.

`session.env` holds the display and session variables, the cursor theme and
input method (`XCURSOR_THEME`, `XCURSOR_SIZE`, `XMODIFIERS`, `GTK_IM_MODULE`,
`QT_IM_MODULE`), and, when set, `XDG_DATA_DIRS` and the locale (`LANG`,
`LANGUAGE`, `LC_ALL`, `LC_CTYPE`, `LC_MESSAGES`). Missing display variables
explicitly clear stale Wayland/X11 values inherited by the user manager; missing
locale and data-directory variables are left to the user manager. A saved
`XAUTHORITY` whose file is gone (Xwayland writes a new cookie at each login, so
a reboot strands the old path) is replaced at startup by the user manager's
current value, or dropped so Xlib uses `~/.Xauthority`. A unit started with no
display at all (the desktop never imported its environment into systemd) adopts
the user manager's `WAYLAND_DISPLAY`, else the newest Wayland socket of yours in
`XDG_RUNTIME_DIR` that answers. One started with only Xwayland's `DISPLAY` does
the same when `XDG_SESSION_TYPE` is `wayland` (from Hex's environment, else the
user manager's): it takes the manager's live `WAYLAND_DISPLAY`, else the newest
answering socket. Any other explicit session type (`x11`, `tty` from startx,
`mir`, …) keeps its X display even when the user manager still names a nested
compositor or another login's display, so paste keeps reaching X11 apps; only
an absent type adopts a live manager display. Launchers (`hex`, `hex start`,
`hex restart`) make the same decision before they write `session.env`, so the
service and the launcher agree on the session; a launcher never adopts without
a display, and when it would start the unit with nothing to write it still
refuses with "Start Hex from your graphical login session". Without the
adoption, a Wayland desktop's paste goes through XWayland, which native Wayland
windows never see, and restoring the clipboard clears the compositor's.
Hyprland imports nothing by default; add this to its config so the service, its
paste helpers, and the HUD get your session:

```ini
exec-once = dbus-update-activation-environment --systemd WAYLAND_DISPLAY XDG_CURRENT_DESKTOP
```

When the HUD can't show, Hex's log says why: no display, no layer-shell (and
no XWayland), or the overlay failing to open, with its error. No
permissions are granted. The unit is `PartOf=graphical-session.target`;
desktops managing a custom session target must bind its lifetime to their
login/logout lifecycle. Systemd restarts a crashed Hex (`Restart=on-failure`)
without a window.
There is one Hex per user: a launch from another desktop session restarts the
supervised unit in the new session, and refuses to take over an unsupervised
one (quit it first).

The socket is owner-only in Hex's data directory, with same-user peer
credentials, versioned bounded frames, bounded queues, and I/O deadlines. It is
not a network API. It carries
`Activate`, `Status`, and Annotate's recording commands. The `hex` control
commands reach Hex through its local API's `POST /control`; dictation
commands go straight from there to the listener, never through the window.
The local API's `/dictations` also
records on the listener while Hex is listening: one capture at a
time, so a client's dictation and the shortcut never end each other.

For development, run `cargo run --release`. `hex dictate` remains an explicit
foreground diagnostic listener without the window or HUD and cannot coexist
with Hex's listener. Like `hex service`, it adopts the session's Wayland
display when its shell has none, or only Xwayland's `DISPLAY` on a Wayland
login (#756, #1506). `hex` commands reach only the
service listener, so a foreground `hex dictate` receives none and says so at
startup when no shortcut is bound. With
`HEX_APPLICATION_SUPPORT_DIR` set, launches never start, stop, or restart the
installed user unit; they forward to the Hex using that data directory or
become it. `HEX_SYSTEMD_UNIT` names another unit for isolated tests.
`hex logs -n 40` is the diagnostic-log view (`hex status --lines` is gone).

The window runs on Rune's app lifetime (`rune::App`): Hex starts it without a
window, each `Activate` opens or focuses it with the forwarded token (on
Wayland through `xdg_activation_v1`; without a token most compositors only
mark it urgent), and closing it keeps the window's state for the next open.

## Install

This fork publishes nothing: no downloads, packages, or install script to
fetch. Install it from a checkout of this fork, with the install script or the
plain build below, or with Nix (the [Nix guide](nix.md)).

### Dependencies

To build: a stable Rust toolchain with Cargo, a C compiler, `pkg-config`, and
the development files of the libraries `nix/package.nix` builds with: ALSA,
OpenSSL, libxkbcommon (with its X11 library), Wayland, libX11, libxcb,
libXcursor, libXi, libXrandr, fontconfig, FreeType, and the Vulkan loader.
Upstream's speech-runtime dependencies (OpenBLAS, shaderc, the Vulkan and
SPIR-V headers, CMake) are not needed.

To run: a Vulkan driver for the window, and on Wayland the paste helpers
`wl-clipboard` and `wtype` (see
[Native Wayland Requirements](#native-wayland-requirements)). Annotate also
loads PipeWire and libva and runs `ffmpeg` and `ffprobe`, and recording a
meeting runs PipeWire's `pw-record`.

On Arch:

```sh
sudo pacman -S --needed base-devel git rustup alsa-lib openssl \
  libxkbcommon libxkbcommon-x11 libx11 libxcb libxcursor libxi libxrandr \
  fontconfig freetype2 wayland vulkan-icd-loader wl-clipboard wtype
rustup default stable
```

The `Install native libraries` step of `.github/workflows/check-linux.yml`
has the Debian and Ubuntu names; it also installs what only the tests need
(Python, GTK, Sway, Xvfb, PulseAudio, a software Vulkan driver).

### Install script

From the checkout, as your desktop user (the script refuses to run as root):

```sh
./scripts/install-linux.sh
~/.local/bin/hex key set    # paste your ElevenLabs API key
~/.local/bin/hex
```

The script runs `cargo build --locked --release` and installs the result for
the current user, without root access:

- `~/.local/share/hex/versions/<version>/hex`, the executable, beside a
  `source-build` marker, and `~/.local/share/hex/current`, a link to that
  version. `<version>` is what the executable's `--version` reports.
- `~/.local/bin/hex`, a link to `current/hex`.
- `~/.local/share/applications/anomaly.hex.desktop`, the launcher entry, and
  `~/.config/autostart/anomaly.hex.desktop`, an XDG autostart entry that runs
  `hex start`.
- `~/.config/systemd/user/hex.service`, the user unit. The script neither
  enables nor starts it; a launch starts it (see
  [One Hex Process](#one-hex-process)).

`HEX_INSTALL_DIR` replaces `~/.local/bin`, `HEX_APPLICATION_SUPPORT_DIR`
replaces `~/.local/share/hex`, and `XDG_DATA_HOME` and `XDG_CONFIG_HOME` are
honoured. The script refuses to replace a `hex` file, unit, or desktop entry it
did not write, and it does not start Hex. Run it again to install a newer
build, for example after `git pull`: it installs whatever the checkout builds,
and versions are never compared. Over an existing install it points `current`
at the new build and removes the install receipt (`install.json`) an upstream
install left. Settings stay; a `settings.json` written by upstream Hex still
loads, with any saved model name read as `scribe_v3`.

When `wl-copy` is missing, the script prints the commands to install the
Wayland helpers: on Debian or Ubuntu, `sudo apt install wl-clipboard wtype`; on
Fedora, `sudo dnf install wl-clipboard wtype`; on Arch, `sudo pacman -S --needed
wl-clipboard wtype`. `wtype` is only needed on wlroots compositors and niri,
where Hex uses it; GNOME and KDE paste through their RemoteDesktop portal.

General › **Launch at login** turns that autostart entry on or off
(`~/.config/autostart/anomaly.hex.desktop`); Hex leaves an entry it did not write alone.
Where the desktop has the Background portal (GNOME, KDE), Hex asks it at startup
to run in the background, and Launch at login goes through it: the portal writes
and removes that same autostart entry (marked `X-XDP-Autostart=anomaly.hex`).
Elsewhere Hex writes the entry itself. Hex itself writes a launcher entry only
when no installed package or data directory provides one, and leaves it as it
is while the program it starts is there. An entry Hex wrote
whose program is gone points at this Hex the next time it starts, or goes
once a package ships its own.
The launcher entry is `~/.local/share/applications/anomaly.hex.desktop`, named
for Hex's app id `anomaly.hex`, which its windows also report. Earlier installs
named both entries `hex.desktop`; Hex renames its own, and the script removes
them.
Its icon is `anomaly.hex`: the Nix package installs it in the hicolor theme,
and an install from a checkout gets it in `~/.local/share/icons/hicolor` when
Hex first starts.

Remove the application with:

```sh
./scripts/install-linux.sh uninstall
```

It stops and removes the unit, then removes the `~/.local/bin/hex` link, Hex's
desktop entries and icons, and the installed versions. Everything else in the
data folder stays, including settings, logs, kept failed dictations, and the
saved API key (`hex key clear` removes the key).

### Plain build

```sh
./scripts/build-linux.sh
```

This runs the same `cargo build --locked --release` and copies the executable
to `dist/hex-<version>-linux-x64` (`-arm64` on aarch64), printing its path;
`HEX_RELEASE_DIST` names another folder. It installs nothing: no launcher,
autostart entry, or unit. Run the executable directly; with no `hex.service`
unit loaded, the launched process becomes Hex itself.

With a custom `HEX_APPLICATION_SUPPORT_DIR`, `cargo run --release` runs an
unmanaged development Hex instead; without one it forwards to the installed
Hex.

## API Key

Hex needs an ElevenLabs API key: every transcription is a request to
ElevenLabs, sent with that key. Without one the listener shows "API key
required" and does not start; it starts by itself once a key is set.

Save the key in Settings › General › Transcription, or from a terminal:

```sh
hex key set       # paste the key at the prompt, or: hex key set < key.txt
hex key status    # whether a key is set, where it comes from, its last four characters
hex key check     # ask ElevenLabs whether it accepts the key; sends no audio
hex key clear     # remove the saved key
```

The saved key is the file `elevenlabs.key` in Hex's data folder
(`~/.local/share/hex`), readable by its owner only (mode 0600). It is never
written to `settings.json` or logged, and Hex shows only its last four
characters. Hex reads the key for each request, so a new one applies to the
next dictation without a restart.

Hex uses the environment variable `ELEVENLABS_API_KEY` when it is set, and the
saved file otherwise. On Linux the saved file is the dependable way. Hex
normally runs as the `hex.service` user unit, which does not inherit your
shell's environment: a launch passes it only a fixed list of display, session,
and locale variables (`SESSION_KEYS` and `INHERITED_KEYS` in
`src/linux/service.rs`). An `ELEVENLABS_API_KEY` exported in a shell reaches
the commands run in that shell (`hex model transcribe`, `hex key check`, a
foreground `hex dictate`) but not the background service. `hex key status`
likewise reports the key its own shell has; `hex status` shows the one the
running Hex has. To give the service the variable, add it to the unit's own
environment and restart Hex:

```sh
systemctl --user edit hex.service   # add the two lines below
hex restart
```

```ini
[Service]
Environment=ELEVENLABS_API_KEY=your-key
```

The key then sits in a unit drop-in file, which is not owner-only by default.
The same holds for `HEX_ELEVENLABS_URL`, which replaces ElevenLabs' base URL
(for example with a data-residency host).

Audio leaves the computer on every transcription: each dictation, meeting
chunk, Annotate narration, History's transcribe again, local-API request, and
`hex model transcribe`. Cost, retention, and what each request carries are in
[features/transcription.md](features/transcription.md).

### Model And Language

There is one model, `scribe_v3`, and it runs at ElevenLabs: nothing is
downloaded or installed, and there is no model picker or speech-engine
setting. The model commands show it, set the language Scribe is told to
expect, and send a file:

```sh
hex model list                           # the model, its language, whether a key is set
hex model status                         # also the key in use and where audio is sent
hex model use scribe_v3 --language de    # an ISO 639-1 or 639-3 code, or auto to detect
hex model check                          # the same check as hex key check
hex model transcribe clip.wav            # send a WAV file to ElevenLabs and print the text
```

## Dictate

Hold **Alt-Space**, speak, then release. Double-tap the shortcut to keep
recording, and press the same chord again to finish. **Shortcut mode** picks how the shortcut
triggers: **Hold or double-tap** (the default), **Hold**, **Tap** (starts and
stops with single presses), or **Double-tap** (waits for two complete taps,
and needs a shortcut with a key).
**Ctrl-Alt-V** pastes the last dictation again. Settings shows listener state,
Start/Stop, and operational errors. Changing a shortcut or a setting the
listener reads at start temporarily stops listening and restores the previous
running state after the edit.

Modes apply corrections, OpenCode rewriting, and transformations per
application or website; the application picker lists `.desktop` entries. On
X11, Hex reads the focused application from the active window's process name
(`code`, `chrome`). On Wayland, compositors with the foreign-toplevel protocol
(wlroots compositors such as Sway, niri, Hyprland) report the activated
window's app ID, which Hex names by its `.desktop` entry
(`google-chrome-stable`, `firefox`). GNOME and KDE don't report it, so the
Global mode applies there.

Website rules read the focused page's address through AT-SPI, the desktop
accessibility bus, on X11 and Wayland. Firefox exposes it whenever the
accessibility bus is running. Chrome and other Chromium browsers expose it
only with accessibility turned on: launch them with
`--force-renderer-accessibility` (for example in
`~/.config/chrome-flags.conf`), or run a screen reader. Without it, the
application rule applies. Completed dictations are kept in History.

**Turn off the mic when not dictating** (General) releases the microphone between dictations; it then opens
on the shortcut press without pre-roll. **While dictating** can mute the default
output through `wpctl` (PipeWire) or `pactl` (PulseAudio), or pause playing
MPRIS media players over D-Bus, once the hold passes 300 ms, so shortcut chords
and short taps never do; Hex restores only what it changed.
Cancelling shortcut capture preserves the old binding unless the new binding
has already been saved; listening resumes only if it was previously running.

### Failed dictations

Every transcription needs ElevenLabs, so one can fail for reasons the audio
would survive: no network, a timeout, an ElevenLabs server error, a rate
limit, a missing or refused key, or spent quota. Hex retries a batch request
once when the failure may be passing. If the dictation still fails, Hex keeps
the clip as an owner-only WAV in `failed-dictations/` in its data folder, and
the failure message (also in `hex logs`) names the file and the command that
transcribes it:

```sh
hex model transcribe ~/.local/share/hex/failed-dictations/dictation-<timestamp>.wav
```

That sends the file to ElevenLabs and prints the text; nothing is pasted or
saved to History. The folder holds at most 20 clips, and clips older than 7
days are removed whenever one is added. No audio is kept for a cancelled
dictation or for a request ElevenLabs rejects outright, which the same clip
would hit again.

### Bluetooth headsets

A Bluetooth headset can't play high-quality audio (A2DP) and use its
microphone at once. While any application records from the headset's
microphone, PipeWire (WirePlumber) or the headset switches it to the
hands-free profile (HFP/HSP): mono, call-quality sound in both directions.
This is how Bluetooth audio works, not something Hex chooses, and how quickly
the headset switches back depends on the headset and WirePlumber's settings.

If Hex records from the headset's microphone, **Turn off the mic when not
dictating** trades off differently on and off:

- **On** opens the microphone only when you press the shortcut,
  so each dictation switches the headset to hands-free and back. Playback
  drops out or pauses during the switch, and the microphone takes longer to
  open, so the first words can be lost.
- **Off** (the default) keeps the microphone open, so the headset stays in hands-free
  mode, with call-quality playback, for as long as Hex listens.

The recommended setup is a separate microphone for input (the laptop's
built-in microphone or a USB microphone) and the Bluetooth earbuds for output
only. Pick that microphone in General's **Microphone** list, or make it the
system's default input (for example with `wpctl set-default`), so nothing
opens the headset's microphone.

General › Advanced's Bluetooth option (`avoid_bluetooth_microphone`, on by
default) helps only part of the way:

- It applies only while the microphone is **System default** and the default
  input is a Bluetooth headset. A Bluetooth microphone you picked in Settings,
  or `--device`, is always used as named.
- It needs another hardware input that isn't reported unplugged, and
  `pactl` (PipeWire's PulseAudio server or PulseAudio) to move the stream.
  With no other input, Hex records from the headset.
- On Linux, Hex can only open ALSA's default device, which is the Bluetooth
  source, and then moves its own stream to the other microphone. The stream
  is briefly on the headset first, so the headset may still switch to
  hands-free mode, and Hex doesn't switch its profile back. Forcing the
  headset's profile is up to your sound settings.
- It changes only Hex's own recording, never other applications or the
  system default.

To avoid the switch entirely, use the recommended setup above.

## Shortcuts

Hex gets its shortcuts one of three ways, and all of them drive the same
dictation rules:

- **X11:** Hex grabs the shortcuts Settings records.
- **Wayland with the GlobalShortcuts portal** (for example niri with
  `xdg-desktop-portal-gnome`, GNOME, or KDE): Hex asks the desktop for the
  shortcuts Settings records. The desktop decides: it may grant them, assign
  another key, or refuse one that it or another binding already uses. Settings
  lists what the desktop granted under **Desktop shortcuts**, with an error for
  any it refused. A desktop may keep an older binding when you record a new
  shortcut (KDE does); where its portal supports it, **Desktop shortcuts >
  Open** shows the desktop's own dialog for Hex's shortcuts, and Settings
  follows what you choose there. KDE asks once in a "Global Shortcuts
  Requested" dialog; Hex waits for your answer, and if you cancel it KDE asks
  again the next time Hex binds. Hex's shortcuts are listed under **Hex** in
  System Settings › Keyboard › Shortcuts. The portal identifies Hex by its app id
  `anomaly.hex`, its installed `anomaly.hex.desktop` entry, and by inspecting
  the Hex process. An executable run without an install (a plain build) has
  no entry, so Hex writes a minimal
  `~/.local/share/applications/anomaly.hex.desktop` that runs it (the install
  script replaces it); if the portal still can't find it, restart the portal with
  `systemctl --user restart xdg-desktop-portal`. Run `hex service` directly:
  a setgid wrapper such as `newgrp`, once used to grant input-device access,
  makes the portal refuse Hex. The desktop delivers the keys to Hex, not to the
  focused application. On GNOME and KDE Plasma, new setups dictate with Ctrl+Alt+Space,
  because GNOME keeps Alt+Space for the window menu and Plasma for KRunner; Settings warns when a shortcut uses a key GNOME
  keeps for itself. Keys you change in GNOME Settings › Keyboard work at once,
  and Hex's Settings and `hex status --json` show them. Annotate's shortcut (F9
  by default) sits under the dictation shortcut in General. **Disable** (General or the tray) keeps Hex disabled
  across restarts until you enable it again. On GNOME, Disable also removes
  Hex's entries from Settings › Keyboard › Global Shortcuts; Enable then asks
  you to approve them again. Hex also removes the duplicate list that versions
  before `anomaly.hex` left under the id `hex`.
- **Hyprland** has the portal but takes every shortcut without a key, so
  nothing happens until you bind one in your config, and the key Settings
  records is not used. Hex can't tell whether your config binds it. Prefer
  keys without modifiers, because Hyprland adds every modifier you still hold
  to Hex's paste (see [Native Wayland Requirements](#native-wayland-requirements)).
  Avoid Alt in the bind too: Hyprland takes the key (Space) but lets Alt
  through to the focused window, so Electron apps with a hidden menu bar
  (Slack, Discord, VS Code) see Alt pressed and released alone, move focus to
  their menu, and the paste lands there instead of the message box
  (#1023). Hex still logs the
  paste as sent. Ctrl+Space or a key without modifiers works.
  Hyprland names each shortcut `anomaly.hex:<shortcut>`: `anomaly.hex:dictation`,
  `anomaly.hex:paste-last`, `anomaly.hex:capture` (Annotate),
  `anomaly.hex:raw-dictation` (Dictate without processing), and
  `anomaly.hex:terminal-paste` (Labs › Terminal paste shortcut, while on).
  The names stay the
  same when you record another key in Settings (#1376), so a bind keeps
  working; `hyprctl globalshortcuts` lists them while Hex runs. For example:

```lua
hl.bind("End", hl.dsp.global("anomaly.hex:dictation"))
hl.bind("F10", hl.dsp.global("anomaly.hex:paste-last"))
```

  Traditional Hyprland configs write
  `bind = , End, global, anomaly.hex:dictation`. Hex 3.3 and earlier put the
  recorded key in the name (`anomaly.hex:dictation-ALT-space`); Hex still
  answers those names, but only for the key recorded in Settings when the
  bind was written, so switch them to the bare names before you record
  another key. Binds to `hex:dictation` or `hex:paste-last` (names from
  before Hex's app id was `anomaly.hex`) do nothing now, so replace them. Remove any `cancel-dictation`
  bind: Hex no longer has it. Hyprland 0.56's Lua binds send no release when the modifier comes
  up before the key (Alt before Z); 0.57 fixes this. On 0.56, Toggle is
  unaffected: the next press still finishes. In Hold, recording continues
  until the next press and release, so lift the key first or use Toggle.
  For hold-to-talk that doesn't depend on the portal's release, bind Hex's
  own commands instead (below), for example
  `bind = , End, exec, hex dictation start` with
  `bindr = , End, exec, hex dictation stop`, or one
  `bind = , End, exec, hex dictation toggle`. Pasting into a terminal needs
  **Terminal paste** (Ctrl+Shift+V) unless the terminal accepts
  Ctrl+V.
- **Wayland without the portal** (for example Sway): nothing registers global
  shortcuts. Settings lists these commands; bind them in your desktop's own
  keyboard configuration. They reach only the listener `hex start` or
  `hex service` runs; a foreground `hex dictate` can't receive them:

```sh
hex dictation start     # bind to a key's press: start recording
hex dictation stop      # bind to the same key's release: stop and transcribe
hex dictation toggle    # one key: start, then stop and transcribe
hex paste-last          # paste the last dictation again
hex typed-text          # Labs › Transform typed text: replace the focused field's text
hex dictation toggle --raw  # dictate without processing: paste the words as heard
```

For example, hold-to-dictate on F9 in Sway, and a toggle in any desktop that
runs commands on key presses only:

```sh
bindsym F9 exec hex dictation start
bindsym --release F9 exec hex dictation stop
bindsym Ctrl+Alt+d exec hex dictation toggle
```

GNOME 46 and older (Ubuntu 24.04) have no shortcuts portal. There, Settings ›
Desktop shortcuts › **Add GNOME keyboard shortcuts** writes Hex's own entries
into GNOME Settings › Keyboard › Custom Shortcuts with your shortcuts:
`hex dictation toggle` (GNOME runs custom shortcuts on the key press only, so
dictation toggles), `hex paste-last`, `hex typed-text` while Transform typed
text is on and its shortcut is bound, and `hex annotate toggle` while Annotate
is on. Hex removes them while it's disabled and puts them back when you enable
it; **Remove** takes them out for good. Keys you change in GNOME Settings stay.
Hex never touches your other custom shortcuts.

`hex dictation stop --print` stops the same way, then waits up to 60 s and
writes the transcript to stdout instead of pasting it, after the same
processing a paste gets. Only that command waits; the listener stops
recording at once, so a following `start` isn't blocked. No speech exits 4
and prints nothing; a failure, cancel, or timeout prints nothing, exits 1,
and never pastes. History keeps the text as Printed, and Paste Last skips it.
See `docs/cli.md`.

The commands are safe to bind anywhere, including X11 beside the grabs. They
talk to the running Hex and never start it: without one, or while Hex is
not listening, they print a one-line error and exit non-zero. Starting while recording,
and stopping while idle do nothing, so key repeat is
harmless. Start and stop bypass Hold, Toggle, and double-tap: start records,
stop finishes. A shortcut press also finishes a recording a command started.

In **Tap** mode, a **Cancel** shortcut (Escape by default) throws away the
recording: nothing is pasted or saved, and dictations already transcribing
still finish. On X11, Hex grabs that key only while a recording runs, so it
works normally everywhere else. On Wayland a desktop shortcut would hold the
key all the time, so Hex binds nothing; bind `hex dictation cancel` to a key in
your desktop's keyboard shortcuts instead. `hex dictation cancel` works in any
mode. In the Hold modes there is no cancel: every dictation finishes and pastes.
On KDE, Hex removes the Cancel dictation shortcut older builds asked for,
so its key returns to your applications.

Shortcuts need a non-modifier key. Modifier-only shortcuts, such as Super
alone, are not supported on Linux.

On X11, Hex grabs Escape only while Annotate draws, where it stops drawing. If
another application holds it, drawing goes on and Hex logs it once.

Recording sounds are enabled at 50% volume by default, including when upgrading
older settings. A start sound is queued immediately when the shortcut starts capture,
a stop sound marks the end of a retained recording (not completed transcription
or paste), and a cancel sound acknowledges cancellation of an active capture.
Brief taps still play the start sound, but recordings shorter than 300 ms remain
discarded and do not play the stop sound. **Sound volume** in Settings
offers Off, 25%, 50%, 75%, and 100%; changes save immediately without restarting
the listener and preview the start sound unless set to Off. `hex dictate` uses
the same saved volume. Playback uses the default audio output through a bounded
background worker; unavailable audio output does not prevent dictation.

Closing the window leaves Hex running on both X11 and Wayland. The
recording/processing pill never takes focus or intercepts clicks, with or
without the window open: a Rune overlay that is a layer-shell surface on
Wayland and an override-redirect window on X11. On a Wayland compositor without
layer-shell (GNOME), only the pill opens on XWayland, as that X11 window with an
empty input shape on the primary monitor; the rest of Hex, including the window
and portal shortcuts, stays on Wayland. It needs the window's event
loop, so it is unavailable when the window cannot start (no display or GPU)
and in `hex dictate`. **Recording indicator** places it at the top or bottom of the
screen, or turns it off.

Insertion uses the clipboard and **Ctrl-V** for new installations. Enable
**Terminal paste** in Settings for targets that require **Ctrl-Shift-V**.
Labs › Terminal paste shortcut (`hex labs on terminal-paste-shortcut`, off by
default) adds a second dictation shortcut whose paste is always Ctrl-Shift-V,
while the main shortcut keeps this setting. On X11 Hex grabs it like the other
dictation shortcuts; on Wayland it needs the desktop's shortcuts portal (Hex
asks for `anomaly.hex:terminal-paste`). There is no `hex` command for it, so
without X11 grabs or the portal the shortcut cannot be registered.
Existing X11 settings retain their previous Ctrl-Shift-V behavior until changed.
The target application must accept the selected binding.

After a paste, Hex puts the previous clipboard back
([#391](https://github.com/anomalyco/hex/issues/391)). Before writing the
transcript it reads every format the clipboard offers (text, rich text, images),
up to 32 MiB within one second, and 500 ms after the paste shortcut serves them
again from Hex until something else copies. It restores only if nothing newer
replaced the transcript in the meantime: a later dictation, a copy, or another
application. An empty clipboard stays empty. A larger or unreadable clipboard
is left holding the transcript, and Hex's log says why. When automatic paste
fails after `wl-copy` has taken the transcript, it stays on the clipboard for
Ctrl+V. Nothing confirms the target read the paste, so a target stalled past
the delay can paste the restored clipboard instead (see
[#24](https://github.com/anomalyco/hex/issues/24)).
On Wayland, restoring needs the compositor's `ext-data-control` protocol
(KDE Plasma 6, recent wlroots compositors) or `wlr-data-control` (wlroots
compositors, niri, Hyprland); without either the transcript stays, and
Settings › Output says so.

GNOME offers neither. There you can opt in to Labs › Restore clipboard on
GNOME (`hex labs on gnome-clipboard-restore`, off by default): Hex then reads
and restores the clipboard through XWayland, which GNOME keeps in sync with
Wayland apps both ways. It works only while XWayland is already running (an X11
app is open and `DISPLAY` is set); Hex never starts XWayland, and otherwise the
transcript stays and the log says why. The same rules apply: nothing newer is
ever overwritten.

On GNOME, Hex pastes through the RemoteDesktop portal, and GNOME shows its
remote-control indicator while that session is open. Hex closes it 15 seconds
after the last paste and reopens it at the next one with the grant you
approved, so you're asked only once.

## Native Wayland Requirements

- Export the compositor's nonempty `WAYLAND_DISPLAY` in the launch environment.
  Hex and its window (winit) select the same native backend even when XWayland's
  `DISPLAY` is also present. `XDG_SESSION_TYPE` alone does not select Wayland.
- Install `wl-clipboard`. Every Wayland copy and paste runs `wl-copy`; without
  it, Settings and `hex status` warn while the listener runs, and a dictation
  fails naming the package to install. Hex copies each transcript, then sends
  Ctrl+V (or Ctrl+Shift+V with terminal paste) through one of two general
  paths, chosen at the first paste:
  - `wtype`, when the compositor offers the virtual keyboard protocol
    (`zwp_virtual_keyboard_manager_v1`; wlroots compositors and niri) and
    `wtype` is installed.
  - Otherwise the desktop's RemoteDesktop portal
    (`org.freedesktop.portal.RemoteDesktop`; KDE, GNOME). The first paste asks
    you to allow Hex to control the keyboard; Hex keeps the portal's restore
    token (`remote-desktop-token` in Hex's data folder), so it asks once. Revoke
    it in the desktop's settings.
  - With neither, the dictation fails with "Copied to the clipboard; press
    Ctrl+V to paste" and, with `wl-copy` installed, the transcript stays on the
    clipboard.

  There is no silent XWayland, direct-typing, uinput, or privileged
  input-injection fallback; the only XWayland uses besides the HUD are the
  opt-in clipboard restore above, the opt-in floating meeting capsules
  below, and the opt-in Annotate overlays below. Transcript text travels on stdin, not command-line
  arguments.
- The overlay uses layer-shell. Without it (GNOME), only the HUD opens on
  XWayland as an override-redirect, click-through window that never takes
  focus. The meeting capsules take clicks, so there a call offer opens the
  Meetings pane instead, unless you opt in to Labs › Floating meeting capsules
  on GNOME (`hex labs on gnome-meeting-capsules`, off by default): the offer
  and recording capsule then float over the call on XWayland, taking clicks
  on their buttons but never keyboard focus. It works only while XWayland is
  already running; Hex never starts it. Annotate's capsule and ink take
  clicks too, so a capture records with no capsule unless you opt in to Labs ›
  Annotate on GNOME (`hex labs on gnome-annotate`, off by default): the
  capsule and ink then float on XWayland the same way. Esc can't stop drawing
  there, so use the capsule's pen button or let go of a held Annotate key (a
  tap of it stops the recording). With neither, Settings reports that the HUD
  is unavailable rather than presenting a normal focus-taking window as an
  overlay.
- Shortcuts come from the GlobalShortcuts portal or from your own keybindings
  for the `hex` commands ([Shortcuts](#shortcuts)). Hex reads no input devices
  and needs no `input` group membership or other device permission. If the
  desktop closes the portal session, the listener stops with an error and an
  active recording is cancelled; enable it again from Settings or `hex start`.
- Hex can't see which keys are down on Wayland, and some compositors merge
  held physical modifiers into its paste: on Hyprland, Alt still held from
  Alt+Z turns `wtype`'s Ctrl+V into Ctrl+Alt+V, and a paste-last bound to
  Ctrl+Alt+V is matched by Hyprland again and never reaches the app. The
  RemoteDesktop portal shares the desktop's keyboard the same way. So when a
  shortcut with modifiers asks for a paste, Hex waits for its release (the
  portal's Deactivated) plus 250 ms, at most 2 s after a press whose release
  never comes. After a `hex` command, whose keybinding Hex can't see, it waits
  250 ms. A modifier still held past that changes the paste, so bind keys
  without modifiers (End, F-keys) or let go of the modifiers before the
  transcript arrives. Hex logs a dictation as pasted once it sent the paste
  shortcut; it can't confirm the application accepted it. A compositor that
  passes a shortcut's Alt through to the app (Hyprland) can also open an
  Electron app's menu when Alt comes up, which takes the paste (see
  [Shortcuts](#shortcuts)).

## Updates And Validation

The updater is off in this fork: Hex never checks for, downloads, or installs
an update, because upstream's release channels would replace this fork with
upstream Hex. The window's sidebar says "Updated from your source checkout",
and `hex check-for-updates` answers that this build doesn't update itself. To
update, pull the checkout and run `scripts/install-linux.sh` again, or rebuild
your Nix configuration.

Verified in this fork: KDE Plasma on Wayland. Not verified: any other desktop
or compositor, and anything against the real ElevenLabs service, which was
never called during development.

The native Wayland implementation builds on the feature
proposed in upstream [PR #28](https://github.com/anomalyco/hex/pull/28), with separate
input-state, lifecycle, and paste regression coverage.
`scripts/test-wayland-paste.sh /path/to/compiled-test-executable /path/to/hex` runs the real
paste helpers against a GTK target under an isolated headless Sway compositor.
Its first paste runs while another virtual keyboard holds Ctrl and Alt.
It then checks that every format of the previous clipboard comes back after the
paste, and that an empty clipboard stays empty.
It also starts an isolated Hex, opens its window through a second `hex app`
(the window must belong to the same process), repeats paste while the window is
open, closes the window, checks that the same process still answers, opens the
window again (`HEX_WAYLAND_REOPEN=0` skips that), and quits. It requires the GTK development
libraries, Sway, `wl-clipboard`, `wtype`, `dbus-run-session`, and a Vulkan driver
(Mesa's software driver is sufficient), and must run as a non-root user. It uses
isolated app data and no microphone; `.github/workflows/check-linux.yml` runs
it with the display-free suite and Xvfb hotkey tests.

`ELEVENLABS_API_KEY=... scripts/test-linux-dictation.sh /path/to/hex /path/to/jfk.wav`
checks the complete X11 app dictation path with a private PulseAudio virtual
microphone and Xvfb display. It opens the window in the running Hex through a
second `hex app`, replays a recording of Kennedy's "ask not what your country
can do for you" line (you supply the WAV), holds and releases the real
shortcut, verifies the transcribed words in a GTK editor, and checks orderly
application shutdown and microphone release. It needs PulseAudio, its ALSA
plugin, `xdotool`, Xvfb, the GTK development libraries, and a Vulkan driver for
the window (Mesa's software driver is sufficient). It runs as a normal user with
temporary settings; it does not use physical audio devices or personal recordings.
The clip is sent to ElevenLabs, so the key must be in the environment (the
test's `HOME` is empty, so Hex finds no saved key) and each run is billed for
one short clip. No workflow runs it, and it has not been run in this fork.

`python3 scripts/test-linux-service.py /path/to/hex` is a display-free,
device-free single-instance check: commands fail fast without Hex, `hex start`
becomes Hex, second launches (`start`, `app --hidden`, `service`, `app`) forward
and exit 0, a window request without a display fails only the window,
same-user socket permissions, version-1 clients get an error, invalid,
oversized, and stalled frames, crash/stale-socket recovery, two racing launches
yield one Hex, `hex quit` drains and removes the socket, and a custom data
directory never runs `systemctl`.
