# Architecture Overview

## Runtime model

Navix runs single-threaded UI/event loop plus background PTY reader threads.

- Main loop responsibilities:
  - poll shell/preview PTY output
  - render three-pane UI
  - process keyboard/mouse events
  - route state transitions (focus, overlays, config)

## Core components

- `App`: global state container for focus, navigation, preview, config, and overlay state.
- `ShellPane`: PTY wrapper for shell and preview command sessions.
- Navigation model:
  - directory entries
  - selection/scroll state
  - permission-aware command availability

## Panel model

- `[1] Navigation`: cwd list and item metadata.
- `[2] Preview`: directory tree or file command hints.
- `[0] Shell`: interactive shell with scrollback.

## Input model

- Esc-prefix shortcuts drive pane focus and global actions.
- Context-specific handlers for:
  - config editor
  - preview overlay sessions
  - shell pane input
  - navigation pane actions

## Overlay model

Preview command overlay has two presentations:

- static fullscreen (scroll + Esc close)
- interactive fullscreen dimmed (pass-through keys, close on process exit)

## Recent structural split

To reduce `src/main.rs` complexity, extracted:

- `src/terminal_keys.rs`: terminal key encoding helpers.
- `src/panel_layout.rs`: shared nav/preview split layout helper.

Further decomposition candidates:

- `app` state transitions
- config subsystem
- navigation subsystem
- rendering subsystem
