//! Audio of dictations whose transcription failed, kept so they can be
//! transcribed again.
//!
//! Every transcription needs ElevenLabs, so a dropped connection, a spent
//! quota, or a missing key would otherwise lose what was said. A failed
//! dictation's 16 kHz mono clip is written as an owner-only WAV in
//! `failed-dictations/` beside `settings.json`, and the failure names the
//! file and the command that transcribes it (`hex model transcribe <file>`).
//! Only failures the same clip could survive are kept, the folder holds at
//! most [`MAX_CLIPS`], and files older than [`MAX_AGE`] are removed whenever
//! one is added. Nothing else ever writes dictation audio to disk by default.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hound::{SampleFormat, WavSpec, WavWriter};

use crate::transcription::scribe::ScribeError;

const DIRECTORY: &str = "failed-dictations";
/// The most clips kept; the oldest go first.
pub const MAX_CLIPS: usize = 20;
/// How long a clip is kept.
pub const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const SAMPLE_RATE: u32 = 16_000;

/// The folder failed clips are kept in.
pub fn directory() -> Option<PathBuf> {
    crate::app_paths::support_dir()
        .ok()
        .map(|support| support.join(DIRECTORY))
}

/// Keeps `samples` when `error` is one the clip could be transcribed after,
/// and returns `error`'s message with where the audio is and how to retry.
/// Any other error, and a clip that could not be written, returns the
/// message alone.
pub(crate) fn describe(samples: &[f32], error: &color_eyre::Report) -> String {
    let message = format!("{error:#}");
    match keep(samples, error) {
        Some(path) => format!(
            "{message}; the audio was kept: hex model transcribe {}",
            path.display()
        ),
        None => message,
    }
}

fn keep(samples: &[f32], error: &color_eyre::Report) -> Option<PathBuf> {
    let worth_keeping = error
        .downcast_ref::<ScribeError>()
        .is_some_and(ScribeError::worth_retrying_later);
    if !worth_keeping || samples.is_empty() {
        return None;
    }
    // Unit tests never write into this computer's data folder.
    if cfg!(test) {
        return None;
    }
    let directory = directory()?;
    match keep_in(&directory, samples, SystemTime::now()) {
        Ok(path) => {
            tracing::info!(
                audio_ms = samples.len() / 16,
                "kept the audio of a dictation that could not be transcribed"
            );
            Some(path)
        }
        Err(error) => {
            tracing::warn!(%error, "could not keep a failed dictation's audio");
            None
        }
    }
}

/// Writes `samples` into `directory` as a WAV named for `now`, then prunes
/// the folder.
fn keep_in(directory: &Path, samples: &[f32], now: SystemTime) -> std::io::Result<PathBuf> {
    private_directory(directory)?;
    let stamp = now
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let path = directory.join(format!("dictation-{stamp}.wav"));
    let staging = directory.join(format!(".dictation-{stamp}.{}.tmp", std::process::id()));
    let written = write_wav(&staging, samples).and_then(|()| fs::rename(&staging, &path));
    if let Err(error) = written {
        let _ = fs::remove_file(&staging);
        return Err(error);
    }
    prune(directory, now);
    Ok(path)
}

fn private_directory(directory: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(directory)
}

fn write_wav(path: &Path, samples: &[f32]) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    // What was said is the owner's alone to read.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let file = options.open(path)?;
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let invalid = |error: hound::Error| std::io::Error::other(error.to_string());
    let mut writer = WavWriter::new(std::io::BufWriter::new(file), spec).map_err(invalid)?;
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        writer.write_sample(value).map_err(invalid)?;
    }
    writer.finalize().map_err(invalid)
}

/// The kept clips in `directory`, newest first.
pub fn clips_in(directory: &Path) -> Vec<PathBuf> {
    let mut clips: Vec<(u128, PathBuf)> = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let stamp = path
                .file_name()?
                .to_str()?
                .strip_prefix("dictation-")?
                .strip_suffix(".wav")?
                .parse()
                .ok()?;
            Some((stamp, path))
        })
        .collect();
    clips.sort_by_key(|clip| std::cmp::Reverse(clip.0));
    clips.into_iter().map(|(_, path)| path).collect()
}

/// Removes clips past [`MAX_AGE`] or beyond the newest [`MAX_CLIPS`].
fn prune(directory: &Path, now: SystemTime) {
    let oldest_kept = now
        .checked_sub(MAX_AGE)
        .and_then(|cutoff| cutoff.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |cutoff| cutoff.as_millis());
    for (index, path) in clips_in(directory).into_iter().enumerate() {
        let stamp = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.strip_prefix("dictation-"))
            .and_then(|stamp| stamp.parse::<u128>().ok())
            .unwrap_or(0);
        if index >= MAX_CLIPS || stamp < oldest_kept {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "hex-failed-clips-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&directory);
        directory
    }

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_800_000_000 + seconds)
    }

    #[test]
    fn a_kept_clip_is_an_owner_only_wav_the_transcriber_reads_back() {
        let directory = directory("wav");
        let samples: Vec<f32> = (0..1_600).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();

        let path = keep_in(&directory, &samples, at(0)).unwrap();

        let bytes = fs::read(&path).unwrap();
        let clip =
            crate::transcription::service::AudioClip::decode_wav(std::io::Cursor::new(bytes))
                .unwrap();
        assert_eq!(clip.samples().len(), samples.len());
        assert!((clip.samples()[100] - samples[100]).abs() < 0.001);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(&directory), 0o700);
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn only_the_newest_clips_are_kept() {
        let directory = directory("count");
        for second in 0..(MAX_CLIPS as u64 + 3) {
            keep_in(&directory, &[0.1; 160], at(second)).unwrap();
        }

        let clips = clips_in(&directory);
        assert_eq!(clips.len(), MAX_CLIPS);
        let newest = at(MAX_CLIPS as u64 + 2)
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        assert!(clips[0].ends_with(format!("dictation-{newest}.wav")));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn old_clips_are_removed_when_a_new_one_is_kept() {
        let directory = directory("age");
        keep_in(&directory, &[0.1; 160], at(0)).unwrap();
        keep_in(&directory, &[0.1; 160], at(60)).unwrap();

        keep_in(&directory, &[0.1; 160], at(MAX_AGE.as_secs() + 30)).unwrap();

        // The first is past the age limit; the second is 30 s inside it.
        assert_eq!(clips_in(&directory).len(), 2);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_request_the_clip_would_fail_again_is_not_kept() {
        let rejected = color_eyre::Report::new(ScribeError::Rejected {
            status: 422,
            detail: "bad audio".into(),
        });
        assert_eq!(
            describe(&[0.1; 160], &rejected),
            "ElevenLabs refused the request (HTTP 422): bad audio"
        );
        let other = color_eyre::eyre::eyre!("something else");
        assert_eq!(describe(&[0.1; 160], &other), "something else");
    }
}
