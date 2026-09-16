use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::{
    atomic,
    constants::DEFAULT_LOG_LEVEL,
    recovery, secret,
    validation::{validate_connection_id, validate_connection_name, validate_url},
};

/* ==========================================================================
Theme
========================================================================== */
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

/* ==========================================================================
Connection Config
========================================================================== */
/// The `access_token` field of [`ConnectionConfig`].
///
/// Replaces the never-wired `MaskedAccessTokens` type with a working
/// redaction mechanism (roadmap 010, task 005): every `Debug` formatting of
/// a connection config — and of any structure containing one — shows
/// `[masked]` instead of the token value.
///
/// The serde shape is unchanged (`#[serde(transparent)]` serializes exactly
/// as `Option<String>`), so the settings DTO channel stays intact: the token
/// keeps reaching the connection edit form through `get_connections` /
/// `get_all_app_settings`, the single legitimate channel (ADR-0024).
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretAccessToken(Option<String>);

impl SecretAccessToken {
    pub fn new(token: Option<String>) -> Self {
        Self(token)
    }

    pub fn none() -> Self {
        Self(None)
    }

    pub fn as_deref(&self) -> Option<&str> {
        self.0.as_deref()
    }

    pub fn into_inner(self) -> Option<String> {
        self.0
    }

    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }
}

impl std::fmt::Debug for SecretAccessToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Some(_) => f.write_str("[masked]"),
            None => f.write_str("None"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub access_token: SecretAccessToken,
}

impl ConnectionConfig {
    pub fn validate(&self) -> Result<()> {
        validate_connection_id(&self.id).map_err(|e| anyhow::anyhow!(e))?;
        validate_connection_name(&self.name).map_err(|e| anyhow::anyhow!(e))?;
        validate_url(&self.url).map_err(|e| anyhow::anyhow!(e))?;
        Ok(())
    }
}

/* ==========================================================================
Logging Settings
========================================================================== */
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSettings {
    pub enabled: bool,
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default)]
    pub module_levels: HashMap<String, String>,
}

fn default_log_level() -> String {
    DEFAULT_LOG_LEVEL.to_string()
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            level: default_log_level(),
            module_levels: HashMap::new(),
        }
    }
}

/* ==========================================================================
Hotkey Settings (NEW)
========================================================================== */
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HotkeySettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub toggle_window: Option<String>,
}

/* ==========================================================================
General Settings (NEW)
========================================================================== */
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralSettings {
    #[serde(default)]
    pub exclude_from_capture: bool,
    #[serde(default)]
    pub hide_on_minimize: bool,
    #[serde(default)]
    pub theme: Option<Theme>,
    #[serde(default = "default_message_clear_interval_seconds")]
    pub message_clear_interval_seconds: u32,
}

fn default_message_clear_interval_seconds() -> u32 {
    30
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            exclude_from_capture: false,
            hide_on_minimize: false,
            theme: None,
            message_clear_interval_seconds: default_message_clear_interval_seconds(),
        }
    }
}

/* ==========================================================================
App Settings
========================================================================== */
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    #[serde(default)]
    pub connections: Vec<ConnectionConfig>,
    #[serde(default)]
    pub logging: LoggingSettings,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub hotkeys: HotkeySettings,
    #[serde(default)]
    pub general: GeneralSettings,
}

impl AppSettings {
    pub fn validate(&self) -> Result<()> {
        let mut connection_ids = HashSet::with_capacity(self.connections.len());
        for connection in &self.connections {
            connection.validate()?;
            if !connection_ids.insert(connection.id.as_str()) {
                anyhow::bail!("Duplicate connection id: {}", connection.id);
            }
        }
        Ok(())
    }

    pub fn with_defaults() -> Self {
        Self {
            connections: Vec::new(),
            logging: LoggingSettings::default(),
            theme: Theme::Dark,
            hotkeys: HotkeySettings::default(),
            general: GeneralSettings::default(),
        }
    }
}

/* ==========================================================================
On-disk schema: schema_version, token encryption, migration (task 004)
========================================================================== */
// The `settings.json` file format carries a `schema_version`. It is a backend
// file-format detail (ADR-0024): the envelope below never reaches the webview,
// the in-memory and DTO representation of settings stays [`AppSettings`].
//
// - v1 — the v0.1.0 format: no `schema_version` field, plaintext tokens.
// - v2 (current) — adds the version marker; on Windows access tokens are
//   stored DPAPI-encrypted (base64, `crate::config::secret`) instead of
//   plaintext. Other platforms store plaintext as at baseline.

