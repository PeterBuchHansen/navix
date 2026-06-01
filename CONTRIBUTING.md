# Contributing to navix

## Development setup

- Rust toolchain: stable (edition 2024 project)
- Run locally:
  - `cargo run --manifest-path Cargo.toml`
  - or `bin/navix`

## Required checks before PR

- Format check: `cargo fmt --all -- --check`
- Lint check: `cargo clippy --all-targets --all-features -- -D clippy::correctness -D clippy::suspicious`
- Test suite: `cargo test --locked --manifest-path Cargo.toml`

## Branch naming rule

- For work targeting `main`, create issue-linked branches using:
  - `main_<issue-number>_<first-name>_<issue-description-short>`
- Define a parent-branch short prefix as:
  - `main_<issue-number>_<first-name>`
- For sub-branches off that parent prefix, use:
  - `main_<issue-number>_<first-name>_<sub-n>_<first-name>_<sub-task-description>`
- Keep description segments lowercase and concise, using underscores instead of spaces.
- Examples:
  - Parent branch: `main_1_peter_post_release_alignment`
  - Sub-branch: `main_1_peter_2_peter_fix_runtime_tests`

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
- navix version (`./navix --version`)

If the bug is not easy to reproduce, logs are required. If possible, always include logs.

Log collection:

- Release artifact users: run `./navix --debug-log=all`, reproduce the issue, then attach:
  - `/tmp/navix-key-debug.log`
  - `/tmp/navix-copy-key-debug.log`
- Source users: see `DEVELOPING.md` for `--debug-log` modes and environment-based logging.
