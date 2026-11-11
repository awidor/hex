# Windows

In this fork the Windows code is type-checked by cross compilation only and
has never been run. Nothing on this page is verified here: it describes what
the code is written to do, not behaviour anyone has observed.

The Windows port is a Rune desktop app (a tray process; see
[The app](#the-app)) with WASAPI capture, transcription by ElevenLabs Scribe,
a global shortcut, and generation-safe automatic paste. Its daily-use surface
includes live microphone, language, and shortcut settings, Hold and Toggle
shortcut modes with a double-tap lock, Paste Last Dictation, recording tones,
a click-through HUD, retained History, text replacements, a resident system
tray, and per-user Launch at login. Global and contextual Modes can rewrite
completed dictation through the same deadline-bounded OpenCode processing
policy as macOS, then run ordered built-in or user-defined TypeScript
transformations.

Every transcript comes from ElevenLabs: Hex sends the audio of each dictation
to the Scribe cloud API with your API key. Nothing is transcribed on the PC,
and nothing works without a network connection. See
[Transcription and the API key](#transcription-and-the-api-key).

The default shortcut is `Ctrl+Space`. In **Hold** mode, hold both keys while
speaking and release Space to transcribe and paste. In **Toggle** mode, press
`Ctrl+Space` to start and again to finish, or press the **Cancel** shortcut
(Escape by default) to throw the recording away; Hex takes that key only while
a recording runs. Hold mode has no cancel shortcut (every dictation finishes
and pastes), and holding a modifier on its own does nothing. Hex consumes the
chord while it matches, so applications and input methods don't see
`Ctrl+Space`; rebind it in Settings if you rely on that chord elsewhere. To
let another app use the same key, such as a Discord push-to-talk key, turn on
Labs › **Share dictation shortcut with other apps**: Hex still reacts to it
but passes its key-down and key-up on, which can also trigger that binding in
other apps. In Hold mode, tap the shortcut twice within 300 ms to lock
recording, then press it again to finish. Paste Last defaults to
`Ctrl+Alt+V`. Saved settings reach a running listener in place, at a moment
when nothing records; it does not stop for them.

The Settings card reads **Enabled** when the shortcut works system-wide in
every app, **Disabled** when it does nothing, and **API key required** while
no ElevenLabs API key is set.

## Transcription and the API key

- There is one model, `scribe_v3`, and it runs at ElevenLabs. There is no
  speech model on the PC and nothing to download.
- Audio is sent to ElevenLabs on every transcription: each dictation, each
  meeting chunk (the other participants' voices included), an Annotate
  recording's narration, `hex record`, `hex model transcribe`, and every
  local API request that carries audio. ElevenLabs handles that audio and its
  text under its own retention policy.
- An API key is required. Save one with `hex key set` (it reads the key from
  standard input) or in Settings › General › Transcription, or put it in the
  environment variable `ELEVENLABS_API_KEY`, which wins over a saved key. Hex
  reads the key for each request, so a key saved while Hex runs applies to the
  next dictation.
- The saved key is the file `%APPDATA%\hex\elevenlabs.key`. On Windows the
  code sets no permissions on it: the file has whatever access the per-user
  `%APPDATA%\hex` folder gives it. `%APPDATA%` is the Roaming profile folder,
  which a managed network can copy to a server at sign-in. The key is never
  written to `settings.json` or the logs, and Hex shows only its last four
  characters.
- Without a key the listener's status reads **API key required** and the
  listener does not start; it starts by itself once a key is set.
  `hex key status` shows where the key comes from, `hex key check` asks
  ElevenLabs whether it works without sending audio, and `hex key clear`
  removes the saved key.
- Settings › General › Transcription also holds Language
  (`transcription.language`, `auto` by default), Final text
  (`scribe.final_text`), Leave out filler words (`scribe.no_verbatim`), and
  Numbers (`scribe.numbers_format`).
- When a dictation's transcription fails for a reason the same audio could
  survive (no network, a timeout, an ElevenLabs failure, a rate limit, a
  missing or refused key, no quota), Hex keeps the clip as a WAV in
  `%APPDATA%\hex\failed-dictations\` (at most 20 clips, for at most 7 days).
  The failure message names the file and the command
  `hex model transcribe <file>`.
- `HEX_ELEVENLABS_URL` names another ElevenLabs host to send requests to, for
  example a data-residency one.

## Build From Source

This fork publishes no Windows build: no installer, no download, no release
channel, no code signing, and no self-updates. The only way to get `hex.exe`
is to build it from a checkout.

- 64-bit Windows 10 or 11, x64 or ARM64.
- PowerShell 7 or Windows PowerShell 5.1.
- `winget` from Microsoft App Installer.
- Visual Studio C++ Build Tools and the stable MSVC Rust toolchain.

`.cargo/config.toml` links the C and C++ runtimes statically, so `hex.exe`
needs no Visual C++ Redistributable.

Run the setup script from the repository root:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\setup-windows.ps1
```

[`scripts/setup-windows.ps1`](../scripts/setup-windows.ps1) needs `winget`.
It installs rustup and the Visual Studio 2022 Build Tools with the C++
workload when they are missing (an existing installation with the C++ tools
is kept), installs the stable toolchain and makes it the default, adds the
`x86_64-pc-windows-msvc` target, `rustfmt`, Clippy, and rust-analyzer,
fetches the Cargo dependencies for that target, and runs
`cargo check --all-targets` (`-SkipCheck` leaves that out).

Then build from Git Bash on Windows:

```sh
scripts/build-windows.sh
```

[`scripts/build-windows.sh`](../scripts/build-windows.sh) refuses to run
anywhere else. It runs `cargo build --locked --release` for `hex` and for
`hex-console` (`crates/console`), checks that both outputs are Windows
executables, and copies them to `dist/hex-<version>-windows-<x64|arm64>.exe`
(the GUI app) and `.com` (its console entry point). `HEX_WINDOWS_ARCH` is
`x86_64` (default) or `aarch64`; the script does not install the Rust target
for it. `HEX_RELEASE_DIST` names another output folder. The script prints the
executable's path. The version in the file name is what the built
`hex.com --version` reports when the build is for this machine's
architecture, otherwise `HEX_VERSION` or the crate version.

To use the build from a terminal, put the two files in one folder as
`hex.exe` and `hex.com`. `hex.exe` is a GUI program, so launching it opens no
console window; `hex.com` runs the `hex.exe` beside it with the same
arguments, waits for it, and returns its exit code (`hex` and `hex app` only
start the app and return). A terminal resolves `hex` to `hex.com` before
`hex.exe` because PATHEXT lists `.COM` first. Nothing adds the folder to PATH
or makes a Start menu entry.

The updater is permanently off: Hex never checks for, downloads, or installs
an update. The window's sidebar footer shows a plain line ("Updated from your
source checkout") where Check for updates would be, and
`hex check-for-updates` answers that this build doesn't update itself.

## Known Issues

**Discord shows "Playing HEX: Shards of Fate"**
(upstream [#445](https://github.com/anomalyco/hex/issues/445)). With game
activity detection on, Discord Desktop matches the running `hex.exe` to the
verified game *HEX: Shards of Fate*. It then shows that game on your profile
for as long as Hex runs. The reporter's test suggests Discord matches on the
executable name. Open Discord's **Settings › Registered Games** (or **Activity
Privacy**) and hide the detected entry, or turn off activity sharing there.

## Streaming Dictation

By default a dictation is recorded while the shortcut is held, and on release
its whole clip is uploaded and `scribe_v3` returns the text (Final text:
Batch). Hex opens a realtime session (`scribe_v3_live`, a WebSocket) for a
dictation only when the session's text is used: with Labs › Live text on,
which shows provisional words while you speak, or with Final text set to
Live, which pastes the session's committed text instead of uploading the clip
again. A session is billed for all it hears, which is why none is opened
otherwise. A session that fails, misses its deadline (1.5 s after release),
or returns nothing falls back to transcribing the whole clip.

Each dictation logs its source in `logs\process.log`: `source=offline` (the
whole clip, by choice), `source=stream` (the live session's text), or
`source=fallback`. "Offline" names the whole-clip path; it still goes over
the network. The WebSocket connects directly: the HTTP proxy Hex uses for its
HTTPS requests is not applied to it. See [dictation](features/dictation.md).

## Commands

These run from a checkout with `cargo run --`, or as `hex` from a built pair.

List the WASAPI inputs, save and check the API key, and transcribe a WAV
file (the file's audio is sent to ElevenLabs):

```powershell
cargo run -- devices
cargo run -- key set
cargo run -- key check
cargo run -- model status
cargo run -- model transcribe .\sample.wav
```

`model check` is the same check as `key check`: it asks ElevenLabs about the
key and sends no audio. For Polish, set the language and record one capture:

```powershell
cargo run -- model use scribe_v3 --language pl
cargo run -- record --language pl --copy
```

Open the desktop application:

```powershell
cargo run -- app
```

The Settings card's button and the tray's **Enable / Disable Hex** switch
between **Enabled** and **Disabled**. Hex enables itself on launch by default
(**Enable on launch**), once first-run setup is done and a key is set.
Changing the language, microphone, a shortcut, double-tap behavior, or the
While dictating setting applies to the running listener without stopping it;
only its keyboard hook, microphone stream, or audio control is replaced as
needed. Text replacements and feedback volume apply the same way. Mode rules,
OpenCode profiles, and ordered transformations are read by the output worker
and apply to the next completed dictation.

`listen` runs the listener in the foreground without the window, with the
saved shortcuts and Modes settings. Its microphone is `--device` (the system
default when left out) and its language is `--language` (`auto` when left
out), not the saved ones. It can't run while the app's listener does (both
take `windows-listener.lock`):

```powershell
cargo run -- listen --language pl
```

`record` opens the microphone before prompting. Press Enter once to start,
then again to stop. Captures shorter than 300 ms are discarded; anything
longer is sent to ElevenLabs and the text is printed (`--copy` also puts it on
the clipboard). `--device` accepts a case-insensitive fragment from
`devices`.

## Data And Logs

Runtime data is stored under `%APPDATA%\hex` (`settings.json`,
`history.json`, `elevenlabs.key`, `failed-dictations\`, `local-api.json`),
except logs, which belong to this computer and live in
`%LOCALAPPDATA%\hex\logs`: on managed networks Windows copies the Roaming
folder to a server at every sign-in. An older Hex's `logs` folder moves there
once (`app_paths::machine_local`); while a running Hex holds it, the Roaming
copy stays in use. `HEX_APPLICATION_SUPPORT_DIR` names another data folder,
logs included. Observations are in `logs\live.ndjson`, and native/runtime
diagnostics are in `logs\process.log`, including every panic with its thread
and backtrace. A worker that stops unexpectedly leaves its cause there.
`hex.exe` is a GUI program, so a launch from Explorer or at login opens no
console window. Run from a terminal, it prints startup errors there, then the
app leaves the terminal and has no stdout or stderr.
Successful pasted dictations are retained, subject to the selected retention
window and hard size limits, in `history.json`. A dictation's audio is not
kept on disk by default, except the clip of a dictation whose transcription
failed (see [Transcription and the API key](#transcription-and-the-api-key)).

## Control commands

The `hex` control commands ([docs/cli.md](cli.md)) drive the running tray app
as they do on macOS: `status`, `start`, `quit`, `restart`, `dictation
start|stop|cancel|toggle` (with `stop --print`), `paste-last`, `settings`,
`model`, `key`, `labs`, `window`, `vocabulary`, `history clear`,
`check-for-updates`, `launch-at-login` (the Run key), `onboarding reset`,
`logs`, and `commands init|check`, `annotate`, `meetings
record|stop|copy-last`, `history`, and `mcp`. They reach it through its local
API (`local-api.json` in `%APPDATA%\hex` and the bearer-authenticated
`POST /control`). Dictation commands go to the listener's own loop as a
shortcut press would, and `annotate` start, stop, cancel, and status wait for
its capture host. `history`, `meetings`, and `mcp` work as on macOS.
`hex restart` quits, waits for the drain, and starts `hex app --hidden`
again. `key`, `model status`, `model check`, and `model transcribe` run in
the `hex` process itself, with or without the app.

## Custom Transformations

The Transformations card in Modes always offers the built-in transformations.
Choose **Set up** there to create `~/.config/hex/hex.config.ts` and load any
transformations registered by that config. Hex runs the config in its embedded
JavaScript engine (QuickJS), so nothing else needs installing.

Transformations run in the displayed order after mode corrections and the
optional OpenCode rewrite. Reordering, adding, or removing them persists in the
global or contextual mode and applies to the next output job. Each
transformation call is limited to one second and the engine to 64 MiB; a
failed step preserves the previous pipeline output. Saving the config reloads
it; a config that fails to load keeps the previous one and shows its error in
the card.

## The App

`hex app` (`--hidden` for a login launch) runs the tray app on Rune. Snapshot
fixtures without starting anything: `hex app -- --snap out.png --scene modes`.

```ts
hex app
├── named event already held -> signal it (the running app shows its window) -> exit
└── becomes the app                                   // windows::rune::open
    ├── the local API's POST /control -> control::host::answer
    ├── runtime thread: WindowsDesktopHost (listener, language, OpenCode, transformations), publishes every 250 ms
    ├── main thread: rune::App's winit loop and the window; rune::Tray runs on its own thread
    ├── --hidden -> windowless start; close -> hide (keep_running)
    ├── tray Show or a relaunch -> AppHandle::open (shows and focuses)
    └── tray Quit / Ctrl-C -> AppHandle::quit -> runtime stops the listener
```

- Quit Hex in the window or the tray stops the listener and ends the app;
  closing the window only hides it.
- The window uses the shared Rune panes (General with the transcription
  language and the API key, Output, History, Labs, first-run setup) with
  Windows rows: Launch at login (the current-user Run key, which names
  the `hex.exe` that set it), and no terminal-paste row in General (Terminal
  paste is a lab on Windows). Shortcuts are recorded by polling the keyboard
  with the listener's hook suspended, because modifier-only chords never
  reach the window as key events.
- The runtime saves a chosen language and hands it to the running listener,
  which uses it from the next dictation. The Labs features that switch with
  the keyboard layout or the frontmost app change only the language.
- Enable on launch waits until first-run setup has recorded completion, and
  the listener waits for an API key.
- The window and the HUD draw on the GPU through Rune (wgpu). The Rune
  revision `Cargo.toml` pins uses Direct3D 12 on Windows, and Vulkan only on
  a PC with no Direct3D 12 adapter. Closing the window gives the window and
  its GPU surface back (`release_gpu_when_closed`), so a Hex sitting in the
  tray holds no Direct3D window.
- The dictation HUD is a Rune overlay (`rune_hud.rs`, shared with Linux): its
  compositor steps the shared indicator model and Dot scope and draws each
  frame on the GPU with `rune_hud.wgsl`. Rune shows it in a topmost tool
  window with `WS_EX_NOACTIVATE|WS_EX_TRANSPARENT|WS_EX_LAYERED`, shown with
  `SW_SHOWNOACTIVATE`, 12 pt from the top or 24 pt above the taskbar. The
  same model, shaders, and overlay run on Linux; on Windows it is only
  type-checked.
- The window gets the executable's icon from Rune's `App::on_window` hook
  (its HWND through `WindowHandle::with_raw_handle`) for the taskbar,
  Alt-Tab, and the caption. The native title bar keeps its icon and title,
  blended into the window: DWM paints the caption the theme's `bg_deep`
  (`DWMWA_CAPTION_COLOR`; Windows 10 has no caption colour and keeps DWM's
  own dark or light caption from `DWMWA_USE_IMMERSIVE_DARK_MODE`, and builds
  before 19041 keep the OS caption) and draws its text and buttons in the
  window's light or dark mode. The view re-applies both when the window's
  theme changes, and a window subclass re-applies that caption after a
  `WM_SETTINGCHANGE` while Appearance forces a theme different from the OS:
  winit repaints the frame from the OS theme on each one, and only reports
  `ThemeChanged` when the OS theme actually changed. The subclass removes
  itself when the window is destroyed.
- Without a tray (creation failed) closing the window quits Hex.

### Type-Checking From Linux

The Windows code is checked by cross-compiling on Linux with
[cargo-xwin](https://github.com/rust-cross/cargo-xwin), for the x64 target:

```sh
rustup target add x86_64-pc-windows-msvc && cargo install cargo-xwin
# clang-cl, lld-link, and llvm-lib must be on PATH (llvm-lib can be a
# symlink to llvm-ar).
cargo xwin check --target x86_64-pc-windows-msvc
```

That type-checks every Rust path for that target, including Rune and the
Windows modules. It compiles the C in build scripts (QuickJS, blake3) for
the target but never links or runs anything, so it says nothing about how
Hex behaves on a PC.

## Local HTTP API

The app starts Hex's local HTTP API (`LocalApi::start_for_app` in
`src/windows/rune.rs`) and writes its discovery file to
`%APPDATA%\hex\local-api.json`. It serves `POST /control` for the `hex`
commands, `POST /transcriptions`, `/dictations`, and
[`/streams`](streams.md). A request that carries audio sends that audio on to
ElevenLabs with your key; without a key it answers `409 api-key-missing`.

## macOS Parity

The target is product and visual parity with macOS, using native Windows
adapters where the operating-system APIs differ. A direct copy of AppKit,
CoreAudio event taps, `SMAppService`, Accessibility, or ScreenCaptureKit code
would not run on Windows; the Windows implementation must preserve the same
user contract through Win32, WASAPI, Rune (wgpu), the current-user startup
registry, UI Automation, and Windows capture APIs.

In the table, Implemented means the code exists and type-checks for Windows.
None of it has been run in this fork.

| Area | Windows status |
| --- | --- |
| Hold-to-dictate, Toggle mode, timestamped boundaries, paste | Implemented |
| Language, shortcut, and microphone changes while listening | Implemented; applied in place when nothing records |
| Double-tap lock and Paste Last Dictation | Implemented |
| Clipboard preservation and generation-safe restoration | Implemented |
| Recording tones and click-through recording/processing HUD | Implemented; a Rune overlay drawn on the GPU (`rune_hud.wgsl`), shared with Linux |
| History retention, list, detail, search, copy, delete, and clear | Implemented |
| Text replacements | Implemented with the shared phrase-boundary replacement engine |
| Application picker for per-app modes | Implemented; lists Start menu shortcuts, App Paths, and apps with an open window, with each executable's icon; "Choose a file…" opens the native dialog through `rune::FileDialogs` |
| Interface language | Implemented on every OS; English, Polish, Simplified and Traditional Chinese, Japanese, German, Spanish, Ukrainian, Portuguese (Brazil), French, and Russian |
| Resident tray and Launch at login | Implemented; the tray menu (shared with Linux) has the recording state, Show Hex, Paste last dictation, the meeting items while Meetings is on, Enable / Disable Hex, and Quit Hex |
| Application modes with per-mode corrections, OpenCode rewriting, and transformations | Implemented on the shared ordered processing policy; global and contextual profiles persist their own corrections, OpenCode model, reasoning variant, deadline, and ordered built-in/TypeScript transformations, while modes match by application name |
| Vocabulary | Sent to Scribe as keyterms while Labs › Vocabulary biasing is on and the boost is not Off; "sounds like" spellings are not sent |
| Release microphone while idle | Implemented; mutually exclusive with audio pre-roll, as documented in Settings |
| While-dictating audio control (mute other apps or pause media) | Implemented via WASAPI session volumes and GSMTC |
| Web-domain mode rules and browser context via UI Automation | Implemented; the page URL comes from the browser's UIA document element, bounded so a hung provider degrades to application-only context |
| First-run onboarding | Implemented; the shared first run asks for the ElevenLabs API key before anything else, and Hex starts listening only once setup is recorded |
| Meetings | Implemented (Labs, off by default): the microphone plus system audio through WASAPI loopback (`src/meeting/windows.rs`). Every chunk, the other participants' voices included, is sent to ElevenLabs. See [meetings](features/meetings.md) |
| Local HTTP API | Started by the app; see [Local HTTP API](#local-http-api) |
| Live session with whole-clip fallback | Implemented; a session is opened only with Labs › Live text on or Final text set to Live |

Settings live in `%APPDATA%\hex\settings.json`, in the format macOS and
Linux use.

The Windows window uses the shared design system
([design-system.md](design-system.md)): the same Rune panes as macOS and
Linux under the native title bar. Mode processing, OpenCode rewriting, and transformations run
through the same `dictation/processor` and `personal_commands` modules as
macOS, and Windows reaches the running OpenCode service the same way: its
registration file and loopback HTTP, never a CLI. Like macOS,
an unset OpenCode model tries the ranked recommended models before the
OpenCode default. An OpenCode service running inside WSL2 is found only with
Labs › OpenCode in WSL on: WSL2 forwards its loopback URL, and the lab adds
the default distribution's registration and a Linux workspace path for
`x-opencode-directory`. Annotate's capture sends use the same service: the
prompt's "Recorded at" line names the capture folder as a Linux `/mnt/…` path
while the summary header keeps the Windows path. Off, Hex reads only the
Windows registration and an OpenCode in WSL isn't found.
Remaining panes should be added only with their complete Windows behavior;
empty look-alike screens do not count as parity.

## Current Port Boundary

The Windows listener preserves physical press/release timestamps, keeps audio
capture off the transcription and paste worker, and restores the previous
clipboard only when no newer clipboard change supersedes it.

[`check-windows.yml`](../.github/workflows/check-windows.yml) runs on pull
requests and by hand. Its x64 job runs `cargo fmt --check`, Clippy, the tests
(`cargo nextest`), and a release build, then checks that `hex.exe` imports no
Visual C++ runtime DLL, does not import `vulkan-1.dll` at load time, and is a
GUI program. Its ARM64 job runs the tests and a release build. Each job
uploads its `hex.exe` as a workflow artifact; nothing is signed or published.
