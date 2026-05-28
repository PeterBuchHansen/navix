# Changelog

All notable changes to this project are documented here.

## [Unreleased]

### Added

- No changes yet.

## [0.3.0] - 2026-05-28

### Added

- Preview path jump input flow with persisted history, compact multi-column completion, completion cycling (`Tab`/`Shift+Tab`), and parent (`../`) completion support.
- Preview input completion navigation via `Up`/`Down` and `Left`/`Right`, including completion-first `Enter` behavior.
- Help overlay scrolling with status + scrollbar rendering, plus copy-by-selection support in Help/Config overlays.
- Mouse selection/copy parity for Preview panel and overlay content snapshots.
- Project hygiene tooling and collaboration templates: `.editorconfig`, `rustfmt.toml`, CI workflow, issue templates, and PR template.

### Changed

- Removed preview bounce behavior; Preview focus is now deterministic for keyboard and click routing.
- Refined panel title rendering so `╭─[x]─Panel...` keeps the leading dash styled as border instead of title text.
- Reworked Preview jump UI styling and interaction (input border focus styling, placeholder clamping, history wheel rendering, completion highlighting/colors).
- Reorganized Help content for readability: active panel section first, aligned shortcut colons, concise Preview input sections, underlined section headers, bold subsection labels, and icon-aware text.
- Simplified footer behavior for overlays so Help and Config both use close-focused footer shortcuts.
- Updated Preview jump `Enter` semantics:
  - default `Enter` jumps and returns focus to Navigation on success,
  - `Shift+Enter` jumps while keeping Preview focus,
  - directory completion accepts first (with trailing slash) before jump.

### Fixed

- Root-agnostic permission behavior in file command access tests.
- Relative completion label rendering + selection highlighting consistency for compact completion output.
- File jump preselection retention across asynchronous directory refresh (`nav_preferred_selection_path` no longer drops early).
- Completion edge cases around parent navigation:
  - `../` forced to last completion entry,
  - `..`/`.../..` queries now only complete to `../`,
  - completion-selected inputs normalize parent traversals to avoid path noise growth.
- Navigation key-repeat regressions:
  - held shortcut keys (`r/w/x`) no longer spam the filter,
  - held Backspace still repeats deletion while filter has text,
  - once filter empties, parent `cd ..` requires a fresh backspace cycle.
- Shortcut/filter interoperability so `r/w/x` can still start filter text while preserving `r+Enter`/`w+Enter`/`x+Enter` file actions.

### Docs

- Updated `README.md` and `docs/CURRENT_BEHAVIOR.md` to reflect Preview jump input, help/overlay interaction updates, and current key behavior.

### Tests

- Expanded regression coverage for Preview jump completions, path normalization, and Navigation shortcut/filter edge handling.
- Total automated coverage now includes 176 passing tests.

## [0.2.0] - 2026-05-26

### Added

- Modular runtime architecture by splitting the former monolithic implementation into focused modules (`app_state`, `config`, `file_logic`, `input_routing`, `navigation`, `runtime_helpers`, `shell`, `theme`, `tui`, etc.).
- Esc-prefixed pane shortcuts and file command shortcut routing for Preview/Shell workflows.
- Mouse click panel focus routing and richer preview overlay interaction handling.
- Preview command overlay modes with static vs interactive session behavior.
- In-app extension configuration editor with validation and persistence.
- Expanded automated test suite across runtime helpers, input routing, shell behavior, config editing, file logic, and TUI rendering.
- Open-source release scaffolding: contributor/security documentation, architecture/conventions docs, and release workflow docs.

### Changed

- `main.rs` was reduced from a large single-file flow to a composition layer coordinating modular subsystems.
- Preview command templating now shell-escapes `{file}` by default.
- PTY output handling now uses bounded draining behavior to avoid unbounded memory growth.
- Key forwarding expanded for more special keys/modifier paths.
- Fullish layout behavior unified for Navigation and Preview active states.

### Docs

- Added and expanded `README.md`, `docs/CURRENT_BEHAVIOR.md`, `docs/ARCHITECTURE.md`, `docs/PROJECT_CONVENTIONS.md`, `CONTRIBUTING.md`, and `SECURITY.md`.

## [0.1.0] - 2026-05-25

### Added

- Initial Navix MVP TUI workflow.
- Three-pane baseline UI: Navigation, Preview, and embedded Shell.
- Core keyboard navigation and command execution flow in a single-file implementation.
- Initial launcher script (`bin/navix`) and foundational project documentation.
