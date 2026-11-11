# Control the running app from a terminal

```ts
Agent or terminal                                  // control.cli; every OS
├── One tree and protocol over the local API's POST /control on every OS;
│   OS can't do it -> unsupported at runtime, exit 1
├── hex status [--json] -> Listener, capture, meeting, model, features,
│   permissions, window, update, version, pid, memory
│   ├── model -> scribe_v3, its language, and the API key by source and last
│   │   four characters (none set -> the listener reads "API key required")
│   ├── update -> "current" ("unavailable" in a test build or with
│   │   HEX_DISABLE_UPDATES=1): no build updates itself
│   ├── Wayland listener -> its warning names a missing wl-copy before paste fails
│   └── Hex not running -> exit 3, "Hex is not running; run `hex start`"
├── hex start -> Already running: nothing | else open Hex.app (Linux and
│   Windows: a detached `hex app --hidden`), wait ≤ 20 s
│   └── The caller's HEX_* variables -> open --env (values never logged)
├── hex quit | restart [--force]
│   ├── Meeting or dictation in progress, no --force -> refused, exit 1
│   ├── Listener didn't answer, no --force -> busy (it might be recording)
│   └── restart -> reopens with the running Hex's HEX_* variables (macOS);
│       Linux unit -> systemctl restart; else waits for the drain, then start
├── hex dictation start|stop|toggle|cancel
│   ├── Same shortcut state machine as the hotkey; start locks the capture
│   ├── start -> intentional at once: tone once the mic is open, recording
│   │   behavior, live stream
│   ├── stop -> done only when the clip went to transcription;
│   │   no audio yet or silent -> discarded, nothing transcribing
│   ├── cancel -> discards the recording in any mode, nothing pasted or saved;
│   │   jobs already transcribing finish; idle -> unchanged
│   ├── Repeat a command -> unchanged, exit 0
│   └── dictation stop --print -> stops at once, then waits ≤ 60 s and prints
│       the transcript (processed as a paste) instead of pasting it
│       ├── No speech -> no-speech, exit 4, prints nothing
│       ├── Not recording -> not-recording, exit 1
│       ├── Failure or timeout -> exit 1, prints nothing, never pastes
│       └── History keeps it as Printed; Paste Last skips it (also on Linux)
│   └── dictation start|toggle --raw -> the dictation it starts pastes the
│       transcript as heard (Dictate without processing); History keeps it as Raw;
│       --raw with stop -> usage error, exit 2
├── hex paste-last -> pastes the last dictation
│   ├── Setup gates the listener -> not-ready
│   └── Recording or shortcut gesture in progress -> busy (menu bar: nothing)
├── hex typed-text (Linux) -> the Transform typed text shortcut's one job
│   ├── Labs › Transform typed text off -> disabled
│   ├── Terminal paste on -> disabled (it would send Ctrl+Shift+V)
│   ├── Setup gates the listener -> not-ready
│   └── Recording, a pending start, or a capture still finishing -> done,
│       but the listener drops the job: no keys are sent and no state changes
├── hex transcribe <file> [--model scribe_v3] [--language <code|auto>]
│   ├── The file's audio is sent to ElevenLabs (transcription.md)
│   ├── Labs › Transcribe files off -> disabled, exit 1; the file is never read
│   ├── Not WAV, or over 64 MiB -> refused here, before Hex is asked
│   ├── A model other than scribe_v3 -> unknown-model; a language that is not
│   │   an ISO 639 code or auto -> refused; default: the saved language
│   ├── Hex running -> the file to POST /transcriptions; the running Hex sends
│   │   it with its own API key, vocabulary, and Scribe settings, queued
│   │   behind any dictation; stdout gets the transcript
│   ├── Hex stopped -> sent from this process with the saved Scribe settings;
│   │   no key -> api-key-missing before anything is uploaded
│   └── No speech -> no-speech, exit 4; nothing pasted, kept, or processed
├── hex meetings record [--title] | stop | copy-last
│   ├── Meetings off -> feature-off; recording shows in the menu bar and HUD
│   │   exactly like a menu-bar recording
│   └── No meetings cancel: Hex cannot discard a recording
├── hex settings list | get <key|prefix> | set <key> <value>
│   ├── The window's save, then ReloadSettings -> live, no restart
│   ├── Unknown key, bad value, read-only key -> refused, nothing written
│   ├── Shortcut conflict -> refused, as the window's recorder would
│   └── apply <file|-> -> a JSON object of key: value, all or none
│       ├── Undeclared settings keep their values; dependent keys in any order
│       ├── transcription.language -> the language Scribe is told to expect;
│       │   transcription.model takes only scribe_v3 (both read-only to set)
│       ├── Hex running -> SettingsApply: saved and adopted live, like set
│       └── Hex stopped -> written to settings.json under the service lock
│           (Linux); a Hex holding it is waited for (≤ 35 s) instead
├── hex model list | use scribe_v3 [--language <code|auto>] | status | check
│   | transcribe <wav> [--language <code|auto>]      // alias: hex models
│   ├── One model, scribe_v3 (ElevenLabs Scribe); nothing is downloaded or
│   │   installed (transcription.md#language)
│   ├── list -> the running Hex's answer (its language and its API key);
│   │   none running -> what this process would use
│   ├── use -> sets the language; the running Hex applies it to its next
│   │   dictation; none running -> saved to settings.json here; without
│   │   --language the saved one stays
│   ├── status -> the saved selection, this process's API key, and where
│   │   audio is sent; runs here, with or without Hex
│   ├── check -> the same check as hex key check
│   └── transcribe -> sends the WAV to ElevenLabs from this process and
│       prints the text; no speech -> exit 4; the retry for a kept failed
│       clip (transcription.md#kept-failed-clips)
├── hex key set [KEY | -] | status | clear | check   // transcription.md#api-key
│   ├── Run in this process; the running Hex reads the key for each request,
│   │   so a change applies to its next dictation without a restart
│   ├── The key is never printed: only its source and last four characters
│   └── check -> asks ElevenLabs whether it takes the key; no audio, no quota;
│       exit 1 when no key is set, it is refused, or ElevenLabs can't be reached
├── hex window show [pane] | hide
│   ├── settings -> the General pane (status window.pane stays "settings")
│   ├── meetings pane with Meetings off -> the pane's opt-in card
│   └── story <story> [--variant V] [--play] -> a playground story, as a
│       hex://story link opens (labs.md); playground off -> feature-off
├── hex history clear --yes -> without --yes refused before Hex is asked
├── hex check-for-updates -> unavailable, "this Hex build doesn't update
│   itself", exit 1: the updater is off in every build
├── hex launch-at-login [on|off] -> SMAppService (changes need Hex.app);
│   Windows Run key; Linux XDG autostart entry
└── hex logs [--process] [-n N] [-f] -> local tail; works with Hex stopped
```

