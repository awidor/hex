//! `hex key`: the ElevenLabs API key every transcription is sent with.
//!
//! These run in this process. The running Hex reads the key for each
//! request, so a key saved or removed here applies to its next dictation
//! without a restart. The key itself is never printed, logged, or part of a
//! reply: only where it comes from and its last four characters.

use std::io::{BufRead, IsTerminal, Read};

use clap::Subcommand;

use super::{Output, print};
use crate::control::{ApiKeyStatus, ControlReply, api_key_report};
use crate::transcription::scribe::key::{self, ENVIRONMENT, Source};
use crate::transcription::scribe::{self, ScribeError};

/// The most standard input is read for a key; the key file's own limit is
/// far below it.
const MAX_INPUT_BYTES: u64 = 4096;

#[derive(Subcommand, Debug)]
pub enum KeyCommand {
    /// Save the ElevenLabs API key, for this user only.
    ///
    /// Without KEY, or with `-`, the key is read from standard input, which
    /// keeps it out of your shell history: `hex key set < key.txt`, or run
    /// `hex key set` and paste it. Audio you dictate is sent to ElevenLabs
    /// with this key.
    Set {
        /// The key. Prefer standard input: an argument is saved in your
        /// shell history and shows in the process list while `hex` runs.
        key: Option<String>,
        #[command(flatten)]
        output: Output,
    },
    /// Whether a key is set, where it comes from, its last four characters,
    /// and the key file. Never prints the key.
    Status {
        #[command(flatten)]
        output: Output,
    },
    /// Remove the saved key. A key in ELEVENLABS_API_KEY is left alone.
    Clear {
        #[command(flatten)]
        output: Output,
    },
    /// Check the key with ElevenLabs.
    ///
    /// Sends no audio and spends no quota. Exits 1 when no key is set,
    /// ElevenLabs refuses it, or ElevenLabs can't be reached.
    Check {
        #[command(flatten)]
        output: Output,
    },
}

pub(super) fn run(command: KeyCommand) -> u8 {
    match command {
        KeyCommand::Set { key, output } => {
            let reply = match entered(key, || read_input(std::io::stdin().is_terminal())) {
                Ok(key) => save(&key),
                Err(reply) => reply,
            };
            print(reply, output)
        }
        KeyCommand::Status { output } => print(status(), output),
        KeyCommand::Clear { output } => print(clear(), output),
        KeyCommand::Check { output } => print(check(), output),
    }
}

/// The key `hex key set` was given: its argument, or standard input when
/// there is none or it is `-`. Surrounding whitespace is not part of a key.
fn entered(
    argument: Option<String>,
    input: impl FnOnce() -> std::io::Result<String>,
) -> Result<String, ControlReply> {
    let key = match argument {
        Some(key) if key != "-" => key,
        _ => input().map_err(|error| {
            ControlReply::error(
                "failed",
                format!("couldn't read the key from standard input: {error}"),
            )
        })?,
    };
    let key = key.trim();
    if key.is_empty() {
        return Err(ControlReply::error(
            "invalid-value",
            "no key was given; pipe it in (`hex key set < key.txt`) or run `hex key set` \
             and paste it",
        ));
    }
    Ok(key.to_owned())
}

/// A key from standard input: one line typed or pasted at a terminal, with
/// echo off where the terminal allows it, or all of a pipe or file.
fn read_input(terminal: bool) -> std::io::Result<String> {
    let stdin = std::io::stdin();
    if !terminal {
        let mut text = String::new();
        stdin
            .lock()
            .take(MAX_INPUT_BYTES)
            .read_to_string(&mut text)?;
        return Ok(text);
    }
    let mut line = String::new();
    let (read, hidden) = {
        let echo = HiddenInput::start();
        let hidden = echo.hides();
        eprint!(
            "ElevenLabs API key{}: ",
            if hidden {
                ""
            } else {
                " (it will show as you paste it)"
            }
        );
        (stdin.lock().read_line(&mut line), hidden)
    };
    // The Enter that ended a hidden line was not echoed either.
    if hidden {
        eprintln!();
    }
    read.map(|_| line)
}

/// Turns the terminal's echo off while a key is typed, and back on when
/// dropped.
#[cfg(unix)]
struct HiddenInput(Option<libc::termios>);

