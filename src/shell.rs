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
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};
use vt100::Parser;

const MAX_OUTPUT_QUEUE_CHUNKS: usize = 2048;

pub(crate) struct ShellPane {
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    rx: Receiver<Vec<u8>>,
    parser: Parser,
    scroll_offset: usize,
    viewport_rows: usize,
    has_overflow: bool,
    pty_rows: u16,
    pty_cols: u16,
    alt_screen_active: bool,
    ansi_tail: Vec<u8>,
    last_known_cwd: PathBuf,
    child: Box<dyn portable_pty::Child + Send>,
}

pub(crate) struct ShellMetrics {
    pub(crate) shown_start: usize,
    pub(crate) shown_end: usize,
    pub(crate) total: usize,
    pub(crate) has_overflow: bool,
}

impl ShellPane {
    pub(crate) fn spawn() -> io::Result<Self> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let navix_launch_shell = std::env::var("NAVIX_LAUNCH_SHELL").ok();
        let parent_process = parent_process_command_name();
        let shell_env = std::env::var("SHELL").ok();
        let shell_path = resolve_launch_shell_path_with(
            navix_launch_shell.as_deref(),
            parent_process.as_deref(),
            shell_env.as_deref(),
            resolve_command_from_path,
        );
        let shell_program = shell_program_name(&shell_path);
        let mut command = CommandBuilder::new(shell_path.clone());
        if shell_program == "zsh" {
            command.arg("-o");
            command.arg("append_history");
            command.arg("-o");
            command.arg("inc_append_history");
            command.arg("-o");
            command.arg("share_history");
            // Ensure Ctrl+r is bound to reverse search inside Navix shell pane.
            command.arg("-o");
            command.arg("emacs");
        }
        command.arg("-i");
        command.cwd(cwd.clone());
        apply_process_environment(&mut command);
        if shell_program == "bash" {
            let prompt_command =
                bash_history_sync_prompt_command(std::env::var("PROMPT_COMMAND").ok().as_deref());
            command.env("PROMPT_COMMAND", prompt_command);
        }
        apply_history_defaults(&mut command, &shell_program, &shell_path);
        Self::spawn_from_command_builder(command, cwd)
    }

    pub(crate) fn spawn_command(command_line: &str, cwd: &Path) -> io::Result<Self> {
        let mut command = CommandBuilder::new("/bin/sh");
        command.arg("-lc");
        command.arg(command_line);
        command.cwd(cwd.to_path_buf());
        apply_process_environment(&mut command);
        Self::spawn_from_command_builder(command, cwd.to_path_buf())
    }

    fn spawn_from_command_builder(command: CommandBuilder, cwd: PathBuf) -> io::Result<Self> {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: 40,
                cols: 160,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(to_io)?;
        let child = pair.slave.spawn_command(command).map_err(to_io)?;
        drop(pair.slave);

        let master = pair.master;
        let mut reader = master.try_clone_reader().map_err(to_io)?;
        let writer = master.take_writer().map_err(to_io)?;

        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(MAX_OUTPUT_QUEUE_CHUNKS);
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = tx.send(buf[..n].to_vec());
                    },
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            master,
            writer,
            rx,
            parser: Parser::new(40, 160, shell_scrollback_limit()),
            scroll_offset: 0,
            viewport_rows: 1,
            has_overflow: false,
            pty_rows: 40,
            pty_cols: 160,
            alt_screen_active: false,
            ansi_tail: Vec::new(),
            last_known_cwd: cwd,
            child,
        })
    }

    pub(crate) fn poll_output(&mut self) -> OutputDrain {
        let mut processed_chunks = 0usize;
        let mut processed_bytes = 0usize;
        let mut hit_limit = false;
        loop {
            if processed_chunks >= MAX_OUTPUT_CHUNKS_PER_TICK
                || processed_bytes >= MAX_OUTPUT_BYTES_PER_TICK
            {
                hit_limit = true;
                break;
            }
            match self.rx.try_recv() {
                Ok(chunk) => {
                    processed_bytes = processed_bytes.saturating_add(chunk.len());
                    processed_chunks = processed_chunks.saturating_add(1);
                    self.track_alt_screen_sequences(&chunk);
                    self.parser.process(&chunk);
                },
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        OutputDrain {
            processed_chunks,
            hit_limit,
        }
    }

    pub(crate) fn render_text_and_metrics(
        &mut self,
        viewport_rows: u16,
        cols: u16,
    ) -> (Text<'static>, ShellMetrics) {
        self.resize(viewport_rows.max(1), cols.max(1));
        let viewport = viewport_rows.max(1) as usize;
        self.viewport_rows = viewport;
        self.parser.screen_mut().set_scrollback(usize::MAX);
        let max_scrollback = self.parser.screen().scrollback();
        self.scroll_offset = self.scroll_offset.min(max_scrollback);
        self.parser.screen_mut().set_scrollback(self.scroll_offset);
        self.scroll_offset = self.parser.screen().scrollback();
        self.has_overflow = max_scrollback > 0;

        let mut bytes: Vec<u8> = Vec::new();
        for (idx, row) in self
            .parser
            .screen()
            .rows_formatted(0, cols)
            .into_iter()
            .enumerate()
        {
            if idx > 0 {
                bytes.extend_from_slice(b"\x1b[0m");
                bytes.push(b'\n');
            }
            bytes.extend_from_slice(&row);
        }

        let text = bytes
            .clone()
            .into_text()
            .unwrap_or_else(|_| Text::raw(String::from_utf8_lossy(&bytes).into_owned()));
        let total = max_scrollback.saturating_add(viewport).max(1);
        let (shown_start, shown_end) = visible_range(total, viewport, self.scroll_offset);
        let metrics = ShellMetrics {
            shown_start,
            shown_end,
            total,
            has_overflow: self.has_overflow,
        };
        (text, metrics)
    }

    pub(crate) fn resize(&mut self, rows: u16, cols: u16) {
        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        let _ = self.master.resize(size);
        self.parser.screen_mut().set_size(rows, cols);
        self.pty_rows = rows;
        self.pty_cols = cols;
    }

    pub(crate) fn send_key(&mut self, key: crossterm::event::KeyEvent) -> io::Result<()> {
        let bytes = terminal_key_bytes(key.code, key.modifiers);
        if !bytes.is_empty() {
            self.send_raw(&bytes)?;
        }
        Ok(())
    }

    pub(crate) fn send_mouse(&mut self, mouse: crossterm::event::MouseEvent) -> io::Result<()> {
        let bytes = terminal_mouse_bytes(mouse);
        if !bytes.is_empty() {
            self.send_raw(&bytes)?;
        }
        Ok(())
    }

    pub(crate) fn send_raw(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    pub(crate) fn cd_to(&mut self, path: &Path, restore_pending_input: bool) -> io::Result<()> {
        let bytes = cd_to_bytes(path, restore_pending_input);
        self.send_raw(&bytes)?;
        self.last_known_cwd = path.to_path_buf();
        Ok(())
    }

    pub(crate) fn scroll_up(&mut self, rows: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(rows);
    }

    pub(crate) fn scroll_down(&mut self, rows: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(rows);
    }

    pub(crate) fn jump_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    pub(crate) fn page_rows(&self) -> usize {
        self.viewport_rows.max(1)
    }

    pub(crate) fn in_alt_screen(&self) -> bool {
        self.alt_screen_active
    }

    pub(crate) fn is_running(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(_)) => false,
            Ok(None) => true,
            Err(_) => false,
        }
    }

    pub(crate) fn terminate(&mut self) {
        let _ = self.child.kill();
    }

    pub(crate) fn request_shutdown(&mut self) {
        if !self.is_running() {
            return;
        }
        // Interrupt running foreground command first so shell can process exit.
        let _ = self.send_raw(&[0x03]);
        let _ = self.send_raw(b"\r");
        // Exit via EOF so navix-internal shutdown commands are not stored in history.
        let _ = self.send_raw(&[0x04]);
        let deadline = Instant::now() + Duration::from_millis(1200);
        while Instant::now() < deadline {
            if !self.is_running() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.send_raw(&[0x04]);
        let second_deadline = Instant::now() + Duration::from_millis(400);
        while Instant::now() < second_deadline {
            if !self.is_running() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        self.terminate();
    }

    fn track_alt_screen_sequences(&mut self, chunk: &[u8]) {
        self.alt_screen_active =
            apply_alt_screen_chunk(self.alt_screen_active, &mut self.ansi_tail, chunk);
    }

    pub(crate) fn visible_cursor(&self, viewport_rows: u16, cols: u16) -> Option<(u16, u16)> {
        if self.scroll_offset != 0
            || viewport_rows == 0
            || cols == 0
            || self.parser.screen().hide_cursor()
        {
            return None;
        }
        let (row, col) = self.parser.screen().cursor_position();
        Some((
            row.min(viewport_rows.saturating_sub(1)),
            col.min(cols.saturating_sub(1)),
        ))
    }

    pub(crate) fn current_cwd(&mut self) -> PathBuf {
        if let Some(pid) = self.child.process_id() {
            let proc_cwd = format!("/proc/{pid}/cwd");
            if let Ok(path) = fs::read_link(proc_cwd) {
                self.last_known_cwd = path;
            }
        }
        self.last_known_cwd.clone()
    }
}

fn non_empty_trimmed(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
}

fn is_supported_shell_program(shell_program: &str) -> bool {
    matches!(
        shell_program,
        "bash" | "zsh" | "fish" | "sh" | "dash" | "ksh" | "mksh" | "ash" | "csh" | "tcsh"
    )
}

fn parent_process_command_name() -> Option<String> {
    let ppid = unsafe { libc::getppid() };
    if ppid <= 1 {
        return None;
    }
    let output = Command::new("ps")
        .arg("-p")
        .arg(ppid.to_string())
        .arg("-o")
        .arg("comm=")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let command = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if command.is_empty() {
        None
    } else {
        Some(command)
    }
}

fn resolve_command_from_path(command: &str) -> Option<String> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    if command.contains('/') {
        let path = Path::new(command);
        if path.exists() {
            return Some(path.to_string_lossy().into_owned());
        }
        return None;
    }
    let path_env = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path_env) {
        let candidate = directory.join(command);
        if candidate.exists() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

pub(crate) fn resolve_launch_shell_path_with<F>(
    navix_launch_shell: Option<&str>,
    parent_comm: Option<&str>,
    shell_env: Option<&str>,
    resolve_command: F,
) -> String
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(explicit_shell) = non_empty_trimmed(navix_launch_shell) {
        return explicit_shell.to_string();
    }
    if let Some(parent_command) = non_empty_trimmed(parent_comm) {
        let parent_program = shell_program_name(parent_command);
        if is_supported_shell_program(&parent_program)
            && let Some(resolved_parent) = resolve_command(parent_command)
        {
            return resolved_parent;
        }
    }
    if let Some(shell_from_env) = non_empty_trimmed(shell_env) {
        return shell_from_env.to_string();
    }
    "/bin/sh".to_string()
}

impl Drop for ShellPane {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(crate) fn shell_single_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\\''"))
}

pub(crate) fn cd_to_bytes(path: &Path, restore_pending_input: bool) -> Vec<u8> {
    let command = format!(
        "cd -- {}\r",
        shell_single_quote(path.to_string_lossy().as_ref())
    );
    let mut out = Vec::with_capacity(command.len().saturating_add(3));
    // Clear current prompt input before running cd, so typed text does not prefix the command.
    out.extend_from_slice(&[0x01, 0x0b]);
    out.extend_from_slice(command.as_bytes());
    if restore_pending_input {
        // Yank previously cleared input back onto the new prompt after cd runs.
        out.push(0x19);
    }
    out
}

pub(crate) fn shell_program_name(shell_path: &str) -> String {
    Path::new(shell_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(shell_path)
        .to_ascii_lowercase()
}

pub(crate) fn bash_history_sync_prompt_command(existing_prompt_command: Option<&str>) -> String {
    let sync = "history -a; history -n";
    let Some(existing) = existing_prompt_command
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return sync.to_string();
    };
    if existing.contains("history -n") && existing.contains("history -a") {
        existing.to_string()
    } else {
        format!("{sync}; {existing}")
    }
}

pub(crate) fn default_history_file_for_shell(shell_path: &str) -> Option<String> {
    let home = std::env::var("HOME")
        .ok()
        .filter(|value| !value.is_empty())?;
    let shell_name = shell_program_name(shell_path);
    let mut candidates: Vec<String> = Vec::new();
    match shell_name.as_str() {
        "bash" => candidates.push(format!("{home}/.bash_history")),
        "zsh" => {
            candidates.push(format!("{home}/.zhistory"));
            candidates.push(format!("{home}/.zsh_history"));
            if let Some(state_home) = std::env::var("XDG_STATE_HOME")
                .ok()
                .filter(|value| !value.is_empty())
            {
                candidates.push(format!("{state_home}/zsh/history"));
            }
        },
        "fish" => {
            if let Some(data_home) = std::env::var("XDG_DATA_HOME")
                .ok()
                .filter(|value| !value.is_empty())
            {
                candidates.push(format!("{data_home}/fish/fish_history"));
            }
            candidates.push(format!("{home}/.local/share/fish/fish_history"));
        },
        _ => candidates.push(format!("{home}/.sh_history")),
    }
    candidates
        .iter()
        .find(|path| Path::new(path.as_str()).exists())
        .cloned()
        .or_else(|| candidates.first().cloned())
}

fn ensure_history_file_ready(history_file: &str) {
    let path = Path::new(history_file);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = OpenOptions::new().create(true).append(true).open(path);
}

fn apply_history_defaults(command: &mut CommandBuilder, shell_program: &str, shell_path: &str) {
    let history_file = std::env::var("HISTFILE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| default_history_file_for_shell(shell_path));
    if let Some(history_file) = history_file {
        ensure_history_file_ready(&history_file);
        command.env("HISTFILE", history_file);
    }

    match shell_program {
        "bash" => {
            if std::env::var("HISTSIZE").is_err() {
                command.env("HISTSIZE", "100000");
            }
            if std::env::var("HISTFILESIZE").is_err() {
                command.env("HISTFILESIZE", "200000");
            }
        },
        "zsh" => {
            if std::env::var("HISTSIZE").is_err() {
                command.env("HISTSIZE", "100000");
            }
            if std::env::var("SAVEHIST").is_err() {
                command.env("SAVEHIST", "100000");
            }
        },
        _ => {},
    }
}

fn apply_process_environment(command: &mut CommandBuilder) {
    for (key, value) in std::env::vars() {
        command.env(key, value);
    }
}

fn shell_scrollback_limit() -> usize {
    const DEFAULT_LIMIT: usize = 500_000;
    const PRACTICAL_UNLIMITED: usize = DEFAULT_LIMIT;
    let candidates = ["NAVIX_SHELL_SCROLLBACK_LIMIT", "NAVIX_SHELL_SCROLLBACK"]
        .map(|key| std::env::var(key).ok());
    resolve_scrollback_limit(&candidates, DEFAULT_LIMIT, PRACTICAL_UNLIMITED)
}

pub(crate) fn resolve_scrollback_limit(
    candidates: &[Option<String>],
    default_limit: usize,
    practical_unlimited: usize,
) -> usize {
    for raw in candidates.iter().flatten() {
        if let Some(value) = parse_scrollback_limit(raw, practical_unlimited) {
            return value;
        }
    }
    default_limit
}

pub(crate) fn parse_scrollback_limit(raw: &str, practical_unlimited: usize) -> Option<usize> {
    let parsed = raw.trim().parse::<usize>().ok()?;
    if parsed == 0 {
        Some(practical_unlimited)
    } else {
        Some(parsed)
    }
}
