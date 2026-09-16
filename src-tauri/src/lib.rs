// The command surface and frontend contracts are still converging (see
// `.work/ai/reviews/review-002-2026-08-04.md`): several getter commands,
// `AppEvent` variants, and manager helpers are defined ahead of being wired
// into `generate_handler!` / `event_loop`. Silence crate-wide dead-code so the
// `-D warnings` gate stays meaningful for real lints without churning these
// reserved APIs one `#[allow]` at a time. Revisit once the connect/disconnect
// and floating-window flows are fully wired.
#![allow(dead_code)]

mod commands;
mod config;
mod connections;
mod event_loop;
mod events;
mod floating;
mod setup;
mod state;
mod tray;

use tauri::{Manager, RunEvent};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Holds the optional `WorkerGuard` for the non-blocking file appender.
///
/// Not `Clone` — `WorkerGuard` is not cloneable, and there is exactly one guard per
/// process lifetime (held in `run()` for the duration of the app). Dropping it flushes
/// and closes the log file.
pub struct LogGuard {
    _guard: Option<WorkerGuard>,
}

/// Build the `EnvFilter` used by both the stdout-only and file-backed logging paths.
///
/// `add_directive` returns `EnvFilter`, not `Result` — each directive's `.parse()` is
/// the only fallible step and is handled with `?` here.
fn build_env_filter() -> Result<EnvFilter, Box<dyn std::error::Error>> {
    Ok(EnvFilter::from_default_env()
        .add_directive("ttsbard_echo=debug".parse()?)
        .add_directive("tauri=warn".parse()?))
}

pub fn init_logging() -> Result<LogGuard, Box<dyn std::error::Error>> {
    let config_dir = dirs::config_dir().ok_or("Failed to get config dir")?;

    let log_dir = config_dir.join("ttsbard-echo").join("logs");
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!(
            "Failed to create log directory: {}, falling back to stdout only",
            e
        );
        // Fall back to stdout logging only.
        let env_filter = build_env_filter()?;
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().with_writer(std::io::stdout))
            .init();
        tracing::info!("Logging initialized with stdout only.");
        return Ok(LogGuard { _guard: None });
    }

    let log_file = log_dir.join("ttsbard-echo.log");
    let file_appender = tracing_appender::rolling::never(log_dir, "ttsbard-echo.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = build_env_filter()?;
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stdout))
        .with(fmt::layer().with_writer(non_blocking))
        .init();

    tracing::info!("Logging initialized. Log file: {:?}", log_file);

    Ok(LogGuard {
        _guard: Some(guard),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _log_guard = init_logging().expect("Failed to initialize logging");

    let app = tauri::Builder::default()
        .setup(|app| {
            // Initialize app on tokio runtime
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = setup::init_app(&handle).await {
                    tracing::error!("Failed to initialize app: {}", e);
                }
            });

            // Initialize system tray
            tray::init_tray(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Connections
            commands::connections::add_connection,
            commands::connections::remove_connection,
            commands::connections::get_connections,
            commands::connections::get_connection_runtime_snapshot,
            commands::connections::connect_connection,
            commands::connections::disconnect_connection,
            commands::connections::update_connection,
            commands::connections::test_connection,
            commands::connections::export_diagnostics,
            // Settings - Get
            commands::settings::get_settings,
            commands::settings::get_all_app_settings,
            commands::settings::get_theme,
            // Settings - Theme
            commands::settings::update_theme,
            // Settings - Logging
            commands::settings::set_logging_enabled,
            commands::settings::set_logging_level,
            // Settings - Hotkeys
            commands::settings::set_hotkey_enabled,
            commands::settings::set_toggle_window_hotkey,
            // Settings - General
            commands::settings::set_exclude_from_capture,
            commands::settings::set_message_clear_interval,
            // Windows
            commands::windows::show_floating_window,
            commands::windows::hide_floating_window,
            commands::windows::get_floating_visibility,
            commands::windows::get_floating_appearance,
            commands::windows::toggle_floating_window,
            commands::windows::set_floating_opacity,
            commands::windows::set_floating_bg_color,
            commands::windows::set_floating_use_custom_color,
            commands::windows::set_clickthrough,
            commands::windows::reset_floating_window_position,
            // App
            commands::app::quit_app,
            commands::app::is_backend_ready,
            commands::app::confirm_backend_ready,
        ])
        .on_window_event(|window, event| {
            if window.label() == "main" {
                match event {
                    tauri::WindowEvent::CloseRequested { api, .. } => {
                        tracing::info!("Main window close requested - hiding to tray");
                        api.prevent_close();
                        let _ = window.hide();
                    }
                    tauri::WindowEvent::Moved(position) => {
                        if let Some(state) = window.try_state::<state::AppState>() {
                            if let Err(error) = state
                                .windows_manager
                                .read()
                                .set_main_position(Some(position.x), Some(position.y))
                            {
                                tracing::warn!(%error, "Failed to persist main window position");
                            }
                        }
                    }
                    _ => {}
                }
            } else if window.label() == "floating" {
                if let tauri::WindowEvent::Moved(position) = event {
                    if let Some(state) = window.try_state::<state::AppState>() {
                        if let Err(error) = state
                            .windows_manager
                            .read()
                            .set_floating_position(Some(position.x), Some(position.y))
                        {
                            tracing::warn!(%error, "Failed to persist floating window position");
                        }
                    }
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let RunEvent::Exit = event {
            // Cooperative shutdown of every running connection (roadmap 011,
            // task 004): cancel all tokens so no retry sleep, attempt, or
            // status event survives the app, then abort the handles as the
            // backstop. The manager is managed during setup; if the app
            // exits before that ran, there is nothing to stop.
            if let Some(manager) = app_handle.try_state::<crate::connections::ConnectionManager>() {
                manager.stop_all();
            }
        }
    });
}
