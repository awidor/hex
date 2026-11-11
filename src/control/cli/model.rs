//! `hex model`: the one transcription model, ElevenLabs Scribe. Nothing is
//! downloaded or installed; a model command shows or sets the language
//! Scribe is told to expect, or checks that ElevenLabs can be reached.
//!
//! `list` and `use` go to the running Hex when there is one, so they show
//! and change what dictation uses; without one they read and write
//! `settings.json` here. `status`, `check`, and `transcribe` always run in
//! this process.

use clap::Subcommand;

use super::{Output, fail, finish_handoff, hand_off, key, print, running, send, transcribe};
use crate::app_settings::AppSettings;
use crate::control::{ControlCommand, ControlError, ControlReply};

#[derive(Subcommand, Debug)]
pub enum ModelCommand {
    /// The transcription model with its language and whether an API key is
    /// set. There is one model: Scribe v3, which runs at ElevenLabs.
    List {
        #[command(flatten)]
        output: Output,
    },
    /// Set the language Scribe is told to expect.
    Use {
        /// The model id: `scribe_v3`.
        model: String,
        /// An ISO 639-1 or 639-3 code such as `en`, `de`, or `yue`, or
        /// `auto` to let Scribe detect the language (default: the saved
        /// language).
        #[arg(long)]
        language: Option<String>,
        #[command(flatten)]
        output: Output,
    },
    /// The model, its language, the API key in use, and where audio is
    /// sent. Runs here, with or without Hex.
    Status {
        #[command(flatten)]
        output: Output,
    },
    /// Check the API key with ElevenLabs.
    ///
    /// Sends no audio and spends no quota. Exits 1 when no key is set,
    /// ElevenLabs refuses it, or ElevenLabs can't be reached.
    Check {
        #[command(flatten)]
        output: Output,
    },
    /// Transcribe a WAV file here by sending it to ElevenLabs, and print
    /// the text. Exits 4, printing nothing, when no speech was heard.
    Transcribe {
        wav: std::path::PathBuf,
        /// An ISO 639 code such as `en` or `yue`, or `auto` (default: the
        /// saved language).
        #[arg(long)]
        language: Option<String>,
        #[command(flatten)]
        output: Output,
    },
}

pub(super) fn run(command: ModelCommand) -> u8 {
    match command {
        ModelCommand::List { output } => list(output),
        ModelCommand::Use {
            model,
            language,
            output,
        } => use_model(&model, language.as_deref(), output),
        ModelCommand::Status { output } => status(output),
        ModelCommand::Check { output } => print(key::check(), output),
        ModelCommand::Transcribe {
            wav,
            language,
            output,
        } => transcribe::run_here(&wav, language, output),
    }
}

/// `hex model list`: the running Hex's answer, which has its language and
/// its API key; without one, what this `hex` would use.
fn list(output: Output) -> u8 {
    if running() {
        return send(ControlCommand::ModelList, output);
    }
    match AppSettings::read() {
        Ok(settings) => print(
            ControlReply::Models {
                models: crate::control::model_infos(&settings.transcription),
            },
            output,
        ),
        Err(error) => fail(&format!("couldn't read settings: {error:#}")),
    }
}

/// `hex model status`: the saved selection and this process's API key.
fn status(output: Output) -> u8 {
    let settings = match AppSettings::read() {
        Ok(settings) => settings,
        Err(error) => return fail(&format!("couldn't read settings: {error:#}")),
    };
    let selected = crate::control::model_infos(&settings.transcription)
        .into_iter()
        .find(|model| model.selected);
    match selected {
        Some(model) => print(ControlReply::Model(Box::new(model)), output),
        None => fail("settings name no transcription model"),
    }
}

/// `hex model use`: the running Hex applies the language to its next
/// dictation; with none running it is saved here, under the lock a starting
/// Hex holds, so a Hex that started meanwhile is never written behind.
fn use_model(model: &str, language: Option<&str>, output: Output) -> u8 {
    let handoff = hand_off(
        || {
            send(
                ControlCommand::ModelUse {
                    model: model.to_owned(),
                    language: language.map(str::to_owned),
                },
                output,
            )
        },
        || print(use_offline(model, language), output),
    );
    finish_handoff(handoff, "hex model use", output)
}

fn use_offline(model: &str, language: Option<&str>) -> ControlReply {
    let internal = |what: &str, error: color_eyre::Report| {
        ControlError::new("internal", format!("couldn't {what} settings: {error:#}"))
    };
    let result = AppSettings::load()
        .map_err(|error| internal("read", error))
        .and_then(|mut settings| {
            let selection = crate::control::model_selection(&settings, model, language)?;
            if settings.transcription == selection {
                return Ok(ControlReply::unchanged(crate::control::model_use_message(
                    &selection, false,
                )));
            }
            settings
                .save_transcription(selection.clone())
                .map_err(|error| internal("save", error))?;
            Ok(ControlReply::done(crate::control::model_use_message(
                &selection, true,
            )))
        });
    result.unwrap_or_else(ControlReply::from)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser, Debug)]
    struct Cli {
        #[command(subcommand)]
        command: ModelCommand,
    }

    fn parse(arguments: &[&str]) -> Result<ModelCommand, clap::Error> {
        Cli::try_parse_from(std::iter::once("model").chain(arguments.iter().copied()))
            .map(|cli| cli.command)
    }

    #[test]
    fn the_model_commands_are_list_use_status_check_and_transcribe() {
        assert!(matches!(
            parse(&["list", "--json"]),
            Ok(ModelCommand::List { output }) if output.json
        ));
        assert!(matches!(
            parse(&["use", "scribe_v3", "--language", "de"]),
            Ok(ModelCommand::Use { model, language: Some(language), .. })
                if model == "scribe_v3" && language == "de"
        ));
        assert!(matches!(
            parse(&["status"]),
            Ok(ModelCommand::Status { .. })
        ));
        assert!(matches!(parse(&["check"]), Ok(ModelCommand::Check { .. })));
        assert!(matches!(
            parse(&["transcribe", "clip.wav", "--language", "auto"]),
            Ok(ModelCommand::Transcribe { wav, language: Some(language), .. })
                if wav.ends_with("clip.wav") && language == "auto"
        ));
    }

    /// Nothing is downloaded, imported, or run on a GPU here any more.
    #[test]
    fn commands_for_models_on_this_computer_are_gone() {
        for gone in [
            "install", "cancel", "devices", "import", "rename", "repair", "remove",
        ] {
            assert!(parse(&[gone]).is_err(), "`hex model {gone}` still parses");
            assert!(
                parse(&[gone, "scribe_v3"]).is_err(),
                "`hex model {gone} scribe_v3` still parses"
            );
        }
    }
}
