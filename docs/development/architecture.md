# Application Architecture

## Overview

This document describes the technical architecture of the TTS Bard Echo application, focusing on the structure and integration patterns aligned with app-tts-v2 standards.

## System Components

### Frontend (Vue.js)
- Single Page Application built with Vue 3
- Composition API for composable logic
- Tauri integration for system-level capabilities  
- Component-based UI architecture with CSS token system

### Backend (Rust/Tauri)
- Cross-platform desktop application using Tauri framework
- Event-driven architecture using Rust event system
- Connection management for external services  
- SSE client implementation for real-time communication
- Windows single-instance guard acquired before logging, settings and Tauri
  initialization; repeated launches signal the existing process through a
  bounded `WM_COPYDATA` receiver and then exit

### Data Flow
1. User interactions in frontend
2. Events sent to backend via Tauri API
3. Backend processes events and manages connections
4. External service responses received (via SSE)
5. Responses converted to events and sent back to frontend

## Integration Points

### Windows process lifecycle

- `single_instance.rs` owns the session-scoped mutex, hidden receiver window,
  bounded second-launch delivery and pending-show handoff.
- `run()` acquires the guard before any shared user data or service is touched.
- `tray::show_main_window` is the common non-toggle path used by tray actions
  and repeated launches to restore, show and focus the existing main window.

### Window lifecycle

- `src/components/AppTitlebar.vue` owns the visible minimize action; full exit
  is exposed by `src/components/Sidebar.vue` and tray actions.
- `src-tauri/src/lib.rs` owns native main-window events. On minimize it reads
  `general.hide_on_minimize`: `false` keeps the minimized taskbar entry and
  `true` additionally hides the window to tray.
- `src-tauri/src/tray.rs::show_main_window` is the only restore path for tray
  and repeated-launch activation: unminimize, show, then focus.
- Floating height is content-owned by
  `src/components/floating/FloatingApp.vue`; equal min/max height constraints
  prevent native vertical resize while width remains user-controlled.
- Persisted fields follow the settings boundary documented in
  [Configuration](configuration.md), and window-specific focused tests live
  next to `AppTitlebar`, `SettingsGeneral`, and `FloatingApp`.

### Server-Sent Events (SSE) 
- Client implementation in Rust (`src-tauri/src/connections/client.rs`)
- Message handling via Tauri events system (`AppEvent::MessageReceived`)  
- Frontend snapshot/event processing via `src/composables/useConnections.ts`

### Connection Management
- Connection configuration and lifecycle management
- Authentication token handling 
- Retry logic with exponential backoff

## CSS Architecture

Modern modular CSS architecture:
- Token-based theming system (src/styles/variables/)
- Component-specific styling with CSS variables
- Consistent design patterns aligned with app-tts-v2

## File Structure Organization

```
src/
├── components/          # UI components, including floating and settings
├── composables/         # Settings and connection state/event owners
├── lib/                 # Small presentation helpers
├── styles/              # CSS and theme files
├── test/                # Shared frontend test doubles and fixtures
└── types/               # TypeScript contracts

src-tauri/src/
├── commands/            # Tauri command adapters
├── config/              # Persisted settings, DTOs and validation
└── connections/         # SSE client and connection manager

docs/ 
├── user/                # User documentation
├── development/         # Developer guides
└── integrations/        # Integration documents
```

## Design Patterns

### Component Architecture
- Reusable UI components following app-tts-v2 pattern
- Consistent styling through CSS token system
- Type-safe component interfaces

### Event System  
- Tauri-based event communication between frontend and backend
- Custom AppEvent enum for typed events
- Easy extensibility for new event types

This architecture provides a foundation for extensible, maintainable application with clear separation of concerns and alignment to app-tts-v2 standards.
