# Open Source Release Checklist

## Ready now

- CI checks defined (`fmt`, `clippy`, `test`).
- MSRV CI check defined (`1.85.0`).
- Contributor docs present.
- Security reporting policy present.
- Changelog started.
- Architecture and behavior docs present.
- License file present.
- Issue and PR templates present.

## Required before public release

- Keep maintainer contact current in `SECURITY.md` and `CODE_OF_CONDUCT.md`.
- Keep `repository` metadata aligned if repo URL changes.

## Quality gate

Before each release:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --manifest-path Cargo.toml`
