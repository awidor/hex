# Live transcription streams (`/streams`)

`/streams` lets another app on this computer transcribe its own audio live
through Hex. The app records the microphone (or anything else) and pushes
16 kHz mono samples; Hex sends partial text while it listens, one final
segment per window, and the final text when the app finishes.

Hex transcribes nothing itself. All audio pushed to `/streams` is sent on to
ElevenLabs (Scribe) with the user's API key and is billed to that key, so an
app that pushes audio here is sending it to ElevenLabs. No request was ever
made to the real ElevenLabs service while this was built (no API key was
available): how the routes behave against it, and how fast text comes back,
are unmeasured.

The running Hex app serves the routes. The code is
`src/local_api/streams.rs` (routes) and `src/transcription/stream.rs`
(windows, priority, lease); the requests to ElevenLabs are in
`src/transcription/scribe.rs` and `src/transcription/scribe/live.rs`.

## Availability

- The running Hex app on macOS, Linux, and Windows, while dictation's engine
  runs (on Linux and Windows, once the listener is on) and an ElevenLabs API
  key is set (`hex key set`, Settings › General, or `ELEVENLABS_API_KEY`).
  This fork has only been run on Linux: its Windows code is type-checked by
  cross compilation only, and its macOS code was edited without being
  compiled or run.
- `GET /capabilities` reports `"streams": true` when the routes exist,
  `"apiKeySet"` for whether a key is set, and `"outputProcessing": true` when
  `process=global` would run (Labs › Process API transcripts on, and the
  running app installed its processor).
- There is one model, `scribe_v3`, which runs at ElevenLabs. A stream uses the
  language dictation holds and never changes it. Without a key, creating a
  stream answers `409 api-key-missing`; without a running engine,
  `503 engine-unavailable`.

## Discovery and auth

Read the owner-only discovery file `local-api.json` in Hex's data folder
(`~/Library/Application Support/hex` on macOS, `$XDG_DATA_HOME/hex` or
`~/.local/share/hex` on Linux, `%APPDATA%\hex` on Windows) (`port`, `token`,
`apiVersion`), then call `http://127.0.0.1:<port>` with:

| Header | On | Value |
| --- | --- | --- |
| `Authorization` | every request | `Bearer <token>` from discovery |
| `X-Hex-Stream-Token` | every `/streams/{id}/…` request | the stream's `ownerToken` |
| `Content-Type` | create / audio | `application/json` / `application/octet-stream` |
| `Content-Length` | every request with a body | required; chunked bodies are refused |

A wrong or missing stream token answers `404 stream-not-found`, the same as an
unknown id, so ids can't be probed.

## Audio format

16 000 Hz, mono, little-endian, declared when the stream is created:

- `f32le`: 32-bit floats in [-1, 1] (values outside are clamped; NaN or
  infinity is refused).
- `s16le`: signed 16-bit integers.

Each audio request carries whole samples only. Push small chunks as you record
them; 100 ms to 1 s per request is typical.

## Endpoints

### `POST /streams`

```json
{ "sampleFormat": "f32le", "sampleRate": 16000, "model": "scribe_v3", "language": "en" }
```

`model` and `language` are optional. `model`, when given, must be
`scribe_v3`; any other name answers `400 unknown-model`. `language`, when
given, must be the one dictation holds, or the answer is `409 model-mismatch`
with the held ones:

```json
{ "code": "model-mismatch", "model": "scribe_v3", "language": "auto" }
```

Without an API key the answer is `409 api-key-missing` with a `message` that
says how to set one. It is checked after the body itself, so a wrong sample
rate, format, or model is reported first.

`201`:

```json
{
  "id": 3,
  "ownerToken": "hex_9f…",
  "model": "scribe_v3",
  "language": "auto",
  "sampleRate": 16000,
  "sampleFormat": "f32le",
  "partials": true,
  "leaseMs": 10000
}
```

