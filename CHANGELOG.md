# Changelog

All notable changes to this project are documented here.

## [Unreleased]

### Added

- No changes yet.

## [0.3.4] - 2026-06-19

Host-directed terminal side channels that nested tools emit in the Shell and
Preview panes now reach the host terminal (or are answered locally) instead of
being silently dropped by the local emulator, matching how the same tools behave
when run directly in the host terminal.

### Added

- Forward child OSC 52 clipboard writes to the host terminal so a nested tool's "copy" reaches the system clipboard instead of being swallowed (#12).
- Forward child desktop notifications (OSC 9 and OSC 777) to the host terminal (#13).
- Forward child taskbar progress reports (OSC 9;4) to the host terminal (#14).
- Answer child color queries (OSC 4 palette, OSC 10 foreground, OSC 11 background, OSC 12 cursor) locally so nested tools detect colors without their replies leaking into Navix key input, with a hardened query matcher that handles both BEL and ST terminators (#15).
- Forward the child terminal bell (BEL) to the host terminal so a nested tool's bell rings the host (audible or visual, per its configuration) (#20).

### Tests

- Added coverage for OSC 52 clipboard passthrough, OSC 9/777 notification and OSC 9;4 progress passthrough, OSC 4/10/11/12 color query replies (BEL and ST termination, chunk reassembly, query vs set), and bell forwarding.

## [0.3.3] - 2026-06-04

Embedded terminal graphics: nested TUI tools (document readers, image viewers,
and other terminal programs) now render inline images and behave as a 1:1
passthrough of the host terminal inside both the Shell and Preview panes.

### Added

- Embedded graphics-protocol support for nested tools in the Shell and Preview panes: Kitty graphics (primary on Linux/Ghostty) and iTerm2 inline images (primary on macOS), with each tool's own halfblock output left intact as a fallback (#3).
- `GraphicsStreamParser` that separates graphics escape sequences (Kitty/iTerm2) from text output so images and the text grid are handled independently (#3).
- Off-thread `GraphicsWriter` that performs blocking host graphics writes on a dedicated thread, keeping the main loop responsive (scrolling and `Ctrl-C` stay live) while multi-MB images transfer (#6).
- Host cell pixel-size detection advertised to child PTYs (winsize pixel fields and CSI 14t/16t replies) so tools reserve the correct number of cells and images render at the intended size (#5).
- Terminal capability query replies (graphics probes, cell size, background color) answered on behalf of nested tools so they detect support identically to running outside Navix (#3).

### Changed

- Kitty images use true verbatim passthrough (Unicode placeholder method): images scroll and clip at the pane edge exactly as in a bare terminal, instead of being re-decoded or resized (#3).
- iTerm2's positioned (cursor-placed) image reflush is routed through the same off-thread `GraphicsWriter` as Kitty, so large inline images no longer block the main loop (#6).
- Composited rendering now skips redraws when neither the visible screen grid nor UI state changed (dirty-skip), and repaints defer while a graphics write is in flight to avoid corrupting an in-flight image (#6).
- Input events are batch-drained per tick so heavy nested-tool output and fast scrolling stay responsive and exit is not delayed behind a backlog (#8).
- Identical back-to-back Kitty image transfers are de-duplicated to prevent re-transmit floods from saturating the host during scrolling (#6).
- `bin/navix` now builds and runs in release mode, and the graphics stream parser uses bulk copying, cutting large-image parse time from seconds to tens of milliseconds (#8).
- Navix stays tool-agnostic: nested commands run verbatim with no tool-specific command rewriting or injected environment, so any TUI of the user's choice behaves as it would outside Navix (#7).

### Fixed

- Eliminated the image/text "tear" artifact caused by `ansi-to-tui` dropping CSI Cursor-Forward sequences, by expanding CUF into spaces in the Shell and Preview render paths (#4).
- Inline images no longer render at the wrong size due to a hardcoded advertised cell size (#5).
- Removed a freeze and input lockup that occurred when scrolling onto large images, caused by a graphics re-transmit backlog and synchronous host writes (#6).

### Docs

- Added the v0.3.2 navigation demo image to `README.md`.

### Tests

- Added coverage for the graphics stream parser, CUF expansion (including an `insta` snapshot), graphics-writer deferral/routing decisions, input batch-draining, Kitty transfer de-duplication, and cell-pixel detection.
- Introduced `insta` snapshot testing as a dev dependency; the suite now includes 251 passing tests.

## [0.3.2] - 2026-06-01

### Added

- Added v0.3.2 demo assets under `docs/demo-v0.3.2` and refreshed the demo generation script defaults.
- Added an issue-linked branch naming convention in `CONTRIBUTING.md` for parent and sub-branch workflows.

### Changed

- Bumped package version to `0.3.2` and updated README demo references to the v0.3.2 asset set.
- Refined issue-reporting/docs guidance (`README.md`, `DEVELOPING.md`, and issue template) for clearer diagnostics and runtime context.

### Fixed

- Preview command overlay now forwards keys to running read-session commands even when they are not detected as alt-screen, restoring navigation for nested terminal tools.
- Shell PTY key forwarding now preserves modified Enter sequences (for example `Shift+Enter`) so multiline prompts work in embedded CLI tools.

### Tests

- Added regression tests for preview overlay key passthrough behavior and modified Enter forwarding.
- Expanded runtime option and terminal key coverage around the new release behavior.

## [0.3.1] - 2026-05-28

### Added

- Added `navix --version` (and `-V`) so release binaries can report their exact package version without launching the TUI.

### Fixed

- Shell startup now prefers the active parent shell command (when it is a supported shell like `zsh`/`bash`) before falling back to `SHELL`, preventing stale login `SHELL` values from launching the wrong shell inside Navix.

### Tests

- Added targeted shell-resolution regression tests covering explicit override priority, parent-shell preference, non-shell parent fallback, and `/bin/sh` fallback behavior.

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
