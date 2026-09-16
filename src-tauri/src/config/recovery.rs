/* ==========================================================================
Configuration corruption recovery
========================================================================== */
//! Recovery of configuration files that fail to load (roadmap 010, task 003).
//!
//! If a config file cannot be read or parsed at load time, startup must not
//! fail: the corrupted file is quarantined next to it as
//! `<name>.corrupt-<timestamp>` (never deleted, kept for analysis), the
//! previous version written by [`crate::config::atomic`] (exactly one,
//! `<name>.bak`) is tried, and if it is missing or corrupted too, defaults
//! are used. The recovered value is rewritten to the main name so the
//! on-disk state matches the in-memory state from that point on.
//!
//! Diagnostics are `warn`-level and name the file and the recovery applied;
//! they never include the file content.
//!
//! Files the backend processes here but does not (re)write — the quarantined
//! `*.corrupt-*` copy and an already existing file that loads fine — are
//! closed to the current user with the restricted DACL from
//! [`crate::config::acl`] (roadmap 010, task 006), best-effort: a permission
//! failure is logged and never fails the load or the recovery.

use crate::config::acl;
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tracing::warn;

/// Load the config at `target` with corruption recovery.
///
/// - `target` is missing → `default` (the caller writes the initial file);
/// - `target` reads and parses → the parsed value;
/// - `target` is unreadable or unparseable → quarantine the corrupted file,
///   recover from the `.bak` previous version when it is valid, otherwise
///   fall back to `default`, and rewrite the recovered value to `target`.
///
/// This function never fails: startup must not depend on the validity of
/// the config file on disk.
pub fn load_with_recovery<T>(target: &Path, default: T) -> T
where
    T: DeserializeOwned + Serialize,
{
    if !target.exists() {
        return default;
    }
    match read_and_parse::<T>(target) {
        Ok(settings) => {
            // The file loaded fine, but it may carry a wide v0.1.0 DACL and
            // would only be rewritten on the next save: close it now.
            close_broad_rights(target);
            settings
        }
        Err(error) => recover(target, error, default),
    }
}

/// Recovery branch: quarantine the corrupted file, pick the last valid
/// state (previous version or defaults), and persist it to the main name.
fn recover<T>(target: &Path, load_error: anyhow::Error, default: T) -> T
where
    T: DeserializeOwned + Serialize,
{
    let quarantine_note = match quarantine(target) {
        Some(quarantined) => format!("corrupted copy saved as {}", quarantined.display()),
        None => "corrupted copy could not be saved".to_string(),
    };
    warn!(
        "Config file {} is corrupted ({}); {}",
        target.display(),
        load_error,
        quarantine_note
    );

    let backup = crate::config::atomic::backup_path_for(target);
    let recovered = if backup.exists() {
        match read_and_parse::<T>(&backup) {
            Ok(settings) => {
                warn!(
                    "Config {} restored from previous backup {}",
                    target.display(),
                    backup.display()
                );
                settings
            }
            Err(error) => {
                warn!(
                    "Config backup {} is corrupted too ({}); starting with default values",
                    backup.display(),
                    error
                );
                default
            }
        }
    } else {
        warn!(
            "No backup available for {}; starting with default values",
            target.display()
        );
        default
    };

    restore_to_target(target, &recovered);
    recovered
}

/// Move the corrupted `target` to `<name>.corrupt-<timestamp>` and return
/// the new path. The corrupted content is never deleted. Best effort: a
/// rename failure is logged and recovery continues without the copy.
fn quarantine(target: &Path) -> Option<PathBuf> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".to_string());
    let quarantined = target.with_file_name(format!("{name}.corrupt-{timestamp}"));
    match std::fs::rename(target, &quarantined) {
        Ok(()) => {
            // The quarantined copy kept the DACL of the original file;
            // restrict it like every other configuration artifact.
            close_broad_rights(&quarantined);
            Some(quarantined)
        }
        Err(error) => {
            warn!(
                "Failed to quarantine corrupted config {} ({}); continuing recovery",
                target.display(),
                error
            );
            None
        }
    }
}

/// Restrict the DACL of a processed configuration file (task 006). Best
/// effort: loading and recovery must not fail on a permissions problem, a
/// failure is only logged. The write mechanism applies the restriction
/// strictly.
fn close_broad_rights(path: &Path) {
    if let Err(error) = acl::restrict_to_current_user(path) {
        warn!(
            "Could not restrict permissions on config {} ({error})",
            path.display()
        );
    }
}

/// Rewrite `settings` to `target` so the recovered in-memory state is
/// persisted immediately. Best effort: a failure here does not stop
/// startup, the in-memory state stays authoritative.
fn restore_to_target<T: Serialize>(target: &Path, settings: &T) {
    // The corrupt target may still exist when quarantine failed (e.g. the
    // file is held open by another process). Clear it before writing:
    // `write_atomic` would otherwise copy the corrupted content over the
    // valid `.bak` backup.
    if target.exists() {
        match std::fs::remove_file(target) {
            Ok(()) => {}
            Err(error) => {
                warn!(
                    "Could not clear corrupted config {} before restore ({}); keeping in-memory state only",
                    target.display(),
                    error
                );
                return;
            }
        }
    }
    match serde_json::to_string_pretty(settings) {
        Ok(content) => {
            if let Err(error) = crate::config::atomic::write_atomic(target, &content) {
                warn!(
                    "Could not persist recovered config {} ({}); continuing with in-memory state",
                    target.display(),
                    error
                );
            }
        }
        Err(error) => {
            warn!(
                "Could not serialize recovered config {} ({})",
                target.display(),
                error
            );
        }
    }
}

/// Read and JSON-parse `path`. Errors carry the path and the failure kind,
/// never the file content.
fn read_and_parse<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Capture `warn`-level diagnostics produced by recovery so tests can
    //! assert on the diagnostic text (roadmap 010, task 003).

    use std::io::{self, Write};
    use std::sync::{Arc, Mutex, MutexGuard};
    use tracing_subscriber::fmt::writer::MakeWriter;

    /// Run `work` with a `warn`-only subscriber that captures formatted
    /// events, and return the result together with the captured log text.
    pub fn capture_warns<R>(work: impl FnOnce() -> R) -> (R, String) {
        let buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_writer(Factory {
                buffer: buffer.clone(),
            })
            .finish();
        let result = tracing::subscriber::with_default(subscriber, work);
        let logs = String::from_utf8_lossy(&buffer.lock().unwrap().clone()).into_owned();
        (result, logs)
    }

    struct Factory {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    struct BufferWriter<'a> {
        guard: MutexGuard<'a, Vec<u8>>,
    }

    impl Write for BufferWriter<'_> {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.guard.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Factory {
        type Writer = BufferWriter<'a>;
        fn make_writer(&'a self) -> BufferWriter<'a> {
            BufferWriter {
                guard: self.buffer.lock().unwrap(),
            }
        }
    }
}
