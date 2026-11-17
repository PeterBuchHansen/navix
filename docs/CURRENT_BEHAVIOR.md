# Navix Current Behavior

This document describes how the current `src/main.rs` implementation behaves.

## Pane model

- `Navigation` is the primary control pane.
- `Preview` shows:
  - directory tree text when directory preview is enabled, or
  - file command shortcuts for selected files.
- `Shell` is an embedded PTY shell panel.

## Focus switching

### Keyboard

- `Esc+0`: focus Shell
- `Esc+1`: focus Navigation
- `Esc+2`: focus Preview
- `Esc+c`: open Config overlay
- `Esc+f`: toggle fullish mode

### Mouse

- Left click on a panel focuses that panel.
- Preview click focuses `Preview`.
- Mouse events for a full-screen child (Shell pane on the alt screen, interactive Preview overlay)
  are forwarded only the way the child asked for them, using the child's mouse mode and encoding
  as tracked by the embedded emulator:
  - no mouse reporting requested: wheel notches become `↑`/`↓` keys (3 per notch, in application
    cursor form when the child enabled it), so programs like `less` scroll; other mouse events are
    dropped
  - mouse reporting requested: only the event kinds the mode covers are sent (press / release /
    drag / motion), in the requested encoding (X10, UTF-8 or SGR)

## Navigation behavior

- `↑/↓`: move selection
- `PgUp/PgDown`: move selection by viewport page
- `Home/End`: jump to first/last entry
- `Ctrl+↑/Ctrl+↓`: scroll viewport without changing selected row
- `←`: go out to parent directory (`cd ../`)
- `→` on ``: `cd` into selected directory
- `→` on ``: run read shortcut (same action as `Esc+r`)
- `Enter` on ``: `cd` into selected directory
- `Enter` on ``: run pending file shortcut, defaulting to read (`r`)

Notes:

- File selection does not auto-render file contents in preview.
- For files, preview panel shows extension command shortcuts.

## Preview command shortcuts

When a file (``) is selected, Navix always renders all three file shortcuts in Preview:

- `Esc+r`
- `Esc+w`
- `Esc+x`

Each shortcut is enabled/disabled from:

- extension command rule (`read_cmd`, `write_cmd`, `exec_cmd`)
- effective file permissions (read/write/exec)

If a shortcut is enabled:

- `Esc+r`: run read command in preview overlay
- `Esc+w`: run write command in preview overlay
- `Esc+x`: prefill shell with exec command

If a shortcut is disabled, it is shown dimmed and triggering it does nothing.

Shell prefill behavior clears the current shell input line first (`Ctrl+a`, `Ctrl+k`) and then inserts command text.

## Command templates

Extension command templates (`read_cmd`, `write_cmd`, `exec_cmd`) support two placeholders:

- `$EDITOR`: replaced with the editor program verbatim
- `{file}`: replaced with the selected file name, escaped for the shell quoting context it appears in

Navix tracks single and double quotes in the template, including `\"` inside double quotes, so `{file}` is
safe and correct wherever it is placed. Names without special characters are inserted unchanged.

| Template position | Encoding | `my cv.pdf` becomes |
|---|---|---|
| unquoted (`bat {file}`) | backslash before shell-special characters | `bat my\ cv.pdf` |
| inside `"…"` | backslash before `\ $ ` "` only | `"my cv.pdf"` |
| inside `'…'` | `'` written as `'\''` | `'my cv.pdf'` |

Unquoted names that are empty or contain control characters (e.g. a newline) fall back to single quotes,
because a backslash cannot protect a newline.

Tools that parse their own argument string need their own quotes around `{file}`. For example,
smbclient splits its `-c` command on spaces:

```toml
exec_cmd = 'smbclient -U user //printserver/queue -c "print \"{file}\""'
```

Limitations:

- A command parsed by a second shell (e.g. `ssh host "cat {file}"`) needs its own quoting for that shell,
  such as `ssh host "cat '{file}'"`.
