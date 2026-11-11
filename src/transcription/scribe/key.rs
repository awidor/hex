//! The ElevenLabs API key: `ELEVENLABS_API_KEY`, else an owner-only file
//! beside `settings.json`.
//!
//! The key is read for each request, so saving one applies to the next
//! dictation without a restart. It is never logged, never written to
//! `settings.json`, and never part of an error.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};

/// The environment variable that overrides the saved key.
pub const ENVIRONMENT: &str = "ELEVENLABS_API_KEY";

/// The key file's name in Hex's data folder.
const FILE: &str = "elevenlabs.key";

/// Longest key Hex accepts. ElevenLabs keys are about 50 characters.
const MAX_KEY_CHARS: usize = 512;

/// Where the key in use comes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Environment,
    File,
}

/// A key and where it came from.
#[derive(Clone, Eq, PartialEq)]
pub struct ApiKey {
    pub secret: String,
    pub source: Source,
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApiKey")
            .field("secret", &redacted(&self.secret))
            .field("source", &self.source)
            .finish()
    }
}

/// The key requests are sent with: the environment's, else the saved one.
pub fn current() -> Option<ApiKey> {
    // Unit tests never read this computer's key or reach ElevenLabs.
    #[cfg(test)]
    return None;
    #[cfg(not(test))]
    resolve(
        std::env::var(ENVIRONMENT).ok().as_deref(),
        path().ok().as_deref(),
    )
}

/// Whether a key is available, without reading it into a caller's hands.
/// The answer is remembered for [`PRESENCE_TTL`], because a recording asks
/// many times a second; saving or clearing a key here forgets it at once,
/// and a key file written by another process is seen within that time.
pub fn is_set() -> bool {
    let mut presence = PRESENCE.lock().unwrap_or_else(|error| error.into_inner());
    if let Some((asked, set)) = *presence
        && asked.elapsed() < PRESENCE_TTL
    {
        return set;
    }
    let set = current().is_some();
    *presence = Some((Instant::now(), set));
    set
}

const PRESENCE_TTL: Duration = Duration::from_secs(1);
static PRESENCE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

fn forget_presence() {
    *PRESENCE.lock().unwrap_or_else(|error| error.into_inner()) = None;
}

fn resolve(environment: Option<&str>, file: Option<&Path>) -> Option<ApiKey> {
    if let Some(secret) = environment.and_then(clean) {
        return Some(ApiKey {
            secret,
            source: Source::Environment,
        });
    }
    let saved = fs::read_to_string(file?).ok()?;
    clean(&saved).map(|secret| ApiKey {
        secret,
        source: Source::File,
    })
}

/// A key as it is sent: trimmed, and `None` when empty or not header-safe.
fn clean(raw: &str) -> Option<String> {
    let key = raw.trim();
    let usable = !key.is_empty()
        && key.chars().count() <= MAX_KEY_CHARS
        && key.chars().all(|c| c.is_ascii_graphic());
    usable.then(|| key.to_string())
}

/// The key file, in Hex's data folder.
pub fn path() -> Result<PathBuf> {
    Ok(crate::app_paths::support_dir()?.join(FILE))
}

/// Saves `key` for this user only, replacing any saved key.
pub fn save(key: &str) -> Result<()> {
    let saved = save_at(&path()?, key);
    forget_presence();
    saved
}

fn save_at(path: &Path, key: &str) -> Result<()> {
    let key = clean(key).ok_or_else(|| {
        eyre!("that is not an API key: it is empty, too long, or has spaces or control characters")
    })?;
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("the key file has no folder"))?;
    fs::create_dir_all(parent).wrap_err("could not create Hex's data folder")?;
    let staging = path.with_extension(format!("key.{}.tmp", std::process::id()));
    if let Err(error) = fs::remove_file(&staging)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(error).wrap_err("could not replace an old staging file");
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    // Whoever reads the key spends the account's quota, so only the owner
    // may. On Windows the file inherits the per-user data folder's ACL.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = (|| -> std::io::Result<()> {
        let mut file = options.open(&staging)?;
        file.write_all(key.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&staging, path)?;
        crate::durable::sync_directory(parent)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&staging);
    }
    written.wrap_err("could not save the API key")
}

/// Removes the saved key. A key from the environment is not Hex's to remove.
pub fn clear() -> Result<bool> {
    let cleared = clear_at(&path()?);
    forget_presence();
    cleared
}

fn clear_at(path: &Path) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).wrap_err("could not remove the saved API key"),
    }
}

/// A key as Settings and `hex key status` show it: its last four characters.
pub fn redacted(key: &str) -> String {
    let tail: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if key.chars().count() <= 8 {
        "••••".into()
    } else {
        format!("••••{tail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "hex-scribe-key-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn the_environment_wins_over_the_saved_key() {
        let directory = directory("environment");
        let file = directory.join(FILE);
        save_at(&file, "sk_saved_key_0001").unwrap();

        let key = resolve(Some("  sk_environment_0002\n"), Some(&file)).unwrap();
        assert_eq!(key.secret, "sk_environment_0002");
        assert_eq!(key.source, Source::Environment);

        let key = resolve(Some("   "), Some(&file)).unwrap();
        assert_eq!(key.secret, "sk_saved_key_0001");
        assert_eq!(key.source, Source::File);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_saved_key_is_readable_by_its_owner_only_and_can_be_cleared() {
        let directory = directory("saved");
        let file = directory.join(FILE);
        save_at(&file, " sk_first_key_000001 ").unwrap();
        save_at(&file, "sk_second_key_00002").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "sk_second_key_00002\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        assert!(clear_at(&file).unwrap());
        assert!(!clear_at(&file).unwrap(), "clearing twice finds nothing");
        assert!(resolve(None, Some(&file)).is_none());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn text_that_cannot_be_a_header_is_not_a_key() {
        let directory = directory("refused");
        let file = directory.join(FILE);
        for bad in ["", "   ", "two words", "line\nbreak", "ключ"] {
            assert!(save_at(&file, bad).is_err(), "{bad:?} must be refused");
        }
        assert!(!file.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_key_is_only_ever_shown_by_its_tail() {
        assert_eq!(redacted("sk_0123456789abcdef"), "••••cdef");
        assert_eq!(redacted("short"), "••••");
        let key = ApiKey {
            secret: "sk_0123456789abcdef".into(),
            source: Source::File,
        };
        assert!(!format!("{key:?}").contains("0123456789"));
    }
}
