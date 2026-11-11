# Project Documentation

This fork of Hex transcribes through the ElevenLabs Scribe cloud API. Start
with the document for your task. Plans do not override current behavior.

## User Guides

- [`../README.md`](../README.md): what the fork is, privacy, cost, and how to
  build, install, and set the API key.
- [`linux.md`](linux.md): Linux X11 and Wayland requirements, the source
  install, the API key, shortcuts, and paste. Linux is the platform this fork
  is developed and verified on.
- [`nix.md`](nix.md): the Nix package, development shell, and per-user
  autostart for Linux.
- [`windows.md`](windows.md): building and running on Windows x64 and ARM64.
  The Windows code is type-checked only and has never been run in this fork.
- [`cli.md`](cli.md): the `hex` control commands, their JSON replies, and the
  settings keys.
- [`transcript-cli.md`](transcript-cli.md): the read-only `hex history` and
  `hex meetings` commands and their JSON.

There is no macOS guide: the macOS code was edited without being compiled or
run in this fork.

## Engineering Reference

- [`../AGENTS.md`](../AGENTS.md): architecture, invariants, development
  commands, and diagnostics. This is the implementation authority.
- [`../ROADMAP.md`](../ROADMAP.md): the work list, including what this fork has
  not verified.
- [`features/README.md`](features/README.md): existing user-facing capabilities,
  entry points, available verification, and gaps.
  [`features/transcription.md`](features/transcription.md) covers what is sent
  to ElevenLabs, the API key, cost, and privacy.
- [`design-system.md`](design-system.md): Rune + `rune-kit` + OpenCode theme
  components, Hex pieces, copy rules, and `--snap` scene verification.
- [`streams.md`](streams.md): the local HTTP API's `/streams` routes, for
  another app that pushes audio to be transcribed.
- [`opencode-compatibility.md`](opencode-compatibility.md): audited OpenCode V2
  source and CLI baseline, integration contract, and validation limits.
- [`perf.md`](perf.md): how to run a copy of Hex that stays away from your own
  data, for a test or a measurement.

## Plans And Decision Records

Written for upstream Hex and kept where the thing they describe still exists.

- [`plans/rune-ui.md`](plans/rune-ui.md): implemented; Rune desktop UI port,
  surfaces, goldens, and the real-window checklist.
- [`plans/single-process.md`](plans/single-process.md): implemented; one Hex
  process per user on macOS, Windows, and Linux hosting Rune windows and overlays.
- [`plans/rune-dependency.md`](plans/rune-dependency.md): decision record for
  Rune as a public MIT Git dependency and Nix output-hash updates.
- [`plans/shared-desktop-ui.md`](plans/shared-desktop-ui.md): implemented; how
  macOS, Windows, and Linux share one window and one settings file.
- [`plans/capture-in-hex.md`](plans/capture-in-hex.md) and
  [`plans/capture-handover-spec.md`](plans/capture-handover-spec.md): Annotate,
  Scrawl's capture engine hosted in Hex.
- [`plans/personal-commands.md`](plans/personal-commands.md): the personal
  command contract and remaining lifecycle work.
- [`plans/quickjs.md`](plans/quickjs.md): why and how configs run in embedded
  QuickJS instead of Bun, with size and latency measurements.
- [`plans/granola-import.md`](plans/granola-import.md): research on importing
  historical Granola meetings into `hex mcp`.
- [`plans/release-service.md`](plans/release-service.md): upstream's release
  service, which the updater code was written against. The updater is
  permanently off in this fork and nothing is published.

When a plan disagrees with `AGENTS.md` about current behavior, update the plan.
