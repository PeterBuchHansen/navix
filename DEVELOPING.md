# Developing Navix

Developer-focused runtime notes, environment variables, and logging workflows.

## Local Run

```bash
# from repository root
./bin/navix
```

```bash
# with explicit startup options
./bin/navix --path ~/repos/navix --preview
```

```bash
# show CLI usage
./bin/navix --help
```

## Runtime Flags

- `--no-mouse-capture`: disable startup mouse capture.
- `--path <PATH>`: startup path (directory or file).
- `--navigation`: startup focus = Navigation (default).
- `--preview`: startup focus = Preview.
- `--shell`: startup focus = Shell.
- `-h`, `--help`: print usage.

`--navigation`, `--preview`, and `--shell` are mutually exclusive.

## Environment Variables

### Debug Logging

- `NAVIX_KEY_DEBUG` (`1|true|yes`): enable full key-event debug logging.
- `NAVIX_KEY_DEBUG_FILE`: output path for full key debug log.
  - Default: `/tmp/navix-key-debug.log`
- `NAVIX_COPY_KEY_DEBUG` (`1|true|yes`): enable copy-shortcut-only key logging.
- `NAVIX_COPY_KEY_DEBUG_FILE`: output path for copy debug log.
  - Default: `/tmp/navix-copy-key-debug.log`

### Shell / Editor Runtime

- `EDITOR`: editor command for write actions and config field defaults.
- `NAVIX_LAUNCH_SHELL`: force shell program used by embedded PTY.
- `SHELL`: fallback shell when `NAVIX_LAUNCH_SHELL` is unset.
- `PROMPT_COMMAND`: read/extended for bash history sync behavior.
- `HISTFILE`, `HISTSIZE`, `HISTFILESIZE`, `SAVEHIST`: shell history behavior inputs.
- `TERM_PROGRAM`, `OSTYPE`: terminal/platform hints used for copy shortcut behavior.
  - `Ctrl+Shift+C` may be reported by some terminals as `Ctrl + 'C'` (uppercase) without an explicit Shift modifier.

### Theme / Paths

- `LS_COLORS`: color source for navigation icon/file styling.
- `XDG_CONFIG_HOME`, `HOME`: config file location resolution.
- `XDG_STATE_HOME`, `HOME`: preview jump history location resolution.
- `XDG_DATA_HOME`, `HOME`: shell history default path resolution.

## Logging to `/tmp`

### Full key-event logging

```bash
NAVIX_KEY_DEBUG=1 \
NAVIX_KEY_DEBUG_FILE=/tmp/navix-key-debug.log \
./bin/navix --preview
```

### Copy shortcut logging only

```bash
NAVIX_COPY_KEY_DEBUG=1 \
NAVIX_COPY_KEY_DEBUG_FILE=/tmp/navix-copy-key-debug.log \
./bin/navix --shell
```

### Both logs enabled

```bash
NAVIX_KEY_DEBUG=1 \
NAVIX_KEY_DEBUG_FILE=/tmp/navix-key-debug.log \
NAVIX_COPY_KEY_DEBUG=1 \
NAVIX_COPY_KEY_DEBUG_FILE=/tmp/navix-copy-key-debug.log \
./bin/navix --path ~/repos/navix
```

### Inspect logs while running

```bash
tail -f /tmp/navix-key-debug.log /tmp/navix-copy-key-debug.log
```

## Test in Docker

```bash
docker run --rm \
  --user "$(id -u):$(id -g)" \
  -e CARGO_TARGET_DIR=/work/target \
  -v "$(pwd):/work" \
  -w /work \
  rust:1 \
  bash -lc 'export PATH=/usr/local/cargo/bin:$PATH; cargo test --locked --manifest-path Cargo.toml'
```

## Related Docs

- Behavioral source of truth: `docs/CURRENT_BEHAVIOR.md`
- Architecture context: `docs/ARCHITECTURE.md`
- Coding standards: `docs/PROJECT_CONVENTIONS.md`
