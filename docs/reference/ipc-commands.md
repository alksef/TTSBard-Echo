# IPC commands

The authoritative command list is the `invoke_handler` in `src-tauri/src/lib.rs`. Use `npm run check:ipc` to detect frontend calls that are not registered. Important UI commands include `get_all_app_settings`, connection CRUD/connectivity commands, `get_connection_runtime_snapshot`, `toggle_floating_window`, `set_clickthrough`, `set_hide_on_minimize`, the floating appearance setters, and `reset_floating_window_position`.