- `Esc+x` prefills the interactive shell. A `!` inside a double-quoted `{file}` may trigger history
  expansion in interactive bash. Unquoted `!` is escaped.
- `{file_raw}` is a legacy alias of `{file}`.

## Preview path jump input

When `Preview` is focused (and no preview command overlay is active), path input is always active:

- Helper text shown: `Select  or  in Navigation panel to Preview it. Or type...`
- Placeholder shown: `...reletive/abselut path of  or  to jump.`
- If history has items, placeholder suffix is appended: `Or history ↑/↓`
- Empty input: show reverse history list (latest first)
- Typed input: replace history with inline compact multi-column completion candidates

Controls:

- Type characters directly into the path input
- `↑/↓`: cycle history or completion candidates
- `Tab` / `Shift+Tab`: cycle completion candidates
- `Enter`: completion-first jump action
  - selected `` completion: accept completion first (keeps trailing `/`), jump on next `Enter`
  - selected `` completion: jump directly
  - plain typed directory path (``): `cd` to directory in Navigation and focus Navigation
  - plain typed file path (``): `cd` to parent and preselect file row in Navigation, then focus Navigation
- `Shift+Enter`: same jump action as `Enter`, but keep focus in Preview on success

## Fallback extension rule

When no extension rule matches, Navix uses fallback:

- `read_cmd = "bat {file}"`
- `write_cmd = "$EDITOR {file}"`
- `exec_cmd = "None"` (disabled)

## Preview overlay behavior

`Esc+r` and `Esc+w` launch a preview command overlay session.

- Starts in static fullscreen presentation.
- If child session enters alt screen, overlay presentation upgrades to interactive mode.

Static mode:

- `Esc` closes overlay and returns focus to `Navigation`.
- `↑/↓/PgUp/PgDown` scroll overlay output.

Interactive mode:

- Keys are forwarded to child TUI.
- Overlay auto-closes when child process exits.
- Focus returns to `Navigation`.

## Fullish behavior

- Shell fullish:
  - toggled by `Esc+f` when active pane is `Shell`
  - also enabled automatically while shell is in alt-screen mode
  - alt-screen mode is read from the embedded terminal emulator, so it follows every way a child
    switches screens, including a full reset (`ESC c`)
  - automatic recovery: when the shell takes the terminal back from a foreground child (it
    exited, was killed, crashed, or its SSH connection dropped), Navix resets what the child left
    on in the emulator: alt screen, mouse reporting and a hidden cursor. Bracketed paste and
    cursor-key/keypad modes are left alone because shells set them at the prompt
  - not covered automatically: a remote program that dies while `ssh` itself keeps running (the
    foreground is still `ssh`). Run `reset` on a clean prompt line (clear it with `Ctrl+C` first)
- Navigation fullish:
  - toggled by `Esc+f` when active pane is `Navigation` or `Preview`
  - narrows preview panel for navigation-first workflow

## Config overlay behavior

Open/close:

- Open with `Esc+c`
- Close with `Esc`

Primary controls:

- `Ctrl+s`: save
- `Ctrl+r`: reload from disk
- `Ctrl+d`: discard unsaved changes
- `Ctrl+n`: new extension rule
- `Ctrl+Delete` / `Ctrl+Backspace` / `Ctrl+h`: delete selected rule
- `↑/↓/←/→`: navigate rows/fields
- `Enter`: start or commit field editing

## Config path and defaults

Config file path:

- `$XDG_CONFIG_HOME/navix/config.toml` if `XDG_CONFIG_HOME` is set
- otherwise `~/.config/navix/config.toml`

Default extension rules:

- `md`: read `bat {file}`, write `$EDITOR {file}`, exec `mdterm {file}`
- `json`: read `jq . {file}`, write `$EDITOR {file}`, exec `fx {file}`
- `sh`: read `bat {file}`, write `$EDITOR {file}`, exec `bash {file}`
