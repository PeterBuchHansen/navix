# Project Conventions (inspired by ratatui, openapi-tui, mdterm)

This document captures conventions derived from:

- https://github.com/ratatui/ratatui
- https://github.com/zaghaghi/openapi-tui
- https://github.com/bahdotsh/mdterm

Goal: keep Navix maintainable for open-source contributors while avoiding behavior regressions.

## What we adopt

## 1) Project structure

Inspired by `openapi-tui` and `mdterm`, organize source by domain, not by technical layer alone.

Current direction in Navix:

- `src/main.rs` remains runtime bootstrap and top-level integration.
- Extract reusable helpers into focused modules:
  - `src/terminal_keys.rs` (PTY key encoding)
  - `src/panel_layout.rs` (layout split helpers)

Further split should follow this rule:

- one module = one clear responsibility
- avoid utility dumping grounds
- move code only with tests preserved

## 2) Function and file naming

Follow Rust defaults (also visible in all three reference repos):

- files/modules: `snake_case` (e.g. `terminal_keys.rs`)
- functions: `snake_case`
- types/enums/traits: `PascalCase`
- constants: `SCREAMING_SNAKE_CASE`
- avoid abbreviations unless domain-standard (`pty`, `cwd`, `msrv`)

Naming should be intent-first:

- prefer `panel_click_focus_target` over generic names like `handle_click`.
- prefer `split_navigation_preview_cols` over `layout_cols`.

## 3) Formatting and linting

From `ratatui` and `openapi-tui`:

- enforce rustfmt and clippy in CI
- keep contributor commands simple and consistent

Navix policy:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --locked --manifest-path Cargo.toml`

Note: `ratatui` uses nightly-only rustfmt options. Navix intentionally keeps stable rustfmt options only.

## 4) CI and release hygiene

From `ratatui` CI maturity and smaller-project clarity in `mdterm`:

- run checks on PR + main
- keep workflow steps explicit (`fmt`, `clippy`, `test`, `msrv`)
- keep lockfile reproducibility with `--locked`

## 5) PR scope and review safety

From `ratatui` contribution guidance:

- separate refactor from behavior changes when possible
- keep PRs focused and reviewable
- include tests for behavior-affecting code

Navix adds one hard rule:

- terminal input routing, PTY output handling, and overlay state transitions require tests in same PR.

## 6) Documentation expectations

From all reference repos:

- keep top-level docs practical (how to run, what to change, how to verify)
- keep architecture notes close to code reality

Navix docs map:

- `README.md`: usage and key controls
- `docs/CURRENT_BEHAVIOR.md`: runtime behavior source of truth
- `docs/ARCHITECTURE.md`: component boundaries
- this file: engineering conventions
