use crate::connections::ConnectionManager;
use crate::event_loop::EventHandler;
use crate::events::AppEvent;
use crate::state::AppState;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;
use tracing::info;

pub async fn init_app(app_handle: &AppHandle) -> anyhow::Result<()> {
    info!("Initializing application");

    // Create app state
    let state = AppState::new(app_handle.clone())?;

    // Store state in app
    app_handle.manage(state.clone());

    let saved_theme = state.settings_manager.read().load().theme;
    let tauri_theme = match saved_theme {
        crate::config::settings::Theme::Dark => tauri::Theme::Dark,
        crate::config::settings::Theme::Light => tauri::Theme::Light,
    };
    for label in ["main", "floating"] {
        if let Some(window) = app_handle.get_webview_window(label) {
            let _ = window.set_theme(Some(tauri_theme));
        }
    }

    if let Some(main_window) = app_handle.get_webview_window("main") {
        let saved_windows = state.windows_manager.read().load();
        if let (Some(x), Some(y)) = (saved_windows.main.x, saved_windows.main.y) {
            let _ = main_window
                .set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
        }
    }

    // Create event channel with larger buffer to reduce chance of overflow
    // Increased from 100 to 512 to handle bursts of events without dropping them
    let (tx, mut rx) = mpsc::channel::<AppEvent>(512);
    state.set_event_sender(tx);

    // Start event loop
    let event_handler = EventHandler::new(app_handle.clone());

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            event_handler.process_event(event);
        }
    });

    // Connection manager: owns the spawned SSE receive tasks so connect/
    // disconnect commands can start/abort them at runtime. Registered via
    // `manage()` so commands resolve it as `State<'_, ConnectionManager>`.
    let manager = ConnectionManager::new(state.clone());
    app_handle.manage(manager.clone());

    // Start all enabled connections (initial bring-up).
    manager.start_all().await?;

    // Set backend ready flag
    state.set_backend_ready();
    info!("Backend ready flag set");

    // Emit backend-ready event
    let _ = app_handle.emit("backend-ready", ());
    info!("Emitted backend-ready event");

    // The main window starts hidden so its saved position, native theme and
    // first WebView frame are ready before Windows displays it. This avoids
    // the white default canvas and the visible jump from the default position.
    if let Some(main_window) = app_handle.get_webview_window("main") {
        main_window.show()?;
        main_window.set_focus()?;
        info!("Main window shown after initialization");
    }

    info!("Application initialized successfully");

    Ok(())
}
