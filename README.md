# navix

Keyboard-first Rust TUI for filesystem navigation, preview command routing, and an embedded shell.

## Platform support

- Primary support target: Linux terminal environments.
- Current implementation relies on Unix/Linux behavior in several runtime paths.

## Quick start

- Run from repo: `bin/navix`
- Or run directly: `cargo run --manifest-path Cargo.toml`

## Layout

- `[1] Navigation` (left): current directory tree, selection, metadata footer.
- `[2] Preview` (right): directory tree preview or file command hints.
- `[0] Shell` (bottom): interactive shell running in a PTY.

## Core controls

- Focus panes:
  - `Esc+0` -> Shell
  - `Esc+1` -> Navigation
  - `Esc+2` -> Preview (may bounce to Navigation; see preview rules)
- Toggle fullish mode:
  - `Esc+f`
- Open config editor:
  - `Esc+c`
- Exit app:
  - `Ctrl+d`

## Navigation controls

- `Up/Down` -> move selection
- `PageUp/PageDown` -> move selection by viewport
- `Enter` on directory -> `cd` into directory
- `Space` on directory -> toggle directory preview on/off
- `Ctrl+Up/Ctrl+Down` -> scroll navigation viewport

## Shell controls

- `Up/Down` are sent to the shell (history behavior stays shell-native)
- `PageUp/PageDown` scroll shell output
- `Esc+Up/Esc+Down` scroll shell output
- Interactive child TUIs can run in the shell pane

## Preview command shortcuts (file selection)

When a file is selected, Navix resolves extension rules and permission bits to show command hints in preview.

- `Esc+r` -> run read command in preview overlay
- `Esc+w` -> run write command in preview overlay
- `Esc+x` -> prefill shell command (clears current shell line first)

If an extension has no explicit rule, Navix uses fallback commands:

- read: `bat {file}`
- write: `$EDITOR {file}`
- exec: disabled (`--`)

## Mouse support

- Left click a panel to focus it (`Navigation`, `Preview`, or `Shell`).
- Preview clicks still follow preview focus bounce rules.
- Mouse is currently used for panel focus selection, not for forwarding rich mouse events into child TUIs.

## Preview focus bounce rules

Preview is intentionally non-sticky in passive states.

- If selected entry is a directory and preview mode is directory tree, focus target becomes `Navigation`.
- If selected entry is a file and preview mode is file command list, focus target becomes `Navigation`.

This applies to both `Esc+2` and mouse-click focus on preview.

## Config file

- Path:
  - `$XDG_CONFIG_HOME/navix/config.toml`, or
  - `~/.config/navix/config.toml`
- Main data: `extension_rules`
- Open in-app editor with `Esc+c`

See `docs/CURRENT_BEHAVIOR.md` for details and config editing controls.

## Project docs

- Behavior reference: `docs/CURRENT_BEHAVIOR.md`
- Architecture notes: `docs/ARCHITECTURE.md`
- Engineering conventions: `docs/PROJECT_CONVENTIONS.md`
- Contribution guide: `CONTRIBUTING.md`
- Security policy: `SECURITY.md`
- Release checklist: `docs/OPEN_SOURCE_RELEASE.md`

## License

GPL-3.0-or-later. See `LICENSE`.
