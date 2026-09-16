use crate::commands::{emit_settings_changed, persist_blocking};
use crate::config::windows::WindowsSettings;
use crate::events::AppEvent;
use crate::state::AppState;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(serde::Serialize)]
pub struct FloatingVisibilityDto {
    pub visible: bool,
}

#[derive(Clone, serde::Serialize)]
pub struct FloatingAppearanceDto {
    pub opacity: u8,
    pub bg_color: String,
    pub use_custom_color: bool,
    pub clickthrough: bool,
}

fn floating_appearance(settings: &WindowsSettings) -> FloatingAppearanceDto {
    FloatingAppearanceDto {
        opacity: settings.floating.opacity,
        bg_color: settings.floating.bg_color.clone(),
        use_custom_color: settings.floating.use_custom_color,
        clickthrough: settings.floating.clickthrough,
    }
}

#[tauri::command]
pub fn get_floating_appearance(app_state: State<'_, AppState>) -> FloatingAppearanceDto {
    floating_appearance(&app_state.windows_manager.read().load())
}

/// Return the actual visibility of the floating window.
#[tauri::command]
pub async fn get_floating_visibility(app_handle: AppHandle) -> Result<bool, String> {
    app_handle
        .get_webview_window("floating")
        .ok_or_else(|| "Floating window is not available".to_string())?
        .is_visible()
        .map_err(|e| format!("Failed to read floating visibility: {e}"))
}

/// Toggle the floating window and return the applied state.
#[tauri::command]
pub async fn toggle_floating_window(
    app_handle: AppHandle,
    app_state: State<'_, AppState>,
) -> Result<FloatingVisibilityDto, String> {
    let visible = crate::floating::toggle_floating_window(&app_handle, &app_state)
        .map_err(|e| format!("Failed to toggle floating window: {e}"))?;
    Ok(FloatingVisibilityDto { visible })
}

#[tauri::command]
pub async fn reset_floating_window_position(
    app_handle: AppHandle,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    persist_blocking(app_state.windows_manager.clone(), move |m| {
        m.set_floating_position(None, None)
    })
    .await?;
    if let Some(window) = app_handle.get_webview_window("floating") {
        window
            .center()
            .map_err(|e| format!("Failed to center floating window: {e}"))?;
    }
    let _ = app_handle.emit(
        "window-position-changed",
        (Option::<i32>::None, Option::<i32>::None),
    );
    Ok(())
}

/// Show floating window.
#[tauri::command]
pub async fn show_floating_window(
    app_handle: AppHandle,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    crate::floating::show_floating_window(&app_handle, &app_state)
        .map_err(|e| format!("Failed to show floating window: {}", e))
}

/// Restore a persisted visible floating window after its WebView has measured
/// and applied the content-driven size. This keeps the initial 200px config
/// height hidden and avoids a visible resize on application startup.
#[tauri::command]
pub async fn restore_floating_window(
    app_handle: AppHandle,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    if app_state.windows_manager.read().load().floating.visible {
        crate::floating::show_floating_window(&app_handle, &app_state)
            .map_err(|e| format!("Failed to restore floating window: {e}"))?;
    }
    Ok(())
}

/// Hide floating window.
#[tauri::command]
pub async fn hide_floating_window(
    app_handle: AppHandle,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    crate::floating::hide_floating_window(&app_handle, &app_state)
        .map_err(|e| format!("Failed to hide floating window: {}", e))
}

/// Set floating window opacity (`u8` 0–100, frontend sends `{ value }`).
#[tauri::command]
pub async fn set_floating_opacity(
    value: u8,
    app_state: State<'_, AppState>,
) -> Result<FloatingAppearanceDto, String> {
    persist_blocking(app_state.windows_manager.clone(), move |m| {
        m.set_floating_opacity(value)
    })
    .await?;
    app_state.emit_event(AppEvent::FloatingAppearanceChanged);
    emit_settings_changed(&app_state.app_handle);
    Ok(get_floating_appearance(app_state))
}

/// Set floating window background colour (frontend sends `{ color }`).
#[tauri::command]
pub async fn set_floating_bg_color(
    color: String,
    app_state: State<'_, AppState>,
) -> Result<FloatingAppearanceDto, String> {
    persist_blocking(app_state.windows_manager.clone(), move |m| {
        m.set_floating_bg_color(color)
    })
    .await?;
    app_state.emit_event(AppEvent::FloatingAppearanceChanged);
    emit_settings_changed(&app_state.app_handle);
    Ok(get_floating_appearance(app_state))
}

/// Select whether floating uses its own background colour.
#[tauri::command]
pub async fn set_floating_use_custom_color(
    enabled: bool,
    app_state: State<'_, AppState>,
) -> Result<FloatingAppearanceDto, String> {
    persist_blocking(app_state.windows_manager.clone(), move |m| {
        m.set_floating_use_custom_color(enabled)
    })
    .await?;
    app_state.emit_event(AppEvent::FloatingAppearanceChanged);
    emit_settings_changed(&app_state.app_handle);
    Ok(get_floating_appearance(app_state))
}

/// Toggle click-through on the floating window (frontend sends `{ enabled }`,
/// expects the applied value back so the UI can reflect the persisted state).
#[tauri::command]
pub async fn set_clickthrough(
    enabled: bool,
    app_handle: AppHandle,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    persist_blocking(app_state.windows_manager.clone(), move |m| {
        m.set_floating_clickthrough(enabled)
    })
    .await?;

    // Apply to the live window immediately if it exists.
    if let Some(window) = app_handle.get_webview_window("floating") {
        let _ = window.set_ignore_cursor_events(enabled);
    }

    app_state.emit_event(AppEvent::ClickthroughChanged(enabled));
    emit_settings_changed(&app_state.app_handle);
    Ok(enabled)
}
