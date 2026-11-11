# Vocabulary

One shared list of names Hex should know (people, projects, products), each
with optional "sounds like" spellings a transcript may show instead. An empty
list (the default) changes nothing. The window and the `hex vocabulary`
commands edit the list on macOS, Linux, and Windows.

```ts
General › Vocabulary   // vocabulary.settings; in the basics card, after the shortcut
├── Row value -> "None", "1 term", "N terms" -> opens the Vocabulary sheet
├── Sheet description -> "Transcription and rewriting spell them right, and
│   search finds them when misheard", then "Terms with spaces match only when
│   said in full"
├── Use in transcription (switch)                // vocabulary.boost
│   ├── On unless `vocabulary_boost` is off; turning it off saves off, on saves normal
│   ├── Labs › Vocabulary biasing off -> "Transcription uses these only with
│   │   Labs › Vocabulary biasing on; rewriting and search always do"
│   ├── On, and ElevenLabs takes fewer terms than the list holds ->
│   │   "Transcription uses 40 of 41 terms; ElevenLabs doesn't take the rest"
│   │   (ModelDefinition::vocabulary_terms_read, the same count a request takes)
│   └── Else -> "Sends these terms to ElevenLabs with each dictation so it favors them"
├── Sheet rows -> Term | Sounds like (comma-separated) | ×; scroll past 8
├── Empty list -> Opens with one blank row focused; opening never animates rows
├── Add term -> New empty row, focused; the list glides to its end and the row
│   rises 8 pt as it fades in (SETTLE, bounce 0; Reduce Motion: fade only);
│   saved once the term has text
├── Edits -> Debounced drafts, like Modes' corrections -> settings.json `vocabulary`
├── × -> Removes that row at any click speed or focus; rows keep an identity,
│   so a late edit, focus change, or second × for a removed row touches nothing
├── Done, Escape, or a press outside -> Saves, drops blank rows, closes
├── Past the limits -> Keep the term (cut to 64 characters) and the spellings
│   that fit; inline danger note    // never drops the term
└── `hex settings set vocabulary_boost off|light|normal|strong`
    (settings.json `vocabulary_boost`, default normal) -> off sends no terms;
    light, normal, and strong send the same terms

Transcription (ElevenLabs Scribe keyterms)  // vocabulary.transcription; dictation::vocabulary::keyterms
├── Only with Labs › Vocabulary biasing on (`labs.vocabulary_biasing`, off by
│   default; labs.md) AND Use in transcription on -> else no request carries
│   the list; ElevenLabs bills keyterms extra
├── Every batch request and live session reads the list at its start:
│   dictation, meetings, captures, the local API
├── The terms Scribe accepts, in list order; limits, what is skipped, and the
│   proof are in transcription.md#vocabulary-as-keyterms
└── "Sounds like" spellings are not sent; rewriting and search use them

Recognition hints, folded in                // vocabulary.hints
├── Loading settings.json -> `recognition_hints` of the saved selection and
│   of `transcription_recents` (newest first), split on commas, semicolons,
│   and line breaks, become terms; names already listed (any case) are
│   skipped, and the rewritten file drops the field
└── `hex settings get|set transcription.recognition_hints` -> deprecated alias:
    reads the terms comma-separated; set adds names (cli.md)

hex vocabulary                           // vocabulary.cli
├── list [--json]
├── add <term> [--sounds-like a,b] -> New term, or new spellings merged into it
│   └── Already there, nothing new -> changed: false
├── remove <term> -> Any case
└── Running Hex saves and reloads settings; an open window adopts the change

A mode's own vocabulary                  // vocabulary.modes; DictationMode::vocabulary
├── settings.json `dictation_processing.modes[].vocabulary`, same entries; no
│   window editor yet (Output lists it as the mode's first step; Edit with
│   agent's prompt explains it)
├── Its terms come first, then the shared list's, each term once (any case),
│   so the keyterm limits keep them
├── OpenCode rewriting -> every OS, the mode chosen for the dictation
└── Transcription -> macOS dictation only: the mode of the app and page the
    context monitor last saw when the live session or whole-clip request
    starts. The app is polled every 0.5 s; a scriptable browser's page is read
    when the browser comes to the front and once more when a dictation capture
    is real (held past its hold threshold, at a finish that lands earlier, or
    at the control command's start), never on every press. A live session
    starts with the page cached when the browser came to the front; the
    refresh applies to the whole-clip request. Meetings, captures, the local
    API, Linux, and Windows hear with the shared list

OpenCode rewriting (Modes)               // vocabulary.prompt
└── Prompt gains "Vocabulary (spell these exactly when the speaker means them):
    term (may be transcribed as a, b); …" before the foreground application

Live meeting chapters (experiment live-chapters)  // vocabulary.chapters
└── Chapter passes and the boundary pass start with "Names and terms that may
    appear: a, b, …" (first 30 terms, no spellings), read on each call

Meeting notes (Labs › Vocabulary in meeting notes)  // vocabulary.notes
├── Off (default) -> notes prompts are unchanged
└── On -> the final title and summary prompt (automatic notes and Regenerate
    summary) starts with the same line, read at each generation, ahead of the
    calendar event; it shares the transcript's excerpt budget (#1154)

Search                                   // vocabulary.search
├── hex meetings search, MCP search_transcripts, hex history --search
├── Query word = a one-word term, fuzzy on
│   ├── Its "sounds like" spellings, or the term as two words -> 0.5 (like a prefix)
│   └── A word that sounds like it (≥2 consonant sounds, ≤ half the letters changed,
│       not a stopword or common word) -> 0.3
└── Aliases never count as exact; exact hits always rank first
```

