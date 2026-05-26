// navix - keyboard-first TUI for file navigation, preview overlays, and embedded shell.
// Copyright (C) 2026 Peter Buch Hansen
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use super::*;

pub(crate) fn ensure_editor_program() -> io::Result<String> {
    if let Ok(existing) = std::env::var("EDITOR") {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    let mut stdout = io::stdout();
    loop {
        write!(stdout, "navix: $EDITOR not set. Enter editor command: ")?;
        stdout.flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let editor = input.trim();
        if editor.is_empty() {
            writeln!(stdout, "navix: editor cannot be empty.")?;
            continue;
        }
        unsafe {
            std::env::set_var("EDITOR", editor);
        }
        return Ok(editor.to_string());
    }
}

pub(crate) fn open_key_debug_log() -> Option<File> {
    let enabled = std::env::var("NAVIX_KEY_DEBUG")
        .ok()
        .map(|value| {
            let lowered = value.trim().to_ascii_lowercase();
            lowered == "1" || lowered == "true" || lowered == "yes"
        })
        .unwrap_or(false);
    if !enabled {
        return None;
    }
    let path = std::env::var("NAVIX_KEY_DEBUG_FILE")
        .ok()
        .filter(|raw| !raw.trim().is_empty())
        .unwrap_or_else(|| "/tmp/navix-key-debug.log".to_string());
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()?;
    let _ = writeln!(file, "---- navix key debug session ----");
    let _ = file.flush();
    Some(file)
}

pub(crate) struct TerminalGuard {
    pub(crate) terminal: Terminal<CrosstermBackend<io::Stdout>>,
    mouse_capture_enabled: bool,
}

impl TerminalGuard {
    pub(crate) fn enter(enable_mouse_capture: bool) -> io::Result<Self> {
        let mut stdout = io::stdout();
        enable_raw_mode()?;
        execute!(stdout, EnterAlternateScreen)?;
        let _ = execute!(
            stdout,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                    | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
            )
        );
        if enable_mouse_capture {
            let _ = execute!(stdout, EnableMouseCapture);
        }
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self {
            terminal,
            mouse_capture_enabled: enable_mouse_capture,
        })
    }

    pub(crate) fn set_mouse_capture(&mut self, enabled: bool) -> io::Result<()> {
        if self.mouse_capture_enabled == enabled {
            return Ok(());
        }
        if enabled {
            execute!(self.terminal.backend_mut(), EnableMouseCapture)?;
        } else {
            execute!(self.terminal.backend_mut(), DisableMouseCapture)?;
        }
        self.mouse_capture_enabled = enabled;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            self.terminal.backend_mut(),
            Show,
            DisableMouseCapture,
            PopKeyboardEnhancementFlags,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
    }
}

pub(crate) fn to_io<E: std::fmt::Display>(err: E) -> io::Error {
    io::Error::other(err.to_string())
}
