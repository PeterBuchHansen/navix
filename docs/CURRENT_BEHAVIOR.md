# Navix Current Behavior

This document describes how the current `src/main.rs` implementation behaves.

## Pane model

- `Navigation` is the primary control pane.
- `Preview` shows:
  - directory tree text when directory preview is enabled, or
  - file command shortcuts for selected files.
- `Shell` is an embedded PTY shell panel.

## Focus switching

### Keyboard

- `Esc+0` -> Shell
- `Esc+1` -> Navigation
- `Esc+2` -> Preview target (with bounce rules below)
- `Esc+c` -> Config overlay
- `Esc+f` -> fullish toggle

### Mouse

- Left click on a panel focuses that panel.
- Preview click uses the same focus target logic as `Esc+2`.

## Preview target (bounce) logic

Preview target is computed by entry type and preview mode:

- Directory selected + `DirectoryTree` mode -> target `Navigation`
- File selected + `FileText` mode -> target `Navigation`
- Otherwise -> target `Preview`

This avoids trapping focus in passive preview states.

## Navigation behavior

- `Up/Down`: move selection
- `PageUp/PageDown`: page selection movement
- `Ctrl+Up/Ctrl+Down`: scroll viewport without changing selected row
- `Enter` on directory: `cd` into selected directory
- `Space` on directory: toggles directory preview on/off

Notes:

- File selection does not auto-render file contents in preview.
- For files, preview panel shows extension command shortcuts.

## Preview command shortcuts

When a file is selected, Navix resolves command availability from:

- extension command rule (`read_cmd`, `write_cmd`, `exec_cmd`)
- effective file permissions (read/write/exec)

If command is enabled and permission allows it:

- `Esc+r`: run read command in preview overlay
- `Esc+w`: run write command in preview overlay
- `Esc+x`: prefill shell with exec command

Shell prefill behavior clears the current shell input line first (`Ctrl+a`, `Ctrl+k`) and then inserts command text.

## Fallback extension rule

When no extension rule matches, Navix uses fallback:

- `read_cmd = "bat {file}"`
- `write_cmd = "$EDITOR {file}"`
- `exec_cmd = "--"` (disabled)

## Preview overlay behavior

`Esc+r` and `Esc+w` launch a preview command overlay session.

- Starts in static fullscreen presentation.
- If child session enters alt screen, overlay presentation upgrades to interactive mode.

Static mode:

- `Esc` closes overlay and returns focus to `Navigation`.
- `Up/Down/PageUp/PageDown` scroll overlay output.

Interactive mode:

- Keys are forwarded to child TUI.
- Overlay auto-closes when child process exits.
- Focus returns to `Navigation`.

## Fullish behavior

- Shell fullish:
  - toggled by `Esc+f` when active pane is `Shell`
  - also enabled automatically while shell is in alt-screen mode
- Navigation fullish:
  - toggled by `Esc+f` when active pane is `Navigation` or `Preview`
  - narrows preview panel for navigation-first workflow

## Config overlay behavior

Open/close:

- Open with `Esc+c`
- Close with `Esc`

Primary controls:

- `Ctrl+s`: save
- `Ctrl+r`: reload from disk
- `Ctrl+d`: discard unsaved changes
- `Ctrl+n`: new extension rule
- `Ctrl+Delete` / `Ctrl+Backspace` / `Ctrl+h`: delete selected rule
- Arrow keys: navigate rows/fields
- `Enter`: start or commit field editing

## Config path and defaults

Config file path:

- `$XDG_CONFIG_HOME/navix/config.toml` if `XDG_CONFIG_HOME` is set
- otherwise `~/.config/navix/config.toml`

Default extension rules:

- `md` -> read `bat {file}`, write `$EDITOR {file}`, exec `mdterm {file}`
- `json` -> read `jq . {file}`, write `$EDITOR {file}`, exec `fx {file}`
- `sh` -> read `bat {file}`, write `$EDITOR {file}`, exec `bash {file}`