`partials` says whether Hex opens a live session for each window and so
sends `partial` events. With Scribe it is always `true`. It does not promise
that a session connects: a window whose session fails still gets its
`segment` (see [Scheduling](#scheduling-dictation-always-comes-first)).

### `POST /streams/{id}/audio`

Body: raw samples in the declared format, at most 256 KiB. `204` when queued.
Renews the lease.

### `GET /streams/{id}/events`

Server-sent events, one JSON object per `data:` line, with `: keep-alive`
comments every 15 s. Each subscriber gets every segment from the start, then
live events, then the end, and the response closes. A newer subscriber
replaces an older one. Events stay readable after the stream ends, until the
next stream is created.

```text
data: {"type":"ready","id":3,"model":"scribe_v3","language":"auto","sampleFormat":"f32le","partials":true}

data: {"type":"partial","segment":0,"stable":"so the plan for","tentative":" today"}

data: {"type":"segment","segment":0,"startMs":0,"endMs":7420,"text":"So the plan for today is the release."}

data: {"type":"final","text":"So the plan for today is the release. …","segments":[…],"durationMs":31250}
```

| `type` | Meaning |
| --- | --- |
| `ready` | Sent first: the stream's model and format. |
| `partial` | The window being transcribed. `stable` is the text the window's live session has committed; `tentative` is its uncommitted text and may still change. Hex asks for a commit only when the window closes. Replaces the previous partial for the same `segment`. Only when `partials` is true. |
| `segment` | A finished window's final text (empty windows send none). |
| `final` | Every segment joined; the stream has ended. |
| `error` | `{"code","message"}`; the stream has ended (see codes below). |
| `cancelled` | The stream was cancelled. |

Word timings are not sent.

### `POST /streams/{id}/finish`

Transcribes what's left and answers with the final, as the `final` event:

```json
{ "text": "…", "segments": [{ "segment": 0, "startMs": 0, "endMs": 7420, "text": "…" }], "durationMs": 31250 }
```

`?process=global` (Labs › Process API transcripts) adds the text after Hex's
Global-mode output processing, exactly as a paste with no matching app or
website mode gets it: global replacements, Global mode corrections, the
optional OpenCode rewrite, and Global's selected transformations. `true` and
`1` are aliases for `global`; any other value, or a repeated parameter,
answers `400 invalid-request`. Without the parameter the response is exactly
as above.

```json
{ "text": "…", "segments": [ … ], "durationMs": 31250, "processed": "…" }
```

`processed` is the paste-ready text. Processing is best-effort and follows
the paste path: a failed stage keeps its input, so the call still succeeds.
It never pastes, saves History, updates Paste Last, or touches the clipboard.
Without an installed processor (for example `hex service`) the answer is `409
processing-unavailable`, even while the lab is off; with a processor installed
and the lab off it is `403 processing-disabled`. Processing runs one at a time
across the local API; while another holds the slot the answer is `429
queue-full`. All of these are checked before the stream finishes, and a
refused `finish` leaves the stream running so the caller can finish again.

No more audio is accepted once finishing. It waits up to 120 s (for example
behind a long dictation) and then answers `504 stream-finish-timeout`; the
stream keeps going, so call `finish` again or read the events. A stream that
already failed answers `409` with its `{"code","message"}`; a cancelled one
`409 stream-cancelled`.

### `POST /streams/{id}/heartbeat`

`204`; renews the lease. `409 stream-ended` once the stream has ended.

### `POST /streams/{id}/cancel`

`204`. Discards everything; the stream ends with `cancelled`.

## Lease

A stream ends with `error` `lease-expired` when it receives neither audio nor
a heartbeat for 10 s (`leaseMs`). A paused recorder should send a heartbeat
every few seconds. The lease doesn't run while finishing.

## Limits

| Limit | Value |
| --- | --- |
| Live streams | 1 (a second create answers `409 stream-busy`; an ended stream doesn't count) |
| Create body | 1 KiB |
| Audio request | 256 KiB, read within 5 s |
| Backlog (received, not yet transcribed) | 120 s of audio, then `429 stream-backlog-full` |
| Window | cut at pauses, at most 25 s |
| Lease | 10 s |
| Finish wait | 120 s |

## Scheduling: dictation always comes first

Hex's dictation shares the engine: one thread that sends every request to
ElevenLabs, in order. The hotkey must never wait for a stream. So:

- Audio is cut into windows at pauses (at most 25 s). Each window opens its
  own realtime session (`scribe_v3_live`, a WebSocket to ElevenLabs) as a
  *yielding* live stream, fed at most 0.4 s ahead of what the engine thread
  has passed on, so a dictation request queues behind very little. When the
  window closes, Hex commits the session and its text becomes the segment.
- Any dictation request (the shortcut's own live session, a whole-clip
  transcription) closes the yielding stream first, and a yielding stream is
  refused while a dictation's live session is open.
- While dictation is busy, a stream sends the engine nothing; its audio waits
  in the backlog and catches up afterwards. On macOS that is from the
  shortcut press until dictation's text is done; on Linux and Windows it is
  while the engine runs a dictation's live session or its final
  transcription.
- A window whose session dictation closed starts a new session from the
  window's start once dictation is idle. A window that completes while that
  can't happen, or whose session failed or did not return its text within
  3 s of the commit, is uploaded whole to the batch model (`scribe_v3`) as
  one foreground request: after any dictation already sent, before meeting
  chunks (the same class as capture-word requests). The code calls this the
  "offline" path because the clip is complete; it still goes over the
  network.

A window that restarts or falls back is sent to ElevenLabs more than once:
to each session that heard part of it, and then as a whole clip. The
WebSocket connects directly, without the HTTP proxy Hex uses for its HTTPS
requests; where it can't connect, windows produce no partials and are
uploaded whole. While Labs › Vocabulary biasing is on and its boost is not
Off, the saved vocabulary is sent as keyterms with a stream's requests too.

Dictation's capture never waits on the engine: its audio is recorded
independently. Only its transcription can wait, and only for a request the
engine thread has already started. Feeding a session only queues audio and
returns at once, so in practice that is one window's whole-clip upload (up
to 25 s of audio) that started while dictation was idle; it runs to its
answer or its timeout (30 s plus the clip's length, with one retry). How
long it takes against the real service is unmeasured. Streams go ahead of
meeting chunks because a stream's app is waiting for text now, while
meetings catch up.

## Error codes

Errors are JSON `{"code": "…"}` (some add `message`), except `401`, which
has no body.

| Status | Code | When |
| --- | --- | --- |
| 400 | `invalid-request` | Create body isn't the JSON above, or `process` is not `global`, `true`, or `1` (or is repeated). |
| 400 | `unsupported-sample-rate` | `sampleRate` isn't 16000. |
| 400 | `unsupported-sample-format` | Not `f32le` or `s16le`. |
| 400 | `unknown-model` | `model` isn't `scribe_v3` (with a `message`). |
| 400 | `invalid-audio` | Partial sample or non-finite float. |
| 401 | (no body) | Missing or wrong bearer token. |
| 403 | `processing-disabled` | `process` was asked for with a processor installed but Labs › Process API transcripts off. |
| 404 | `stream-not-found` | Unknown id, replaced stream, or wrong stream token. |
| 409 | `api-key-missing` | No ElevenLabs API key is set (with a `message`). |
| 409 | `model-mismatch` | Requested language isn't the one dictation holds. |
| 409 | `processing-unavailable` | `process` was asked for and no processor is installed (for example `hex service`). |
| 409 | `stream-busy` | Another stream is live. |
| 409 | `stream-ended` | Audio or heartbeat after the end. |
| 409 | `stream-finishing` | Audio after `finish`. |
| 409 | `stream-cancelled` | `finish` on a cancelled stream. |
| 409 | `lease-expired`, `model-changed`, `transcription-failed`, `hex-quitting` | `finish` on a stream that failed that way (same codes as the `error` event). |
| 411 | `length-required` | No `Content-Length`. |
| 413 | `request-too-large`, `audio-too-large` | Over the limits. |
| 415 | `unsupported-content-type` | Wrong `Content-Type`. |
| 429 | `queue-full` | A processing run is already under way; retry after it ends. |
| 429 | `stream-backlog-full` | 120 s not yet transcribed; slow down or wait. |
| 500 | `token-generation-failed`, `stream-start-failed` | Hex could not create the stream. |
| 503 | `engine-unavailable` | Hex's dictation engine isn't running. |
| 503 | `model-not-loaded` | The engine holds no language selection yet. |
| 503 | `shutting-down` | Hex is quitting. |
| 504 | `stream-finish-timeout` | `finish` waited 120 s; the stream continues. |

`model-changed` means dictation's language changed during the stream (the
model is always `scribe_v3`); start a new one. `transcription-failed` carries
the reason in `message`, such as a refused key, no quota, or no connection.
`hex-quitting` means Hex quit.

## Meeting finished events (`GET /meetings/events`, Labs)

Not part of `/streams`, but on the same local API with the same discovery
and bearer token. Only while Labs › Meeting finished event
(`labs.meeting_finished_event`, `hex labs on meeting-finished-event`) is on;
off, it answers `404 labs-off`. Code: `src/local_api/meeting_events.rs`
(route) and `src/meeting/finished_events.rs` (the hub the recorder publishes
to).

Server-sent events with `: keep-alive` comments every 15 s. Once per meeting,
after its final transcript and Markdown are written and its manifest reads
`complete` (a Stop, or the final pass of a meeting Hex quit while recording):

```text
data: {"type":"meeting.finished","id":"1730700000000-4242"}
```

Only the id is sent: no title, path, transcript, or audio. Read the rest with
`hex meetings show <id>` (or `hex mcp`). One subscriber at a time; a
second answers `409 subscriber-busy`. Nothing is queued while nobody listens,
so a client that reconnects should list meetings to catch up. A meeting whose
final transcript fails sends nothing. The stream ends when Hex quits or the
toggle turns off. `hex service` serves the route but records no meetings, so
it never sends one.

### Catching up after being offline

The event is a best-effort signal, not a queue. To miss nothing, treat the
meetings folder as the record and the event as a hint to look now:

1. Keep a cursor: the end time (`timestamp_ms + duration_ms`, Unix ms) of the
   newest meeting you've handled, and the ids you've handled near it.
2. On start or reconnect, subscribe to `GET /meetings/events` first, then run
   `hex meetings list --since <cursor> --limit 200 --json` (or MCP's
   `list_meetings` with `since`). Subscribing first means a meeting that
   finishes while you list still arrives as an event.
3. A meeting is finished when its `status` is `complete` and its
   `transcript_source` is `final`; read it with `hex meetings show <id> --json`.
   Leave the cursor before any meeting that is still `recording`,
   `transcribing`, or `interrupted`, so the next list sees it again.
4. Delivery is at least once: the same id can come from the list and the
   event, or from overlapping lists. Deduplicate by meeting `id` (stable for
   the meeting's life) and keep your own receipts of what you processed.

Limits:

- One list returns at most 200 meetings, newest first, with `truncated` set
  when there were more; page back with `--until <oldest timestamp_ms>`.
- `--since`/`--until` include every meeting whose recording overlaps the
  range, so the meeting at the cursor comes back; the dedupe handles it.
- A meeting retention already removed is gone and can't be caught up.
- A meeting Hex quit while recording reads `interrupted` until its final
  pass on the next launch with Meetings on; then it reads `complete` and the
  event is sent.
- There is no separate transcript revision id; the meeting id is the key.
  The final transcript is published atomically, and the event is sent once
  per meeting.

Final-transcript readiness is separate from notes. The event and `complete`
mean the transcript and Markdown are written; notes (Apple Intelligence, or
OpenCode with Summarize with OpenCode) are written after that, may fail, and
can be regenerated later. Read them from `notes` in `hex meetings show <id>
--json` (`null` until written; `generated_at_ms` changes when they're
rewritten). Subscribing, listing, and showing are read-only: they run no
command, upload nothing, and write nothing.