#[cfg(unix)]
impl HiddenInput {
    fn start() -> Self {
        // SAFETY: termios is plain data that tcgetattr fills.
        let mut shown = unsafe { std::mem::zeroed::<libc::termios>() };
        // SAFETY: the pointer refers to a live termios for standard input.
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &raw mut shown) } != 0 {
            return Self(None);
        }
        let mut hidden = shown;
        hidden.c_lflag &= !libc::ECHO;
        // SAFETY: `hidden` is the terminal's own settings with echo cleared.
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw const hidden) } != 0 {
            return Self(None);
        }
        Self(Some(shown))
    }

    fn hides(&self) -> bool {
        self.0.is_some()
    }
}

#[cfg(unix)]
impl Drop for HiddenInput {
    fn drop(&mut self) {
        if let Some(shown) = &self.0 {
            // SAFETY: `shown` is what tcgetattr read from this terminal.
            unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, shown) };
        }
    }
}

/// Windows consoles keep their echo; the prompt says so.
#[cfg(not(unix))]
struct HiddenInput;

#[cfg(not(unix))]
impl HiddenInput {
    fn start() -> Self {
        Self
    }

    fn hides(&self) -> bool {
        false
    }
}

fn save(secret: &str) -> ControlReply {
    if let Err(error) = key::save(secret) {
        return ControlReply::error("failed", format!("{error:#}"));
    }
    ControlReply::done(format!(
        "Saved the API key ({}); audio you dictate is sent to ElevenLabs with it. \
         `hex key check` verifies it{}",
        key::redacted(secret),
        environment_note(key::current().map(|key| key.source)),
    ))
}

fn status() -> ControlReply {
    ControlReply::ApiKey(ApiKeyStatus {
        api_key: api_key_report(),
        key_file: key::path().ok(),
    })
}

fn clear() -> ControlReply {
    let note = || environment_note(key::current().map(|key| key.source));
    match key::clear() {
        Ok(true) => ControlReply::done(format!("Removed the saved API key{}", note())),
        Ok(false) => ControlReply::unchanged(format!("No API key is saved{}", note())),
        Err(error) => ControlReply::error("failed", format!("{error:#}")),
    }
}

/// What a saved key's command adds while the environment's key overrides it.
fn environment_note(in_use: Option<Source>) -> String {
    match in_use {
        Some(Source::Environment) => {
            format!("; {ENVIRONMENT} is set, and Hex uses that key instead of a saved one")
        }
        Some(Source::File) | None => String::new(),
    }
}

/// `hex key check` and `hex model check`: asks ElevenLabs whether it takes
/// the key in use. No audio is sent.
pub(super) fn check() -> ControlReply {
    let Some(key) = key::current() else {
        return checked(None, Err(ScribeError::MissingKey));
    };
    let answer = scribe::check_key(&key.secret);
    checked(Some(&key), answer)
}

fn checked(key: Option<&key::ApiKey>, answer: Result<(), ScribeError>) -> ControlReply {
    match answer {
        Ok(()) => ControlReply::unchanged(match key {
            Some(key) => format!(
                "ElevenLabs accepted the API key ({})",
                super::human::api_key_detail(&key.into())
            ),
            None => "ElevenLabs accepted the API key".into(),
        }),
        Err(error) => ControlReply::error(error_code(&error), check_failure(&error)),
    }
}

/// The control code for a failed request to ElevenLabs.
pub(super) fn error_code(error: &ScribeError) -> &'static str {
    match error {
        ScribeError::MissingKey => "api-key-missing",
        ScribeError::InvalidKey => "api-key-invalid",
        _ => "failed",
    }
}

/// Why a check failed, in terms of `hex key` rather than Settings.
fn check_failure(error: &ScribeError) -> String {
    match error {
        ScribeError::MissingKey => missing_key_message().to_owned(),
        ScribeError::InvalidKey => {
            "ElevenLabs refused the API key; save a working one with `hex key set`".into()
        }
        other => other.to_string(),
    }
}

