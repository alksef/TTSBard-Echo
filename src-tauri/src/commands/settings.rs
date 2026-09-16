use crate::commands::{emit_settings_changed, persist_blocking};
use crate::config::dto::AppSettingsDto;
use crate::config::settings::Theme;
use crate::events::AppEvent;
use crate::state::AppState;
use tauri::{Manager, State};
use tracing::info;

/// Get all application settings in a single DTO call.
#[tauri::command]
pub async fn get_all_app_settings(
    app_state: State<'_, AppState>,
) -> Result<AppSettingsDto, String> {
    info!("get_all_app_settings: Loading all settings");
    let config = app_state.settings_manager.read().load();
    let windows_settings = app_state.windows_manager.read().load();
    Ok(AppSettingsDto::from_all_sources(&config, &windows_settings))
}

/// Get all settings in one call (frontend-facing name).
#[tauri::command]
pub async fn get_settings(app_state: State<'_, AppState>) -> Result<AppSettingsDto, String> {
    let config = app_state.settings_manager.read().load();
    let windows_settings = app_state.windows_manager.read().load();
    Ok(AppSettingsDto::from_all_sources(&config, &windows_settings))
}

/// Get current theme.
#[tauri::command]
pub fn get_theme(app_state: State<'_, AppState>) -> Theme {
    app_state.settings_manager.read().load().theme
}

/// Set theme.
#[tauri::command]
pub async fn set_theme(theme: Theme, app_state: State<'_, AppState>) -> Result<(), String> {
    let tauri_theme = match theme {
        Theme::Dark => tauri::Theme::Dark,
        Theme::Light => tauri::Theme::Light,
    };
    let theme_name = match theme {
        Theme::Dark => "dark",
        Theme::Light => "light",
    };
    persist_blocking(app_state.settings_manager.clone(), move |m| {
        m.set_theme(theme)
    })
    .await?;
    for label in ["main", "floating"] {
        if let Some(window) = app_state.app_handle.get_webview_window(label) {
            let _ = window.set_theme(Some(tauri_theme));
        }
    }
    app_state.emit_event(AppEvent::ThemeChanged(theme_name.to_string()));
    emit_settings_changed(&app_state.app_handle);
    Ok(())
}

/// Update theme (frontend-facing alias of `set_theme`).
#[tauri::command]
pub async fn update_theme(theme: Theme, app_state: State<'_, AppState>) -> Result<(), String> {
    set_theme(theme, app_state).await
}

/// Set logging enabled.
#[tauri::command]
pub async fn set_logging_enabled(
    enabled: bool,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    persist_blocking(app_state.settings_manager.clone(), move |m| {
        m.set_logging_enabled(enabled)
    })
    .await?;
    app_state.emit_event(AppEvent::LoggingChanged);
    emit_settings_changed(&app_state.app_handle);
    Ok(())
}

/// Set logging level.
#[tauri::command]
pub async fn set_logging_level(
    level: String,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    persist_blocking(app_state.settings_manager.clone(), move |m| {
        m.set_logging_level(level)
    })
    .await?;
    app_state.emit_event(AppEvent::LoggingChanged);
    emit_settings_changed(&app_state.app_handle);
    Ok(())
}

/// Set hotkey enabled.
#[tauri::command]
pub async fn set_hotkey_enabled(
    enabled: bool,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    persist_blocking(app_state.settings_manager.clone(), move |m| {
        m.set_hotkey_enabled(enabled)
    })
    .await?;
    app_state.emit_event(AppEvent::HotkeysChanged);
    emit_settings_changed(&app_state.app_handle);
    Ok(())
}

/// Set toggle window hotkey.
#[tauri::command]
pub async fn set_toggle_window_hotkey(
    hotkey: Option<String>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    persist_blocking(app_state.settings_manager.clone(), move |m| {
        m.set_toggle_window_hotkey(hotkey)
    })
    .await?;
    app_state.emit_event(AppEvent::HotkeysChanged);
    emit_settings_changed(&app_state.app_handle);
    Ok(())
}

/// Set exclude from capture.
#[tauri::command]
pub async fn set_exclude_from_capture(
    exclude: bool,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    persist_blocking(app_state.settings_manager.clone(), move |m| {
        m.set_exclude_from_capture(exclude)
    })
    .await?;
    app_state.emit_event(AppEvent::GeneralChanged);
    emit_settings_changed(&app_state.app_handle);
    Ok(())
}

/// Choose whether minimizing the main window also hides it from the taskbar.
#[tauri::command]
pub async fn set_hide_on_minimize(
    value: bool,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    persist_blocking(app_state.settings_manager.clone(), move |m| {
        m.set_hide_on_minimize(value)
    })
    .await?;
    app_state.emit_event(AppEvent::GeneralChanged);
    emit_settings_changed(&app_state.app_handle);
    Ok(())
}

/// Set how long the latest received message remains visible.
#[tauri::command]
pub async fn set_message_clear_interval(
    seconds: u32,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    persist_blocking(app_state.settings_manager.clone(), move |m| {
        m.set_message_clear_interval_seconds(seconds)
    })
    .await?;
    app_state.emit_event(AppEvent::GeneralChanged);
    emit_settings_changed(&app_state.app_handle);
    Ok(())
}
