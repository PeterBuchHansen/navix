# Changelog

All notable changes to this project are documented here.

## [Unreleased]

### Added

- Esc-prefixed pane shortcuts and preview command shortcuts.
- Mouse click panel focus routing with preview bounce policy.
- Smart preview overlay behavior for static vs interactive sessions.

### Changed

- Preview command templating now shell-escapes `{file}` by default.
- PTY output pipeline now uses bounded queue to avoid unbounded memory growth.
- Key forwarding expanded for additional special keys and modifier paths.
- Fullish layout behavior unified for Navigation and Preview active states.

### Docs

- Added current behavior reference and contributor/security docs.
