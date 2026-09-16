/* ==========================================================================
Atomic configuration file writes
========================================================================== */
//! Every configuration file write goes through [`write_atomic`]: the new
//! content is first written to a temporary file in the same directory as the
//! target (same volume, so the replace is atomic), flushed and synced to
//! disk, and only then the previous version is kept as `<name>.bak` and the
//! temporary file replaces the target via rename. A failure at any point
//! leaves the existing target file untouched and removes the temporary file.
//!
//! This module is the single point through which all configuration file
//! writes pass (roadmap 010, task 002). It is also the single point that
//! applies the restricted DACL to every written artifact (task 006, see
//! [`crate::config::acl`]): on Windows the temporary file is restricted
//! before the rename (a rename moves its security descriptor onto the
//! target) and the backup copy is restricted right after it is made.

use crate::config::acl;
use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Atomically write `content` to `target`.
///
/// If `target` already exists, its current content is first copied to the
/// backup path (exactly one previous version is kept), then the temporary
/// file is renamed over `target`. On any error the temporary file is
/// removed and `target` is left as it was.
pub fn write_atomic(target: &Path, content: &str) -> Result<()> {
    let temp_path = temp_path_for(target);
    let result = write_and_replace(target, &temp_path, content);
    if result.is_err() {
        // Best-effort cleanup; the original error is reported instead.
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

fn write_and_replace(target: &Path, temp_path: &Path, content: &str) -> Result<()> {
    write_to_temp(temp_path, content)
        .with_context(|| format!("Failed to write temporary file {}", temp_path.display()))?;
    // The rename below moves the temporary file's security descriptor onto
    // the target, so the restricted DACL (task 006) must be applied before
    // the swap. A failure aborts the write atomically: the target stays
    // untouched and the caller removes the temporary file.
    acl::restrict_to_current_user(temp_path).with_context(|| {
        format!(
            "Failed to restrict permissions on temporary file {}",
            temp_path.display()
        )
    })?;
    replace_with_backup(target, temp_path)
}

/// Path of the temporary file used while writing `target`.
fn temp_path_for(target: &Path) -> PathBuf {
    let dir = target.parent().unwrap_or(Path::new("."));
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".to_string());
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    dir.join(format!("{}.{}.{}.tmp", name, std::process::id(), counter))
}

/// Path of the backup copy of `target` (the previous version).
///
/// `pub(crate)` so the corruption-recovery path (roadmap 010, task 003)
/// reuses exactly the same backup name the writer produces.
pub(crate) fn backup_path_for(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".to_string());
    target.with_file_name(format!("{name}.bak"))
}

fn write_to_temp(temp_path: &Path, content: &str) -> std::io::Result<()> {
    let mut file = std::fs::File::create_new(temp_path)?;
    file.write_all(content.as_bytes())?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

fn replace_with_backup(target: &Path, temp_path: &Path) -> Result<()> {
    let backup_path = backup_path_for(target);
    if target.exists() {
        std::fs::copy(target, &backup_path).with_context(|| {
            format!(
                "Failed to back up {} to {}",
                target.display(),
                backup_path.display()
            )
        })?;
        // The backup copy carries the previous target's DACL — possibly a
        // wide v0.1.0 one — so it is restricted like every other written
        // artifact. A failure aborts the write before the rename: the
        // target stays untouched.
        acl::restrict_to_current_user(&backup_path).with_context(|| {
            format!(
                "Failed to restrict permissions on backup file {}",
                backup_path.display()
            )
        })?;
    }
    std::fs::rename(temp_path, target).with_context(|| {
        format!(
            "Failed to replace {} with {}",
            target.display(),
            temp_path.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir() -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "ttsbard-echo-atomic-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn file_names(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn successful_write_produces_valid_file_and_previous_backup() {
        let dir = test_dir();
        let target = dir.join("settings.json");

        write_atomic(&target, "v1").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "v1");
        assert!(!dir.join("settings.json.bak").exists());
        assert!(file_names(&dir).iter().all(|name| !name.ends_with(".tmp")));

        write_atomic(&target, "v2").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "v2");
        assert_eq!(
            std::fs::read_to_string(dir.join("settings.json.bak")).unwrap(),
            "v1"
        );

        write_atomic(&target, "v3").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "v3");
        // Exactly one previous version is kept.
        assert_eq!(
            std::fs::read_to_string(dir.join("settings.json.bak")).unwrap(),
            "v2"
        );

        assert!(file_names(&dir).iter().all(|name| !name.ends_with(".tmp")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_failure_preserves_previous_file() {
        let dir = test_dir();
        let target = dir.join("settings.json");
        write_atomic(&target, "v1").unwrap();

        // Make the backup step fail deterministically (portably): the backup
        // path exists as a directory, so copying the target into it errors.
        std::fs::create_dir(dir.join("settings.json.bak")).unwrap();
        let result = write_atomic(&target, "v2");
        std::fs::remove_dir(dir.join("settings.json.bak")).unwrap();

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "v1");
        assert!(file_names(&dir).iter().all(|name| !name.ends_with(".tmp")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_failure_leaves_no_temp_files() {
        let dir = test_dir();
        let target = dir.join("missing-subdir").join("settings.json");

        assert!(write_atomic(&target, "v1").is_err());
        assert!(file_names(&dir).iter().all(|name| !name.ends_with(".tmp")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
