//! Data Transfer Objects for unified settings loading
//!
//! This module defines DTOs for the `get_all_app_settings` command.
//! These structures serialize all application settings into a single response.

use serde::{Deserialize, Serialize};

use crate::config::settings::{ConnectionConfig, Theme};

// ============================================================================
// Logging Settings DTO
// ============================================================================

/// Logging settings DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSettingsDto {
    pub enabled: bool,
    pub level: String,
    #[serde(default)]
    pub module_levels: std::collections::HashMap<String, String>,
}

impl From<crate::config::settings::LoggingSettings> for LoggingSettingsDto {
    fn from(s: crate::config::settings::LoggingSettings) -> Self {
        Self {
            enabled: s.enabled,
            level: s.level,
            module_levels: s.module_levels,
        }
    }
}

impl From<LoggingSettingsDto> for crate::config::settings::LoggingSettings {
    fn from(dto: LoggingSettingsDto) -> Self {
        Self {
            enabled: dto.enabled,
            level: dto.level,
            module_levels: dto.module_levels,
        }
    }
}

// ============================================================================
// Hotkey Settings DTO
// ============================================================================

/// Hotkey settings DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeySettingsDto {
    pub enabled: bool,
    pub toggle_window: Option<String>,
}

impl From<crate::config::settings::HotkeySettings> for HotkeySettingsDto {
    fn from(s: crate::config::settings::HotkeySettings) -> Self {
        Self {
            enabled: s.enabled,
            toggle_window: s.toggle_window,
        }
    }
}

impl From<HotkeySettingsDto> for crate::config::settings::HotkeySettings {
    fn from(dto: HotkeySettingsDto) -> Self {
        Self {
            enabled: dto.enabled,
            toggle_window: dto.toggle_window,
        }
    }
}

// ============================================================================
// General Settings DTO
// ============================================================================

/// General settings DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralSettingsDto {
    pub exclude_from_capture: bool,
    pub hide_on_minimize: bool,
    pub theme: Option<String>,
    pub message_clear_interval_seconds: u32,
}

impl From<crate::config::settings::GeneralSettings> for GeneralSettingsDto {
    fn from(s: crate::config::settings::GeneralSettings) -> Self {
        let theme_str = s.theme.as_ref().map(|t| {
            match t {
                Theme::Dark => "dark",
                Theme::Light => "light",
            }
            .to_string()
        });

        Self {
            exclude_from_capture: s.exclude_from_capture,
            hide_on_minimize: s.hide_on_minimize,
            theme: theme_str,
            message_clear_interval_seconds: s.message_clear_interval_seconds,
        }
    }
}

impl From<GeneralSettingsDto> for crate::config::settings::GeneralSettings {
    fn from(dto: GeneralSettingsDto) -> Self {
        let theme = dto.theme.as_ref().and_then(|t| match t.as_str() {
            "dark" => Some(Theme::Dark),
            "light" => Some(Theme::Light),
            _ => None,
        });

        Self {
            exclude_from_capture: dto.exclude_from_capture,
            hide_on_minimize: dto.hide_on_minimize,
            theme,
            message_clear_interval_seconds: dto.message_clear_interval_seconds,
        }
    }
}

// ============================================================================
// Window Position DTO
// ============================================================================

/// Window position DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowPositionDto {
    pub x: Option<i32>,
    pub y: Option<i32>,
}

impl From<crate::config::windows::WindowPosition> for WindowPositionDto {
    fn from(pos: crate::config::windows::WindowPosition) -> Self {
        Self { x: pos.x, y: pos.y }
    }
}

impl From<WindowPositionDto> for crate::config::windows::WindowPosition {
    fn from(dto: WindowPositionDto) -> Self {
        Self { x: dto.x, y: dto.y }
    }
}

// ============================================================================
// Floating Window DTO
// ============================================================================

/// Floating window DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FloatingWindowDto {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub opacity: u8,
    pub bg_color: String,
    pub clickthrough: bool,
    pub use_custom_color: bool,
    pub visible: bool,
}

impl From<crate::config::windows::FloatingWindowSettings> for FloatingWindowDto {
    fn from(fw: crate::config::windows::FloatingWindowSettings) -> Self {
        Self {
            x: fw.position.x,
            y: fw.position.y,
            opacity: fw.opacity,
            bg_color: fw.bg_color,
            clickthrough: fw.clickthrough,
            use_custom_color: fw.use_custom_color,
            visible: fw.visible,
        }
    }
}

impl From<FloatingWindowDto> for crate::config::windows::FloatingWindowSettings {
    fn from(dto: FloatingWindowDto) -> Self {
        Self {
            position: crate::config::windows::WindowPosition { x: dto.x, y: dto.y },
            opacity: dto.opacity,
            bg_color: dto.bg_color,
            clickthrough: dto.clickthrough,
            use_custom_color: dto.use_custom_color,
            visible: dto.visible,
        }
    }
}

// ============================================================================
// Windows Settings DTO
// ============================================================================

/// Windows settings DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowsSettingsDto {
    pub main: WindowPositionDto,
    pub floating: FloatingWindowDto,
}

impl From<crate::config::windows::WindowsSettings> for WindowsSettingsDto {
    fn from(ws: crate::config::windows::WindowsSettings) -> Self {
        Self {
            main: ws.main.into(),
            floating: ws.floating.into(),
        }
    }
}

impl From<WindowsSettingsDto> for crate::config::windows::WindowsSettings {
    fn from(dto: WindowsSettingsDto) -> Self {
        Self {
            main: dto.main.into(),
            floating: dto.floating.into(),
        }
    }
}

// ============================================================================
// App Settings DTO
// ============================================================================

/// Application settings DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettingsDto {
    pub connections: Vec<ConnectionConfig>,
    pub logging: LoggingSettingsDto,
    pub hotkeys: HotkeySettingsDto,
    pub general: GeneralSettingsDto,
    pub windows: WindowsSettingsDto,
}

impl AppSettingsDto {
    /// Aggregate settings from both managers into a single DTO.
    ///
    /// `get_all_app_settings` (the only caller) builds this inline today;
    /// this constructor keeps the aggregation logic in one place.
    pub fn from_all_sources(
        app_settings: &crate::config::settings::AppSettings,
        windows_settings: &crate::config::windows::WindowsSettings,
    ) -> Self {
        let mut general = GeneralSettingsDto::from(app_settings.general.clone());
        general.theme = Some(
            match app_settings.theme {
                Theme::Dark => "dark",
                Theme::Light => "light",
            }
            .to_string(),
        );

        Self {
            connections: app_settings.connections.clone(),
            logging: app_settings.logging.clone().into(),
            hotkeys: app_settings.hotkeys.clone().into(),
            general,
            windows: windows_settings.clone().into(),
        }
    }
}
