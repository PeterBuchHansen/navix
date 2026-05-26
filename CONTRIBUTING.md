# Contributing to navix

## Development setup

- Rust toolchain: stable (edition 2024 project)
- Run locally:
  - `cargo run --manifest-path Cargo.toml`
  - or `bin/navix`

## Required checks before PR

- Format check: `cargo fmt --all -- --check`
- Lint check: `cargo clippy --all-targets --all-features -- -D warnings`
- Test suite: `cargo test --locked --manifest-path Cargo.toml`

## PR scope rules

- Keep changes narrow and behavior-focused.
- Include tests for all behavior changes.
- Avoid mixing refactor + feature + docs in one PR unless tightly coupled.

## Commit guidance

- Use concise imperative commit titles.
- Explain why in body when behavior changes are non-obvious.

## Areas needing extra care

- Input routing and Esc-prefix state machine.
- PTY IO and terminal rendering paths.
- Preview overlay mode transitions.

## Naming and structure conventions

- Follow Rust naming defaults:
  - files/modules/functions: `snake_case`
  - types/traits/enums: `PascalCase`
  - constants: `SCREAMING_SNAKE_CASE`
- Prefer domain-specific module names over generic utility buckets.
- Keep one module focused on one responsibility.

Reference: `docs/PROJECT_CONVENTIONS.md`.

## Refactor safety rule

- Separate pure refactor from behavior changes when feasible.
- If behavior must change in same PR, document why and include focused tests.

## Reporting issues

When reporting bugs, include:

- OS and terminal emulator
- exact repro steps
- expected vs actual behavior
- keybindings used
