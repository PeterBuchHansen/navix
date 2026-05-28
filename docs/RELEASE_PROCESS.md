# Release Process

This document defines the official flow for publishing Navix releases on GitHub.

## Release Policy

Official release binaries are published for:

- Linux: `x86_64-unknown-linux-gnu`
- macOS (Intel): `x86_64-apple-darwin`
- macOS (Apple Silicon): `aarch64-apple-darwin`

Windows native binaries are not published.
Windows users can run Navix through WSL by using the Linux release binary (`x86_64-unknown-linux-gnu`).

## Trigger Model

Releases are tag-driven.

- Stable releases use `vX.Y.Z`.
- Release candidates use `vX.Y.Z-rc.N`.

Pushing either format triggers the release workflow.

Example:

```bash
# stable
git tag -a v0.3.1 -m "navix v0.3.1"
git push origin v0.3.1

# release candidate
git tag -a v0.3.1-rc.1 -m "navix v0.3.1-rc.1"
git push origin v0.3.1-rc.1
```

## Validation Rules

Before publishing artifacts, the release workflow validates:

- tag format is exactly `vX.Y.Z`
- or release-candidate format `vX.Y.Z-rc.N`
- `Cargo.toml` package version matches the tag version
- `CHANGELOG.md` contains a matching section header:
  - stable: `## [X.Y.Z] - YYYY-MM-DD`
  - RC: `## [X.Y.Z-rc.N] - YYYY-MM-DD`

The matching changelog section is used as the GitHub Release notes body.
RC tags are published as GitHub pre-releases automatically.

## Quality Gate

Release workflow quality checks run before build/publish:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D clippy::correctness -D clippy::suspicious`
- `cargo test --locked --manifest-path Cargo.toml`
- MSRV test on Rust `1.88.0`

## Artifacts

For tag `vX.Y.Z`, release artifacts are:

- `navix-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz`
- `navix-vX.Y.Z-x86_64-apple-darwin.tar.gz`
- `navix-vX.Y.Z-aarch64-apple-darwin.tar.gz`
- `SHA256SUMS.txt`

Each archive contains:

- `navix` binary at archive root (extractable with `tar -xzf <archive> navix`)
- `README.md`
- `LICENSE`

## Maintainer Procedure

1. Update `Cargo.toml` version.
2. Add/update the matching `CHANGELOG.md` section.
3. Ensure CI on `main` is green.
4. Create and push an annotated release tag.
5. Monitor release workflow in GitHub Actions.
6. Verify artifacts/checksums on the GitHub Release page.

## Post-Release Verification

- All expected platform archives are present.
- `SHA256SUMS.txt` is present and matches uploaded files.
- Release notes match the changelog section for the tagged version.
- Downloaded binary runs (`./navix --help`) on at least one Linux and one macOS machine.