Every command except `start`, `restart`, `logs`, `settings apply`, `key`,
`model`, and the read commands talks only to the running Hex; none of them
launch Hex. Every OS reaches it through
the local API's owner-only discovery file and the bearer-authenticated
`POST /control` route. Commands dispatch before logging and the instance lock on every OS,
so they never compete with the running app for its files. Opt-ins stay
explicit settings: no command turns Meetings on as a side effect.

`hex transcribe` reaches the running Hex without `/control`: it reads and
checks the WAV here, then sends it to the same discovery file's
`POST /transcriptions` (`Content-Type: audio/wav`, 64 MiB cap) with
`model=scribe_v3` and the language, and the running app sends it to
ElevenLabs. Its Labs toggle is read from
`settings.json` (`AppSettings::read`), the same read `hex labs` lists from, so
the check works with Hex running or stopped; a running Hex saves the toggle
there at once. Off, nothing is read.

On Linux and Windows `control::host::answer` carries every command out through
the OS's adapter (`linux::app::LinuxControl`, `windows::rune::WindowsControl`):
dictation, paste-last, and Linux's typed-text go to the listener's own queue
(Windows: the same hotkey loop, as a toggle press would), settings save and
reload as the window's do, and `settings set` refuses a key whose capability
the OS lacks
(`settings list` marks it "not on this OS"). `meetings record|stop|copy-last`
go to the meeting recorder (`meeting/runtime`) as the tray's items do;
`window story` answers `unsupported`; Windows sends every `annotate`
command to the running Hex. `hex history`, `hex meetings list|show|search`, and
`hex mcp` read files on every OS.