/// Schema version written by this code.
const CURRENT_SCHEMA_VERSION: u32 = 2;

/// Version assumed when a file has no `schema_version` field (v0.1.0).
const LEGACY_SCHEMA_VERSION: u32 = 1;

fn default_schema_version() -> u32 {
    LEGACY_SCHEMA_VERSION
}

/// The on-disk `settings.json` envelope: schema version plus the flattened
/// settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSettings {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    #[serde(flatten)]
    settings: AppSettings,
}

impl StoredSettings {
    /// The state recovery writes when the file cannot be parsed: defaults at
    /// the current schema version.
    fn defaults_at_current_version() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            settings: AppSettings::with_defaults(),
        }
    }

    /// Wrap in-memory settings for writing: tokens are encrypted for storage
    /// where the platform provides encryption, and the version marker is set
    /// to the current version.
    fn for_writing(settings: &AppSettings) -> Result<Self> {
        let mut settings = settings.clone();
        for connection in &mut settings.connections {
            if let Some(token) = connection.access_token.as_deref() {
                let stored = secret::encrypt_token(token).with_context(|| {
                    format!(
                        "Failed to encrypt access token for connection {}",
                        connection.id
                    )
                })?;
                connection.access_token = SecretAccessToken::new(Some(stored));
            }
        }
        Ok(Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            settings,
        })
    }

    /// Unwrap into the in-memory representation: tokens are decrypted where
    /// the file format stores them encrypted.
    fn into_app_settings(self) -> AppSettings {
        let mut settings = self.settings;
        if self.schema_version >= CURRENT_SCHEMA_VERSION {
            decrypt_connection_tokens(&mut settings);
        }
        settings
    }
}

/// Decrypt every stored connection token in place (a no-op on platforms
/// without token encryption). A token that cannot be decrypted becomes `None`
/// with a warning — loading never fails or panics, the connection starts
/// without the token until it is re-entered through the form, and the stored
/// value is kept on disk. The warning carries the fixed, content-free error
/// text, never the stored value.
#[cfg(windows)]
fn decrypt_connection_tokens(settings: &mut AppSettings) {
    for connection in &mut settings.connections {
        if let Some(stored) = connection.access_token.as_deref() {
            match secret::decrypt_token(stored) {
                Ok(token) => connection.access_token = SecretAccessToken::new(Some(token)),
                Err(error) => {
                    tracing::warn!(
                        "Access token for connection {} is not usable: {error}",
                        connection.id
                    );
                    connection.access_token = SecretAccessToken::none();
                }
            }
        }
    }
}

#[cfg(not(windows))]
fn decrypt_connection_tokens(_settings: &mut AppSettings) {}

/// Serialize and atomically write `settings` in the current on-disk format
/// (task 002's write path, so a failure never damages the existing file).
fn write_settings(settings_file: &Path, settings: &AppSettings) -> Result<()> {
    let stored = StoredSettings::for_writing(settings)?;
    let content = serde_json::to_string_pretty(&stored)?;
    atomic::write_atomic(settings_file, &content)
}

/// One-time migration to the current schema version: a file written by an
/// older version of the format is rewritten atomically at the current version
/// (on Windows with encrypted tokens). Idempotent: a file already at the
/// current version is left untouched, so repeated runs do not repeat the
/// migration. A failed rewrite does not fail startup — the in-memory state
/// stays authoritative, and the next save writes the current version anyway.
fn migrate_schema(settings_file: &Path, file_version: u32, settings: &AppSettings) {
    if file_version >= CURRENT_SCHEMA_VERSION {
        return;
    }
    match write_settings(settings_file, settings) {
        Ok(()) => tracing::info!(
            "Migrated settings file {} from schema version {file_version} to {CURRENT_SCHEMA_VERSION}",
            settings_file.display()
        ),
        Err(error) => tracing::warn!(
            "Could not persist migrated settings {} ({error}); keeping in-memory state, the migration is retried on the next save or start",
            settings_file.display()
        ),
    }
}

