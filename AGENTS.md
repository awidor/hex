<!-- tallow -->
# Hex Agent Guide

## This fork

This is a personal fork of [anomalyco/hex](https://github.com/anomalyco/hex)
(MIT) on branch `scribe`. Upstream Hex transcribes with speech models on the
computer; this fork has none. Every transcript comes from the ElevenLabs
Scribe cloud API, so audio leaves the computer on every dictation, meeting
chunk, capture narration, History "Transcribe again", local-API request, and
`hex model transcribe`. Nothing is transcribed on the computer and nothing
works without a network connection and an ElevenLabs API key. Never write
code, copy, or docs that say or imply otherwise.

- There is no release pipeline: no release service, installers, package
  feeds, signing, notarization, or website. The updater is permanently off
  (`has_update_channel` in `src/updater.rs`), because upstream's channels
  would replace this fork with upstream Hex. Installs come from a checkout
  (`scripts/install-linux.sh`).
- Work on `scribe`. Do not push, open pull requests, or merge unless asked.
  CI is `compile.yml`, `check-linux.yml`, `check-macos.yml`, and
  `check-windows.yml`, on pull requests and by hand only.
- Any change to what the window draws regenerates the goldens in the same
  change: run `HEX_UPDATE_GOLDENS=1 cargo test --locked --bin hex rune_ui`
  on Linux (the golden tests only run there), look at every changed image,
  and keep them with the change.
- Platforms: developed on Linux (KDE Wayland), where it compiles, its unit
  tests pass, and the app was installed with `scripts/install-linux.sh` and launched on KDE Plasma (Wayland)
  on 2026-11-11: the window opened, KDE's portal granted the shortcuts, the
  listener reported Enabled, and the running service transcribed speech clips
  sent to its local API with the saved key. A
  dictation spoken into the microphone and pasted was not exercised. The Windows code is type-checked by cross
  compilation only and has never been run. The macOS code was edited without
  being compiled or run. Say this plainly
  wherever platform support is described, and never claim a Windows or macOS
  behavior was tested here. Statements below about macOS and Windows describe
  what the code is written to do.
- Against the real ElevenLabs service only the command line and the two
  request paths were run (2026-11-11; see "Verified Against ElevenLabs" in
  `docs/features/transcription.md`): the batch request, the key check, and
  the realtime session, which exposed two things the fake servers could not
  (a key restricted to Speech to Text cannot read account endpoints, and
  ElevenLabs' own commit after about 35 s loses the words that follow).
  Meetings, captures, `/streams`, and the app as a whole were not run against
  it. `scripts/scribe-latency.py` times the batch and
  live paths with the requests Hex sends; it needs a key and every run is
  billed.

## Purpose

Build Hex as an observable voice dictation app whose transcription comes from
the ElevenLabs Scribe cloud API: Linux first (X11 and Wayland), with the macOS
and Windows (x64 and ARM64) code kept in the same tree. Keep the app native
Rust and keep consequential behavior explicit. Deterministic personal transcript
transformations live in the explicit TypeScript user config (`~/.config/hex/hex.config.ts`). User-facing
runtime settings persist in Application Support.

Every build shows General, Output, History, and Labs. Labs holds features
we're unsure about, especially ones people asked for that we built: **new or
user-requested features land in Labs, off by default, not on by default.**
Labs has no master switch; Settings › Labs lists every lab this OS has as a
card (name, description, toggle), each off by default, under one collapsible
header per group (Dictation, Output, History, Meetings, Annotate, Shortcuts,
Models; `labs::Feature::group`), plus Send feedback and Run setup again. Each feature has an audience: `Everyone` or `Dev` (dev, local, test, and
dev-channel builds only; a stable build never lists, accepts, or runs it).
A feature with its own pane joins the sidebar as a normal item while
on and is hidden while off; a behaviour-only feature is just its toggle. Today:
Meetings (beta), Voice Filter (Dev; locked on while its macOS driver is
installed, and a stable build with the driver keeps its pane for Uninstall
alone), Playground (Dev), Live text, Live chapters, Vocabulary biasing, Switch
language with keyboard layout (#216), Mouse shortcuts (#209, macOS), Also
pause the browser (#301, macOS), Share dictation shortcut (#176, Windows),
Terminal paste (Windows), and Terminal paste
shortcut (Windows, Linux). Externally contributed features land there too.
Annotate is a normal feature in every build, stable included, on macOS,
Linux, and Windows; only developer tools (`dev_build::is_dev_build()` with
`DEVELOPER_FEATURES_ENABLED`) stay gated to dev builds.

## Feature Map

[`docs/features/README.md`](docs/features/README.md) indexes existing user-facing
capabilities, entry points, checks, and gaps. Before designing, changing,
debugging, or reviewing a user-facing feature, read the index and relevant
feature files, then check their referenced implementation. The map is not a
substitute for source inspection or a second roadmap.

- Update the affected map in the same change when behavior, entry points,
  defaults, prerequisites, platform availability, recovery, or verification
  coverage changes. Do not churn unrelated maps for internal-only refactors.
- Organize by user capability, not Rust module or UI pane; link shared flows and
  recovery rather than duplicating their descriptions. Keep the index current
  when adding or splitting a feature file.
- Prefer small action/outcome diagrams in `ts` fences with Unicode tree
  characters, not tables. Preserve stable behavior IDs and keep source/check
  references and proof limits beside the behavior they explain.
- Distinguish implemented behavior, available checks, and observed verification.
  A test's existence is not a passing run, and a preview is not native end-to-end
  proof. Record missing coverage and known defects explicitly; never rewrite an
  expected outcome merely to hide a regression. A map does not authorize live
  device tests. Keep future work in `ROADMAP.md`.

## Architecture

- `audio`: `cpal` device enumeration, input resolution (`--device`, the saved
  microphone, else the system default, never Hex Microphone), the CoreAudio
  default-input and device-list listeners, timestamped mono float PCM
  delivery, live selection, and bounded stream recovery. `audio/fixture`:
  with `HEX_AUDIO_FIXTURE=<wav>` every microphone open plays that file
  instead (10 ms chunks at real-time pace, from its start at each open,
  looping) and no device is enumerated or opened, so a scripted run drives
  the real capture, stream, and output pipeline without recording anyone
  (the file's audio is still sent to ElevenLabs when a key is set); the
  microphone permission then reads as ready.
- `suppression`: the macOS event taps, shortcut suppression, and the
  configurable dictation-hotkey state machine.
- `keyboard`: active-layout key resolution and balanced synthetic shortcuts.
- `keyboard_input_sources`, `transcription/keyboard_model_switching`: keyboard input-source
  observation (macOS TIS, Windows foreground-window layout, X11 XKB groups;
  Wayland has no general protocol) and the shared debounced policy that turns a
  mapped source into a transcription-language choice (Labs › Switch language
  with keyboard layout). There is one model, so only the language changes.
- `transcription/app_model_switching`: the matching debounced policy for a
  dictation mode's optional transcription language (Labs › Switch language
  with app, macOS and Windows): the frontmost app's mode choice is applied
  once idle. Both OSes match by application,
  never by website, and Hex's own window (or no app in front) is ignored;
  Linux has no host for it.
- `dictation`, `dictation/diagnostics`: warm pre-roll, growable capture, 16 kHz
  resampling for transcription, and owner-only diagnostic WAV retention when
  `HEX_RETAIN_DICTATION_AUDIO` is set.
- `transcription/pause_segmenter`: pure 16 kHz pause detection that cuts meeting audio into
  rolling transcription chunks, and the level test (`below_speech_level`,
  -55 dBFS) that keeps audio without speech from being sent.
- `dictation/audio`: the authoritative microphone timeline, lazy stream
  lifecycle, recording owner, exact shortcut boundaries, recovery handoff,
  disposable bounded command audio projection, and the bounded capture sink
  (`open_capture_sink`/`close_capture_sink`) that copies the same timeline to
  a Scrawl capture.
- `audio/input_level`: the latest microphone peak as one atomic, stored per chunk by
  each OS's audio owner and read by the window's level bars (General and the
  practice dictation); a stale peak reads as no audio.
- `audio/recording_environment`: serialized RAII ownership of idle-sleep prevention,
  output muting and lowering, and supported media-player pause/resume
  behavior on macOS, each held from the first dictation or capture session
  that wants it to the last.
- `audio/output_ducking`: the shared Lower audio ramp (relative, channel-preserving,
  respecting manual changes, rolled back on failure) over each OS's volume
  adapter: CoreAudio output channels, the Linux default sink's channels
  (`pactl`, else `wpctl`), and other apps' WASAPI session volumes.
- `audio/idle_sleep`: idle-sleep prevention while a dictation records: an IOKit
  assertion on macOS, a logind `idle:sleep` inhibitor (only `idle` where
  polkit refuses a process outside the seat session) on Linux, and a
  system-required power request on Windows.
- `dictation/processor`: the one OpenCode client and ordered processing policy
  for macOS, Windows, and Linux: context-selected corrections, deadline-bounded
  OpenCode rewrite profiles with corrected-transcript fallback, the ranked
  model catalog, and the transformation stage, over `desktop/opencode_service`.
- `desktop/opencode_service`: the only way Hex reaches OpenCode, on every OS. It reads
  the running service's owner-only registration
  (`${XDG_STATE_HOME:-~/.local/state}/opencode/service.json`, then other
  channels' `service-*.json`), confirms the registered PID through
  `/api/info`, and talks authenticated loopback HTTP over `TcpStream` with
  cancellation and deadlines. It never runs an OpenCode executable or curl;
  without a service, features say "OpenCode isn't running".
- `macos_dictation`: the macOS dictation worker that hands clips to the
  transcription engine and runs bounded, ordered processing, output, paste,
  last-result, and meeting-delta work, plus `transcribe_with_words`, the
  capture path's word-timestamped foreground requests on the same engine.
- `transcription`: speech to text: the engine thread, the Scribe transcriber
  it owns, and the model-and-language selection every caller shares. Nothing
  here runs a speech model; every transcript is a request to ElevenLabs.
- `transcription/scribe`: `ScribeTranscriber`, the engine's model. It holds
  no weights: a selection is a language, a whole clip is one HTTPS request,
  and a live stream is one WebSocket session. Also `ScribeSettings` (the
  `scribe` object in `settings.json`: `no_verbatim`, `numbers_format`,
  `final_text`), `stream_wanted` (a dictation opens a live session only when
  its text is shown or pasted, and a key is set), the base URL
  (`HEX_ELEVENLABS_URL`, else `https://api.elevenlabs.io`), the connection
  prewarm when a capture starts (a `HEAD` request: no audio, no key),
  `WarmScribe` for callers that pass a selection with each request (the
  local API, History), and `check_key` (`GET /v1/user`: no audio, no quota).
- `transcription/scribe/client`: the batch request, `POST /v1/speech-to-text`
  with model `scribe_v3`. The 16 kHz mono clip goes up as raw 16-bit PCM
  (`file_format=pcm_s16le_16`), so nothing is encoded or written to disk.
  Keyterms the API would refuse are skipped, word times are asked for only
  when a caller needs them, a failure that may pass is retried once after
  400 ms, and segments are built from Scribe's words (a 1 s gap starts one).
- `transcription/scribe/live`: one realtime session (`scribe_v3_live`,
  `/v1/speech-to-text/realtime` over WebSocket) as the engine's live stream:
  audio goes up as it is recorded, partial text comes back, and a commit at
  release returns the final text. One thread owns the WebSocket; the engine
  thread only queues audio and reads the latest text, so connecting or a
  dead network never holds a capture, and a failed session reports itself on
  the next feed or at the finish. It connects directly: the proxy `net` uses
  for HTTPS is not applied to the WebSocket.
- `transcription/scribe/key`: the ElevenLabs API key, read for each request
  from `ELEVENLABS_API_KEY`, else from the file `elevenlabs.key` beside
  `settings.json` (mode 0600 on Unix; on Windows it inherits the per-user
  data folder's ACL). Saving one applies to the next dictation without a
  restart. Never logged, never in `settings.json`, never part of an error,
  and shown only by its last four characters.
- `transcription/scribe/error`: `ScribeError`, why a request failed in words
  a person can act on, which failures are retried at once (`retryable`), and
  which keep the clip for later (`worth_retrying_later`). It never carries
  the key, the audio, or the transcript.
- `transcription/engine`: the thread that owns the Scribe transcriber on
  every OS and sends every request in order: live sessions fed during
  capture, their finish, whole-clip requests, and language changes
  (`reload`). The capture owns its `LiveStream` and hands it to the finished
  clip; finishing returns a `PendingTranscript` whose wait enforces the
  stream deadline and the job deadline. Every finish carries the complete
  clip, and the batch model transcribes it unless the live session's text is
  the one asked for; in this module "offline" names that whole-clip path,
  not the network. Foreground work (a capture's word chunk, a `/streams`
  window) waits for an open stream instead of interrupting it, and
  background work (meeting chunks, local API host audio) yields to any
  dictation request sent before it starts. The listener shares its engine
  (`TranscriptionEngine::share`) on every OS;
  `transcription::engine::dictation_busy` is the press-to-paste capture state
  on macOS, and on Linux and Windows only the engine's dictation streams and
  finishes. `engine/streaming` drives a stream and caps the pauses it hears.
- `transcription/models`, `transcription/languages`: what a transcription is
  asked for: the one model id (`scribe_v3`) and a language (`auto`, or an
  ISO 639-1 or 639-3 code), and the languages Settings names. There is
  nothing to download or install. The type and wire names stay stable so
  `settings.json`, the CLI, and the local API keep one vocabulary; any saved
  model name loads as `scribe_v3` with its language kept, and a request that
  names another model is refused.
- `transcription/failed_clips`: the audio of dictations whose transcription
  failed for a reason the same clip could survive, kept as owner-only 16 kHz
  WAVs in `failed-dictations/` beside `settings.json` (at most 20 clips, at
  most 7 days, pruned when one is added). The failure names the file and
  `hex model transcribe <file>`.
- `transcription/cancel`: the cancel flag a stream or job's waiter raises and
  the engine checks between steps.
- `transcription/service`: bounded host-audio admission, hostile WAV validation,
  normalization, cancellation, and transcription. In the app, clips up to
  30 s go through the dictation engine's transcriber when it holds the
  requested selection, so host audio queues behind dictation; otherwise the
  service uses its own (`scribe::WarmScribe`).
- `transcription/stream`: live `/streams` transcription of another app's
  pushed audio through the dictation engine: pause-cut windows fed through
  yielding live sessions that any dictation request closes, a bounded
  backlog held while dictation is busy, whole-clip fallback windows, the
  lease, and one live stream at a time. The pushed audio is sent to
  ElevenLabs like any other.
- `local_api`: authenticated loopback discovery or direct-child endpoint
  handoff, bounded HTTP parsing, the raw transcription route
  (`POST /transcriptions`), the `/streams` routes (contract:
  [`docs/streams.md`](docs/streams.md)), plus the always-on authenticated
  `POST /control`, and `GET /profile/vitals` (process footprint and CPU from
  Rune's `procwatch`, sampled by `macos_rune`; debug builds also count the
  heap). There are no model routes: nothing is listed, downloaded, or
  prepared. Audio another app sends here goes on to ElevenLabs, so
  `local_api/key` refuses a request that would send audio at its headers
  while no API key is set, and `/capabilities` reports `apiKeySet`. The running
  app serves it on every OS, as does `hex service [--embedded]`; the discovery
  file is `local-api.json` in the data folder (mode 0600 on Unix, the per-user
  `%APPDATA%` ACL on Windows). `/dictations` (`local_api::dictation`) runs on
  the macOS recognition loop and the Linux and Windows listeners, sharing the
  protocol, owner tokens, lease, and levels in `dictation::service`; a start
  with `"partials": true` also streams the live text on
  `/dictations/{id}/partials` where the listener streams the recording
  (Linux and Windows, `dictationPartials`). `hex service` answers
  `service-capture-unavailable`, and off macOS `/profile/vitals` answers 503.
  `/control` carries the `hex` control commands on every OS (`control`).
- `control`: the release `hex` control surface on every OS: the versioned
  `ControlRequest`/`ControlReply` protocol, the settings registry behind
  `hex settings` (one dotted key per persisted setting, with the window's
  rules and the capability each needs), and the one CLI tree and client
  (`control::cli`), which each OS's `main` flattens beside its own commands.
  `control/cli/key` is `hex key set|status|clear|check`, which runs in the
  calling process and never prints the key or puts it in a reply;
  `control/cli/model` is `hex model list|use|status|check|transcribe` for the
  one model and its language.
  The transport on every OS is the local API's `/control`.
  `control::host` carries commands out on Linux and Windows through a thin
  adapter (`linux::app::LinuxControl`, `windows::rune::WindowsControl`);
  `macos_rune::control` is macOS's host. What an OS can't do answers
  `unsupported` at runtime. Contract: `docs/cli.md`.
- `control/developer_control`: the typed debug-only command/reply protocol the local
  API uses to drive the running app from `hex dev`.
- `paste`: clipboard insertion, continuation joins, and generation-safe
  clipboard restoration.
- `accessibility`: bounded focused-window and selected-text capture through
  Accessibility without touching the clipboard.
- `recognition`: the macOS listener coordinator for hotkey and programmatic
  dictation controls, workers, and observations. It does not own the
  authoritative microphone timeline.
- `personal_commands`: the user's `hex.config.ts` (or `.js`) in
  `~/.config/hex`, run in-process by embedded QuickJS (`rquickjs`) on one
  worker thread: TypeScript erased to same-position JavaScript by
  `personal_commands/typescript` (oxc's parser), the `@hex/commands` module
  and managed `hex.d.ts` typings, watch-reloaded transformation calls with
  per-transformation timeouts and a runtime memory limit, status snapshots,
  and `hex commands init|check`. Its host APIs are thin native bindings:
  `fetch` through `net` (`personal_commands/fetch`),
  `node:child_process` `exec`/`execFile` over `std::process` with a process
  group or job object per run (`personal_commands/process`), and timers,
  settled through QuickJS's job queue by `personal_commands/io`. They work
  only once the person allows the config's content hash (`personal_commands/trust`,
  the Output pane's trust prompt). The API is documented by the typings and
  guide `hex commands init` writes into the workspace
  (`src/personal_commands/hex.d.ts`, `src/personal_commands/AGENTS.md`);
  the window still links to upstream's page,
  https://anoma.ly/hex/docs/transforms, because this fork has no website.
  macOS, Windows, and Linux share it.
- `desktop/background_process`: the one constructor for child processes Hex starts for
  its own work (OpenCode CLI, curl, PowerShell); on Windows they
  never open a console window.
- `dictation/text_replacements`: phrase-boundary-aware compilation of configured text
  replacements applied longest-match-first to transcripts.
- `dictation/config`: dictation profiles.
- `dictation/context`: the foreground context snapshot and selectors for modes,
  plus native macOS capture. Browser host is the domain concept. Only while a
  website mode, an OpenCode rewrite, or a script transformation is set up,
  macOS reads the front tab of a table of
  scriptable browsers (Safari and Chromium
  dictionaries) through AppleScript and any other app's focused AXWebArea
  `AXURL`, and the front window's title only while a script
  transformation is set up (`reads_window_title`). The cheap 500 ms poll never
  runs AppleScript: a scriptable browser's page is cached per frontmost pid
  (so switching away and back reads it again) and read once more when a
  capture is real, not on every press: the shortcut held past its hold
  threshold, a finish that lands earlier, or a control start asks for one
  refresh, while a press a chord discards before that threshold reads nothing
  (holding past the threshold first still reads the page once);
  `windows/context` reads any browser through UI Automation and
  `linux/atspi` through AT-SPI.
- `application_catalog`: cached installed-application discovery for the Modes
  picker, one module per OS: macOS bundles, Windows executables from Start menu
  shortcuts, App Paths, and open windows, Linux `.desktop` entries.
- `app_settings`: the one persisted `settings.json` on every OS and its live
  runtime projection for hotkeys, microphone and transcription
  selection (`transcription`: the model id and language), the Scribe options
  (`scribe`), recording behavior, processing, sound volume, indicator position,
  interface language, and Dock policy. It records one-time changes in
  `migrations`. The API key is never stored here.
- `desktop/login_item`: the native `SMAppService.mainAppService` adapter and its worker
  thread. macOS owns registration state; it is deliberately not duplicated in
  `settings.json`.
- `policy`: macOS managed-preference feature locks in `anomaly.hex`
  (`DisableMeetings`, `DisableAnnotate`, `DisableOpenCodeProcessing`,
  `DisableTransforms`, `DisableLabs`, `RestrictTransformsToLoopback`), forced
  values only. Locked switches read off at load and keep their saved values on
  save; list-driven work checks the policy where it starts.
- `app_paths`: the Application Support owner (`hex` on every OS) for runtime
  logs and shared state. The logs folder and its files are owner-only on
  Unix (0700/0600, tightened when an older Hex left them open).
- `desktop/onboarding`, `desktop/permission_guide`: first-run setup state: the
  ElevenLabs API key every OS needs before it can transcribe, required
  permission health on macOS, Windows' microphone switch, and the native
  floating System Settings guide. `SetupStatus::transcription_model` keeps
  its name and now means "an API key is set".
- `audio/hex_microphone`, `audio/voice_filter`: the experimental virtual microphone
  (`Hex Microphone`: a CoreAudio driver on macOS, a PipeWire virtual source
  fed through `pw-cat` on Linux, none on Windows, which would need a signed
  kernel driver), the lock-free capture-to-sink ring buffer, real-time
  voice-filter DSP presets, and the offline WAV renderer behind
  `hex dev voice-filter render`.
- `linux`, `linux/app`, `linux/dictation`, `linux/paste`:
  the Linux beta CLI (including the `hex dictation` and
  `hex paste-last` keybinding commands and launch forwarding), the one Linux
  process (host thread, window on demand through one seam, Quit), the hotkey
  capture-transcribe-paste loop with Paste Last,
  and paste.
- `linux/service` also adopts the session's Wayland display at `hex service`
  start when the unit has none, or has only Xwayland's `DISPLAY` on a Wayland
  login (Hyprland imports nothing into systemd; #756): with `DISPLAY` set, an
  explicit `wayland` `XDG_SESSION_TYPE` (Hex's, else the user manager's) takes
  the manager's live `WAYLAND_DISPLAY`, else the newest answering
  `$XDG_RUNTIME_DIR/wayland-*` socket the user owns. Any other explicit type
  (`x11`, `tty`, `mir`, …) keeps its X display even when the manager still
  names a nested compositor or another login's display, and only an absent
  type adopts the manager's live display. Launchers (`hex`, `hex start`'s app,
  `hex restart`) make the same decision before they compute or write
  `session.env`, so both sides agree on the session; an explicit non-Wayland
  type skips the user-manager read entirely. `linux/rune_hud`'s `limitation()`
  names the HUD's failure: no display, no layer-shell, or Rune's overlay error.
- `linux/gnome_shortcuts`: on GNOME without a GlobalShortcuts portal
  (GNOME 46, #781), Hex's own custom keybindings under
  `…/media-keys/custom-keybindings/hex-*/` via `gsettings`
  (`linux/gsettings`): `hex dictation toggle`, `hex paste-last`,
  `hex annotate toggle`. Opt-in (`gnome_keyboard_shortcuts`), written while Hex
  listens and removed when it's disabled; an existing entry keeps the key the
  user set in GNOME, and other custom shortcuts are never touched.
- `linux/clipboard`: the clipboard a Linux paste restores: every format of the
  CLIPBOARD selection read within a size cap and deadline, then served again
  from Hex (ext-data-control, else wlr-data-control on Wayland, its own selection owner with INCR on
  X11) until another client takes it. Without either protocol (GNOME), only
  the Labs opt-in `gnome-clipboard-restore` reads and restores through an
  already running XWayland's selection.
- `linux/input`: the one shortcut state machine (hold, double-tap lock, toggle,
  double-tap only, Paste Last, and `hex` commands) and the
  monitor that feeds it from X11 key grabs, the Wayland GlobalShortcuts portal,
  or, without the portal, the `hex` commands alone.
- `linux/portal`: Hex's Linux app id `anomaly.hex` (reverse-DNS, #579; the
  windows' Wayland app_id and X11 WM_CLASS, and the `anomaly.hex.desktop`
  entry installs ship), the move of a managed pre-#579 `hex.desktop` to that
  name, the portal bus (a private zbus connection, `Register("anomaly.hex")`
  after writing a managed fallback entry for a loose binary, requests and
  sessions) and the GlobalShortcuts client on it: one session binding all of
  Hex's shortcuts once, granted-trigger descriptions, ShortcutsChanged, ConfigureShortcuts, and
  session-closed reporting.
- `linux/remote_desktop`: the RemoteDesktop portal client Wayland paste uses
  without the virtual keyboard protocol: a keyboard-only session with a
  persisted owner-only restore token, and Ctrl+V as keysyms.
- `linux/context`: the focused application for modes: the X11 active
  window's process, or on Wayland the activated `zwlr_foreign_toplevel`
  window's app ID named by its `.desktop` entry (none on GNOME and KDE).
- `linux/atspi`: the focused window's page address through AT-SPI on X11 and
  Wayland, matched by title and process and bounded by a deadline. Chromium
  exposes pages only with renderer accessibility on.
- `linux/audio_control`: While-dictating mute through `wpctl`/`pactl`, Lower
  audio through `audio/output_ducking`, and MPRIS pause over D-Bus (zbus),
  restoring only what it changed, plus idle-sleep prevention for dictations.
- `linux/input_route`: System default's Bluetooth avoidance on Linux. ALSA's
  default cannot name another PipeWire source, so Hex marks its ALSA streams
  (`PIPEWIRE_ALSA`: `state.restore-target = false` and its PID, set on the
  main thread at startup) and moves them off a Bluetooth default source to
  the built-in (PCI) input, else the first other hardware input.
- `linux/session`: display-backend selection matching winit's (Rune's)
  nonempty `WAYLAND_DISPLAY` rule, independent of persisted preferences.
- `linux/service`: the single-instance lock and owner-only, same-user Unix
  socket that forwards `Activate` (with the activation token), shortcut
  commands, `Status`, and `Quit` to the one Hex process, plus the per-user
  systemd lifecycle (`Type=notify` readiness, `session.env`, start/restart).
- `linux/rune_hud`: the Linux side of `rune_hud`: `LinuxIndicator` turns the
  listener's capture, meter, and pending-output reports into HUD events and
  never waits on the HUD; it shows once the window's app runs, whether or not
  the window is open (layer-shell on Wayland, override-redirect on X11, and
  override-redirect on XWayland when Wayland has no layer-shell).
- `rune_hud`: the dictation HUD on every OS: a click-through Rune overlay
  whose compositor steps `dictation/indicator`'s model on Rune's frame clock
  (the display's refresh) and draws Dot scope on the GPU with
  `rune_hud.wgsl`. `rune_hud/macos` places it on the pointer's screen
  through `rune_hud/hud_placement` (below the menu bar, in it, or out of the notch);
  Linux and Windows place it at the top or bottom edge.
  `rune_hud_frames` renders a fixed dictation through the shaders offscreen.
- `linux/tray`: the optional tray icon, a `rune::Tray` (StatusNotifierItem
  with a dbusmenu built by `desktop/tray_menu`). It registers whenever a tray host
  (`org.kde.StatusNotifierWatcher`) appears; without one Hex runs unchanged.
- `desktop/tray_menu`: the tray menu Linux and Windows share (recording state, Show
  Hex, Paste last dictation, the meeting items, the update item, which never
  shows because the updater is off, Enable / Disable Hex, Quit Hex). It lists
  no models. Each tray maps a `TrayCommand` onto its own runtime.
- `desktop/tray_icon`: the tray mark Linux and Windows share: the white Hex
  mark, drawn black on a light panel or taskbar, where white would vanish.
- `linux/login_item`: Launch at login on Linux, the managed XDG autostart
  entry the installer also writes; the file is the only state.
- `net`: Hex's own HTTPS (Scribe's batch requests and key check,
  transformation `fetch`): ureq over the platform's TLS and trusted roots, the proxy the
  computer uses (environment variables everywhere, then Windows' WinHTTP
  settings with PAC/WPAD or macOS' CFNetwork settings), bounded resumable
  ranged downloads, and failures named for a person.
  Use it for any new outbound HTTPS request; don't shell out to curl. Scribe's
  realtime WebSocket does not go through it.
- `updater`: upstream's in-app updater, permanently off in this fork:
  `has_update_channel` returns false, so every build is treated as a source
  build and never checks, downloads, or installs anything. The code under it
  (`updater/protocol`, `updater/transport`, `updater/{linux,macos,windows}`,
  the test vectors in `tests/fixtures/release-protocol`) is the client of
  upstream's release service
  ([`docs/plans/release-service.md`](docs/plans/release-service.md)); it
  stays in the tree and its tests still run, but no build acts on it.
- `windows`, `windows/app`, `windows/rune`, `windows/dictation`,
  `windows/input`, `windows/paste`, `windows/context`, `windows/audio_control`, `windows/login_item`: the Windows
  beta CLI, the runtime host (`WindowsDesktopHost`: listener, transcription
  language, OpenCode catalog, transformations, updater) and single-instance
  claim, the tray app on `rune::App` (tray, window icon, and runtime thread;
  `rune_ui/windows_host.rs` is the window's host), the WASAPI dictation loop,
  low-level keyboard hook, clipboard paste, UI Automation context, While
  dictating (mute, lower, or pause other apps' audio) with idle-sleep
  prevention, and the login item.
- `dictation/live_text`: Labs' live text of the recording in
  progress: the recording's `Publisher` shows it and its drop clears it, and
  each OS's host installs `rune_ui::live_text`, one click-through line just
  past the HUD, as a watcher, placed where `rune_hud` last showed the HUD
  (`rune_hud::live_text_place`).
- `dictation/modes`: the mode runtime the Windows and Linux listeners share:
  mode selection by application and website, then global replacements, mode
  corrections, OpenCode rewriting, and transformations, and the History entry.
- `desktop/i18n`: interface translations keyed by the English source string,
  `tr`, `tr_fill`, and `tr_named`. A test fails if any string the window
  translates lacks a language.
- `history`: the owner-only bounded retained-dictation store: retention
  windows with hard entry and byte caps, atomic crash-safe persistence, and
  search. Text and bounded metadata; audio only with Labs › Keep recordings
  (`history/recordings`: one owner-only WAV per entry; Transcribe again
  sends that audio to ElevenLabs).
- `control/transcript_cli`: read-only release commands `hex history` and
  `hex meetings list|show|search` on every OS, versioned JSON projections and
  filters.
  Reads history snapshots without opening the writable store, and shares
  meeting selection, search, and Markdown with `meeting::query` / MCP.
- `desktop/events`, `control/dashboard`: bounded asynchronous append-only NDJSON observations
  and bounded incremental reading; the debug `dashboard` TUI and the Activity
  pane are read-only projections. Context events name the foreground app,
  written only when it changes, never a page address or window title, and no
  event holds dictated or transcribed text: History is the only place Hex
  keeps what was said.
- `desktop/activity`: the shared listener, device, session, and latest-failure
  projection over `EventReader`. Transcript rows read retained events directly.
- `desktop/host`: the in-process listener, transcription, shortcut, and update
  snapshot-and-action types shared by the Linux and Windows hosts, the window,
  and `hex status`.
- `rune_ui`: the one window on macOS, Windows, and Linux, on Rune with
  `rune-kit` and the OpenCode theme: General, Output, and History, then in dev
  builds the opt-in Meetings once on (each feature owns its
  settings, rarer ones in the pane's Advanced fold), and under Labs (`labs`:
  the Labs switch and the opt-ins) Annotate and the Voice Filter, plus in
  Playground (`⇧⌘D`) HUD Lab's live Dot scope sliders (`hud_stories`),
  the Sounds sheet (its stage replays `rune_hud`'s HUD, drawn by the
  window's compositor with `rune_hud.wgsl`), the onboarding pages, the macOS
  meeting capsules, the `--snap`
  fixture scenes, and the macOS `hex://story/…` playground links
  (`story_link`, `hex-test://` in test builds). `platform::Capabilities` decides which panes and rows exist (and, through `rune_ui::capabilities`, which `hex settings` keys act and what `hex status` reports); every difference between the OSes names a reason in `platform::DIFFERENCES` (an OS pattern, impossible, or pending), which `parity_tests` enforce;
  `rune_ui/linux_host.rs`, `windows_host.rs`, and `macos_host.rs` connect it to
  each OS's host. Keep shared UI in the pane modules and only OS behavior in
  the hosts.
- `rune_ui/general/transcription`, `rune_ui/api_key`, `rune_ui/speech_language`:
  Settings › General › Transcription: the ElevenLabs API key (typed into a
  field, checked with ElevenLabs off the UI thread, shown only by its last
  four characters), the language dictation is transcribed in, which text a
  dictation pastes (Final text: Batch or Live), and how Scribe writes it
  (filler words, numbers). The first run's key step shares `api_key`.
  `speech_language` is the language choice an input source's mapping and a
  dictation mode offer.
- `rune_ui/accessibility_tests`: the window's accessibility tree, which Rune
  builds with AccessKit for screen readers and Voice Control
  (`docs/features/accessibility.md`). Rows, choices, lists, and
  icon-only buttons get their names, values, states, and places in Hex's
  shared pieces (`controls`, `lists`, `list_detail`, `sidebar`); names go
  through `tr`. Every control in every scene must have a name.
- `desktop/opencode_catalog`: the OpenCode model catalog as the window shows it:
  loading off the UI thread, the missing/failing notice, model names, search,
  and thinking variants.
- `macos_rune/status_item`: the persistent macOS menu-bar owner for Settings, Paste Last
  Dictation, meeting recording state and Stop & Transcribe, and
  orderly application shutdown.
- `macos_rune`: the macOS app as one menu-bar process on
  `rune::App`: the status item, the updater, main menu, Dock reopen Apple event,
  and HUD on the main AppKit loop, and a runtime thread that saves the
  transcription language, starts the listener once the permissions are
  granted and an API key is set, and publishes the window's state.
  `rune_ui/macos_host.rs` is the window's host and the panes' `NativeHost`.
- `rune_hud/hud_placement`: pure geometry for where a HUD sits on its screen (below
  the menu bar, in it, or as an island grown out of the notch), shared by
  the dictation HUD and, later, the capture capsule.
- `dictation/indicator`, `rune_hud/dot_scope`, `rune_hud/spring`: the HUD's events, phases,
  jobs, and Dot scope (the one HUD look: a plate of dots lit by a braided,
  auto-levelled oscilloscope trace), the same on every OS and drawn by
  `rune_hud`; `rune_hud/spring` holds the frame-rate-independent springs.
- `meeting`: explicit capture through `meeting/capture`, owner-only WAV and transcript
  artifacts, final transcript publication, the user-facing Markdown copy, and
  recovery, on every OS. Both tracks' audio, the other participants'
  included, is sent to ElevenLabs for transcription. Its `os` adapter (`meeting/macos`, `meeting/linux`,
  `meeting/windows`) owns the system audio source and its consent, idle-sleep
  prevention while recording, and reveal and Trash.
- `meeting/capture`: independent self-healing microphone (cpal) and system
  audio sources (macOS: Core Audio process tap, ScreenCaptureKit fallback;
  Linux: `pw-record` of the default output's monitor; Windows: WASAPI
  loopback); one failing never stops the other, and resumed audio is padded
  onto the timeline.
- `meeting/runtime`: the Linux and Windows apps' meeting controller thread,
  capsules, recording state, and tray items (macOS runs the same controller
  from `macos_rune`).
- `durable`: crash-safe writes on every OS (`write`, `sync_file`,
  `sync_directory`). Windows flushes only through a writable handle and
  cannot open a directory, so file syncs and directory syncs go through it.
- `meeting/generator`: who writes notes and live chapters: Apple
  Intelligence on a Mac that has it, else OpenCode when
  `meetings.summarize_with_opencode` is on (off by default; it sends the
  transcript excerpt).
- `transcription/background_priority`: the priority background
  transcription work (meeting chunks, notes, and chapters) runs at, so it
  never competes with dictation. Whether dictation is capturing or
  transcribing, so that work and the local API yield, is
  `transcription::engine::dictation_busy`.
- `meeting/transcript`: streaming 16 kHz resampling, short pause-aware
  overlapping chunks sent to Scribe as background work on the dictation
  engine (a process without the engine, such as the CLI, uses its own
  transcriber), with chunks that never reach speech level not sent at all,
  seam deduplication, echo suppression, the rolling in-call transcript and its
  `TranscriptReader`, and Markdown rendering.
- `meeting/echo`: 20 ms energy envelopes of both tracks and the signal-level
  bleed/local classification of microphone segments.
- `meeting/notes`: a bounded provisional title while recording, then one
  post-publication call over a bounded transcript excerpt for the final title
  and a two-sentence summary, with deadline, manifest storage, and
  marker-delimited in-place Markdown updates.
- `meeting/chapters`: live chapters (Labs): one pass of the notes writer
  (`meeting/generator`) per ~3 minutes of rolling transcript while recording, with
  Rust deciding topic changes, the per-boundary fix-up after publication,
  chapter-extended titles that never touch a user-typed one, and section
  Markdown for Copy section.
- `meeting/query`: read-only meeting queries for agents (`meeting::query`):
  lock-aware listing, in-memory BM25 full-text search with stemming and fuzzy
  matching, transcript paging, stored notes, and Markdown rendering. It never
  writes to the meetings folder.
- `meeting/mcp`: the thin `rmcp` stdio adapter behind `hex mcp`, exposing
  `meeting/query` as read-only tools and `hex-meeting://` resources.
- `apple_intelligence`: the weak-linked FoundationModels Swift bridge:
  availability, context size, and guided-generation notes at utility priority.
- `microphone_activity`: permission-light observation of which apps use the
  microphone: CoreAudio processes (macOS), a `pw-dump --monitor` of PipeWire
  capture streams (Linux), audio sessions on capture devices (Windows). No
  samples are read.
- `meeting/detection`: pure provider classification, debounce, per-call snooze,
  and call-ended presence.
- `calendar`: calendar meetings on every OS as a pure model (which events
  count as meetings, call-link parsing, the wall-clock reminder scheduler),
  plus the macOS EventKit bridge (`calendar.swift`): access, the change
  counter, bounded event reads, and the Settings snapshot.
- `meeting/watcher`: the meeting controller loop: call detection, offers,
  calendar reminders, recording workers, call-ended presence, notes, and the
  menu-bar meeting state. The capsules that show its events are
  `rune_ui::meeting_capsules`.
- `desktop/file_dialog`: the one native open dialog for a file or folder (Modes'
  "Choose a file…", the macOS meetings folder) through `rune::FileDialogs` on
  its own thread. Answers on a channel; cancel and a dialog that never opened
  read as nothing chosen.
- `labs`: the Labs feature registry (`labs::Feature`: name, settings key,
  description, audience, group), the runtime gates (`labs::live_text`, `live_chapters`,
  `playground`, `vocabulary_biasing`, `mouse_shortcuts`), and
  `hex labs [on|off <feature>]`. `dev_build`: whether this is a dev, local, test,
  or dev-channel build.
- `capture_host`: Scrawl's capture engine (the `scrawl-capture` crate in
  `crates/capture`, copied from Scrawl's `engine/split` at `e351ddb` and
  kept out of the workspace; `scripts/check` names it on macOS) hosted in
  Hex as Annotate (its user-facing name; code keeps `capture`;
  `docs/plans/capture-in-hex.md`),
  compiled as `cfg(hex_capture)` into every macOS, Linux, and Windows build
  (`scripts/build-app.sh` bundles and signs `scrawl-helper` beside `hex`).
  Everything here is shared by every OS; the engine's own platform
  adapter (`crates/capture/src/capture/{macos,linux,windows}.rs`) is the seam for
  the screen, window and area source, cursor, permission flow, and encoder.
  The Linux adapter (`crates/capture/src/linux/`) asks the ScreenCast
  portal over `zbus` (a screen keeps the portal's restore token at
  `<support>/screencast-token`; a window is picked each time), reads the
  PipeWire stream with `libpipewire-0.3` loaded at run time, imports each
  frame as a linear DMA-BUF into VA-API (`libva`, also loaded at run time),
  scales and encodes H.264 on the GPU with Hex-written SPS/PPS/slice
  headers, and muxes and reads movies with the `ffmpeg`/`ffprobe`
  executables; without VA-API it copies frames and encodes with x264.
  Host-side, Linux differs only in where the audio comes from
  (`capture_host::ListenerTimeline`, fed by `linux/dictation`'s
  microphone), "While dictating" (`Environment`: an `AudioSuppressor` per
  capture), words (`ScribeTranscriber::transcribe_words` as one foreground
  request on the listener's engine), the clipboard and paste (`linux/paste`),
  and the shortcut (`linux/input`'s `Shortcut::Capture`). Windows records
  with Windows.Graphics.Capture (`crates/capture/src/win/media.rs`) and
  shares Linux's host side (`ListenerTimeline` fed by `windows/dictation`,
  the same words path) except "While dictating" (`Environment`: a
  `windows::audio_control::AudioSuppressor` per capture), paste and copy
  (the engine's own Ctrl+V and clipboard), and input: `windows/input`'s
  hooks give the Annotate key (`HotkeyAction::Capture`/`CaptureUp`), Escape
  while drawing, and, while a capture records, every pointer move, click,
  wheel turn, and modifier change in physical pixels
  (`capture_host::report_input`, as the macOS tap does). The overlays lay
  out in the primary display's points, so the picker and ink convert with
  `capture_host::pixels_per_point`. Hex's `Host`,
  `ConfigSource`, and `Transcriber` (a capture's narration is sent to
  ElevenLabs through the dictation engine, with word times) for the engine, the
  listener-owned `CaptureHost` behind `hex capture` and the capture shortcut,
  its 16 kHz gap-padded timeline handed to the engine as `AudioIn::External`,
  the tap's pointer input forwarded to `Capture::input()`, Scrawl.app
  coexistence, the retention thread, and the `CaptureSnapshot` every view
  reads (`capture_host::snapshot`/`watch`, published on each change, so a
  capsule never reaches into the host), with `last.route` from
  `capture_host::routing` (Scrawl's `crates/router` in-process: look, decide,
  undo window, send, Undo and Redirect) and `phone` from a `capture-phone`
  look for a cabled iPhone. The engine records the screen on macOS through
  `crates/capture-core` (`Recorder`, one source per target, also outside the
  workspace), which writes `recording.mp4` and its `capture.json` manifest
  and changes what a recording leaves out with `Recording::set_exclusions`.
  `capture_host::library` reads the captures
  folder for `hex capture list|show|send|route`. `rune_ui/scrawl.rs` is its pane
  (Labs › Scrawl) and `CaptureSettings` its `capture.*` settings;
  `hex capture` is `control::cli::capture` (macOS through the running app;
  Linux reads the folder itself and hands start/stop/cancel to the
  listener over `linux/service`; Windows sends every `hex annotate` command
  to the running Hex).
- `desktop/instance`: the exclusive Application Support lock that prevents concurrent
  command listeners.
- `desktop/process_exit`: how the macOS app ends. Every quit path (menu, `hex quit`
  and `hex restart`, Ctrl-C, and AppKit's terminate) stops and joins Hex's
  workers, bounded by a 5 s deadline, then calls `_exit`, so C++ static
  destructors and `atexit` handlers never run
  under live threads. A restart
  schedules its relaunch first; the new copy waits for the old pid to exit.
- `feedback`: bounded volume-controlled capture and failure tones.

Keep modules deep. Callers should not coordinate
CoreAudio formats, AppleScript details, or event serialization.

## Invariants

- Settings errors never prevent launch. `app_settings/persistence` defaults
  only invalid fields in memory and logs each key and reason; unknown keys
  are ignored. Invalid JSON or a file that isn't a JSON object uses defaults
  without changing the original, and launch reports the parse error. An
  unreadable file also uses defaults without changing it, but reports no
  launch note. Before any save over invalid fields, preserve the exact
  original beside it as `settings.json.unreadable-<sha256>` (owner-only;
  identical originals share one backup). A failed backup blocks the save. A
  file that isn't a JSON object is never backed up or replaced: every save is
  refused until a restart. A process that loaded such a file keeps refusing
  saves even after the file is fixed, because its in-memory settings may be
  stale defaults. On Linux only, the two-second poll also clears that refusal
  when it adopts a valid or removed file. The refusal and launch note use the
  same wording on every OS.
  Keep saved values readable by older builds: an unchosen
  `meetings.retention` is omitted, not `null`. Do not omit optional fields
  whose absence restores a nonempty default, such as `paste_last_hotkey`.
- Every transcript comes from ElevenLabs, so the listener starts only once an
  API key is set (`transcription::models::is_ready`): until then its status is
  "API key required" on every OS, and it starts by itself when a key appears,
  with no restart. The key is read for each request from `ELEVENLABS_API_KEY`,
  else the owner-only `elevenlabs.key` in the data folder. It is never
  written to `settings.json`, logged, printed, or put in an error or a
  control reply, and it is shown only by its last four characters. On Linux the saved file is
  the dependable source: the systemd unit's environment is a fixed list of
  desktop variables (`SESSION_KEYS` and `INHERITED_KEYS` in
  `src/linux/service.rs`), so a variable exported in a shell reaches commands
  run in that shell but not the background service. The same holds for
  `HEX_ELEVENLABS_URL`.
- On a new Mac, dictation starts only after Microphone and Accessibility are
  granted and an API key is set. Hex never asks for Input
  Monitoring: Accessibility also grants the listen-event access its shortcut
  taps need (measured upstream on macOS 26 and 27).
- The dictation shortcut defaults to Option but supports modifier-only,
  modifier-plus-key, standalone Globe/Fn, and standalone function-key bindings.
  Capturing a new binding suspends global matching.
- Every shortcut is a chord: the set of keys held together, any order and
  nothing else, each modifier key with its side (left, right, or any) plus at
  most one other key or mouse button. It is down on the edge that completes
  the set and up when any of its keys lifts; hold, double-tap lock, Toggle,
  the hold threshold, the provisional start tone, extra-key discards, stale
  recovery, and the release tail all run on those edges, on the events' own
  timestamps. A one-key chord is down at once, with no added wait. Both sides
  of one modifier (`[⌥ left, ⌥ right]`) are down only when the second side
  goes down within `BOTH_SIDES_WINDOW` (80 ms) of the first; one held and the
  other added later presses nothing. A sided key never matches with its twin
  also down; any side matches one or both, as before chords, so it conflicts
  with a both-sides chord. Settings store `{"keys": [...]}`; the old
  `modifiers`/`key` shape loads as the same chord and is written back once.
  macOS and Windows match sides (device flags, the low-level hook); Linux
  binds a key with side-less modifiers only, and its recorder says so when
  modifiers are pressed alone.
- Windows accepts one modifier alone (Right Alt, Right Ctrl) as a dictation
  shortcut. Any key or mouse button joining a held modifier-only shortcut
  ends it as a chord (Cancel only while it records); Escape never does, except
  as Toggle's cancel shortcut. Releasing Alt or Windows
  after a modifier-only shortcut held them is swallowed and replayed after a
  tap of unassigned VK 0xE8, so the menu bar and Start stay closed (not with
  Share shortcut). Hex marks every event it injects with `HEX_INJECTED` and its
  hook skips them, so a paste's Ctrl never presses a Ctrl-alone shortcut.
- Windows onboarding shows its permissions page, with one Microphone row,
  only while Settings' microphone switch (device, user, or "Let desktop apps
  access your microphone") is off; Open Settings opens
  `ms-settings:privacy-microphone`, and setup waits for it. Linux has no
  permissions page. The practice game on every OS follows Hex's own
  dictation of the saved shortcut (the listener's event log on Linux and
  Windows), and Done always skips it.
- GUI startup must successfully call `keyboard::initialize_layout` on the OS
  main thread before settings, keyboard-using workers, or Rune start. When the
  selected input source changes, the main thread rebuilds the snapshot
  (`keyboard::refresh_layout`, from the one always-on input-source observer)
  and publishes it whole; workers read the old or the new snapshot, never a
  mix, and cached misses never re-enter TIS/TSM. After a rebuild the runtime
  shortcuts are resolved again from the last applied settings; a macOS
  shortcut on a letter key follows its letter, and the listener rebinds it
  only once idle. Uncached headless lookups retain live-layout resolution,
  with all native lookup and snapshot construction serialized by the same lock.
- macOS launch order: `main` calls `rune::prewarm_gpu()` (macOS draws with Rune's native Metal
  backend by default; Hex never picks a backend) right after parsing
  the command line, only when it will run the app (`hex app` without Rune's
  headless flags, or the bundled executable with no command), never for a CLI
  command. A hidden or login launch prewarms too and Rune drops it: whether
  the window opens depends on settings, which load after the keyboard layout.
  `macos_rune` observes AppKit's will-terminate before the loop runs, so a
  terminate at any point drains workers. It sets up the status item, Dock
  policy, HUD, main menu, reopen handler, and pump timer once, after the
  window's first frame (`App::on_ready` queues it as a frameless `update_if`),
  or at most 1 s after the loop starts if no frame comes; a launch without a
  window sets them up in the loop's first turn. Nothing the window's first
  frame needs may wait on them.
- Another app's global shortcut (Carbon hotkeys, such as Spaceman's Option-1)
  consumes its key before the session taps. A listen-only HID tap reports key
  presses as chord evidence only: it discards an early hold or pending gesture
  like any unrelated key, never tracks key state, and skips Escape and Hex's own
  shortcuts. Its timestamps may be Mach ticks or nanoseconds.
- Stale shortcut recovery requires at least 100 ms of sampled keyboard neutrality,
  no pending input, and no hold or pending gesture. A lock is repaired only when
  it tracks a held key, and it stays locked so a fresh press can still finish it.
  It emits no capture actions.
  One exception skips the 100 ms wait: when tracked keys alone block a press of
  the binding itself (Idle or Locked for a modifier-only binding, or Dirty for
  any binding; never a pending gesture), the press resynchronizes and counts
  as a fresh press (a lock stays locked, so the press finishes it) only if a
  modifier-change event reported no modifiers after each tracked key's own
  press (typing in between is fine), only the binding's modifiers went down
  since, and every tracked key and every other modifier reads up in
  combined-session state (Fn metadata as below). It fences input stamped
  before the press, not at it, so a same-tick chord still discards, and keeps
  the binding's key safe from an older delayed release. Fenced edges, edges
  from before a disabled tap, and a suspension leave no modifier evidence.
  Any other press stays ignored and logs `shortcut press ignored` with its
  reason and key codes.
  Native `SecondaryFn` navigation metadata may be ignored only with timestamped
  modifier-change evidence that Fn is up; the full key scan and delayed-event
  fences remain required. Never strip Fn globally from shortcut matching.
- Hold the shortcut to dictate and release to transcribe. Captures shorter than
  300 ms discard. A 450 ms hotkey pre-roll
  protects speech onset, except when "While dictating" mutes, pauses, or lowers
  other audio: that audio predates the suppression and holds whatever was
  playing, so those hotkey captures keep no pre-roll (#1309, #1468;
  `dictation::hotkey_pre_roll`, every OS). Do nothing keeps 450 ms. There is no
  separate pre-roll setting. Capture has
  no automatic duration limit; release or
  an explicit stop ends it.
- Only Toggle mode has a cancel (#786, #1022), except a Labs › Caps Lock
  dictation capture, which is hands-free in every shortcut mode and whose own
  machine always cancels on Escape. Its cancel shortcut
  (`cancel_hotkey`, Escape by default, rebindable in General under Shortcut
  mode) discards the active recording with the cancel tone: nothing is
  transcribed, pasted, or saved to History, and jobs already transcribing
  always finish. Hex takes the key only while a toggled recording runs, and
  then keeps it (repeats and release included) from the app in front; idle,
  and always in Hold mode, it stays with every app. macOS suppresses it in
  the session tap only while the listener arms it (`set_takes_cancel`),
  Windows' hook only while its capture records, and X11 grabs it only for
  the recording. A shortcuts portal holds a key for as long as Hex runs, so
  on Wayland Hex binds nothing for it and Settings says to bind
  `hex dictation cancel` in the desktop. `hex dictation cancel` discards
  whatever records, in any mode. Hold mode has no cancel: Escape ends no
  capture there, not even as a chord, except a Labs › Caps Lock dictation
  capture, whose own machine always cancels on Escape. Every other intentional
  dictation finishes and pastes; only short taps, chords, and silent captures
  discard, and Hex itself ends a capture only when its input or microphone is
  lost.
- A dictation discarded for silence (its loudest 30 ms window under the
  -50 dBFS silence threshold; there is no voice-activity model) is discarded
  quietly on every OS: no error tone, no note, nothing in Settings, and no
  whole-clip request (a live session that was open already heard the audio).
  The log keeps the measured level and peak against the threshold.
- When enabled, a second shortcut tap within 300 ms locks dictation. Press the
  shortcut again to finish.
- Recording audio behavior and idle-sleep prevention begin only after the
  intentional-hold threshold. Ordinary shortcut chords must not mute output,
  pause media, or prevent sleep.
- A modifier-only shortcut (not Globe/Fn) passes through, so once Hex owns
  the press (hold past the threshold, Toggle start after the 40 ms chord
  grace, lock-finishing press) it posts one marked press of unassigned key
  code 0x44 at HID while exactly its modifiers are still down, so other
  apps' modifier-tap hotkeys read a chord. Hex's annotated-session key tap
  drops it, so the focused app never receives it (and never beeps). Short
  taps and chords never get one,
  and it never reaches Hex's state machines.
- The start tone confirms the shortcut press itself, not the hold threshold: it
  is provisional, sounds after a 40 ms chord grace, and a chord or short tap
  retracts it (silently inside the grace, else
  a 20 ms fade). With the microphone released while idle, the tone waits for
  the open microphone and the hold.
- When `Release microphone while idle` is
  enabled, pressing the shortcut opens the selected device asynchronously with
  no pre-roll, preserves the physical press for the hold threshold, discards a
  release before readiness, and closes only after the authoritative capture is
  idle. Accepted jobs never keep the stream open. A stream failure before the
  hold threshold returns the press to pending and reopens; on macOS, a later
  failure keeps the audio already delivered as a copy-only dictation, or fails
  the capture when no audio after the start arrived or it is near-silent. Cold
  opens retry the selected device briefly before falling back.
- Transcription requests, optional post-processing, and paste must
  never block authoritative audio capture, however slow or absent the
  network is. Their queues remain bounded.
- CoreAudio capture timestamps and annotated-session CGEvent shortcut timestamps
  share the macOS boot-time nanosecond clock. Do not apply the Mach timebase to
  annotated-session timestamps. Physical HID-tap timestamps arrived as raw Mach
  ticks on the tested Apple Silicon system, while some HID events already used
  nanoseconds; changing tap location requires revalidating timestamp units.
  Delayed press handling reconstructs the original onset from the timeline;
  delayed release handling excludes audio captured after the physical release
  plus the release tail.
- A shortcut release or locked press that finishes a transcribed capture keeps
  audio through that exact event plus at least the 200 ms tail
  (`POST_RELEASE_TAIL`). When sustained speech (100 ms of speech frames) runs
  into the tail's end, the tail grows until 160 ms of quiet or 600 ms after the
  event (`MAX_RELEASE_TAIL`), whichever comes first. A key click is too short
  to grow it, and the stop tone cannot: speech must also be heard once the
  tone's own length plus 60 ms has passed. The event timestamp is never moved;
  the hold threshold uses it alone. The audio owner waits at most the base tail
  plus 100 ms, or 600 ms plus 100 ms once speech grows it, and a stream stall,
  failure, or gap in the tail finishes with the audio already delivered instead
  of failing the capture. Escape, short taps, programmatic stops, and meetings
  get no tail. A press during the tail is handled after the old clip finishes.
- Active dictation capture is lossless with respect to event, UI,
  context, and worker stalls.
- The dictation shortcut has a persisted `shortcut_mode`: `hold` (default)
  or `toggle`. Toggle reuses the lock state on every platform: each press acts
  as the second tap of a double-tap, so release locks and the next shortcut
  press finishes. A modifier pressed alone never ends a key-chord capture. Double-tap settings apply to Hold only.
- A dictation's final text comes from the batch model by default
  (`scribe.final_text`: `batch`): the complete clip is uploaded at release.
  A live session (`scribe_v3_live`) is opened for a capture only when its text
  is shown (Labs › Live text, Labs › Live typing, the first run's practice
  field, or a `/dictations` start with `"partials": true`) or pasted (Final
  text: Live), and only with a key set (`scribe::stream_wanted`), because a
  session is billed for all it hears. For the same reason the session hears each long
  pause cut to its first 2.5 s and last 0.3 s, while the clip keeps every
  sample. The finish always carries the complete clip: with Final text Batch
  the session's text is provisional and the whole clip decides; with Final
  text Live the session's committed text is used, and a failed, empty,
  overfed, late (1.5 s), or interrupted session falls back to the whole clip
  on the batch model. Nothing is pasted before the final text. With Labs ›
  Live text on, the provisional text is for Hex's own live-text line only; it
  is never pasted, copied, or kept in History or Paste Last. Labs › Live
  typing (Linux, Wayland) shows it in the focused field as an input-method
  preedit and types the final text there instead of pasting. One engine
  thread sends every request in order, so requests never overlap. A
  dictation is one clip, never split at pauses; meetings are.
- Starting a new capture never cancels accepted dictation work. Pending jobs
  preserve submission-order output while capture remains immediately available.
- Completing or pasting an older job must not reconcile or finish a newer
  capture. Shortcut boundaries come from delivered CGEvent timestamps; do not
  fabricate a release timestamp from later physical state. Caps Lock dictation
  is the exception: its capture boundary is the HID device timestamp that its
  AlphaShift edge confirms.
- A programmatic dictation's owner cancels it through the API, and the lease
  ends an abandoned one. While one records, Hex claims no Escape at all.
- There is one transcription model, `scribe_v3`, and nothing to download,
  install, or verify. `transcription.model` in `settings.json`, the CLI's
  model arguments, and the local API's `model` parameter take only that id.
  A settings file written by upstream Hex still loads: any saved model name
  becomes `scribe_v3`, its language is kept, and removed keys are ignored. A
  language change reaches the running listener through the engine's
  `reload`, once no transcription is pending.
- A dictation whose transcription failed for a reason the same audio could
  survive (no key, a refused key, no quota, a rate limit, no network, a
  timeout, an ElevenLabs 5xx) keeps its clip in `failed-dictations/`
  (`transcription/failed_clips`), and the failure names the file and
  `hex model transcribe <file>`. A request ElevenLabs refused as such, or one
  whose waiter gave up, keeps nothing. Apart from that folder, Labs › Keep
  recordings, and `HEX_RETAIN_DICTATION_AUDIO`, dictation audio is never
  written to disk.
- The vocabulary's terms are sent to Scribe as keyterms only while Labs ›
  Vocabulary biasing is on and the boost is not Off, because ElevenLabs bills
  keyterms extra. The API's limits apply (batch: 1,000 terms of at most 50
  characters and 5 words; realtime: 50 terms of at most 20 characters), terms
  it would refuse are skipped, and "sounds like" spellings are never sent.
  Search and replacements use the vocabulary either way.
- Hex has one onboarding, on every build and OS, stable and dev alike: the
  first-run story (`rune_ui/onboarding_kit/live.rs`) over the tour state
  machine (`rune_ui/onboarding.rs`, `desktop::onboarding::Flow`). No flag,
  experiment, or build type chooses another. `Ui::new` opens it whenever
  `Tour::start` says this run needs it: a new install, a run quit part-way
  (resumed at its saved step), or an upgrader whose setup isn't ready (#475).
  Run setup again (Labs) and `hex onboarding reset`
  replay it from the start on every build. The story opens on the API key
  step: Continue checks the typed key with ElevenLabs off the UI thread and
  saves it once accepted, a key already set passes the step, and Skip for now
  goes on without one, passing over the practice, which needs a key.
  Completion is recorded once the permissions are on, with or without a key;
  without one, dictation waits and Settings says so. A quiet Skip also shows
  on the optional pages, the demo and the practice: it passes over that part
  to the next step setup still needs, else Hex is ready, and never records
  completion. Permissions and the paste permission have no Skip.
- The microphone is either System default (`microphone` unset) or one saved
  device. System default follows the OS default input live: macOS through
  CoreAudio listeners on the default input and the device list (their callbacks
  only bump a counter, never on the audio callback), Windows through the
  default-input stream's change report, and Linux at the next stream open
  (ALSA's default resolves when it opens). Hex never captures from Hex
  Microphone: a Hex Microphone default stands in with the built-in microphone,
  else the first real input, logged and shown in Settings. With a MacBook's
  lid closed (`AppleClamshellState`), the built-in microphone is off in
  hardware but still listed, so it is passed over the same way, and with no
  other input Hex reports why instead of recording silence. With
  `avoid_bluetooth_microphone` on (the default; General › Advanced), a
  Bluetooth default is passed over the same way (macOS: CoreAudio transport
  Bluetooth or Bluetooth LE; Windows: WASAPI enumerator `BTHENUM`, or a
  headset/handset of unknown bus such as a `BTHHFENUM` hands-free endpoint;
  Linux: a `bluez5` default source, whose stream Hex moves after opening), so the headset never drops into call audio, but kept when
  no other usable input exists; a saved Bluetooth microphone and `--device`
  are always honoured, and a change to the setting re-resolves like a
  device-list change. Settings names the stand-in. A saved microphone
  that is missing or will not open falls back to the system default without
  changing the setting, and Hex switches back when it returns. Any switch
  (selection, default, device list, or lid) happens only while capture is idle,
  after the current capture, and opening the replacement must succeed before
  the old stream is dropped; a device that resolves to the one already open is
  kept. An explicit CLI `--device` remains authoritative for that listener
  process and never falls back. A stream that already failed or delivered
  digital silence is stopped before its replacement opens, so the replacement
  starts with no dictation stream on the device, as at launch. On macOS, a
  runtime stream failure keeps a shortcut dictation held past the hold
  threshold, or a `hex dictation start` control capture, along with the audio
  delivered before the failure: transcribed and processed like a dictation
  (Dictate without processing copies it unprocessed), then copied rather than
  pasted (the disruption may have moved focus), still Paste Last Dictation and
  History, with no error tone. A clip with no audio after its
  start, or a near-silent one, fails as before; a programmatic capture keeps
  its audio for its owner's `/finish`, which answers `microphone-lost` when
  there is none. Audio from after the loss and stale chunks are dropped, and
  the stream retries with bounded backoff; it must not require a settings
  change or process restart. An initial open failure uses the same bounded
  recovery without stopping the listener. Cancellation does not stop
  warm-microphone retries; enabling release-while-idle stops them once capture
  is idle.
- On Linux, opening ALSA's default device loads PipeWire's ALSA plugin into
  Hex's own process, so `main` sets `DISABLE_RTKIT` before any stream opens:
  PipeWire's realtime module would otherwise lower this process's realtime CPU
  time limit to the desktop portal's advertised budget, which can be zero, and
  the kernel then kills Hex. Hex never takes realtime scheduling through the
  portal or rtkit.
- Feedback volume is persisted from zero through one; zero disables tones.
  Volume changes apply immediately and preview one recording-start tone.
- Launch at Login uses `SMAppService` for the signed main app. Treat both
  `NotRegistered` and `NotFound` as disabled before registration, represent
  `RequiresApproval` with a link to Login Items settings, and poll macOS as the
  source of truth instead of persisting a parallel Boolean.
- Mode processing is best-effort and ordered: corrections, optional OpenCode
  rewriting, then selected transformations. A failed stage preserves its input;
  the transformation chain is one stage, so failure discards its partial results.
  It applies to Paste, not meetings.
- A config reaches `fetch` or `node:child_process` only after the person
  allows its exact content hash; any file change asks again, and approval is
  stored outside the config folder. Each transformation runs under its own
  timeout (1 s, 30 s with host APIs, 60 s cap); when it settles or expires,
  Hex abandons its requests and kills every process tree it started. Requests
  and programs stay bounded (8 and 4 at once). Logs name hosts and programs
  with status, duration, and byte counts, never text, bodies, arguments, or
  environment values.
- Hex never starts, probes, or searches for an OpenCode executable, and never
  starts the service: it only talks to an already running OpenCode service
  found through its registration file (`desktop/opencode_service`). Nothing touches
  OpenCode unless a feature that uses it runs: mode rewriting, meeting notes
  with Summarize with OpenCode, sending an Annotate capture, or the model
  catalog while a picker that uses it shows: the Modes pane, or Meetings ›
  Settings › Advanced's Summary model (Labs › Choose meeting summary model,
  with Summarize with OpenCode on). Both read the one catalog loader
  (`rune_ui/models`). Checks stay off the UI thread. A stopped service is
  looked for again at a coarse interval only while such a picker shows;
  catalog failures require an explicit retry. Hex's OpenCode workspace folder is
  created by the first request that uses it.
- Personal transformations run in-process: no child process,
  no Node or Bun, and no filesystem, network, process, or timer API in the
  script engine; scripts import only `@hex/commands` and files inside the
  config folder. One worker thread owns the QuickJS runtime; each
  transformation call is interrupted after 1 s and a load after 2 s, and the
  runtime is capped at 64 MiB. A config that fails to load or compile keeps
  the previous registry and reports its error with file, line, and column;
  a removed config unregisters everything. The built-in transformations stay
  native Rust and never touch the script engine.
- Clipboard insertion is the fixed text-insertion path. Restore the previous
  clipboard only if no newer paste or external clipboard change superseded it.
  The one exception: on macOS, a frontmost app embedding Chromium 155 or 156
  (read from its bundle's framework version) gets the text typed, because
  those versions can drop a ⌘V that races their pasteboard notification.
- Dictate without processing (`raw_dictation_hotkey`, Output pane) is
  unbound by default and registers, grabs, or reserves nothing until bound
  (on Linux a portal entry and X11 grab exist only while bound). It follows
  hold, toggle, double-tap lock, and queueing as dictation does; the
  shortcut that started a capture owns it. It pastes the trimmed transcript
  with no Output step, records History kind Raw, and updates Paste Last.
  `hex dictation start|toggle --raw` does the same; a printing stop of a raw
  dictation prints the transcript as heard.
- `hex dictation stop --print` (macOS and Linux) stops through the listener's
  own queue with a one-shot reply: recording stops at once and only that
  command waits, at most 60 s, for the text, processed as a paste. It never
  pastes, even on failure or timeout; no speech exits 4. History records it as
  Printed and Paste Last skips it.
- Option-Shift-V pastes the last completed dictation. Option-Control-V pastes
  completed meeting turns added since the previous successful invocation.
- Do not persist captured audio by default, with one exception: the clip of
  a dictation whose transcription failed is kept so it is not lost
  (`failed-dictations/`, owner-only, at most 20 clips and 7 days). Explicit
  foreground meeting
  recording must remain visibly active. Diagnostic dictation retention is an
  explicit, bounded, owner-only opt-in through `HEX_RETAIN_DICTATION_AUDIO`.
  Labs › Keep recordings (`keep-recordings`, off by default) is the only
  other opt-in that stores dictation audio: while on, each new History entry
  keeps its 16 kHz clip as an owner-only WAV beside the store. The audio
  follows its entry's retention, entry cap, Delete, and Clear all, under its
  own 512 MiB cap (oldest audio dropped first, text kept). Turning it off
  stops saving; existing audio stays until it expires or History is cleared.
- Retained dictation history records only successful pasted output: raw and
  final text plus bounded metadata, never full browser URLs or window titles,
  and no audio unless Labs › Keep recordings is on. Entries name their speech
  model; with Keep recordings, Replace keeps up to five earlier versions
  inside the entry, counted by the same caps. History defaults to seven days, and every retention window remains
  subject to hard entry and byte caps. No new entries are accepted while Off.
  Turning retention Off preserves existing entries; clearing history is explicit.
  Time-based retention expires during idle uptime through the shared history
  owner; searches never return expired entries while waiting for disk cleanup.
  The hard caps are 10,000 entries and 16 MiB of raw + final text (oldest evicted
  first), with existing per-entry metadata and text limits. There is no archive.
- `hex history` and `hex meetings` are read-only, release-available on every OS,
  independent of feature opt-ins. Dispatch before process
  logging, the instance lock, local API, GUI, permissions, or Hex Microphone.
  History reads one open
  file through concurrent atomic replacements, applies the saved retention at
  query time, and reports malformed files/settings without repairing them.
  JSON contracts are documented in `docs/transcript-cli.md`; incompatible
  changes require a version increment. Debug `meeting list/show` use the same
  read-only path; recording and other debug commands keep their existing roles.
- Meeting detection may inspect process audio metadata but must not capture
  samples. Detection offers recording, and starts it automatically only for a
  provider (website or app) the user explicitly set to Always record in
  Meetings › Settings › Detected calls or an offer's "…"; every provider
  defaults to Ask. An automatic start passes the same stability time and
  snooze as an offer, so a stopped or dismissed call never restarts until its
  app leaves the microphone, and it shows as visibly as a manual recording. An
  offer that times out only closes. Release builds keep the meeting controller
  idle, without inspecting microphone activity, until the user opts in. When the
  detected call app leaves the microphone, offer `Keep Recording` or
  `Stop & Transcribe`; never stop automatically. An active recording stays
  visible and stoppable from the menu bar without the Settings window.
- Calendar access is asked for only by Meetings › Settings' Connect calendar, never at
  launch, and only with Meetings on; Meetings off means no EventKit reads and
  no reminders. `meetings.calendar.connected` defaults off. A meeting has other
  attendees or a call link and is not all-day, declined, free, or cancelled.
  Reminders are Hex capsules, not notifications, shown once per occurrence at
  the lead time (or on a wake within 5 min of the start) and withdrawn when the
  meeting is cancelled or moved. One reminder shows at a time, never over a
  recording. EventKit is read on the controller thread on store changes or once
  a minute and on Settings' reader thread, bounded, with no network, never on
  the audio or UI thread; nothing touches the store before Connect. A reminder
  never records without a click; Always record still needs the detected call.
  Recording or dismissing a reminder snoozes the same call's offer and
  automatic start; without a call link, only the next call near the start.
- Meeting transcription defers to dictation: each chunk is one background
  request on the dictation engine, starts only while no dictation capture
  (from the shortcut press) or dictation transcription is active, never
  interrupts a live session, and is retried when a dictation request was sent
  first. A chunk request already sent cannot be withdrawn, so a dictation
  released during it waits for its answer or timeout; how long that is against
  the real service is unmeasured. Chunks are short (first pause after 6 s,
  hard cut at 10 s of new audio plus 2 s overlap), and a chunk that never
  reaches speech level is not sent. It uses the engine's language, so a
  language change applies from the next chunk; a process without the engine
  uses its own transcriber. Its queue
  (one-second audio blocks) and chunk backlog stay bounded; a source that
  falls behind skips its oldest audio, keeps going, and is rebuilt from its
  WAV after Stop. A capture gap restarts that source's chunks after the gap.
- Quitting while a meeting records stops capture and saves its WAVs and
  manifest (`Interrupted`, `final.pending`) without transcribing; once
  quitting begins, no meeting work starts a chunk. The next
  launch with Meetings on runs the final pass through the dictation engine
  (or its own transcriber after 30 s without an engine).
- Every in-call meeting view and copy (the Meetings pane,
  Option-Control-V, provisional titles, `hex meetings`, and MCP) reads the
  rolling transcript, which publishes only entries no later chunk
  can replace; after Stop they read the final transcript.
- Meeting system audio uses a Core Audio process tap on macOS (System Audio
  Recording consent), never requiring a display; ScreenCaptureKit is only the
  fallback. Linux records the default output's PipeWire monitor through
  `pw-record` and Windows its WASAPI loopback; neither asks for consent.
  Losing either source never stops the other or the meeting: it retries until
  Stop, and resumed audio is padded with silence so WAV positions stay on the
  shared capture clock.
- Meeting recording prevents idle sleep but does not apply dictation mute or
  pause-media behavior.
- Meeting notes (Apple Intelligence, or OpenCode only once the user turns on
  Summarize with OpenCode) run only after final publication, on a
  utility-QoS thread that yields to dictation, within a bounded chunk count and
  total deadline. Failures record `notes_error` and leave the Markdown
  untouched. Notes rewrite only the heading Hex wrote and the
  `<!-- hex:notes -->` block. With Labs › Descriptive meeting file names on,
  notes or chapter refinement that change the title, and user renames, also
  rename the exported Markdown to the new title, keeping its date and time
  prefix and never overwriting.
- Annotate (captures; `hex annotate`, alias `hex capture`) is compiled into
  every macOS, Linux, and Windows build and on by default in every build,
  stable included (`capture.enabled`, the pane's switch; the `annotate-on-by-default`
  migration turns it on once for older settings). It is not an experiment.
  Nothing asks for a screen permission until the first
  capture: macOS's Screen Recording at its first capture, Linux's portal
  dialog at its first Wayland Screen capture (X11 reads the screen from the
  X server, with no portal or dialog); Windows asks nothing. Default shortcuts
  share no key with the other defaults: ⌥⇧S on macOS, F9 on Linux and
  Windows. One key does everything:
  a tap (under `DRAW_HOLD`) starts and stops; held while recording, the
  pointer draws with no click until it comes up (Linux: portal
  Activated/Deactivated). The recording capsule is `● 0:07 │ ✎ │ ■ ✕`,
  icons only, with no target chips; its pen stays on until clicked
  again and draws only while the mouse button is down (drag to draw,
  move freely otherwise), and Esc stops drawing, never the recording; on
  GNOME Wayland with Labs › Annotate on GNOME the ink can't take keys, so the
  capsule's pen button stops drawing, as does letting go of a held Annotate
  key (a tap of it stops the recording instead). Shift still draws on the
  Mac. While drawing, the ink overlay takes the pointer with
  a pen cursor (Linux leaves a hole over the capsule's pill), so clicks
  reach neither the app nor anything but the capsule. Ink has no pins and
  no undo. There is no separate draw shortcut and no send-to list
  shortcut; Send to… is a capsule click. `hex annotate draw` toggles the
  pen for desktops without the portal. While a capture records, the
  dictation shortcut starts nothing on either OS. Sends
  carry the contact sheet and keyframes as inline `data:` image parts, never
  a local path. The Linux capsule doesn't open suggestions by itself.
  `validate-app.sh` requires
  `scrawl-helper` and the camera description in a macOS bundle. Linux
  never links PipeWire, VA-API, or FFmpeg: the first two load when a
  capture starts and `ffmpeg` runs as a child, so a missing one fails that
  capture with a reason and nothing else. On X11 (never XWayland) Screen
  and Area are captured natively (`scrawl_capture::linux::x11`: MIT-SHM,
  XFixes cursor, x264), so desktops without a ScreenCast backend record; a
  Window still needs the portal and fails with why rather than widening to
  the screen. With `capture.enabled` off, the
  listener builds no capture host: no engine, sink, thread, bundle folder,
  or work. Scrawl's engine
  records the screen, runs with `SharedResources::none()` (Hex never calls
  `install_shared_resources`: no ingest server, ingest token, or global
  shortcuts), logs to tracing, and writes only to the captures folder
  (`capture.folder`, Scrawl's own by default) and Scrawl's router folders. A
  written capture is copied to the clipboard (never pasted) and, with
  `capture.routing`'s autoSend on, routed by Scrawl's router in Hex's process
  (`capture_host::routing`): one `capture-look` thread per stop, one
  `capture-route` thread with a bounded queue, a rule's send held 3 s for
  Undo (a page naming its session, by meta tag or `.opencode-session`, sends
  like a rule and beats one), guesses sent only when picked. Queued sends retry only while neither
  Scrawl.app runs nor anything serves the ingest port, and never in tests.
  `capture.routing` never holds a Jev key. A cabled iPhone records through
  `scrawl-helper mirror --no-microphone`, narrated by the dictation timeline
  (macOS only: Linux lists no phone, so it never shows). Hex's capture
  shortcut (default ⌥⇧S, F9 on Linux, a key required, never overlapping
  another binding) is, on Linux, the portal's or an X11 grab's press and
  release: idle a press starts, recording a tap stops (debounced) and a
  hold draws. On macOS it is a fresh press: idle, it starts once one of its
  modifiers comes up (Esc before then records nothing); recording, a tap
  stops and a hold draws; with the capsule's picker open, the picker
  records. A switch (CLI) discards the recording (no outcome) rather than
  retargeting it; while Scrawl.app
  (`dev.kitlangton.scrawl`) runs, Hex registers no capture shortcut and runs
  no retention, and only the pane's "Use Hex for captures" quits Scrawl.
  Escape never ends a recording capture (while drawing it only stops
  drawing), and otherwise Hex's tap lets it through to the app being
  recorded; discarding is the capsule's two-step ✕ (✕, then
  Discard). Pointer
  events and Shift reach the engine from the one session tap, only while a
  capture records, through a bounded queue that drops rather than block; they
  never reach the listener. Scrawl's settings are imported once, read-only;
  Screen Recording is asked at the first capture or from the pane, never in
  onboarding. The capsule, picker, and ink are left out of every screen
  recording while shown; hidden (prewarmed at alpha zero on every Space)
  they are shareable, since one non-shareable window blanks every Space's
  Mission Control thumbnail. A capture leaves out all of Hex's chrome, unless
  `capture.show_overlays_in_recordings` (default off; the pane's Show in
  screen recordings) is on; it applies at once
  to open overlays and a recording capture, never recreating a window. A
  capture never opens a
  microphone, installs a tap of its own, or runs a transcriber of its own: it copies the dictation
  timeline through a bounded sink that records a gap rather than block, so a
  stalled capture never stalls dictation, and while a sink is open the
  release-while-idle close waits for it. While a capture records it holds a
  recording-environment session with the same "While dictating" policy
  (`recording_audio_behavior`, read at each start; no capture setting),
  released at stop, cancel, or quit; it composes with dictation's,
  so neither restores what the other still holds. One capture engine per
  process, one capture recording at a time. Its words are foreground requests on the
  dictation engine in 30 s chunks, each sent to ElevenLabs: after dictation
  already sent, before meeting chunks, and dictation output is
  unchanged. A chunk waits while dictation captures or transcribes and never
  interrupts a live stream; the engine holds it until the stream ends. `stop`
  replies at once and the screen recording stops then (`Capture::stop`), not
  when the capture is written. The movie finishes after the stop on the
  core's own bounded thread (detached before its stream stops, its final
  frame held to the stop); only the steps that read `recording.mp4` wait
  for it, and a discarded capture's bundle is removed only once it is done.
  One thread writes stopped captures one at a
  time, at most two wait, and a start beyond that is refused `busy`. The
  engine's log goes to tracing from before it is built, and its settings come
  from Hex (`ConfigSource`), never `~/.config/scrawl` after the import. Bundles are
  owner-only. Quitting
  cancels a recording capture, stops transcription between chunks, and waits
  at most 2 s for capture threads, leaving a movie still finishing behind.
- `hex mcp` is read-only and available in release builds regardless of the
  Meetings opt-in. It dispatches before process logging, the instance lock,
  GUI, or permission setup; never writes, recovers, or indexes on disk inside
  the meetings folder; transcribes nothing; and makes no network request. Stdout
  carries only MCP messages.
- `hex` control commands (`status`, `quit`, `dictation`, `meetings record`,
  `settings set`, `model use`, `window show`, …) are release-available on
  every OS and drive only the one running Hex through the owner-only
  discovery file and bearer-authenticated `POST /control` (64 KiB request,
  1 MiB reply, 10 s in-app deadline). A command an OS can't do answers `unsupported` at runtime. They dispatch before process logging,
  or the instance lock, and never launch Hex; only `hex start` and
  `hex restart` open it. Every command is carried out by the app's own paths
  (the menu bar's meeting request, the window's settings save followed by
  `ReloadSettings`, the shortcut state machine), so CLI-started recordings
  look and stop exactly like the UI's and opt-ins stay explicit settings.
  Commands are idempotent (`changed: false`). `settings::every_persisted_setting_has_a_cli_key`
  fails when a persisted setting has no `hex settings` key; a protocol change
  that breaks either shape increments `CONTROL_VERSION` and `docs/cli.md`.
- Meeting echo is judged on one timeline built from WAV sample positions, never
  from packet arrival. A `You` line or rolling entry is judged only once the
  system audio that could explain it has arrived (bounded when the system track
  stalls). Measure echo changes with `meeting_echo_replay` on a recorded
  speaker-mode call, counting genuine `You` lines lost as well as echo kept.
- The rolling transcript remains available if final transcription fails. Final
  publication is atomic; adjacent same-source entries within three seconds form
  one displayed turn labeled `You` or `Them`. Speaker echo never shows as `You`:
  the rolling snapshot, live paste, and final transcript drop a microphone
  utterance when delayed system audio explains its energy, or when its words
  repeat overlapping remote speech unless the signal shows local speech
  (double-talk). Each finished
  meeting is also written as Markdown to the configured folder (default
  `~/Documents/Hex Meetings`) without overwriting existing files; an export
  failure does not fail the meeting.
- Meeting paste cursors advance only after successful insertion and reset when
  Hex restarts. They track, per source, the end of the text already handed
  out, so rewrites near the live edge and the switch to the final transcript
  neither repeat nor drop turns.
- Deleting a meeting moves its exported Markdown and then its recording folder
  to the Trash (never `rm`), after a two-step confirmation, and is refused while
  it records, transcribes, or writes notes. Renaming rewrites the manifest and
  only a Markdown heading that still reads the previous title; files keep
  their names unless Labs › Descriptive meeting file names is on, which renames
  the exported Markdown to the new title, keeping its date and time prefix and
  never overwriting.
- Meeting data is a liability: until the user chooses a `meetings.retention`,
  meetings recorded since the default began (manifest `retention_applies`)
  are kept 24 hours and older ones stay; a chosen retention covers every
  finished meeting. The meeting controller expires them at launch and hourly,
  Meetings on or off, measured from when they ended: the recording folder
  moves to the Trash (never `rm`), and the exported Markdown only while it
  still matches the SHA-256 of the copy Hex last wrote (`export_sha256`), so
  an edited copy stays. Recording, transcribing, writing-notes, and
  final.pending meetings wait. Queries, `hex meetings`, `hex mcp`, and the
  pane never return an expired meeting.
- Visible state remains `Transcribing` until every accepted transcription or
  paste output has completed, including ordered post-processing and insertion.
- The HUD is observational and click-through. It must not alter capture
  boundaries, steal focus, or block controls in the foreground application.
- Desktop UI is governed by [`docs/design-system.md`](docs/design-system.md);
  read it before changing any pane, capsule, or picker. Rune is the only UI
  toolkit: compose `rune-kit` components and Hex's pieces in
  `rune_ui/controls.rs` and `rune_ui/lists.rs`, and name only the OpenCode
  theme's tokens, never colour literals. Use sentence case and the product name
  "Hex" in all copy. Reserve the accent (Primary) fill for at most one main
  action per screen; states such as recording or an available update use a
  Secondary button with a status dot. Extend a piece instead of adding a
  parallel helper, and verify visual changes with `--snap` scenes in light and
  dark.
- Neither Hex nor Rune (`anomalyco/rune`, public MIT) is published to
  crates.io; never run `cargo publish`. Hex depends on `rune`, `rune-kit`,
  `rune-theme-opencode`, and (with `capture`) `rune-motion` at one pinned Git
  rev in `Cargo.toml` and `Cargo.lock`; every pin bump
  must also update `cargoGitDependencies."rune-0.0.1"` (its rev and output hash)
  in `nix/package.nix` and keep `windows` 0.62.2 for `gpu-allocator` in
  `Cargo.lock`.
- Bump Hex's Rune pin only when asked. To do it: read the `## dev` section
  of Rune's `CHANGELOG.md` since the current pin,
  adapt to its breaking changes, update Rune's rev and output hash in
  `nix/package.nix`, and run `scripts/check rune_ui`, goldens included.
- Hex runs as one long-lived process per user and data root on every OS: a
  menu-bar process on macOS (`macos_rune`, native `NSStatusItem`, native Metal
  `NSPanel` HUD, Rune meeting-capsule overlays), a tray process on Windows
  (`windows/rune`, named-event single instance, Rune overlay HUD), and a
  single-instance process on Linux (`linux/app`, socket + lock in the data
  root, Rune overlay HUD). Closing the window never stops the runtime, listener,
  HUD, or accepted work.
- The updater is permanently off. `has_update_channel` in `src/updater.rs`
  returns false, so every build is a source build to the updater: it never
  checks, downloads, stages, or installs anything, and makes no update
  request. Do not turn it back on or point it at upstream's channels: they
  publish upstream Hex, which would replace this fork with a different
  program. A newer build is installed by building it
  (`scripts/install-linux.sh`).
- The Wayland beta requires compatible clipboard protocols, and layer-shell or
  XWayland for the HUD. Copy and paste need `wl-copy` from `wl-clipboard`,
  which the installer doesn't bring; while it's missing Hex warns in Settings
  and `hex status`, and a paste fails naming the package.
  Paste sends its shortcut through `wtype` when the compositor advertises
  `zwp_virtual_keyboard_manager_v1`, and otherwise through the
  RemoteDesktop portal, the general injection path: one keyboard-only session
  approved once (restore token, persist mode 2) and kept with the paster.
  With neither, the transcript stays copied and the failure names the
  missing portal. Shortcuts come from the GlobalShortcuts portal when the
  desktop offers it; otherwise the user binds the `hex dictation` and
  `hex paste-last` commands in the desktop's own
  keybindings. One general path: no compositor-specific code, no raw input
  devices, no input-group requirement, and no silent fallback to XWayland or
  privileged input injection (uinput). One allowed exception: on a compositor
  without layer-shell (GNOME), Rune opens only the HUD, an input-free overlay,
  as an override-redirect X11 window on XWayland when `DISPLAY` is set (empty
  input shape, never focused); without XWayland Settings reports the HUD
  unavailable. One explicit opt-in exception: Labs › Restore clipboard on
  GNOME (`gnome-clipboard-restore`, off by default) snapshots and restores the
  clipboard through XWayland's CLIPBOARD selection, which Mutter bridges both
  ways, only on a Wayland session with no data-control protocol and only while
  this user's XWayland process already runs with `DISPLAY` set; Hex never
  connects first, so it never starts XWayland. While it is off there, Settings
  › Output says the clipboard can't be restored and points to the toggle.
  A second explicit opt-in exception: Labs › Floating meeting capsules on
  GNOME (`gnome-meeting-capsules`, off by default) lets the meeting capsules
  (recording offer, recording capsule) open on XWayland through Rune's
  `OverlayOptions::xwayland_input`, as override-redirect windows that take
  clicks in their input shape and never keyboard focus, only on a Wayland
  session without layer-shell and only while this user's XWayland process
  already runs; Hex decides at each show and never starts XWayland. Off, or
  when the overlay still can't open, an offer opens the Meetings pane (#1271).
  A third: Labs › Annotate on GNOME (`gnome-annotate`, off by default) does the
  same for Annotate's recording capsule, ink, and picker, with the same
  conditions and per-show decision. Rune never shows an `OnDemand` overlay on
  XWayland, so on such a session the ink takes `KeyPolicy::None` and pointer
  regions from the start, keeping regions with no hit area while not drawing
  (an XWayland window made click-through can never take the pointer later);
  Esc can't stop drawing there, so the capsule's pen button ends it, as does
  letting go of a held Annotate key (a tap stops the recording). Everything
  else stays on Wayland. The desktop may grant a different trigger, keep an
  older one, or refuse one; Settings shows what it granted, follows
  ShortcutsChanged, and opens ConfigureShortcuts (portal version 2).
  Hex's app id is `anomaly.hex`: every install (`scripts/install-linux.sh`,
  Nix) ships `anomaly.hex.desktop` with `StartupWMClass=anomaly.hex`, and the
  windows report that id, so there is one launcher entry. Every entry says
  `Icon=anomaly.hex`: the Nix package installs the hicolor PNGs in
  `packaging/linux/icons` (16–512 px), and without a packaged icon Hex writes the same embedded
  PNGs into `~/.local/share/icons/hicolor` at startup and names that icon in
  the managed entries it or the installer wrote (#652). The PNGs carry an
  `X-Hex-Managed=true` text chunk: Hex replaces and the installer removes only
  marked regular files, never a user's own icon or a symlink. Launch at login is
  `~/.config/autostart/anomaly.hex.desktop`; where the Background portal exists
  (`linux/background`, #812) Hex asks it at startup to run in the background
  and Launch at login goes through it (the portal writes or removes that file,
  `X-XDP-Autostart=anomaly.hex`). Every RequestBackground rewrites or removes
  the file, so Hex asks only while it is absent or Hex's (managed or the
  portal's); the installer treats the portal's entry as Hex's too. At startup Hex renames its own
  managed `hex.desktop` entries (the names before #579) and never touches
  unmanaged ones. Only a loose binary without any entry writes its own
  managed one so the portal can identify it, NoDisplay beside an older
  package's `hex.desktop`, and removed once a package ships the entry. Every portal shortcut is bound
  in one session with one BindShortcuts call, and that set never changes
  while Hex runs: desktops keep one set per app, and KDE replaces it with
  each call (a second session once wiped Hex's shortcuts,
  [#482](https://github.com/anomalyco/hex/issues/482)). Keys the desktop
  changes in its own settings arrive as ShortcutsChanged and work at once; Hex
  shows the granted trigger in Settings (row note and the Status hint) and in
  `hex status --json` (`listener.shortcuts`). On GNOME the Linux dictation
  default is Ctrl+Alt+Space (GNOME keeps Alt+Space; so does KDE Plasma's
  KRunner, #898), Settings flags a shortcut
  on a key GNOME keeps for itself, and Annotate's shortcut shows beside
  dictation's in General (#723). Enable and Disable
  (window, tray) also save `enable_on_launch`, so a disabled Hex stays disabled
  after a restart (#575). On GNOME only, Disable then binds an empty set in a
  fresh session: GNOME Settings stores exactly the set an app binds, so Hex's
  entries leave its list without a dialog, and Enable asks for approval again.
  Never on KDE, where an empty set would drop the user's chosen keys (#482).
  At startup on GNOME, Hex also removes its pre-#579 `hex` set from GNOME's
  `org.gnome.settings-daemon.global-shortcuts` settings through `gsettings`,
  since the portal can no longer reach that id; it touches no other app. A portal shortcut
  is held for as long as it is bound and consumed before the focused app
  sees it, so the portal never binds Escape. On KDE, Hex removes the
  Cancel dictation entries older builds bound (`cancel`, `cancel-dictation`),
  which could hold Escape or another key from every app. Modifier-only
  shortcuts stay unsupported on Linux.
- Every Linux shortcut source (X11 grabs, portal presses, `hex` commands) feeds
  the same `linux/input` state machine and emits the same events. Commands are
  idempotent: starting while recording or stopping while idle does nothing.
  They talk only to a running, listening Hex and fail with a clear error
  otherwise; they never start one. The control host hands them straight to
  the listener's queue, never through the window's thread.
- One Linux Hex process per user and data root owns the microphone, the
  transcription engine, saved settings, History, the HUD, and the window. The window is
  optional and opened on demand; closing it never stops capture, accepted
  work, or the HUD, and cancels only an uncommitted shortcut capture. A
  relaunch forwards `Activate` (with any activation token) and exits 0;
  hidden and autostart launches never show the window. With the managed unit
  loaded and the default data root, launches start Hex through systemd
  (`Type=notify`), which owns its lifetime and restart; a crash restart opens
  no window. Otherwise the launching process becomes Hex. The socket stays
  owner-only and peer-UID checked, with bounded framing, queues, and I/O, and
  carries only `Activate`, shortcut commands, `Status`, and `Quit`. Window
  failures (no display, no GPU) are logged and never stop Hex. The unit's
  `ExecStart … service`, `hex app`, `hex app --hidden`, and `hex start` stay
  valid entry points forever.
- On Linux and Windows, saved settings apply to the running listener in place
  while no dictation records; a settings change never stops listening or
  changes the shown status. Shortcut changes register the shortcuts again,
  a microphone change reopens the stream, and a transcription-language
  change is handed to the engine (`reload`) once no
  transcription is pending. Recording a
  shortcut in the window suspends only global matching.
- Linux shortcut capture uses focused input in the window. Recording a
  shortcut suspends the running listener's global matching and registers the
  shortcuts again when it ends; cancellation restores the old binding unless
  its save already committed. The window and the CLI must expose Hex's state, recovery, and
  errors. `hex quit`, the window's Quit Hex, and SIGTERM drain workers before
  exit. HUD teardown only hides that listener's HUD.
- Linux paste keeps transcripts off helper argv and bounds helper I/O. X11 waits
  for physical modifiers without discarding accepted output, and shutdown
  cancels that wait. Wayland hides held keys, and some compositors (Hyprland)
  merge held physical modifiers into `wtype`'s virtual keyboard, as the
  RemoteDesktop portal shares the desktop's keyboard. So Wayland paste waits on
  `ShortcutKeys`: a modifier shortcut's portal release plus 250 ms (at most
  2 s after a press without release), or 250 ms after a `hex` command. Docs
  recommend modifier-free binds there. A sent paste is not a verified one.
  New installs use Ctrl-V;
  the terminal-paste preference selects Ctrl-Shift-V and defaults on for
  persisted legacy X11 settings.
- Linux paste restores the previous clipboard by default, as macOS does: it
  snapshots every format of the selection (32 MiB cap, 1 s deadline) before
  writing the transcript, and 500 ms after the paste shortcut restores it only
  if the paste's generation is still the latest and the clipboard still holds
  the paste (the same bytes on Wayland, the paste's owner on X11). An empty
  clipboard is cleared again; an oversized or unreadable one is not restored,
  and the log says why. A paste that only reached the clipboard keeps it for
  Ctrl+V, and a capture copy is never restored over. Wayland restoration needs
  ext-data-control (KWin) or wlr-data-control, or, without either, the
  explicit `gnome-clipboard-restore` Labs opt-in with XWayland already running
  (same generation and still-holds-the-paste rules, the same bytes read back
  through XWayland, bounded reads); there is no consumption acknowledgment. The RemoteDesktop paste session closes 15 s after the last
  paste when its grant restores, so GNOME's remote-control indicator shows
  only around pastes (#718); the next paste reopens it without a dialog, and
  only a session that showed the dialog waits the 250 ms focus settle.

## Development

```sh
cargo run -- dictate            # Linux: foreground hotkey dictation, no window
cargo run -- status --json      # asks the running Hex (docs/cli.md)
cargo run -- key status         # whether an API key is set; never prints it
cargo run -- mcp --meetings-dir tests/fixtures/meetings
cargo run -- app -- --list-scenes
cargo run -- app -- --snap /tmp/hex-settings.png --scene mac-settings
cargo run -- app -- --snap /tmp/hex-modes.png --scene modes --theme light
cargo run -- app -- --snap /tmp/hex-onboarding.png --scene mac-onboarding
scripts/install-linux.sh        # build and install for this user
scripts/check                   # fmt, code shape, Nix pins, clippy, every test
scripts/check meeting::query    # fmt, then only matching tests
git diff --check
```

macOS only, and never run in this fork:

```sh
cargo run -- listen
cargo run -- dev dashboard   # debug recognition TUI
cargo run -- meeting record --title "Design sync"
cargo run -- meeting probe
./scripts/install-app.sh
./scripts/test-app-identity.sh
```

Unit tests never read this computer's API key and never contact ElevenLabs:
`scribe::key::current` returns none under `cfg(test)`, the client tests talk
to a fake server on loopback, and failed clips are not written. Keep it that
way: a test must not need a key, the network, or quota. A dictation run by
hand (`cargo run -- dictate`, the installed app, `hex model transcribe`) does
send its audio to ElevenLabs and spends the key's quota.

New, unproven, user-requested, and externally contributed features land in
Labs, off by default
([docs/features/labs.md](docs/features/labs.md)): add a `labs::Feature` with
its settings key, description, and group (`Feature::group`), a card on the Labs page, and a
`hex settings` key; gate where the feature's work starts (worker, thread,
capture), not just its UI. Never add a master switch or ship such a feature
on by default. Remove it from Labs when it graduates or is dropped.

Use `scripts/check [filter]` for the local loop, not raw `cargo test`. It runs
tests through cargo-nextest when it is installed, with
`.config/nextest.toml`, which kills a hung test and reports it by name; CI uses
the same config's `ci` profile. Read its output whole rather than piping it
through `grep | head`; failures are summarized at the end.

A copy of Hex run for a test or a measurement gets its own
`HEX_APPLICATION_SUPPORT_DIR` and `HOME`, so it never touches the user's own
data folder or `~/.config/hex` ([`docs/perf.md`](docs/perf.md)). Derive user
folders through `app_paths`
(`home_dir`, `data_dir`, `document_dir`) and, in Annotate's crates,
`scrawl_router::user_dirs`, never `dirs` directly: on Windows `dirs` ignores
the redirected `USERPROFILE`/`APPDATA`/`LOCALAPPDATA` and reaches the real
user's folders.

Keep source files under 800 non-test lines (`#[cfg(test)]` code excluded);
1,500 is a hard cap, and older files above it live in
`scripts/file-size-exceptions.txt` with ceilings that only go down. Keep
folders shaped by capability: `x.rs` or `x/` beside two or more `x_*.rs` (or
three or more `x_*.rs` alone) moves into `x/`, and a folder over 40 non-test
source files warns and over 60 fails (`scripts/folder-shape-exceptions.txt`).
`scripts/check` and the `code_shape_tests` suite run `scripts/code-shape.py`
rules on every check; refresh ratcheted ceilings with
`python3 scripts/code-shape.py --update-exceptions`.

Use Rune's headless renders as the desktop-UI iteration loop: `hex app --
--snap <png> --scene <name>` draws one named fixture scene from `SCENES` in
`src/rune_ui/scenes.rs` without starting the runtime or touching the data folder
(no settings, History, or log; only `live` reads, and never writes), on
every OS (`--list-scenes` lists them; `--theme light|dark`, `--size WxH`, and `--scale` adjust the
render). Scenes cover every pane and state the goldens check: the Linux and
Windows panes (`modes`, `history`, `onboarding`,
`windows-settings`, …) and the macOS ones (`mac-settings`,
`meetings*`, `mac-voice-filter*`, `mac-hud-lab`, `mac-activity`, `mac-onboarding*`).
`live` renders the running Linux Hex's real state. A fixture never reads,
saves, or checks a real API key. On a host without a GPU,
render on lavapipe with `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`.
`cargo test rune_ui` compares the light and dark goldens in `tests/golden/`.
A scene is a fixture, not native end-to-end proof; the meeting capsules and
HUDs have no scene.

After changing onboarding, run `scripts/check-onboarding.sh` (Linux only,
like every golden). It renders every onboarding scene in both appearances
against the approved goldens and checks that Skip shows only on the optional
pages. The macOS and Windows scenes are fixtures of the same window and
render there too, but nothing renders on a real Mac or Windows GPU.

This fork is built and run on Linux. `scripts/build-linux.sh` is a plain
`cargo build --locked --release` copied to `dist/`; the build needs no
speech runtime, shader compiler, or model download, and `scripts/setup.sh`
does nothing. [`docs/linux.md`](docs/linux.md) lists the native build and
runtime dependencies, and `.github/workflows/check-linux.yml` installs the
Ubuntu ones. For a source install, run `scripts/install-linux.sh`, then
`hex key set`. The installer owns the user-local version layout (marked
`source-build`), the launcher and autostart entries, and the `hex.service`
user unit.

`nix develop` supplies the native build environment of the Nix package;
`nix flake check` checks modules, shell configuration, session readiness, and
the display-free Rust suite. See `docs/nix.md`. Automated Linux checks do not replace real
X11/Wayland, portal or keybinding shortcuts, microphone, and target-application
smoke tests.

Microphone selection is System default (the OS default input, live) unless a
saved microphone is connected; override everything with `--device`.

Windows: `scripts/build-windows.sh` runs from Git Bash on Windows, once per
architecture with `HEX_WINDOWS_ARCH=x86_64` (the default) or `aarch64`, and
copies `hex.exe` and the console entry point `hex.com` (`crates/console`) to
`dist/`. The static runtime in `.cargo/config.toml` keeps the Visual C++
Redistributable unnecessary. Release executables are stripped
(`[profile.release]`). In this fork the Windows code has only been
type-checked by cross compilation.

macOS: the Rust app is named `Hex`, packaged as `Hex.app`, with bundle
identifier `anomaly.hex` (deliberately not reverse-DNS) and executable `hex`;
the virtual microphone driver is `anomaly.hex.microphone.driver`.
`scripts/build-app.sh` builds the bundle: Developer ID signed with an explicit
`HEX_TEAM_ID`, ad-hoc signed with `HEX_RELEASE_UNSIGNED=1`, or as `Hex
Test.app` with `HEX_TEST_BUILD=1` (bundle ID `anomaly.hex.test`, its own
`hex-test` data folder and `~/.config/hex-test`; it must never read the real
`hex` data). `scripts/validate-app.sh` checks a bundle's identity. None of
this was built in this fork, and nothing is notarized or published.

`SMAppService` is meaningful only from a signed app installed in
`/Applications`. When replacing a local bundle outside Finder during a login-item
smoke test, register the new bundle with Launch Services before launch:

```sh
'/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister' \
  -f /Applications/Hex.app
```

Pass the actual installed Rust host path to `lsregister` if it was renamed.

Verify both registration and unregistration with `sfltool dumpbtm`, and restore
the user's original login-item state after the test. Do not add release test
hooks or persist a second launch-at-login flag.

Use `termctrl` to verify TUI changes at both wide and narrow dimensions. The
`hex dev dashboard` keys are `1` for the activity log, `2` for meetings,
Tab to cycle, and `q`/Escape to quit.

## Diagnostics

Linux keeps everything in `~/.local/share/hex` (or under `XDG_DATA_HOME`):

- `logs/live.ndjson`: state, transcript, outcome, processing, and context
  events.
- `logs/process.log`: Rust and native diagnostics, including why a Scribe
  request failed (`ScribeError` never carries the key, the audio, or the
  transcript).
- `settings.json`, and beside it `elevenlabs.key` (the saved API key, mode
  0600) and `failed-dictations/` (clips whose transcription failed).
- `meetings/`: manifests, separate tracks, the rolling in-call transcript,
  and atomically published final transcripts.

`hex logs` prints the recent lines of either log without Hex running, and
`hex key status` and `hex model status` say which key is in use and where
audio is sent.

macOS uses `~/Library/Application Support/hex`. Windows uses `%APPDATA%\hex`,
with logs in `%LOCALAPPDATA%\hex`
(machine-local, so roaming profiles don't copy them). Hex keeps no compatibility with earlier `voice-control`
folders or per-OS settings files.

## Future Direction

See `ROADMAP.md`. Do not add hypothetical seams for roadmap items. Introduce a
seam once there are two real adapters or a current test requires substitution.

## Current Work

`ROADMAP.md` is the work list. Keep these constraints visible:

- Nothing has been measured against the real ElevenLabs service: make one
  real dictation, a live session, a meeting chunk, and a failed-then-retried
  clip with an API key before relying on any of them, and record the latency.
- The Windows code has not been run and the macOS code has not been compiled
  in this fork. Treat both as unverified until a build on that OS says
  otherwise.
- Validate native Wayland on a real compatible compositor, including physical
  portal shortcuts, the `hex` commands from a desktop keybinding,
  focus/click-through, target paste, and tray-less shutdown.
- Add a second real browser adapter before generalizing browser context.
- Do not turn concrete macOS, Linux X11, or command modules into hypothetical
  platform or plugin frameworks.
