# Hex (ElevenLabs Scribe fork)

Voice dictation for Linux: hold a key, speak, let go, and the text is pasted
into whatever app you're in. This is a personal fork of
[anomalyco/hex](https://github.com/anomalyco/hex) (MIT) that transcribes
through the [ElevenLabs Scribe](https://elevenlabs.io/speech-to-text) cloud API
instead of speech models on the computer.

It is developed on Linux (KDE Plasma on Wayland): there it compiles, its unit
tests pass, it installs and starts, and the installed app transcribes through
the real ElevenLabs service. A dictation spoken into the microphone and pasted
has not been tried yet (see [Not verified](#not-verified)). The Windows and macOS code from upstream is
still in the tree, but in this fork the Windows code has only been type-checked
by cross compilation and never run, and the macOS code was edited without being
compiled or run.

## Privacy

Hex sends your audio to ElevenLabs for every transcription: each dictation,
each chunk of a recorded meeting, an Annotate narration, History's "Transcribe
again", a request to the local HTTP API, and `hex model transcribe`. It is not
private and it does not work offline. Nothing is transcribed on the computer.
ElevenLabs handles the audio and the transcripts under its normal retention
policy; its zero-retention mode is for enterprise customers only. Recording a
meeting sends the other participants' audio to ElevenLabs too.

## Requirements

- An ElevenLabs API key. Without one Hex shows "API key required" and does not
  listen.
- A network connection whenever you dictate.
- A Vulkan driver, for the window only (the UI draws through wgpu).
- On Wayland, `wl-clipboard`, and `wtype` on wlroots compositors and niri.

## Build and install (Linux)

Nothing is published from this fork: no downloads, packages, releases, or
automatic updates. Install it from a checkout of this repository (branch
`scribe`), as your desktop user. You need a stable Rust toolchain and the
libraries listed in [docs/linux.md](docs/linux.md#dependencies).

```sh
./scripts/install-linux.sh
~/.local/bin/hex key set    # paste your ElevenLabs API key
~/.local/bin/hex
```

`scripts/install-linux.sh` runs `cargo build --locked --release` and installs
the result for the current user, without root: the executable under
`~/.local/share/hex/versions/<version>/`, a `~/.local/bin/hex` link, a launcher
entry, an autostart entry, and a `hex.service` systemd user unit. Run it again
to install a newer build. `./scripts/install-linux.sh uninstall` removes the
application and keeps settings, logs, and the saved key.

`scripts/build-linux.sh` only builds: it copies the release executable to
`dist/` and installs nothing.

The updater is permanently off. Upstream's update channels publish upstream
Hex, which would replace this fork, so Hex never checks for, downloads, or
installs an update. To update, pull and run the install script again.

## API key

Set the key in one of these ways:

- `hex key set`, then paste the key at the prompt (or `hex key set < key.txt`).
- Settings › General › Transcription in the window.
- The environment variable `ELEVENLABS_API_KEY`, which wins over a saved key.

The saved key is the file `elevenlabs.key` in Hex's data folder
(`~/.local/share/hex`), readable only by you. It is never written to
`settings.json` or logged, and Hex shows only its last four characters.
`hex key status` says which key is in use, `hex key check` asks ElevenLabs
whether it accepts the key without sending audio, and `hex key clear` removes
the saved one.

On Linux, save the key rather than exporting the variable. Hex normally runs
as a systemd user service that does not inherit your shell's environment, so
an `ELEVENLABS_API_KEY` exported in a terminal reaches the commands you run
there (`hex model transcribe`, `hex key check`, a foreground `hex dictate`)
but not the background service that handles the shortcut. To use the variable
anyway, add it to the unit with `systemctl --user edit hex.service`. The same
applies to `HEX_ELEVENLABS_URL`, which replaces the ElevenLabs base URL (for
example with a data-residency host). Details are in
[docs/linux.md](docs/linux.md#api-key).

## Transcription settings

There is one model, `scribe_v3`. Its language is `auto` by default; set
another in Settings or with `hex model use scribe_v3 --language de`. Three
more settings are in Settings › General › Transcription and in `hex settings`:

| Setting | Values | Default | What it does |
| --- | --- | --- | --- |
| `scribe.no_verbatim` | `on`, `off` | `off` | Leaves out filler words and false starts. |
| `scribe.numbers_format` | `auto`, `digits`, `words` | `auto` | How numbers are written. |
| `scribe.final_text` | `batch`, `live` | `batch` | Which model's text is pasted. |

With `batch`, the whole clip is uploaded when you release the shortcut and
transcribed by `scribe_v3`. With `live`, the audio is streamed to the realtime
model `scribe_v3_live` while you speak and its text is pasted; if that session
fails, the whole clip is transcribed by the batch model instead. A realtime
session is also opened when Labs › Live text or Live typing shows the words as
you speak.

When a transcription fails for a reason the audio could survive (no network, a
timeout, an ElevenLabs error, a rate limit, a missing or refused key, no
quota), Hex keeps the clip in `~/.local/share/hex/failed-dictations/` (at most
20 clips, for at most 7 days) and names the command that retries it:
`hex model transcribe <file>`.

## Cost

ElevenLabs bills the key's account for the audio it receives. From its
published pricing in November 2026: Scribe v3 (batch) about $0.20 per hour of
audio and Scribe v3 Live about $0.22 per hour. Silence is billed like speech.
Keyterm prompting costs extra, so the vocabulary is sent as keyterms only
while Labs › Vocabulary biasing is on. Check ElevenLabs' current prices before
relying on these numbers.

## Not verified

- Against the real ElevenLabs service only the command line and the two
  request paths were run (2026-11-11): `hex key check`, `hex model transcribe`
  on real speech (word for word on two clips), and the realtime session on a
  3 s and a 42 s clip. From this computer the batch model answered in about
  0.5 s for a 3 s clip, 0.8 s for 10 s, and 1.5 s for 42 s; the realtime model
  answered its final commit in about 0.17 s at any length.
  `scripts/scribe-latency.py` repeats the measurement; each run is billed.
  Details are in
  [docs/features/transcription.md](docs/features/transcription.md#verified-against-elevenlabs).
- A spoken dictation: holding the shortcut, the microphone path, and pasting
  were not exercised, and nobody walked through the first-run onboarding. The
  app was installed with `scripts/install-linux.sh` and launched on KDE Plasma (Wayland)
  on 2026-11-11: the window opened, KDE's portal granted the shortcuts, the
  listener reported Enabled, and the running service transcribed speech clips
  sent to its local API with the saved key.
- Windows: type-checked by cross compilation only, never run.
- macOS: edited without being compiled or run.

## What changed from upstream

- Every transcript comes from ElevenLabs. The speech models that ran on the
  computer, and everything for choosing, downloading, verifying, importing,
  and benchmarking them, are gone, along with the GPU/CPU speech engine
  setting, the AVX2 CPU requirement, and the Apple Speech backend.
- An ElevenLabs API key is required: `hex key set|status|clear|check`.
- `hex model` keeps `list`, `use`, `status`, `check`, and `transcribe`, with
  `scribe_v3` as the only model. `install`, `cancel`, `devices`, `import`,
  `rename`, `repair`, and `remove` are gone.
- No voice-activity model. Dictation keeps its level-only silence gate, and a
  meeting chunk that never reaches speech level is not sent.
- Vocabulary terms are sent as Scribe keyterms; "sounds like" spellings are
  not.
- The updater never runs.
- The website, the iOS app, the TypeScript SDK, the benchmarks, the release
  and signing tooling, and the Homebrew, AUR, `.deb`/`.rpm`, DMG, and Windows
  installer packaging were removed from the repository.

A `settings.json` written by upstream Hex still loads: its saved model name is
read as `scribe_v3`, its language is kept, and settings that no longer exist
are ignored.

More documentation is in [docs/](docs/README.md).

## License

MIT, like upstream. See [LICENSE](LICENSE) and
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