The list lives in `settings.json` as `vocabulary`, each entry a plain term
(`"Rune"`) or `{term, sounds_like}`; both load, and Hex writes a term without
spellings as a plain string. `hex vocabulary list --json` keeps the object form.
It is written only when non-empty, so older files load unchanged and are not
rewritten. Terms are unique ignoring case; at most 500 terms, 64 characters
each, and 16 spellings per term. Read-only commands (`hex history`,
`hex meetings`, `hex mcp`) read the field straight from the file and never load
or migrate settings.

Short names with few consonant sounds (Rune, Dax) would sound like too many
words, so search finds their mis-transcriptions only through "sounds like"
spellings.

Provisional defaults, each changed in one place:

```ts
hex vocabulary listed in --help          // control/cli.rs: add #[command(hide = true)] to hide it
Prompt wording                           // dictation/processor.rs: vocabulary_lines
Chapter prompt wording                   // meeting/chapters.rs: vocabulary_line
Short-name rule                          // meeting/query.rs: SOUND_ALIKE_MIN_CONSONANTS
```

Sources: [dictation/vocabulary.rs](../../src/dictation/vocabulary.rs),
[rune_ui/vocabulary.rs](../../src/rune_ui/vocabulary.rs),
[meeting/query.rs](../../src/meeting/query.rs) (`vocabulary_aliases`,
`sounds_alike`), [dictation/processor.rs](../../src/dictation/processor.rs)
(`vocabulary_lines`), [control.rs](../../src/control.rs)
(`edit_vocabulary`), and [control/host.rs](../../src/control/host.rs) (the
Linux and Windows `hex vocabulary` host).

Checks: `dictation::vocabulary::tests` (`plain_terms_load_and_save_as_strings`,
`adding_dedupes_by_case_and_merges_spellings`,
`clamped_adds_keep_the_term_and_the_spellings_that_fit`,
`removing_ignores_case`, `sounds_like_splits_on_commas`,
`saved_entries_read_without_loading_settings`,
`keyterms_are_the_terms_scribe_accepts_in_list_order`,
`every_saved_term_fits_scribes_keyterm_limit`,
`a_modes_terms_come_first_and_each_term_once`,
`only_dictation_hears_with_the_focused_modes_terms`,
`hints_fold_into_the_list_without_duplicates`,
`boost_reads_and_writes_as_its_name`);
`loading_folds_saved_recognition_hints_into_the_vocabulary_once` in
[app_settings.rs](../../src/app_settings.rs);
`recognition_hints_alias_the_vocabulary` in
[control/settings.rs](../../src/control/settings.rs);
`vocabulary_round_trips_and_stays_out_of_files_without_one` in
[app_settings.rs](../../src/app_settings.rs);
`a_vocabulary_term_finds_its_mis_transcriptions_below_exact_hits`,
`sounds_like_spellings_and_split_words_alias_a_term`,
`vocabulary_never_pulls_in_common_words`, and
`sound_alikes_need_two_consonant_sounds_and_few_changes` in
[meeting/query_tests.rs](../../src/meeting/query_tests.rs);
`prompt_names_vocabulary_terms_and_their_mistranscriptions`;
`vocabulary_edits_report_whether_they_changed_the_list` and the request round
trip in control.rs; `vocabulary_lists_terms_with_their_spellings` in
[control/cli.rs](../../src/control/cli.rs);
`rows_project_to_cleaned_merged_entries`,
`rows_past_the_limits_keep_their_term_and_what_fits`,
`the_sheet_starts_empty_lists_with_a_row_and_saves_on_close`,
`removing_a_focused_row_leaves_the_others_and_late_edits_touch_nothing`,
`removing_the_last_row_then_editing_it_is_harmless`,
`rapid_removals_from_one_frame_remove_only_their_own_rows`,
`the_switch_says_how_many_terms_transcription_uses`,
`the_switch_saves_off_or_normal`, and `a_reload_retires_the_old_rows` in
[rune_ui/vocabulary.rs](../../src/rune_ui/vocabulary.rs);
`the_vocabulary_row_opens_its_sheet` and
`add_term_scrolls_the_new_row_into_view` in
[rune_ui/tests.rs](../../src/rune_ui/tests.rs). Scenes:
`mac-settings-vocabulary`, `mac-settings-vocabulary-empty`.

No test covers the Labs gate or the switch inside `keyterms()`. Unverified in
this fork: the sheet and its switch (macOS: not compiled; Windows:
type-checked by cross compilation only; Linux: compiled and unit-tested), and
any effect of keyterms on a transcript, since no request was made to
ElevenLabs ([transcription.md](transcription.md#not-verified)). Not yet
observed: the sheet edited in a running Hex, a CLI edit against a running Hex,
or a rewrite with a live provider.
