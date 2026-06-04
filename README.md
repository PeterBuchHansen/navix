# navix

Keyboard-first Rust TUI for navigating files, previewing commands, and keeping a live shell in the same workspace.

![Navix navigation panel](docs/demo-v0.3.2/navix-navigation.png)

## Why Navix

- **Three-pane workflow**: `[1] Navigation`, `[2] Preview`, `[0] Shell`.
- **Fast directory movement**: move with keys, filter live, and jump by path.
- **Preview-first commands**: file-aware `read`, `write`, and `exec` shortcuts.
- **Shell continuity**: embedded PTY keeps context while you browse.
- **Keyboard-native UX**: no mouse dependency for core flows.

## Quick Start

```bash
# run from repository root
./bin/navix

# or via Cargo
cargo run --manifest-path Cargo.toml
```

## Startup Examples

```bash
# open with Preview focused
./bin/navix --preview

# start in a specific path
./bin/navix --path ~/repos/navix/docs --navigation

# disable startup mouse capture
./bin/navix --no-mouse-capture
```

## Demo v0.3.2

![Navix demo](docs/demo-v0.3.2/navix-demo.gif)
![Navix navigation panel](docs/demo-v0.3.2/navix-navigation.png)
![Navix preview panel](docs/demo-v0.3.2/navix-preview.png)
![Navix shell panel](docs/demo-v0.3.2/navix-shell.png)
![Navix help overlay](docs/demo-v0.3.2/navix-help.png)
![Navix config overlay](docs/demo-v0.3.2/navix-config.png)

## Core Keys

- `Esc+0`, `Esc+1`, `Esc+2`: focus Shell / Navigation / Preview
- `Esc+c`: open config editor
- `Esc+f`: toggle fullish mode
- `Ctrl+d`: exit

For full behavior and exact panel semantics, see `docs/CURRENT_BEHAVIOR.md`.

## Reporting Issues

When reporting a bug, include:

- OS + terminal emulator
- navix version (`./navix --version`)
- exact repro steps
- expected vs actual behavior

If the bug is not easy to reproduce, logs are required. If possible, always include logs.

From release artifacts (same folder as the `navix` binary):

```bash
# collect both debug logs into /tmp while reproducing
./navix --debug-log=all
```

After reproducing, attach these files to the GitHub issue:

- `/tmp/navix-key-debug.log`
- `/tmp/navix-copy-key-debug.log`

If you are running from source instead of release artifacts, see `DEVELOPING.md` for debug log modes and examples.

## Documentation

- Behavior reference: `docs/CURRENT_BEHAVIOR.md`
- Developer/runtime guide: `DEVELOPING.md`
- Architecture notes: `docs/ARCHITECTURE.md`
- Engineering conventions: `docs/PROJECT_CONVENTIONS.md`
- Contribution guide: `CONTRIBUTING.md`
- Security policy: `SECURITY.md`
- Release process: `docs/RELEASE_PROCESS.md`
- Release checklist: `docs/OPEN_SOURCE_RELEASE.md`

## Platform

- Primary support target: Linux terminal environments.
- macOS is supported via release binaries for Intel and Apple Silicon.
- Native Windows is not supported.
- Windows users can run Navix through WSL using the Linux release binary (`x86_64-unknown-linux-gnu`).
- Current runtime behavior relies on Unix/Linux shell and path semantics.

## License

GPL-3.0-or-later. See `LICENSE`.
