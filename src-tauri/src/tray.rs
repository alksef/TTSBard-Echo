use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager};
use tracing::{info, warn};

/// Show and activate the existing main window without toggling its state.
///
/// Tray actions and a repeated application launch share this path so hidden,
/// minimized and already-visible windows behave consistently.
pub fn show_main_window(app_handle: &AppHandle, source: &'static str) {
    let Some(window) = app_handle.get_webview_window("main") else {
        warn!(source, "Main window is not available");
        return;
    };

    // `unminimize` is idempotent and avoids a synchronous state query before
    // the restore request. This path can be entered from the native
    // single-instance receiver while the WebView event loop is settling.
    if let Err(error) = window.unminimize() {
        warn!(%error, source, "Failed to restore minimized main window");
    }

    if let Err(error) = window.show() {
        warn!(%error, source, "Failed to show main window");
    }
    if let Err(error) = window.set_focus() {
        warn!(%error, source, "Failed to focus main window");
    }
}

/// Initialize system tray with icon and menu
pub fn init_tray(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.handle().clone();

    // Embed the generated tray icon so development and production use the same asset.
    let icon_data = include_bytes!("../icons/32x32.png");
    let decoded_image = image::load_from_memory(icon_data)?;
    let rgba_image = decoded_image.to_rgba8();
    let icon = tauri::image::Image::new_owned(rgba_image.into_raw(), 32, 32);

    // Create menu items
    let show_item = MenuItem::with_id(&app_handle, "show", "Показать", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(&app_handle, "quit", "Выход", true, None::<&str>)?;
    let menu = Menu::with_items(&app_handle, &[&show_item, &quit_item])?;

    // Build tray icon
    info!("Initializing system tray");
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("TTSBard Echo")
        .menu(&menu)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = event
            {
                if matches!(button, MouseButton::Left)
                    && matches!(button_state, MouseButtonState::Up)
                {
                    show_main_window(tray.app_handle(), "tray-click");
                }
            }
        })
        .on_menu_event(|tray, event| match event.id.as_ref() {
            "show" => {
                show_main_window(tray.app_handle(), "tray-menu");
            }
            "quit" => {
                tray.app_handle().exit(0);
            }
            _ => {}
        })
        .build(&app_handle)?;

    info!("System tray initialized");
    Ok(())
}
