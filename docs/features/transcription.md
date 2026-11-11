# Transcription (ElevenLabs Scribe)

Every transcript comes from the ElevenLabs Scribe cloud API. Audio leaves the
computer on every dictation, meeting chunk, Annotate narration, History
"Transcribe again", local-API request, `hex transcribe`, and
`hex model transcribe`. Nothing is transcribed on this computer, and nothing is
transcribed without a network connection and an ElevenLabs API key. There is
one model, `scribe_v3`, with nothing to download or install; what a person
chooses is the language.

No request was ever made to ElevenLabs while this was built (no API key was
available). Everything below is implemented behavior with unit tests against
fake servers on loopback; see [Not Verified](#not-verified).

## How To Get To It

```ts
Transcription                                    // one model; every OS
├── API key                                      // transcribe.key
│   ├── Settings › General › Transcription › ElevenLabs API key
│   ├── First run -> the onboarding's key step
│   ├── hex key set | status | clear | check
│   └── ELEVENLABS_API_KEY (overrides the saved key)
├── Language                                     // transcribe.language
│   ├── Settings › General › Transcription › Language
│   ├── hex model use scribe_v3 --language <code|auto>
│   └── Labs: keyboard language and the app in front switch it
├── Final text: Batch | Live                     // transcribe.final-text
├── Leave out filler words, Numbers              // transcribe.settings
├── Vocabulary as keyterms                       // transcribe.keyterms
│   └── General › Vocabulary + Labs › Vocabulary biasing
├── hex model list | status | check              // what is selected, key, where audio goes
└── hex model transcribe <wav>                   // transcribe.failed-clips: retry a kept clip
```

## Sub-features

### API Key

```ts
The key audio is sent with                       // transcribe.key
├── Read for every request and every live session
│   ├── ELEVENLABS_API_KEY set and usable -> it wins over the saved key
│   ├── Else the saved file
│   └── Trimmed; empty, over 512 characters, or not printable ASCII -> not a key
├── Saved file -> elevenlabs.key in Hex's data folder (the folder that holds
│   settings.json; ~/.local/share/hex on Linux)
│   ├── Unix: created 0600, staged and renamed into place
│   └── Windows: inherits the per-user data folder's ACL
├── Never written to settings.json, never logged, never part of an error or reply
├── Shown only as ••••abcd, its last four characters (8 characters or fewer -> ••••)
├── Settings › General › Transcription › ElevenLabs API key
│   ├── Status -> "Not set. Dictation won't work until you add one" (danger),
│   │   "••••abcd saved in Hex", or "From the ELEVENLABS_API_KEY environment variable"
│   ├── Secure field + Save (or Return) -> saves, replacing any saved key,
│   │   empties the field, then checks it
│   ├── Test key -> checks the typed key, else the key in use
│   │   -> "Checking…", then "Key works" or the reason
│   ├── Remove -> only while a saved key is the one in use
│   ├── Key from the environment -> Test key only; no field, Save, or Remove
│   ├── Always under the row: "Audio you dictate is sent to ElevenLabs for transcription"
│   └── A key set or removed outside the window shows within about a second
├── First run -> the onboarding opens on the key step
│   ├── Continue -> checks the typed key off the UI thread; saves it only once
│   │   ElevenLabs accepts it
│   ├── Skip for now -> setup finishes without a key; the practice is passed over
│   └── A key already set passes the step
├── hex key set [KEY | -] -> saves for this user
│   ├── No KEY, or `-` -> standard input: a prompt with echo off at a Unix
│   │   terminal (Windows consoles echo; the prompt says so), or a pipe or
│   │   file read up to 4096 bytes
│   ├── Empty -> invalid-value; text that cannot be a header -> failed
│   └── Reply names only the last four characters, and says when
│       ELEVENLABS_API_KEY overrides the saved key
├── hex key status [--json] -> set or not, source, last four characters, key file path
├── hex key clear -> removes the saved key; a key in the environment is left alone
└── hex key check, hex model check, Test key -> GET /v1/user with the key
    ├── Sends no audio and spends no quota
    ├── 2xx -> accepted
    ├── 403 -> accepted (a key limited to Speech to Text may not read the account)
    ├── No key -> api-key-missing, before any request
    ├── 401 -> api-key-invalid
    ├── Quota, rate limit, 5xx, unreachable (15 s limit) -> failed, with the cause
    └── CLI exits 1 on every failure

Without a key                                    // transcribe.key-gate
├── The listener does not start; its status is "API key required"
│   (window, tray, hex status)
│   └── It starts by itself once a key is set (asked for again on the host's
│       refresh; a key's presence is remembered for 1 s)
├── Meetings: recording, importing, and finishing a pending transcript are
│   refused with the missing-key message
├── hex model transcribe -> api-key-missing before anything is uploaded
├── Local API /transcriptions and /streams -> 409 api-key-missing at the
│   request's headers, before any audio is uploaded
├── No live session opens and the connection is not warmed
└── Any other request fails with "no ElevenLabs API key is set
    (Settings › General, or `hex key set`)"; a dictation's clip is kept
    (transcribe.failed-clips)
```

On Linux the saved key file (`hex key set`, or Settings) is the dependable way
to set the key. Hex normally runs as a systemd user service (`hex.service`)
whose desktop environment is a fixed list of variables (`SESSION_KEYS` and
`INHERITED_KEYS` in [linux/service.rs](../../src/linux/service.rs)), so an
`ELEVENLABS_API_KEY` exported in a shell reaches commands run in that shell
(`hex model transcribe`, `hex key check`, a foreground `hex dictate`) but not
the background service, unless it is added to the unit's own environment
(`systemctl --user edit hex.service`). The same holds for `HEX_ELEVENLABS_URL`.

Source: [transcription/scribe/key.rs](../../src/transcription/scribe/key.rs)
(`current`, `is_set`, `save`, `clear`, `redacted`), `check_key` in
[transcription/scribe.rs](../../src/transcription/scribe.rs),
[control/cli/key.rs](../../src/control/cli/key.rs),
[rune_ui/api_key.rs](../../src/rune_ui/api_key.rs),
[rune_ui/general/transcription.rs](../../src/rune_ui/general/transcription.rs)
(`key_rows`),
[rune_ui/onboarding_kit/live.rs](../../src/rune_ui/onboarding_kit/live.rs)
(`continue_key`, `skip_key`), and
[desktop/onboarding.rs](../../src/desktop/onboarding.rs). The gate is
`transcription::models::is_ready` in
[transcription/models.rs](../../src/transcription/models.rs), asked by
`start` and `refresh` in [linux/app.rs](../../src/linux/app.rs) and
[windows/app.rs](../../src/windows/app.rs), by `setup_ready` in
[macos_rune/runtime.rs](../../src/macos_rune/runtime.rs), by
`require_api_key` in [meeting.rs](../../src/meeting.rs), and by `KeyGate` in
[local_api/key.rs](../../src/local_api/key.rs).

Checks: `the_environment_wins_over_the_saved_key`,
`a_saved_key_is_readable_by_its_owner_only_and_can_be_cleared`,
`text_that_cannot_be_a_header_is_not_a_key`, and
`a_key_is_only_ever_shown_by_its_tail` in scribe/key.rs (temporary folders);
`a_key_is_checked_without_audio` and
`a_refused_key_is_reported_and_a_scoped_key_is_accepted` in scribe.rs (fake
server on loopback); `the_key_commands_are_set_status_clear_and_check`,
`set_reads_standard_input_unless_given_a_key`,
`set_refuses_an_empty_or_unreadable_key_without_echoing_anything`,
`a_check_without_a_key_fails_before_asking_elevenlabs`,
`a_check_reports_what_elevenlabs_said_and_never_the_key`,
`status_names_no_key_in_tests_and_never_carries_one`, and
`a_saved_keys_commands_say_when_the_environment_overrides_it` in
control/cli/key.rs; `a_saved_key_shows_only_its_tail_and_empties_the_field`,
`testing_with_no_key_says_so_without_asking_elevenlabs`,
`the_first_runs_key_is_saved_only_once_it_passes`, and
`only_a_saved_key_can_be_removed` in rune_ui/api_key.rs (a fixture window; no
key is read, saved, or checked);
`dictation_setup_requires_permissions_and_an_api_key`,
`setup_waits_for_the_api_key_as_well_as_the_permissions`,
`skipping_the_key_finishes_setup_and_passes_over_the_practice`, and
`platforms_without_permissions_ask_only_for_the_key` in desktop/onboarding.rs;
`a_settings_parse_note_does_not_block_capture` in linux/app.rs (a start
without a key stops at "API key required");
`transcribing_here_without_a_key_is_refused_before_any_upload` in
[control/cli/transcribe.rs](../../src/control/cli/transcribe.rs);
`a_request_without_a_key_is_told_how_to_set_one` in local_api/key.rs. Unit tests
never read this computer's key: `key::current` answers none under test. Not
verified against ElevenLabs: that `GET /v1/user` answers a valid key with 2xx
and a Speech-to-Text-only key with 403, and that a real key file survives the
service's environment as described. Not observed: the listener starting by
itself after a key is set in a running Hex.

### What Is Sent Where

```ts
Whole clip -> batch model scribe_v3              // transcribe.batch
├── POST https://api.elevenlabs.io/v1/speech-to-text, header xi-api-key
├── multipart/form-data
│   ├── model_id=scribe_v3
│   ├── file -> the clip as raw 16 kHz mono 16-bit little-endian PCM
│   │   (file_format=pcm_s16le_16); nothing is encoded or written to disk
│   ├── tag_audio_events=false, diarize=false
│   ├── timestamps_granularity=word only for callers that need word times
│   │   (meetings, Annotate, the local API with timestamps); else none
│   ├── language_code only when a language is chosen       // transcribe.language
│   ├── no_verbatim=true, numbers_format only when set     // transcribe.settings
│   └── keyterms, one field per term                       // transcribe.keyterms
├── A clip under 100 ms is padded with silence to 100 ms
├── Limits: 8 s to connect; 30 s plus the clip's own length per attempt;
│   a 16 MiB answer
├── Answer -> text, plus words with start and end times
│   ├── audio_event entries dropped
│   └── Segments are Hex's grouping of the words: a new one after sentence
│       punctuation or a 1 s gap
├── Through the proxy the computer uses for HTTPS (net)
└── Who sends one
    ├── A dictation's finish (shortcut, hex dictation, local API /dictations)
    ├── Each meeting chunk that reaches speech level (meetings.md#transcribe)
    ├── Annotate narration (capture.md); macOS with capture.transcriber
    │   `apple` uses Apple's recognizer instead
    ├── History › Transcribe again (Labs › Keep recordings)
    ├── Local API POST /transcriptions, and /streams windows
    ├── hex transcribe <file> (Labs › Transcribe files)
    └── hex model transcribe <wav>, from the CLI's own process

Live session -> realtime model scribe_v3_live    // transcribe.live
├── WebSocket wss://api.elevenlabs.io/v1/speech-to-text/realtime, header xi-api-key
│   └── Query: model_id=scribe_v3_live, audio_format=pcm_16000,
│       commit_strategy=manual, language_code, no_verbatim, keyterms
├── Opened for a dictation capture only when its text is used
│   ├── Its text is shown: Labs › Live text, Labs › Live typing (Linux
│   │   Wayland), the first run's practice page, or a local API /dictations
│   │   client that asked for partials
│   ├── OR Final text is Live
│   ├── AND a key is set
│   └── Else no session (the default): a session is billed for all it hears
├── Connecting never holds the capture: one thread owns the socket (4 s to
│   connect); audio fed meanwhile is sent once the session is up
├── Audio -> input_audio_chunk messages, base64 PCM, at most 0.5 s each
├── partial_transcript -> tentative text; committed_transcript -> stable text
├── Release -> one empty chunk with commit=true -> the committed text
│   └── Nothing said -> insufficient_audio_activity or commit_throttled -> no text
├── Its text is provisional unless Final text is Live    // transcribe.final-text
├── A failed session reports itself on the next feed or at the finish; the
│   whole clip is transcribed by the batch model instead
├── numbers_format is not sent to a live session
├── Connects directly: the HTTP proxy settings are NOT applied to the WebSocket
└── Local API /streams opens one too, as a session that gives way to dictation

Where requests go                                // transcribe.endpoint
├── https://api.elevenlabs.io unless HEX_ELEVENLABS_URL names another http(s)
│   base (such as a data-residency host); https becomes wss for the session
├── hex model list | status -> the model, its live model, the language, the
│   key in use (last four characters), and this URL
└── A capture's start opens the HTTPS connection in the background (a HEAD
    request with no key and no audio) so the upload at release does not wait
    for DNS and TLS; at most once per 20 s, and never without a key
```

One engine thread sends every request, in order, so requests never overlap.
Meeting chunks and local-API host audio are background work that gives way to
a dictation request sent before they start. A job fails as timed out 90 s plus
3 s per second of audio after the engine starts it; time queued behind other
work is not counted. The capture-side mechanics of a live session (when it
opens relative to the hold threshold, feed cadence, long pauses cut for the
session only) are in [dictation.md](dictation.md#streaming-finish).

Source: [transcription/scribe/client.rs](../../src/transcription/scribe/client.rs)
(`transcribe`, `multipart`, `parse`),
[transcription/scribe/live.rs](../../src/transcription/scribe/live.rs)
(`LiveSession`, `url`, `connect`, `receive`),
[transcription/scribe.rs](../../src/transcription/scribe.rs)
(`ScribeTranscriber`, `stream_wanted`, `base_url`, `warm`),
[transcription/engine.rs](../../src/transcription/engine.rs) (`begin`,
`finish`, `foreground`, `JOB_DEADLINE`), and
[control/cli/model.rs](../../src/control/cli/model.rs).

Checks: `a_clip_is_uploaded_as_raw_pcm_with_its_options`,
`defaults_send_no_optional_field_and_no_word_times`,
`a_clip_shorter_than_the_api_takes_is_padded_with_silence`,
`words_keep_their_times_and_group_into_sentences`, and
`a_long_pause_ends_a_segment_without_punctuation` in scribe/client.rs;
`words_come_back_as_spans_for_a_capture` in scribe.rs;
`audio_goes_up_partials_come_back_and_a_commit_returns_the_text`,
`committed_segments_accumulate_ahead_of_the_tentative_text`,
`a_host_that_is_down_fails_the_session_not_the_caller`,
`a_cancelled_finish_stops_waiting`, and
`a_commit_with_nothing_said_returns_no_text` in scribe/live.rs;
`the_job_deadline_grows_with_the_clip` and
`background_work_yields_to_dictation_sent_before_it_runs` in engine.rs;
`the_model_commands_are_list_use_status_check_and_transcribe` and
`commands_for_models_on_this_computer_are_gone` in control/cli/model.rs. The
client and live tests are unit tests with a fake server on loopback: they
prove what Hex sends and how it reads the answers the tests script, not that
ElevenLabs accepts those requests or answers in those shapes. Not verified
against ElevenLabs: any field, the PCM upload, the message names of the
realtime protocol, `HEX_ELEVENLABS_URL` against a real residency host, the
proxy path, and the connection warm-up's effect. No test covers the warm-up
or that the WebSocket bypasses the proxy; both are read from the code.

### Final Text

```ts
Settings › General › Transcription › Final text  // transcribe.final-text
├── Batch (default) -> "Most accurate: the whole clip is transcribed when you release"
│   ├── No live text shown -> no live session; one batch request at release
│   └── Live text shown -> a live session shows provisional words and is
│       never committed; the batch request still decides the text
│       (source=offline), so the audio is sent twice
└── Live -> "Fastest: uses the live model's text"
    ├── The session's committed text is the transcript (source=stream); no
    │   second upload
    └── Whole clip sent to the batch model instead (source=fallback) when
        ├── the session could not connect or failed (key refused, quota,
        │   rate limit, connection lost, closed by ElevenLabs)
        ├── it returned no text
        ├── it was fed more than 250 ms past the clip's end
        ├── it heard only quiet audio while a pause was cut
        ├── no committed text arrived within 1.5 s of the finish
        └── it was closed before the finish (another command interrupted it)
```

The choice is read as each session opens and applies to the next dictation.
Each dictation logs `source=stream|offline|fallback` and `finish_ms`;
"offline" names the whole clip in one batch request, not work without a
network.

Source: `FinalText`, `final_from_live`, and `stream_wanted` in
[transcription/scribe.rs](../../src/transcription/scribe.rs); `stream`,
`provisional_only`, and `STREAM_FINISH_DEADLINE` in
[transcription/engine.rs](../../src/transcription/engine.rs); `finish_stream`
and `PauseCap` in
[transcription/engine/streaming.rs](../../src/transcription/engine/streaming.rs);
`PROVISIONAL` in
[transcription/scribe/live.rs](../../src/transcription/scribe/live.rs).

Checks: `settings_reach_the_next_request` in scribe.rs;
`a_provisional_session_never_gives_the_transcript` in scribe/live.rs (fake
server on loopback); `a_provisional_stream_is_told_apart_from_a_failed_one`,
`a_stream_that_heard_only_quiet_audio_falls_back_to_the_whole_clip`,
`source_labels_distinguish_fallbacks`,
`an_unusable_stream_falls_back_to_the_complete_clip`, and
`a_finish_without_a_stream_transcribes_the_clip_offline` in engine.rs (fake
streams and offline closures; no network);
`scribe_settings_default_to_the_accurate_path_and_round_trip` in
[app_settings.rs](../../src/app_settings.rs). Not verified against
ElevenLabs: either path end to end, how the two models' texts differ, and the
1.5 s finish deadline against the real commit round trip. Latency of both
paths is unmeasured.

### Language

```ts
Settings › General › Transcription › Language    // transcribe.language
├── Auto (default) -> no language_code is sent; Scribe detects the language
├── A named language -> its ISO code is sent with every request and session
├── The list -> Auto first, then the languages Hex names (languages.rs)
│   └── A saved code the list lacks (set from the CLI) shows as its own choice
├── hex model use scribe_v3 --language <code|auto>
│   ├── Any ISO 639-1 or 639-3 code: two or three lowercase ASCII letters;
│   │   whether Scribe knows it is ElevenLabs' to say
│   ├── Anything else ("EN", "en-US", "english") -> refused
│   ├── Running Hex -> applies to its next dictation; none running -> saved
│   │   to settings.json here
│   └── A model other than scribe_v3 -> refused
├── hex settings apply -> transcription.language (transcription.model takes
│   only scribe_v3); both read-only to hex settings set
├── No saved selection -> Auto
├── Labs › keyboard-language and app switching change this language only
│   (README.md#switch-models-with-keyboard-language,
│   README.md#switch-models-with-the-app-in-front)
└── settings.json -> "transcription": {"model": "scribe_v3", "language": "<code>|auto"}
    └── Written by upstream Hex -> any saved model name loads as scribe_v3,
        the language is kept, removed keys are ignored
```

Source: [transcription/models.rs](../../src/transcription/models.rs)
(`TranscriptionSelection`, `validate`),
[transcription/languages.rs](../../src/transcription/languages.rs),
`language_row` in
[rune_ui/general/transcription.rs](../../src/rune_ui/general/transcription.rs),
`ScribeTranscriber::language` in
[transcription/scribe.rs](../../src/transcription/scribe.rs),
[control/cli/model.rs](../../src/control/cli/model.rs), and
[control/settings/registry.rs](../../src/control/settings/registry.rs).

Checks: `the_model_saves_as_its_wire_name`,
`a_selection_saved_by_upstream_hex_keeps_its_language`,
`a_request_naming_a_local_model_is_refused`,
`auto_and_iso_codes_validate_and_anything_else_does_not`,
`settings_offer_auto_first_and_every_named_language`, and
`a_language_scribe_cannot_be_told_falls_back_to_auto` in models.rs;
`a_selection_is_a_language_and_auto_sends_none` in scribe.rs;
`settings_saved_by_upstream_hex_load_as_scribe_with_their_language` and
`automatic_language_selection_round_trips_explicitly` in app_settings.rs;
`only_scribe_can_be_named_and_the_language_can_be_given` in
control/cli/transcribe.rs. Not verified against ElevenLabs: that Scribe
accepts each code the list names, what it answers for a code it does not
know, and Auto's detection.

### Scribe Settings

```ts
settings.json `scribe`                           // transcribe.settings
├── no_verbatim (false) -> General › Transcription › Leave out filler words
│   ├── "Also drops false starts and sounds that aren't speech"
│   └── On -> no_verbatim=true on batch requests and live sessions
├── numbers_format (auto) -> General › Transcription › Numbers: Auto | Digits | Words
│   ├── Auto -> the field is not sent; Scribe chooses
│   └── Batch requests only
├── final_text (batch) -> General › Transcription › Final text  // transcribe.final-text
├── hex settings get|set scribe.no_verbatim | scribe.numbers_format | scribe.final_text
├── Applied whenever settings load or save -> the next request uses them
└── A settings file without `scribe` loads the defaults
```

Labs › Remove filler words is a different thing: Hex's own text step on the
finished transcript ([labs.md](labs.md)), independent of `no_verbatim`.

Source: `ScribeSettings` and `apply` in
[transcription/scribe.rs](../../src/transcription/scribe.rs), `NumbersFormat`
and `multipart` in
[transcription/scribe/client.rs](../../src/transcription/scribe/client.rs),
[rune_ui/general/transcription.rs](../../src/rune_ui/general/transcription.rs),
and [control/settings/registry.rs](../../src/control/settings/registry.rs).

Checks: `settings_reach_the_next_request` in scribe.rs;
`a_clip_is_uploaded_as_raw_pcm_with_its_options` and
`defaults_send_no_optional_field_and_no_word_times` in scribe/client.rs (fake
server on loopback);
`scribe_settings_default_to_the_accurate_path_and_round_trip` in
app_settings.rs. Not verified against ElevenLabs: what `no_verbatim` and
`numbers_format` do to a real transcript. Not observed: the three rows in a
running window.

### Vocabulary As Keyterms

```ts
The saved vocabulary as Scribe keyterms          // transcribe.keyterms
├── Sent only while Labs › Vocabulary biasing is on AND vocabulary_boost is
│   not off; ElevenLabs bills keyterms extra
│   └── Light, Normal, and Strong send the same terms: Scribe takes no strength
├── Read at the start of each request and session, in list order
├── Batch -> up to 1000 terms, each at most 50 characters and 5 words, with
│   none of < > { } [ ] \; one `keyterms` form field per term
├── Live session -> up to 50 terms of at most 20 characters, same rules
├── A term over a limit is skipped, not cut; the rest are still sent
├── "Sounds like" spellings are not sent; rewriting and search use them
└── macOS dictation -> the focused mode's own terms first, then the shared list
```

The list, its editor, and the other things that read it are in
[vocabulary.md](vocabulary.md).

Source: `keyterms` and `keyterms_of` in
[dictation/vocabulary.rs](../../src/dictation/vocabulary.rs); `keyterms`,
`keyterm_ok`, and `MAX_KEYTERMS` in
[transcription/scribe/client.rs](../../src/transcription/scribe/client.rs);
`url` in [transcription/scribe/live.rs](../../src/transcription/scribe/live.rs).

Checks: `keyterms_are_the_terms_scribe_accepts_in_list_order` and
`every_saved_term_fits_scribes_keyterm_limit` in dictation/vocabulary.rs;
`a_clip_is_uploaded_as_raw_pcm_with_its_options` in scribe/client.rs (fake
server on loopback: terms arrive as separate fields and refused ones are
left out); `audio_goes_up_partials_come_back_and_a_commit_returns_the_text`
in scribe/live.rs. No test covers the Labs gate or the boost switch in
`keyterms()`; they are read from the code. Not verified against ElevenLabs:
the limits themselves (taken from its API reference and keyterm guide), that
keyterms change a transcript, and what they cost.

### Retry And Failures

```ts
A batch request that fails                       // transcribe.retry
├── Retried once, 400 ms later, when the failure may pass
│   ├── Could not reach ElevenLabs (no network, DNS, TLS, connection lost)
│   ├── HTTP 5xx
│   └── HTTP 429 or a rate-limit status
├── Not retried
│   ├── A timeout: it already spent the clip's whole allowance
│   ├── A refused key, spent quota, another 4xx, an unreadable answer
│   └── A job its waiter already abandoned: nothing is sent
└── The second failure is the answer

Failure messages by cause                        // transcribe.failures
├── No key -> "no ElevenLabs API key is set (Settings › General, or `hex key set`)"
├── HTTP 401, or status invalid_api_key | missing_api_key
│   -> "ElevenLabs refused the API key; check it in Settings"
├── Status naming quota (ElevenLabs answers it with 401 too), or HTTP 402
│   -> "the ElevenLabs account has no transcription quota left"
├── HTTP 429, or a rate-limit status
│   -> "ElevenLabs is rate-limiting this account; try again shortly"
├── No network, DNS, TLS, timeout -> "couldn't reach ElevenLabs: <reason>"
├── HTTP 5xx -> "ElevenLabs failed (HTTP <status>)"
├── Another 4xx -> "ElevenLabs refused the request (HTTP <status>): <its
│   message, cut to 200 characters>"
├── A 2xx that is not the documented JSON
│   -> "ElevenLabs sent an answer Hex can't read: line <n>, column <n>"
├── Never carries the key, the audio, or the transcript
├── Live session failures are logged as the fallback's reason; the person
│   sees the batch request's result
├── Local API -> 500 transcription-failed with the message
└── hex model transcribe -> "couldn't transcribe it: <message>", exit 1;
    no speech -> exit 4, nothing printed
```

Source: [transcription/scribe/error.rs](../../src/transcription/scribe/error.rs)
(`ScribeError`, `from_response`, `retryable`), `transcribe` in
[transcription/scribe/client.rs](../../src/transcription/scribe/client.rs),
`receive` and `connect` in
[transcription/scribe/live.rs](../../src/transcription/scribe/live.rs),
`transcription_error` in
[local_api/transcriptions.rs](../../src/local_api/transcriptions.rs), and
`here` in [control/cli/transcribe.rs](../../src/control/cli/transcribe.rs).

Checks: `a_refused_key_is_named_whatever_the_body_says`,
`spent_quota_and_rate_limits_are_told_apart`,
`a_rejected_request_keeps_the_servers_reason`,
`a_server_failure_is_retried_and_its_audio_kept`, and
`a_long_server_message_is_clipped` in scribe/error.rs;
`a_refused_key_fails_at_once_without_a_retry`,
`a_server_failure_is_retried_once`, `a_second_server_failure_is_the_answer`,
`nothing_is_sent_for_a_cancelled_job`, `an_unreachable_host_is_named_as_such`,
and `an_answer_that_is_not_a_transcript_is_malformed` in scribe/client.rs
(fake server on loopback); `a_refused_key_fails_the_next_feed` in
scribe/live.rs; `a_failed_request_to_elevenlabs_says_why` in
local_api/transcriptions.rs; `an_empty_transcript_is_no_speech` in
control/cli/transcribe.rs. The error bodies in these tests are written from
ElevenLabs' documentation. Not verified against ElevenLabs: the real status
codes and body shapes for a refused key, spent quota, and rate limiting, and
so whether each cause gets the message above. No test covers a timed-out
request skipping its retry.

### Kept Failed Clips

```ts
A dictation whose transcription fails            // transcribe.failed-clips
├── Kept when the same audio could still be transcribed later
│   ├── No key, a refused key, no quota, a rate limit, HTTP 5xx,
│   │   unreachable or timed out, an unreadable answer
│   └── Not kept: a request ElevenLabs refused as such (another 4xx), an
│       abandoned job, a failure that is not ElevenLabs', an empty clip
├── Where -> failed-dictations/dictation-<unix ms>.wav in Hex's data folder
│   ├── 16 kHz mono 16-bit WAV; folder 0700 and file 0600 on Unix
│   └── With Labs › Skip long pauses on, the clip as it was sent (pauses cut)
├── The failure message gains "; the audio was kept: hex model transcribe <file>"
├── At most 20 clips and at most 7 days; older ones are removed whenever a
│   clip is added
├── Only dictation finishes keep a clip; meetings, Annotate, the local API,
│   and the CLI do not
├── A clip that could not be written -> the message alone, and a log warning
└── Retry -> hex model transcribe <file> [--language <code|auto>] [--json]
    ├── Sends the WAV from the CLI's own process with the saved language and
    │   Scribe settings; prints the text
    ├── Nothing is pasted, saved to History, or processed
    └── The clip stays until its age or the count removes it
```

Nothing retries a kept clip by itself. Outside this folder, dictation audio is
written to disk only by Labs › Keep recordings and the
`HEX_RETAIN_DICTATION_AUDIO` diagnostic.

Source: [transcription/failed_clips.rs](../../src/transcription/failed_clips.rs)
(`describe`, `keep_in`, `prune`), `worth_retrying_later` in
[transcription/scribe/error.rs](../../src/transcription/scribe/error.rs),
`finish` in [transcription/engine.rs](../../src/transcription/engine.rs)
(Linux and Windows), [macos_dictation.rs](../../src/macos_dictation.rs)
(macOS), and `run_here` in
[control/cli/transcribe.rs](../../src/control/cli/transcribe.rs).

Checks: `a_kept_clip_is_an_owner_only_wav_the_transcriber_reads_back`,
`only_the_newest_clips_are_kept`,
`old_clips_are_removed_when_a_new_one_is_kept`, and
`a_request_the_clip_would_fail_again_is_not_kept` in failed_clips.rs
(temporary folders); `a_server_failure_is_retried_and_its_audio_kept` and
`a_rejected_request_keeps_the_servers_reason` in scribe/error.rs. Unit tests
never write into this computer's data folder (`keep` returns nothing under
test), so no test covers a failed dictation producing a file and its message
end to end. Not observed: a kept clip retried with `hex model transcribe`
against ElevenLabs.

### Cost And Privacy

From ElevenLabs' published pricing and documentation, November 2026; Hex
measures and enforces none of it.

```ts
Cost
├── Scribe v3 batch -> about $0.20 per hour of audio
├── Scribe v3 Live -> about $0.22 per hour; a session is billed for all it
│   hears, which is why one opens only when its text is used
├── Keyterm prompting -> billed extra; sent only on request  // transcribe.keyterms
├── Silence is billed
│   ├── Dictation: a capture under the level-only silence gate is discarded
│   │   without a batch request; an open live session has already heard it
│   │   (dictation.md)
│   ├── Live session: long pauses are cut to 2.5 s and 0.3 s
│   └── Meetings: chunks that never reach speech level (-55 dBFS) are not sent
└── Sent twice (the session, then the whole clip): a dictation whose live
    text is shown under Final text: Batch, and a Final text: Live dictation
    whose session fails

Privacy
├── Audio and transcripts are handled under ElevenLabs' normal retention
│   policy; zero-retention mode is for its enterprise customers only
├── Recording a meeting sends the other participants' audio to ElevenLabs too
├── Sent with each request: the audio, the language, the settings above,
│   and vocabulary terms when biasing is on
└── On this computer: the key file, kept failed clips, and whatever
    History, Meetings, and Annotate retain by their own rules
```

`HEX_ELEVENLABS_URL` can point requests at an ElevenLabs data-residency host;
it changes where audio goes, not how long it is kept.

## Not Verified

- No real request was ever made to ElevenLabs during development: no API key
  was available. Every wire detail above (fields, message names, status codes,
  error bodies, limits) is from ElevenLabs' documentation and is exercised only
  against fake servers on loopback.
- Latency is unmeasured on both paths.
  [`scripts/scribe-latency.py`](../../scripts/scribe-latency.py) measures it
  with the same requests Hex sends (batch upload, and a live session streamed
  at speaking pace then committed); it has not been run.
- Linux (KDE Wayland): compiled and unit-tested; not run against ElevenLabs.
- Windows: type-checked by cross compilation only; never run.
- macOS: edited without being compiled or run.
- A listed check is a locator, not a recorded passing run.
