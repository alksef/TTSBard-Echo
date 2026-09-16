use crate::events::{connection_status_payload, AppEvent};
use tauri::{AppHandle, Emitter};
use tracing::debug;

/// Bridges internal `AppEvent`s to Tauri webview events.
///
/// Event **names** come from the single source of truth `AppEvent::to_tauri_event`
/// (no string literals duplicated here). Event **payloads** differ per variant
/// (a theme string, a structured status object, a clickthrough bool, …), so they are
/// matched here alongside a debug log line.
pub struct EventHandler {
    app_handle: AppHandle,
}

impl EventHandler {
    pub fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }

    pub fn process_event(&self, event: AppEvent) {
        let name = event.to_tauri_event();
        match &event {
            AppEvent::ThemeChanged(theme) => {
                debug!("Theme changed to: {}", theme);
                let _ = self.app_handle.emit(name, theme);
            }
            AppEvent::FloatingAppearanceChanged => {
                debug!("Floating appearance changed");
                let _ = crate::floating::update_floating_appearance(&self.app_handle);
            }
            AppEvent::ClickthroughChanged(enabled) => {
                debug!("Clickthrough changed to: {}", enabled);
                let _ = self.app_handle.emit(name, enabled);
            }
            AppEvent::FloatingVisibilityChanged(visible) => {
                debug!("Floating visibility changed to: {}", visible);
                let _ = self
                    .app_handle
                    .emit(name, serde_json::json!({ "visible": visible }));
            }
            AppEvent::ConnectionsChanged => {
                debug!("Connections changed");
                let _ = self.app_handle.emit(name, ());
            }
            AppEvent::ConnectionStatusChanged(id, status) => {
                debug!("Connection status changed: {} -> {}", id, status);
                let _ = self
                    .app_handle
                    .emit(name, connection_status_payload(id, status));
            }
            AppEvent::MessageReceived(id, _message) => {
                debug!("Message received from {}", id);
                let _ = self.app_handle.emit(name, (id, _message));
            }
            AppEvent::MessageCleared(id) => {
                debug!("Message cleared for {}", id);
                let _ = self.app_handle.emit(name, id);
            }
            AppEvent::ConnectionAdded(id) => {
                debug!("Connection added: {}", id);
                let _ = self.app_handle.emit(name, id);
            }
            AppEvent::ConnectionRemoved(id) => {
                debug!("Connection removed: {}", id);
                let _ = self.app_handle.emit(name, id);
            }
            AppEvent::TypingChanged(id, is_typing, preview) => {
                debug!("Typing changed for {}: {}", id, is_typing);
                let mut payload = serde_json::json!({ "id": id, "isTyping": is_typing });
                if let Some(text) = preview {
                    payload["previewText"] = serde_json::json!(text);
                }
                let _ = self.app_handle.emit(name, payload);
            }
            AppEvent::SettingsChanged => {
                debug!("Settings changed");
                let _ = self.app_handle.emit(name, ());
            }
            AppEvent::LoggingChanged => {
                debug!("Logging changed");
                let _ = self.app_handle.emit(name, ());
            }
            AppEvent::HotkeysChanged => {
                debug!("Hotkeys changed");
                let _ = self.app_handle.emit(name, ());
            }
            AppEvent::GeneralChanged => {
                debug!("General settings changed");
                let _ = self.app_handle.emit(name, ());
            }
        }
    }
}