/// What every command that needs a key says when there is none.
pub(super) fn missing_key_message() -> &'static str {
    "no ElevenLabs API key is set; run `hex key set`, or set ELEVENLABS_API_KEY"
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser, Debug)]
    struct Cli {
        #[command(subcommand)]
        command: KeyCommand,
    }

    fn parse(arguments: &[&str]) -> Result<KeyCommand, clap::Error> {
        Cli::try_parse_from(std::iter::once("key").chain(arguments.iter().copied()))
            .map(|cli| cli.command)
    }

    fn error(reply: ControlReply) -> (String, String) {
        match reply {
            ControlReply::Error { code, message } => (code, message),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    const SECRET: &str = "sk_0123456789abcdef";

    #[test]
    fn the_key_commands_are_set_status_clear_and_check() {
        assert!(matches!(
            parse(&["set"]),
            Ok(KeyCommand::Set { key: None, .. })
        ));
        assert!(matches!(
            parse(&["set", "-", "--json"]),
            Ok(KeyCommand::Set { key: Some(key), output }) if key == "-" && output.json
        ));
        assert!(matches!(
            parse(&["set", SECRET]),
            Ok(KeyCommand::Set { key: Some(key), .. }) if key == SECRET
        ));
        assert!(matches!(
            parse(&["status", "--json"]),
            Ok(KeyCommand::Status { output }) if output.json
        ));
        assert!(matches!(parse(&["clear"]), Ok(KeyCommand::Clear { .. })));
        assert!(matches!(parse(&["check"]), Ok(KeyCommand::Check { .. })));
        assert!(parse(&["show"]).is_err(), "nothing prints the key");
    }

    #[test]
    fn set_reads_standard_input_unless_given_a_key() {
        let piped = || Ok(format!("  {SECRET}\r\n"));
        assert_eq!(entered(None, piped).unwrap(), SECRET);
        assert_eq!(entered(Some("-".into()), piped).unwrap(), SECRET);
        // An argument is used as it is, and standard input is left unread.
        let unread = || -> std::io::Result<String> { panic!("standard input was read") };
        assert_eq!(
            entered(Some(format!(" {SECRET} ")), unread).unwrap(),
            SECRET
        );
    }

    #[test]
    fn set_refuses_an_empty_or_unreadable_key_without_echoing_anything() {
        for empty in ["", "  \n"] {
            let (code, message) = error(entered(None, || Ok(empty.into())).unwrap_err());
            assert_eq!(code, "invalid-value");
            assert!(message.contains("hex key set"), "{message}");
        }
        let (code, _) = error(entered(Some("  ".into()), || Ok(String::new())).unwrap_err());
        assert_eq!(code, "invalid-value");
        let closed = || Err(std::io::Error::other("closed"));
        let (code, message) = error(entered(None, closed).unwrap_err());
        assert_eq!(code, "failed");
        assert!(message.contains("standard input"), "{message}");
    }

    #[test]
    fn a_check_without_a_key_fails_before_asking_elevenlabs() {
        // Unit tests have no key, so this never reaches the network.
        let (code, message) = error(check());
        assert_eq!(code, "api-key-missing");
        assert!(message.contains("hex key set"), "{message}");
        assert_eq!(
            super::super::human::exit_code(&check()),
            super::super::EXIT_REFUSED
        );
    }

    #[test]
    fn a_check_reports_what_elevenlabs_said_and_never_the_key() {
        let key = key::ApiKey {
            secret: SECRET.into(),
            source: Source::File,
        };
        let accepted = checked(Some(&key), Ok(()));
        assert_eq!(
            accepted,
            ControlReply::unchanged("ElevenLabs accepted the API key (••••cdef, saved)")
        );
        assert_eq!(super::super::human::exit_code(&accepted), 0);

        let (code, message) = error(checked(Some(&key), Err(ScribeError::InvalidKey)));
        assert_eq!(code, "api-key-invalid");
        assert!(message.contains("hex key set"), "{message}");
        assert!(!message.contains("0123456789"), "{message}");

        let unreachable = ScribeError::Unreachable("no network".into());
        let (code, message) = error(checked(Some(&key), Err(unreachable)));
        assert_eq!(code, "failed");
        assert!(message.contains("couldn't reach ElevenLabs"), "{message}");
        let (code, _) = error(checked(Some(&key), Err(ScribeError::QuotaExceeded)));
        assert_eq!(code, "failed");
    }

    #[test]
    fn status_names_no_key_in_tests_and_never_carries_one() {
        let ControlReply::ApiKey(status) = status() else {
            panic!("a key status");
        };
        assert_eq!(status.api_key, None);
    }

    #[test]
    fn a_saved_keys_commands_say_when_the_environment_overrides_it() {
        assert_eq!(environment_note(None), "");
        assert_eq!(environment_note(Some(Source::File)), "");
        assert!(environment_note(Some(Source::Environment)).contains("ELEVENLABS_API_KEY"));
    }
}