Sources: [control.rs](../../src/control.rs) (protocol, status, quit rules),
[control/host.rs](../../src/control/host.rs) (the Linux and Windows host),
[control/settings.rs](../../src/control/settings.rs) (settings registry),
[control/cli.rs](../../src/control/cli.rs) and
[control/cli/human.rs](../../src/control/cli/human.rs) (clap, output, exit codes, start,
logs), [control/cli/model.rs](../../src/control/cli/model.rs),
[control/cli/key.rs](../../src/control/cli/key.rs), and
[control/cli/transcribe.rs](../../src/control/cli/transcribe.rs) (the commands
that run here), [macos_rune/control.rs](../../src/macos_rune/control.rs) (the macOS host),
the `/control` route in [local_api.rs](../../src/local_api.rs), and early
dispatch in [main.rs](../../src/main.rs).
[Command and JSON contracts](../cli.md) document flags, exit codes, and schema.

Checks: `control::tests` round-trip every command and reply (including
`a_printing_stop_answers_through_its_reply_channel`), refuse other
protocol versions, and cover the quit rules (an unanswered listener refuses)
and `model_selection`
(`model_selection_takes_only_scribe_and_a_language_it_can_be_told`), the model
list (`the_model_list_is_scribe_with_its_language_and_no_key`), and the key
report (`a_key_is_reported_by_its_source_and_tail_only`); `control::settings::tests` include
`every_persisted_setting_has_a_cli_key`, value parsing, shortcut conflicts,
and zero volume; `control::cli::tests` pin
exit codes, output (`status_shows_the_language_and_the_key_by_its_tail`,
`the_model_list_and_status_say_where_audio_goes_and_nothing_about_installs`,
`key_status_shows_its_source_tail_and_file_and_never_the_key`), and
`hex start`'s `open` arguments; `control::cli::model::tests` and
`control::cli::key::tests` pin the `model` and `key` trees
(`commands_for_models_on_this_computer_are_gone`,
`the_key_commands_are_set_status_clear_and_check`);
`desktop::onboarding::tests::relaunch_forwards_only_the_hex_environment`
pins restart's; `macos_rune::control::tests` check that `model use` applies
its exact selection (`model_use_applies_the_exact_selection`; macOS, not
compiled in this fork);
`dictation::audio::tests::marked_captures_are_intentional_without_a_hold`
covers the immediate stream and `ReadyIntentional`; `recognition::tests`
cover the stop replies and the paste-last refusal; `control::tests::cancel_parses_and_takes_no_flags`
covers `cancel` and its flags; local API's
`control_requests_are_authenticated_versioned_and_typed` covers authentication,
bounds, and error codes; `tests::control_commands_parse_beside_the_read_commands`
covers parsing next to `history` and `meetings`; `control::host::tests` cover
the quit refusal, printing stops, `model use`
(`using_the_model_applies_its_language_and_answers_at_once`), status
(`status_reports_the_language_and_that_no_key_is_set`), and runtime
`unsupported` replies;
`linux::tests` parse the shared tree beside Linux's own commands;
`rune_ui::platform::parity_tests` fail when a capability differs across OSes
without a recorded reason; `settings_name_real_capabilities_and_keys` checks
the settings-to-capability table; `control::cli::transcribe::tests` cover the
lab-off refusal, a non-WAV file refused before any transcription, the selected
model and language, only `scribe_v3` being nameable, a keyless run here
refused before any upload, and an empty transcript as `no-speech`.

Observed upstream (Oct 30, apollo, an isolated data root and `hex service`
without a display): vocabulary add/list/remove, history clear, window hide,
commands check, quit, start, and restart answered as documented. `hex status`,
`hex settings`, `hex model`, `hex key`, `hex transcribe`, and
`hex check-for-updates` are unverified in this fork: Linux is compiled and
unit-tested, Windows is type-checked by cross compilation only, macOS is not
compiled, and no command has sent anything to ElevenLabs.

Gaps: no command has run against a live app; the unit checks substitute the
host. The control loop's start/stop wiring (tone timing, recording behavior)
has no end-to-end test. A CLI volume change does not preview a tone as the window does. Onboarding
permission prompts, calendar connection, and meeting deletion or renaming have
no command. Linux and Windows dictation through the CLI has not run with a
microphone, and meeting recording through the CLI has not run on Linux or
Windows. No transcript answer from `hex transcribe` or `hex model transcribe`
has been observed, and neither has `hex key check` against ElevenLabs.
