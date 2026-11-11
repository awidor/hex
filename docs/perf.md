# Running An Isolated Copy

This fork has no benchmark cases: upstream's strobe profile and its `perf`
cases are not in this repository. What remains is how to run a copy of Hex
that stays away from your own Hex, for a test or a measurement.

## Isolation

Start the copy with these in its environment:

- `HEX_APPLICATION_SUPPORT_DIR=<folder>`: settings, History, logs, locks,
  `local-api.json`, the saved API key, and kept failed dictations live in
  that folder instead of your data folder. A data folder other than the
  default also keeps the launch away from your `hex.service` unit on Linux and
  claims its own single-instance event on Windows, so the copy never signals
  or yields to your running Hex.
- `HOME` (and `USERPROFILE` on Windows): locates `~/.config/hex`, so your
  `hex.config.ts` stays out of the copy.
- `HEX_DISABLE_UPDATES=1`: no update checks. The updater is permanently off
  in this fork, so the variable changes nothing here.
- `HEX_AUDIO_FIXTURE=<wav>`: every microphone open plays that file instead,
  from its start, in 10 ms chunks at real-time pace, and no device is opened,
  so nothing is recorded.

A `settings.json` with `"enable_on_launch": false` in that folder keeps the
listener off: no keyboard hook and no microphone.

Never point a test or a measurement at your own data folder.

## What still reaches ElevenLabs

An isolated copy is not an offline copy. With an API key in its environment
or its data folder, every dictation it finishes is sent to ElevenLabs and
billed, including audio played from `HEX_AUDIO_FIXTURE`. Without a key its
listener shows "API key required" and does not start.

To time the service itself, use `scripts/scribe-latency.py`, which sends the
same batch and live requests Hex sends. It needs a key and every run is
billed. No latency has been measured in this fork.