/* ==========================================================================
Settings Manager
========================================================================== */
pub struct SettingsManager {
    config_dir: PathBuf,
    cache: Arc<RwLock<AppSettings>>,
}

impl SettingsManager {
    pub fn new() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Failed to get config dir"))?
            .join("ttsbard-echo");

        std::fs::create_dir_all(&config_dir)?;

        let settings = Self::load_initial(&config_dir)?;

        Ok(Self {
            config_dir,
            cache: Arc::new(RwLock::new(settings)),
        })
    }

    /// Load the initial settings from `config_dir` (roadmap 010, tasks 003
    /// and 004).
    ///
    /// A config file that cannot be read or parsed must not fail startup:
    /// recovery quarantines the corrupted file, restores the previous
    /// `.bak` version when it is valid, falls back to defaults otherwise,
    /// and persists the recovered state under the main name. A file in an
    /// older schema version is migrated in place to the current version
    /// (tokens encrypted on Windows); stored tokens are decrypted for the
    /// in-memory state.
    fn load_initial(config_dir: &Path) -> Result<AppSettings> {
        let settings_file = config_dir.join("settings.json");
        if settings_file.exists() {
            let stored: StoredSettings = recovery::load_with_recovery(
                &settings_file,
                StoredSettings::defaults_at_current_version(),
            );
            let file_version = stored.schema_version;
            let settings = stored.into_app_settings();
            migrate_schema(&settings_file, file_version, &settings);
            Ok(settings)
        } else {
            let settings = AppSettings::with_defaults();
            write_settings(&settings_file, &settings)?;
            Ok(settings)
        }
    }

    pub fn load(&self) -> AppSettings {
        self.cache.read().clone()
    }

    pub fn save(&self, settings: &AppSettings) -> Result<()> {
        // Validate before saving
        settings.validate()?;

        let settings_file = self.config_dir.join("settings.json");
        write_settings(&settings_file, settings)?;
        *self.cache.write() = settings.clone();
        Ok(())
    }

    /* ---------------------------------------------------------------------
    Connections
    --------------------------------------------------------------------- */
    pub fn add_connection(&self, connection: ConnectionConfig) -> Result<()> {
        // Validate the connection before adding it
        connection.validate()?;

        let mut settings = self.load();
        settings.connections.push(connection);
        self.save(&settings)
    }

    pub fn remove_connection(&self, id: &str) -> Result<()> {
        let mut settings = self.load();
        let previous_len = settings.connections.len();
        settings.connections.retain(|c| c.id != id);
        if settings.connections.len() == previous_len {
            anyhow::bail!("Connection not found: {id}");
        }
        self.save(&settings)
    }

    pub fn update_connection(&self, id: &str, updated: ConnectionConfig) -> Result<()> {
        // Validate the updated connection
        updated.validate()?;
        if updated.id != id {
            anyhow::bail!(
                "Connection id cannot be changed from {id} to {}",
                updated.id
            );
        }

        let mut settings = self.load();
        if let Some(conn) = settings.connections.iter_mut().find(|c| c.id == id) {
            *conn = updated;
            self.save(&settings)
        } else {
            Err(anyhow::anyhow!("Connection not found: {}", id))
        }
    }

    /* ---------------------------------------------------------------------
    Theme
    --------------------------------------------------------------------- */
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        let mut settings = self.load();
        settings.theme = theme;
        self.save(&settings)
    }

    /* ---------------------------------------------------------------------
    Logging
    --------------------------------------------------------------------- */
    pub fn set_logging_enabled(&self, enabled: bool) -> Result<()> {
        let mut settings = self.load();
        settings.logging.enabled = enabled;
        self.save(&settings)
    }

    pub fn set_logging_level(&self, level: String) -> Result<()> {
        let mut settings = self.load();
        settings.logging.level = level;
        self.save(&settings)
    }

    /* ---------------------------------------------------------------------
    Hotkeys
    --------------------------------------------------------------------- */
    pub fn set_hotkey_enabled(&self, enabled: bool) -> Result<()> {
        let mut settings = self.load();
        settings.hotkeys.enabled = enabled;
        self.save(&settings)
    }

    pub fn set_toggle_window_hotkey(&self, hotkey: Option<String>) -> Result<()> {
        let mut settings = self.load();
        settings.hotkeys.toggle_window = hotkey;
        self.save(&settings)
    }

    /* ---------------------------------------------------------------------
    General
    --------------------------------------------------------------------- */
    pub fn set_exclude_from_capture(&self, exclude: bool) -> Result<()> {
        let mut settings = self.load();
        settings.general.exclude_from_capture = exclude;
        self.save(&settings)
    }

    pub fn set_hide_on_minimize(&self, value: bool) -> Result<()> {
        let mut settings = self.load();
        settings.general.hide_on_minimize = value;
        self.save(&settings)
    }

    pub fn set_message_clear_interval_seconds(&self, seconds: u32) -> Result<()> {
        if !(1..=3600).contains(&seconds) {
            return Err(anyhow::anyhow!(
                "Message clear interval must be between 1 and 3600 seconds"
            ));
        }
        let mut settings = self.load();
        settings.general.message_clear_interval_seconds = seconds;
        self.save(&settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manager() -> (SettingsManager, PathBuf) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let config_dir = std::env::temp_dir().join(format!(
            "ttsbard-echo-settings-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&config_dir).unwrap();
        let initial = AppSettings::with_defaults();
        atomic::write_atomic(
            &config_dir.join("settings.json"),
            &serde_json::to_string_pretty(&initial).unwrap(),
        )
        .unwrap();

        let manager = SettingsManager {
            config_dir: config_dir.clone(),
            cache: Arc::new(RwLock::new(initial)),
        };
        (manager, config_dir)
    }

    #[test]
    fn duplicate_connection_ids_are_rejected_without_changing_state() {
        let (manager, config_dir) = test_manager();
        manager
            .add_connection(connection_with_token("same-id", None))
            .unwrap();

        let error = manager
            .add_connection(connection_with_token("same-id", None))
            .unwrap_err();

        assert!(error.to_string().contains("Duplicate connection id"));
        assert_eq!(manager.load().connections.len(), 1);
        std::fs::remove_dir_all(config_dir).unwrap();
    }

    #[test]
    fn connection_id_cannot_change_during_update() {
        let (manager, config_dir) = test_manager();
        manager
            .add_connection(connection_with_token("original", None))
            .unwrap();

        let error = manager
            .update_connection("original", connection_with_token("replacement", None))
            .unwrap_err();

        assert!(error.to_string().contains("cannot be changed"));
        assert_eq!(manager.load().connections[0].id, "original");
        std::fs::remove_dir_all(config_dir).unwrap();
    }

    #[test]
    fn removing_an_unknown_connection_returns_an_error() {
        let (manager, config_dir) = test_manager();

        let error = manager.remove_connection("missing").unwrap_err();

        assert!(error.to_string().contains("Connection not found"));
        std::fs::remove_dir_all(config_dir).unwrap();
    }

    fn leftover_temp_files(config_dir: &PathBuf) -> bool {
        std::fs::read_dir(config_dir).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        })
    }

    #[test]
    fn hide_on_minimize_defaults_to_false_for_existing_settings() {
        let mut value = serde_json::to_value(AppSettings::with_defaults()).unwrap();
        value["general"]
            .as_object_mut()
            .unwrap()
            .remove("hide_on_minimize");

        let settings: AppSettings = serde_json::from_value(value).unwrap();

        assert!(!settings.general.hide_on_minimize);
    }

    #[test]
    fn hide_on_minimize_round_trips_through_settings_manager() {
        let (manager, config_dir) = test_manager();

        manager.set_hide_on_minimize(true).unwrap();

        assert!(manager.load().general.hide_on_minimize);
        let persisted: AppSettings = serde_json::from_str(
            &std::fs::read_to_string(config_dir.join("settings.json")).unwrap(),
        )
        .unwrap();
        assert!(persisted.general.hide_on_minimize);

        std::fs::remove_dir_all(config_dir).unwrap();
    }

    #[test]
    fn save_writes_valid_json_and_previous_backup() {
        let (manager, config_dir) = test_manager();

        manager.set_theme(Theme::Light).unwrap();

        let persisted: AppSettings = serde_json::from_str(
            &std::fs::read_to_string(config_dir.join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(persisted.theme, Theme::Light);

        let backup: AppSettings = serde_json::from_str(
            &std::fs::read_to_string(config_dir.join("settings.json.bak")).unwrap(),
        )
        .unwrap();
        assert_eq!(backup.theme, Theme::Dark);

        assert!(!leftover_temp_files(&config_dir));

        std::fs::remove_dir_all(config_dir).unwrap();
    }

    #[test]
    fn save_failure_preserves_previous_file() {
        let (manager, config_dir) = test_manager();

        // Make the backup step fail deterministically (portably): the backup
        // path exists as a directory, so copying the target into it errors.
        std::fs::create_dir(config_dir.join("settings.json.bak")).unwrap();
        let result = manager.save(&AppSettings::with_defaults());
        std::fs::remove_dir(config_dir.join("settings.json.bak")).unwrap();

        assert!(result.is_err());

        let persisted: AppSettings = serde_json::from_str(
            &std::fs::read_to_string(config_dir.join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(persisted.theme, Theme::Dark);
        assert_eq!(manager.load().theme, Theme::Dark);

        assert!(!leftover_temp_files(&config_dir));

        std::fs::remove_dir_all(config_dir).unwrap();
    }

    /* ------------------------------------------------------------------
    Corruption recovery (roadmap 010, task 003)
    ------------------------------------------------------------------ */

    /// Sentinel placed into corrupted test files: it must never appear in
    /// the diagnostic text.
    const CORRUPTION_SENTINEL: &str = "SENTINEL-DO-NOT-LEAK-9f3k";

    fn fresh_config_dir() -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let config_dir = std::env::temp_dir().join(format!(
            "ttsbard-echo-settings-recovery-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&config_dir).unwrap();
        config_dir
    }

    fn corrupt_copies(config_dir: &Path) -> Vec<String> {
        std::fs::read_dir(config_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("settings.json.corrupt-"))
            .collect()
    }

    #[test]
    fn corrupted_config_recovers_from_backup() {
        let config_dir = fresh_config_dir();
        let settings_file = config_dir.join("settings.json");
        let backup_file = config_dir.join("settings.json.bak");

        // The previous valid state (theme Light) lives in the backup.
        let mut previous = AppSettings::with_defaults();
        previous.theme = Theme::Light;
        atomic::write_atomic(
            &backup_file,
            &serde_json::to_string_pretty(&previous).unwrap(),
        )
        .unwrap();

        // Corrupted main file: broken JSON (unquoted key, no closing brace)
        // around the sentinel.
        std::fs::write(&settings_file, format!("{{ broken: {CORRUPTION_SENTINEL}")).unwrap();

        let (result, logs) =
            recovery::test_support::capture_warns(|| SettingsManager::load_initial(&config_dir));
        let settings = result.unwrap();
        assert_eq!(settings.theme, Theme::Light);

        // The corrupted file was quarantined next to the main name...
        let quarantined = corrupt_copies(&config_dir);
        assert_eq!(quarantined.len(), 1);
        assert_eq!(
            std::fs::read_to_string(config_dir.join(&quarantined[0])).unwrap(),
            format!("{{ broken: {CORRUPTION_SENTINEL}")
        );

        // ...and a fresh valid file starts a new life at the main name
        // with the recovered state.
        let persisted: AppSettings =
            serde_json::from_str(&std::fs::read_to_string(&settings_file).unwrap()).unwrap();
        assert_eq!(persisted.theme, Theme::Light);
        assert!(!leftover_temp_files(&config_dir));

        // The diagnostic names the file and the recovery, not the content.
        assert!(logs.contains("settings.json"));
        assert!(logs.contains("restored from previous backup"));
        assert!(!logs.contains(CORRUPTION_SENTINEL));

        std::fs::remove_dir_all(config_dir).unwrap();
    }

    #[test]
    fn corrupted_config_and_backup_start_from_defaults() {
        let config_dir = fresh_config_dir();
        let settings_file = config_dir.join("settings.json");
        let backup_file = config_dir.join("settings.json.bak");

        std::fs::write(&backup_file, "not-json-at-all").unwrap();
        std::fs::write(&settings_file, CORRUPTION_SENTINEL).unwrap();

        let (result, logs) =
            recovery::test_support::capture_warns(|| SettingsManager::load_initial(&config_dir));
        let settings = result.unwrap();
        let expected = AppSettings::with_defaults();
        assert_eq!(settings.theme, expected.theme);
        assert!(settings.connections.is_empty());

        // A corrupt copy of the main file is kept next to it...
        let quarantined = corrupt_copies(&config_dir);
        assert_eq!(quarantined.len(), 1);

        // ...and defaults are persisted at the main name.
        let persisted: AppSettings =
            serde_json::from_str(&std::fs::read_to_string(&settings_file).unwrap()).unwrap();
        assert_eq!(persisted.theme, expected.theme);
        assert!(persisted.connections.is_empty());
        assert!(!leftover_temp_files(&config_dir));

        assert!(logs.contains("settings.json"));
        assert!(logs.contains("starting with default values"));
        assert!(!logs.contains(CORRUPTION_SENTINEL));

        std::fs::remove_dir_all(config_dir).unwrap();
    }

    #[test]
    fn corruption_diagnostic_does_not_leak_file_content() {
        let config_dir = fresh_config_dir();
        let settings_file = config_dir.join("settings.json");
        let backup_file = config_dir.join("settings.json.bak");

        let main_sentinel = "MAIN-SENTINEL-7d2e";
        let backup_sentinel = "BACKUP-SENTINEL-4a1c";
        std::fs::write(&backup_file, backup_sentinel).unwrap();
        std::fs::write(&settings_file, main_sentinel).unwrap();

        let (result, logs) =
            recovery::test_support::capture_warns(|| SettingsManager::load_initial(&config_dir));
        let settings = result.unwrap();
        assert!(settings.connections.is_empty());

        // The diagnostic says which file and which recovery was applied...
        assert!(logs.contains("settings.json"));
        assert!(logs.contains("starting with default values"));
        // ...and contains no content of either corrupted file.
        assert!(!logs.contains(main_sentinel));
        assert!(!logs.contains(backup_sentinel));

        std::fs::remove_dir_all(config_dir).unwrap();
    }

    /* ------------------------------------------------------------------
    Schema version, token encryption, migration (roadmap 010, task 004)
    ------------------------------------------------------------------ */

    /// Sentinel placed into plaintext tokens under test: it must never
    /// appear in the persisted file on platforms with token encryption, nor
    /// in any diagnostic.
    const TOKEN_SENTINEL: &str = "SENTINEL-TOKEN-k8m4-plaintext-secret";

    fn legacy_v1_fixture() -> AppSettings {
        let mut settings = AppSettings::with_defaults();
        settings.connections.push(ConnectionConfig {
            id: "conn-legacy".to_string(),
            name: "Legacy".to_string(),
            url: "https://example.com".to_string(),
            enabled: true,
            access_token: SecretAccessToken::new(Some(TOKEN_SENTINEL.to_string())),
        });
        settings
    }

    fn stored_from_file(config_dir: &Path) -> StoredSettings {
        let content = std::fs::read_to_string(config_dir.join("settings.json")).unwrap();
        serde_json::from_str(&content).unwrap()
    }

    fn connection_with_token(id: &str, token: Option<String>) -> ConnectionConfig {
        ConnectionConfig {
            id: id.to_string(),
            name: format!("Connection {id}"),
            url: "https://example.com".to_string(),
            enabled: true,
            access_token: SecretAccessToken::new(token),
        }
    }

    #[test]
    fn legacy_plaintext_config_migrates_once_and_is_idempotent() {
        let config_dir = fresh_config_dir();
        let settings_file = config_dir.join("settings.json");

        // v0.1.0 fixture: no schema_version field, plaintext token.
        let legacy = legacy_v1_fixture();
        let legacy_text = serde_json::to_string_pretty(&legacy).unwrap();
        atomic::write_atomic(&settings_file, &legacy_text).unwrap();

        let settings = SettingsManager::load_initial(&config_dir).unwrap();

        // The rest of the backend keeps seeing the decrypted token...
        assert_eq!(
            settings.connections[0].access_token.as_deref(),
            Some(TOKEN_SENTINEL)
        );

        // ...while the file was rewritten at the current version and, on
        // platforms with token encryption, holds no plaintext token — just
        // the base64 DPAPI blob at the same position.
        let stored = stored_from_file(&config_dir);
        assert_eq!(stored.schema_version, CURRENT_SCHEMA_VERSION);
        let migrated_text = std::fs::read_to_string(&settings_file).unwrap();
        let stored_token = stored.settings.connections[0]
            .access_token
            .clone()
            .into_inner()
            .unwrap();
        #[cfg(windows)]
        {
            assert!(!stored_token.contains(TOKEN_SENTINEL));
            assert_eq!(
                secret::decrypt_token(&stored_token).unwrap(),
                TOKEN_SENTINEL
            );
        }
        #[cfg(not(windows))]
        assert_eq!(stored_token, TOKEN_SENTINEL);

        // The pre-migration file is kept as the previous-version backup.
        let backup = std::fs::read_to_string(config_dir.join("settings.json.bak")).unwrap();
        assert_eq!(backup, legacy_text);

        // Idempotent: a second load neither rewrites the main file (its
        // bytes are unchanged) nor touches the backup — the backup is the
        // decisive proof, a repeated write would move the migrated content
        // over the pre-migration backup.
        let reloaded = SettingsManager::load_initial(&config_dir).unwrap();
        assert_eq!(
            reloaded.connections[0].access_token.as_deref(),
            Some(TOKEN_SENTINEL)
        );
        assert_eq!(
            std::fs::read_to_string(&settings_file).unwrap(),
            migrated_text
        );
        let backup_after = std::fs::read_to_string(config_dir.join("settings.json.bak")).unwrap();
        assert_eq!(backup_after, backup);

        std::fs::remove_dir_all(config_dir).unwrap();
    }

    #[test]
    fn saved_config_keeps_no_plaintext_token_and_loads_transparently() {
        let (manager, config_dir) = test_manager();

        manager
            .add_connection(connection_with_token(
                "conn-new",
                Some(TOKEN_SENTINEL.to_string()),
            ))
            .unwrap();

        // After a plain settings save the file is at the current version and
        // the plaintext token is gone from it (on encryption platforms).
        let file_text = std::fs::read_to_string(config_dir.join("settings.json")).unwrap();
        assert!(file_text.contains("\"schema_version\": 2"));
        #[cfg(windows)]
        assert!(!file_text.contains(TOKEN_SENTINEL));
        #[cfg(not(windows))]
        assert!(file_text.contains(TOKEN_SENTINEL));

        // Transparent for the rest of the backend: `load()` — the source of
        // get_connections and the settings DTOs — returns the token as it
        // was entered.
        assert_eq!(
            manager.load().connections[0].access_token.as_deref(),
            Some(TOKEN_SENTINEL)
        );

        // A fresh load from disk decrypts it back as well.
        let reloaded = SettingsManager::load_initial(&config_dir).unwrap();
        assert_eq!(
            reloaded.connections[0].access_token.as_deref(),
            Some(TOKEN_SENTINEL)
        );

        std::fs::remove_dir_all(config_dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn undecryptable_token_reads_as_unavailable_without_leaking_the_value() {
        let config_dir = fresh_config_dir();
        let settings_file = config_dir.join("settings.json");

        // A schema-v2 file whose token is valid base64 but not a blob this
        // Windows user can decrypt (other profile, other machine, damage).
        let garbage = "AAAABBBBCCCCDDDDEEEE";
        let mut settings = AppSettings::with_defaults();
        settings.connections.push(connection_with_token(
            "conn-lost",
            Some(garbage.to_string()),
        ));
        atomic::write_atomic(
            &settings_file,
            &serde_json::to_string_pretty(&StoredSettings {
                schema_version: CURRENT_SCHEMA_VERSION,
                settings,
            })
            .unwrap(),
        )
        .unwrap();

        let (result, logs) =
            recovery::test_support::capture_warns(|| SettingsManager::load_initial(&config_dir));

        // No panic, no failed startup: the connection reads as "no token",
        // which the existing edit form turns into a normal re-entry flow.
        let settings = result.unwrap();
        assert!(settings.connections[0].access_token.is_none());

        // The stored value stays on disk — loading does not destroy it, so
        // moving the file back to the original profile recovers the token.
        let file_text = std::fs::read_to_string(&settings_file).unwrap();
        assert!(file_text.contains(garbage));

        // The warning names the connection and what to do, never the value.
        assert!(logs.contains("conn-lost"));
        assert!(logs.contains("re-enter"));
        assert!(!logs.contains(garbage));

        std::fs::remove_dir_all(config_dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn decryption_failure_does_not_block_the_rest_of_the_settings() {
        let config_dir = fresh_config_dir();
        let settings_file = config_dir.join("settings.json");

        let mut settings = AppSettings::with_defaults();
        settings.theme = Theme::Light;
        settings.connections.push(connection_with_token(
            "conn-lost",
            Some("AAAABBBBCCCC".to_string()),
        ));
        settings
            .connections
            .push(connection_with_token("conn-fine", None));
        atomic::write_atomic(
            &settings_file,
            &serde_json::to_string_pretty(&StoredSettings {
                schema_version: CURRENT_SCHEMA_VERSION,
                settings,
            })
            .unwrap(),
        )
        .unwrap();

        let loaded = SettingsManager::load_initial(&config_dir).unwrap();
        // Only the undecryptable token is affected.
        assert_eq!(loaded.theme, Theme::Light);
        assert!(loaded.connections[0].access_token.is_none());
        assert!(loaded.connections[1].access_token.is_none());

        std::fs::remove_dir_all(config_dir).unwrap();
    }

    /* ------------------------------------------------------------------
    Token redaction audit (roadmap 010, task 005)
    ------------------------------------------------------------------ */

    /// Any `Debug` formatting of the config structures must not reproduce
    /// the token value: the redacting [`SecretAccessToken`] field type is
    /// the last line of defense against a `{:?}` of a config reaching logs
    /// or errors.
    #[test]
    fn debug_output_of_config_structures_is_token_free() {
        let mut settings = AppSettings::with_defaults();
        settings.connections.push(connection_with_token(
            "conn-dbg",
            Some(TOKEN_SENTINEL.to_string()),
        ));

        let config_debug = format!("{:?}", settings.connections[0]);
        assert!(
            !config_debug.contains(TOKEN_SENTINEL),
            "ConnectionConfig Debug leaks the token: {config_debug}"
        );
        // Positive control: the field is visibly redacted, not silently
        // dropped from the output.
        assert!(config_debug.contains("[masked]"));

        let settings_debug = format!("{settings:?}");
        assert!(
            !settings_debug.contains(TOKEN_SENTINEL),
            "AppSettings Debug leaks the token: {settings_debug}"
        );
    }

    /// The settings DTO is the single legitimate token channel (ADR-0024):
    /// the redacting Debug must not have changed its serde shape.
    #[test]
    fn settings_dto_serialization_keeps_the_legitimate_token_channel_intact() {
        let config = connection_with_token("conn-form", Some(TOKEN_SENTINEL.to_string()));
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["access_token"], TOKEN_SENTINEL);

        let without_token = connection_with_token("conn-empty", None);
        let json = serde_json::to_value(&without_token).unwrap();
        assert_eq!(json["access_token"], serde_json::Value::Null);

        // Deserialization is unchanged too: explicit null, a plain string,
        // and a field missing entirely (pre-token era files) all parse.
        let base = r#"{"id":"conn-x","name":"X","url":"https://example.com","enabled":true"#;
        let parsed: ConnectionConfig = serde_json::from_str(&format!("{base}}}")).unwrap();
        assert!(parsed.access_token.is_none());

        let parsed: ConnectionConfig =
            serde_json::from_str(&format!("{base},\"access_token\":null}}")).unwrap();
        assert!(parsed.access_token.is_none());

        let parsed: ConnectionConfig =
            serde_json::from_str(&format!("{base},\"access_token\":\"{TOKEN_SENTINEL}\"}}"))
                .unwrap();
        assert_eq!(parsed.access_token.as_deref(), Some(TOKEN_SENTINEL));
    }

    /// Loading a file that carries the plaintext token never puts the value
    /// into the diagnostics (the migration/decryption logging surface).
    #[test]
    fn load_diagnostics_never_contain_the_plaintext_token() {
        let config_dir = fresh_config_dir();
        atomic::write_atomic(
            &config_dir.join("settings.json"),
            &serde_json::to_string_pretty(&legacy_v1_fixture()).unwrap(),
        )
        .unwrap();

        let (_settings, logs) =
            recovery::test_support::capture_warns(|| SettingsManager::load_initial(&config_dir));

        assert!(!logs.contains(TOKEN_SENTINEL));

        std::fs::remove_dir_all(config_dir).unwrap();
    }
}
