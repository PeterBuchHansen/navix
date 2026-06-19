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

use super::graphics_stream::{GraphicsEvent, GraphicsProtocol, GraphicsStreamParser, StreamEvent};
use super::*;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::collections::{HashMap, VecDeque, hash_map::DefaultHasher};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};
use std::{hash::Hash, hash::Hasher};
use vt100::Parser;

// Fallback cell pixel size advertised to nested tools (PTY winsize pixel fields
// and CSI 14t/16t replies) when the host terminal does not report its real cell
// size. Tools divide image pixel dimensions by the cell size to compute how many
// cells an image occupies, so the advertised value must match the *host's* real
// cell size or the image is reserved for the wrong number of cells and renders
// at the wrong size (e.g. a tool computing 1544px / 8px = 193 cols against a
// host whose cells are not 8px wide). `detect_host_cell_pixels` reads the real
// size from the controlling terminal; these are only used if that fails.
const CELL_WIDTH_PX: u16 = 8;
const CELL_HEIGHT_PX: u16 = 16;
// Sized so one large image (≈10MB / ~2600 4KB chunks) fits in the channel and
// the child can write it without blocking mid-transfer; the main loop then
// drains and forwards it in one tick. Bounded so a runaway flood still applies
// backpressure rather than growing memory without limit.
const MAX_OUTPUT_QUEUE_CHUNKS: usize = 4096;
// Graphics drain budget per poll tick. A nested tool can send multi-megabyte
// uncompressed images (e.g. 2123x1200 RGBA ≈ 10MB) as ~4KB multipart chunks. We
// stage chunks and flush the host once a transfer completes (see the kitty arm
// of `poll_output`). The budget is large enough that a whole image drains in a
// single tick and is forwarded at once — matching how a bare terminal loads it
// in one fast blip. A smaller budget (tried at 2MB) only *looked* responsive: it
// throttled image throughput to ~60MB/s so the image dribbled in over many
// frames, which reads as "the image takes a while to render". The per-image hash
// is cheap (prefix + length, see `dedup_kitty_transfer`), so a one-tick drain no
// longer carries the full-buffer-hash cost that made the old 32MB budget freeze.
// Text parsing (vt100) has its own, smaller budget.
const MAX_GRAPHICS_EVENTS_PER_TICK: usize = 8192;
const MAX_GRAPHICS_EVENT_BYTES_PER_TICK: usize = 16 * 1024 * 1024;
const MAX_GRAPHICS_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_GRAPHICS_PENDING_OPS: usize = 2048;
const MAX_GRAPHICS_PENDING_BYTES: usize = 32 * 1024 * 1024;
const MAX_GRAPHICS_FLUSH_OPS_PER_FRAME: usize = 192;
const MAX_GRAPHICS_FLUSH_BYTES_PER_FRAME: usize = 4 * 1024 * 1024;
// Base glyph of a kitty Unicode-placeholder cell (`U=1`). Counting these at each
// stage of the composited pipeline reveals where placeholder cells are lost.
const KITTY_PLACEHOLDER_BASE: char = '\u{10EEEE}';
// Cap on placeholder-trace lines emitted to the output debug log per pane.
const PLACEHOLDER_TRACE_LINES: usize = 80;
// Cap on query-trace lines (small escape-bearing child chunks) per pane.
const QUERY_TRACE_LINES: usize = 60;
// Below this length a child chunk is treated as a control/query exchange rather
// than image payload, so the query trace can ignore multi-KB graphics chunks.
const QUERY_TRACE_MAX_CHUNK: usize = 256;

#[derive(Debug, Clone, Copy)]
struct GraphicsQueuedOp {
    protocol: GraphicsProtocol,
    payload_hash: u64,
    payload_len: usize,
    cursor_row: u16,
    cursor_col: u16,
    // True when this op continues an in-progress multipart kitty transfer.
    // Continuation chunks must be emitted contiguously, without an interleaved
    // cursor-positioning escape, or the kitty image fails to assemble.
    is_continuation: bool,
}

#[derive(Default)]
struct GraphicsRenderState {
    cache: HashMap<u64, Vec<u8>>,
    cache_order: VecDeque<u64>,
    cache_bytes: usize,
    pending_ops: VecDeque<GraphicsQueuedOp>,
    pending_bytes: usize,
    // Tracks whether the previously-queued kitty chunk announced more chunks
    // (control `m=1`), so the next kitty chunk is treated as a continuation.
    kitty_mid_transfer: bool,
}

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
    graphics_parser: GraphicsStreamParser,
    graphics_render_state: GraphicsRenderState,
    // Raw graphics bytes to forward verbatim to the host terminal (transparent
    // passthrough). Only holds *complete* transfers; a nested tool's images reach
    // the host exactly as they would in a bare terminal.
    graphics_passthrough: Vec<u8>,
    // Staging buffer for an in-progress kitty multipart transfer (`m=1` chunks).
    // The kitty protocol requires every chunk of a transfer to arrive
    // back-to-back with no intervening output. Because a large transfer is split
    // across several poll ticks (and therefore several render frames), we hold
    // its chunks here and only promote them to `graphics_passthrough` once the
    // final chunk (`m=0`) lands — otherwise ratatui's per-frame draw bytes would
    // be interleaved between chunks and corrupt the image (visible as a tear).
    graphics_passthrough_pending: Vec<u8>,
    // Complete OSC 52 clipboard *set* sequences emitted by the child, forwarded
    // verbatim to the host terminal so a nested tool's "copy" reaches the system
    // clipboard. The local emulator silently drops OSC 52, so without this the
    // child reports a copy that never lands anywhere.
    clipboard_passthrough: Vec<u8>,
    // Complete host-directed OSC sequences (e.g. OSC 9 / OSC 777 desktop
    // notifications) emitted by the child, forwarded verbatim to the host
    // terminal. The local emulator silently drops these, so without this a
    // nested tool's notification never reaches the desktop.
    host_passthrough: Vec<u8>,
    // Hash of the most recently forwarded complete kitty transfer. A nested tool
    // (e.g. ratatui-image) may re-transmit a byte-identical image on every
    // repaint; under heavy scrolling these pile up in the bounded channel
    // (observed: ~1770 copies / 7.3 MB) and forwarding them all to the host in
    // one burst freezes the UI. Re-uploading identical data to a terminal is a
    // no-op (the image id is already loaded), so consecutive identical transfers
    // are collapsed to one. Reset on a delete so a later reload is not dropped.
    last_kitty_transfer_hash: Option<u64>,
    // Host terminal's real cell pixel size, advertised to the child so it sizes
    // graphics exactly as it would running directly in the host terminal.
    cell_width_px: u16,
    cell_height_px: u16,
    output_debug_log: Option<File>,
    output_debug_seq: u64,
    // Wall clock for the debug log, so every line carries milliseconds since the
    // pane spawned and open→first-render latency can be read off directly.
    output_debug_start: Instant,
    // Remaining placeholder-trace lines to emit (gated by the output debug log).
    // Bounded so the diagnostic capture of kitty U=1 placeholder cells across the
    // PTY → vt100 → ansi-to-tui pipeline never floods the log at frame rate.
    placeholder_trace_budget: usize,
    // Remaining query-trace lines: small child chunks carrying escape sequences
    // (terminal capability probes/responses). Reveals a probe we fail to answer,
    // which would make a nested tool stall on a response timeout before drawing.
    query_trace_budget: usize,
    // Diagnostic: wall clock of the previous `poll_output` entry and a count of
    // empty (unlogged) polls since the last logged one. Lets a single logged
    // poll reveal whether the main loop kept ticking (small `since_prev_poll_ms`,
    // many `empty_polls`) or was blocked (large `since_prev_poll_ms`, zero empty
    // polls) during an output gap — distinguishing a slow child from a stalled
    // loop without flooding the log with per-tick heartbeats.
    last_poll_instant: Option<Instant>,
    empty_polls_since_log: usize,
    last_known_cwd: PathBuf,
    child: Box<dyn portable_pty::Child + Send>,
}

pub(crate) struct ShellMetrics {
    pub(crate) shown_start: usize,
    pub(crate) shown_end: usize,
    pub(crate) total: usize,
    pub(crate) has_overflow: bool,
}

impl GraphicsRenderState {
    fn queue_event(&mut self, event: GraphicsEvent, cursor_row: u16, cursor_col: u16) {
        let payload_len = event.bytes.len();
        let payload_hash = bytes_hash(&event.bytes);
        // Determine multipart grouping before any early return so the transfer
        // state machine stays in sync with the actual byte stream.
        let is_continuation = if event.protocol == GraphicsProtocol::Kitty {
            let continuation = self.kitty_mid_transfer;
            self.kitty_mid_transfer = kitty_chunk_has_more(&event.bytes);
            continuation
        } else {
            self.kitty_mid_transfer = false;
            false
        };
        if !self.cache.contains_key(&payload_hash) {
            self.cache_bytes = self.cache_bytes.saturating_add(payload_len);
            self.cache_order.push_back(payload_hash);
            self.cache.insert(payload_hash, event.bytes);
            while self.cache_bytes > MAX_GRAPHICS_CACHE_BYTES {
                let Some(evicted) = self.cache_order.pop_front() else {
                    break;
                };
                if let Some(bytes) = self.cache.remove(&evicted) {
                    self.cache_bytes = self.cache_bytes.saturating_sub(bytes.len());
                }
            }
        }

        if self.pending_ops.back().is_some_and(|last| {
            last.payload_hash == payload_hash
                && last.protocol == event.protocol
                && last.cursor_row == cursor_row
                && last.cursor_col == cursor_col
        }) {
            return;
        }
        if self.pending_ops.len() >= MAX_GRAPHICS_PENDING_OPS
            || self.pending_bytes.saturating_add(payload_len) > MAX_GRAPHICS_PENDING_BYTES
        {
            // Keep earlier transfer chunks intact; dropping oldest chunks can corrupt
            // multipart kitty uploads and result in missing images.
            return;
        }
        self.pending_bytes = self.pending_bytes.saturating_add(payload_len);
        self.pending_ops.push_back(GraphicsQueuedOp {
            protocol: event.protocol,
            payload_hash,
            payload_len,
            cursor_row,
            cursor_col,
            is_continuation,
        });
    }

    fn drain_frame_bytes(
        &mut self,
        pane_inner: Rect,
        scroll_offset: usize,
        interactive: bool,
    ) -> Vec<u8> {
        if pane_inner.width == 0 || pane_inner.height == 0 {
            return Vec::new();
        }
        // Prioritize input responsiveness while user inspects historical output.
        if scroll_offset > 0 {
            return Vec::new();
        }
        // Avoid repaint storms when inactive pane receives background image streams.
        if !interactive {
            return Vec::new();
        }
        let mut frame_bytes = Vec::new();
        let mut emitted_ops = 0usize;
        let mut emitted_bytes = 0usize;
        while emitted_ops < MAX_GRAPHICS_FLUSH_OPS_PER_FRAME
            && emitted_bytes < MAX_GRAPHICS_FLUSH_BYTES_PER_FRAME
        {
            let Some(next_op) = self.pending_ops.pop_front() else {
                break;
            };
            self.pending_bytes = self.pending_bytes.saturating_sub(next_op.payload_len);
            let Some(payload) = self.cache.get(&next_op.payload_hash) else {
                continue;
            };
            if payload.is_empty() {
                continue;
            }
            if frame_bytes.is_empty() {
                frame_bytes.extend_from_slice(b"\x1b7");
            }
            // Kitty placements are addressed by the tool itself (Unicode
            // placeholder cells under U=1, or its own cursor handling), so we
            // pass the transmission through verbatim with no cursor positioning
            // — that is what makes images clip at the pane edge while scrolling,
            // exactly as in a bare terminal. iTerm2 inline images are drawn at
            // the cursor, so they still need to be repositioned.
            //
            // Continuation chunks of a multipart kitty transfer must follow the
            // previous chunk's bytes directly; an interleaved cursor escape
            // corrupts the in-progress transmission.
            if !next_op.is_continuation && next_op.protocol != GraphicsProtocol::Kitty {
                let row_offset = next_op.cursor_row.min(pane_inner.height.saturating_sub(1));
                let col_offset = next_op.cursor_col.min(pane_inner.width.saturating_sub(1));
                let cursor_row = pane_inner.y.saturating_add(1).saturating_add(row_offset);
                let cursor_col = pane_inner.x.saturating_add(1).saturating_add(col_offset);
                frame_bytes
                    .extend_from_slice(format!("\x1b[{cursor_row};{cursor_col}H").as_bytes());
            }
            match next_op.protocol {
                GraphicsProtocol::Kitty | GraphicsProtocol::Iterm2Inline => {
                    frame_bytes.extend_from_slice(payload)
                },
            }
            emitted_ops = emitted_ops.saturating_add(1);
            emitted_bytes = emitted_bytes.saturating_add(payload.len());
        }
        if !frame_bytes.is_empty() {
            frame_bytes.extend_from_slice(b"\x1b8");
        }
        frame_bytes
    }
}

fn bytes_hash(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

impl ShellPane {
    fn log_output_event(&mut self, stage: &str, message: &str) {
        let Some(log) = self.output_debug_log.as_mut() else {
            return;
        };
        self.output_debug_seq = self.output_debug_seq.saturating_add(1);
        let elapsed_ms = self.output_debug_start.elapsed().as_millis();
        let _ = writeln!(
            log,
            "#{:06} t={elapsed_ms}ms stage={} {message}",
            self.output_debug_seq, stage
        );
        let _ = log.flush();
    }

    /// Log the wall time a host-side graphics forward took, measured by the
    /// caller (main loop) around the actual `write_all` + `flush` to the host
    /// terminal. Separates "time spent parsing in `poll_output`" from "time
    /// spent blocking on the host accepting/decoding the image".
    pub(crate) fn log_graphics_forward(&mut self, bytes: usize, dur_ms: u128) {
        if bytes == 0 {
            return;
        }
        self.log_output_event(
            "graphics_forward",
            &format!("bytes={bytes} dur_ms={dur_ms}"),
        );
    }

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
        Self::spawn_from_command_builder(command, cwd, 40, 160)
    }

    pub(crate) fn spawn_command(command_line: &str, cwd: &Path) -> io::Result<Self> {
        Self::spawn_command_sized(command_line, cwd, 40, 160)
    }

    /// Spawn a child whose PTY starts at the given size. Use this when the
    /// rendering area is already known so the child's very first paint targets
    /// the correct geometry — avoiding an initial repaint at the wrong size,
    /// which leaves a torn/garbled dense region (e.g. images) until it settles.
    pub(crate) fn spawn_command_sized(
        command_line: &str,
        cwd: &Path,
        rows: u16,
        cols: u16,
    ) -> io::Result<Self> {
        let mut command = CommandBuilder::new("/bin/sh");
        command.arg("-lc");
        command.arg(command_line);
        command.cwd(cwd.to_path_buf());
        apply_process_environment(&mut command);
        Self::spawn_from_command_builder(command, cwd.to_path_buf(), rows.max(1), cols.max(1))
    }

    fn spawn_from_command_builder(
        command: CommandBuilder,
        cwd: PathBuf,
        init_rows: u16,
        init_cols: u16,
    ) -> io::Result<Self> {
        let (cell_width_px, cell_height_px) = detect_host_cell_pixels();
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: init_rows,
                cols: init_cols,
                pixel_width: init_cols.saturating_mul(cell_width_px),
                pixel_height: init_rows.saturating_mul(cell_height_px),
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

        let mut pane = Self {
            master,
            writer,
            rx,
            parser: Parser::new(init_rows, init_cols, shell_scrollback_limit()),
            scroll_offset: 0,
            viewport_rows: 1,
            has_overflow: false,
            pty_rows: init_rows,
            pty_cols: init_cols,
            alt_screen_active: false,
            ansi_tail: Vec::new(),
            graphics_parser: GraphicsStreamParser::new(),
            graphics_render_state: GraphicsRenderState::default(),
            graphics_passthrough: Vec::new(),
            graphics_passthrough_pending: Vec::new(),
            clipboard_passthrough: Vec::new(),
            host_passthrough: Vec::new(),
            last_kitty_transfer_hash: None,
            cell_width_px,
            cell_height_px,
            output_debug_log: open_output_debug_log(),
            output_debug_seq: 0,
            output_debug_start: Instant::now(),
            placeholder_trace_budget: PLACEHOLDER_TRACE_LINES,
            query_trace_budget: QUERY_TRACE_LINES,
            last_poll_instant: None,
            empty_polls_since_log: 0,
            last_known_cwd: cwd,
            child,
        };
        pane.log_output_event(
            "spawn",
            &format!("pty_rows={} pty_cols={}", pane.pty_rows, pane.pty_cols),
        );
        Ok(pane)
    }

    pub(crate) fn poll_output(&mut self) -> OutputDrain {
        let poll_start = Instant::now();
        let since_prev_poll_ms = self
            .last_poll_instant
            .map(|t| t.elapsed().as_millis())
            .unwrap_or(0);
        self.last_poll_instant = Some(poll_start);
        let mut processed_chunks = 0usize;
        let mut processed_bytes = 0usize;
        let mut processed_graphics_events = 0usize;
        let mut processed_graphics_bytes = 0usize;
        let mut processed_text_events = 0usize;
        let mut processed_text_bytes = 0usize;
        let mut kitty_events = 0usize;
        let mut kitty_dropped = 0usize;
        let mut iterm2_events = 0usize;
        let mut query_reply_bytes = 0usize;
        let mut hit_limit = false;
        loop {
            if processed_chunks >= MAX_OUTPUT_CHUNKS_PER_TICK
                || processed_bytes >= MAX_OUTPUT_BYTES_PER_TICK
                || processed_text_bytes >= MAX_TEXT_BYTES_PER_TICK
                || processed_graphics_events >= MAX_GRAPHICS_EVENTS_PER_TICK
                || processed_graphics_bytes >= MAX_GRAPHICS_EVENT_BYTES_PER_TICK
            {
                hit_limit = true;
                break;
            }
            match self.rx.try_recv() {
                Ok(chunk) => {
                    processed_bytes = processed_bytes.saturating_add(chunk.len());
                    processed_chunks = processed_chunks.saturating_add(1);
                    self.track_alt_screen_sequences(&chunk);
                    let query_replies = terminal_query_replies(
                        &chunk,
                        self.pty_rows,
                        self.pty_cols,
                        self.cell_width_px,
                        self.cell_height_px,
                    );
                    if !query_replies.is_empty() {
                        query_reply_bytes = query_reply_bytes.saturating_add(query_replies.len());
                        let _ = self.send_raw(&query_replies);
                    }
                    if self.output_debug_log.is_some()
                        && self.query_trace_budget > 0
                        && chunk.len() <= QUERY_TRACE_MAX_CHUNK
                        && chunk.contains(&0x1b)
                    {
                        // Capture small escape-bearing chunks (capability probes /
                        // responses). A probe we never answer leaves a nested tool
                        // blocked on its response timeout before it starts drawing,
                        // which shows up as a long silent gap before the first image.
                        self.query_trace_budget = self.query_trace_budget.saturating_sub(1);
                        self.log_output_event(
                            "query_chunk",
                            &format!(
                                "reply_bytes={} len={} raw={}",
                                query_replies.len(),
                                chunk.len(),
                                escape_for_log(&chunk, 240)
                            ),
                        );
                    }
                    let events = self.graphics_parser.push_chunk(&chunk);
                    for event in events {
                        match event {
                            StreamEvent::Text(bytes) => {
                                if !bytes.is_empty() {
                                    processed_text_events = processed_text_events.saturating_add(1);
                                    processed_text_bytes =
                                        processed_text_bytes.saturating_add(bytes.len());
                                    if self.output_debug_log.is_some()
                                        && self.placeholder_trace_budget > 0
                                    {
                                        // Layer A: what the tool actually emitted, before
                                        // vt100 stores it. Compare its placeholder count to
                                        // the `placeholder_row` lines (vt100/ansi stages).
                                        let pty_base = String::from_utf8_lossy(&bytes)
                                            .matches(KITTY_PLACEHOLDER_BASE)
                                            .count();
                                        if pty_base > 0 {
                                            self.placeholder_trace_budget =
                                                self.placeholder_trace_budget.saturating_sub(1);
                                            self.log_output_event(
                                                "placeholder_pty",
                                                &format!(
                                                    "pty_base={pty_base} bytes={} raw={}",
                                                    bytes.len(),
                                                    escape_for_log(&bytes, 240)
                                                ),
                                            );
                                        }
                                    }
                                    self.parser.process(&bytes);
                                }
                            },
                            StreamEvent::Clipboard(bytes) => {
                                // Forward the child's OSC 52 clipboard write to
                                // the host verbatim; the local emulator would
                                // otherwise drop it and the copy would never
                                // reach the system clipboard.
                                self.clipboard_passthrough.extend_from_slice(&bytes);
                            },
                            StreamEvent::HostPassthrough(bytes) => {
                                // Forward host-directed OSC (e.g. notifications)
                                // verbatim; the local emulator would drop it.
                                self.host_passthrough.extend_from_slice(&bytes);
                            },
                            StreamEvent::Graphics(graphics_event) => {
                                let (cursor_row, cursor_col) =
                                    self.parser.screen().cursor_position();
                                processed_graphics_events =
                                    processed_graphics_events.saturating_add(1);
                                processed_graphics_bytes = processed_graphics_bytes
                                    .saturating_add(graphics_event.bytes.len());
                                if self.scroll_offset > 0 {
                                    // While user is reading scrolled history, prioritize key/mouse
                                    // responsiveness and defer live graphics updates.
                                    continue;
                                }
                                if graphics_event.protocol == GraphicsProtocol::Kitty
                                    && let Some(reply) = kitty_graphics_query_reply(
                                        &graphics_event.bytes,
                                    )
                                    .or_else(|| {
                                        kitty_graphics_query_reply_fallback(&graphics_event.bytes)
                                    })
                                {
                                    query_reply_bytes =
                                        query_reply_bytes.saturating_add(reply.len());
                                    self.log_output_event(
                                        "kitty_query_reply",
                                        &format!(
                                            "query_bytes={} reply_bytes={}",
                                            graphics_event.bytes.len(),
                                            reply.len()
                                        ),
                                    );
                                    let _ = self.send_raw(&reply);
                                    // Keep kitty capability probes local: they should not hit the host
                                    // terminal, which would emit responses into Navix key input.
                                    continue;
                                } else if graphics_event.protocol == GraphicsProtocol::Kitty
                                    && graphics_event.bytes.windows(3).any(|w| w == b"a=q")
                                {
                                    self.log_output_event(
                                        "kitty_query_unhandled",
                                        &format!("query_bytes={}", graphics_event.bytes.len()),
                                    );
                                }
                                match graphics_event.protocol {
                                    GraphicsProtocol::Kitty => {
                                        kitty_events = kitty_events.saturating_add(1);
                                        if let Some(control) =
                                            kitty_control_prefix(&graphics_event.bytes)
                                            && control.contains("a=")
                                        {
                                            self.log_output_event(
                                                "kitty_control",
                                                &format!(
                                                    "control={control} total_bytes={}",
                                                    graphics_event.bytes.len()
                                                ),
                                            );
                                        }
                                    },
                                    GraphicsProtocol::Iterm2Inline => {
                                        iterm2_events = iterm2_events.saturating_add(1)
                                    },
                                }
                                match graphics_event.protocol {
                                    GraphicsProtocol::Kitty => {
                                        // Transparent passthrough: forward the
                                        // bytes to the host verbatim. Placement is
                                        // the tool's job (U=1 placeholder cells ride
                                        // the text path), so no queue, cache, budget,
                                        // or cursor repositioning — exactly what a
                                        // bare terminal receives. Chunks of a
                                        // multipart transfer are staged and only
                                        // released once the transfer completes so
                                        // they reach the host contiguously.
                                        self.graphics_passthrough_pending
                                            .extend_from_slice(&graphics_event.bytes);
                                        if !kitty_chunk_has_more(&graphics_event.bytes) {
                                            let (forward, new_hash) = dedup_kitty_transfer(
                                                &self.graphics_passthrough_pending,
                                                self.last_kitty_transfer_hash,
                                            );
                                            self.last_kitty_transfer_hash = new_hash;
                                            if forward {
                                                self.graphics_passthrough
                                                    .append(&mut self.graphics_passthrough_pending);
                                            } else {
                                                kitty_dropped = kitty_dropped.saturating_add(1);
                                                self.graphics_passthrough_pending.clear();
                                            }
                                        }
                                    },
                                    GraphicsProtocol::Iterm2Inline => {
                                        // Cursor-placed protocol still uses the
                                        // positioned reflush path.
                                        self.graphics_render_state.queue_event(
                                            graphics_event,
                                            cursor_row,
                                            cursor_col,
                                        );
                                    },
                                }
                            },
                        }
                    }
                },
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        if processed_chunks > 0 || hit_limit || processed_graphics_events > 0 {
            let dur_ms = poll_start.elapsed().as_millis();
            let empty_polls = self.empty_polls_since_log;
            self.empty_polls_since_log = 0;
            self.log_output_event(
                "poll_output",
                &format!(
                    "dur_ms={dur_ms} since_prev_poll_ms={since_prev_poll_ms} empty_polls={empty_polls} chunks={} bytes={} text_events={} text_bytes={} gfx_events={} gfx_bytes={} kitty={} kitty_dropped={} iterm2={} query_reply_bytes={} queue_ops={} queue_bytes={} cache_items={} cache_bytes={} hit_limit={} alt_screen={}",
                    processed_chunks,
                    processed_bytes,
                    processed_text_events,
                    processed_text_bytes,
                    processed_graphics_events,
                    processed_graphics_bytes,
                    kitty_events,
                    kitty_dropped,
                    iterm2_events,
                    query_reply_bytes,
                    self.graphics_render_state.pending_ops.len(),
                    self.graphics_render_state.pending_bytes,
                    self.graphics_render_state.cache.len(),
                    self.graphics_render_state.cache_bytes,
                    hit_limit,
                    self.alt_screen_active,
                ),
            );
        } else if self.output_debug_log.is_some() {
            // Empty poll: not logged on its own, but counted so the next logged
            // poll reports how many ticks elapsed during the gap.
            self.empty_polls_since_log = self.empty_polls_since_log.saturating_add(1);
        }
        OutputDrain {
            processed_chunks,
            hit_limit,
            // Only text events mutate the vt100 grid that the composited frame
            // is built from. Graphics events are split off upstream and never
            // reach the parser, so a pure image (re)transmit leaves the grid
            // identical and the redraw can be skipped.
            screen_changed: processed_text_events > 0,
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

        let trace_placeholders =
            self.output_debug_log.is_some() && self.placeholder_trace_budget > 0;
        let formatted_rows: Vec<Vec<u8>> = self.parser.screen().rows_formatted(0, cols).collect();
        let mut bytes: Vec<u8> = Vec::new();
        for (idx, row) in formatted_rows.iter().enumerate() {
            if idx > 0 {
                bytes.extend_from_slice(b"\x1b[0m");
                bytes.push(b'\n');
            }
            // vt100 skips runs of blank/default cells with CSI Cursor-Forward
            // (`\x1b[<n>C`) instead of literal spaces. ansi-to-tui does not honor
            // CUF, so it would drop those columns and shift the rest of the row
            // left — misaligning, e.g., the first row of a kitty placeholder grid
            // against the rows below it (a visible image tear). Expand CUF back
            // into spaces so every column lands where vt100 placed it.
            bytes.extend_from_slice(&expand_cursor_forward(row));
        }

        let text = bytes
            .clone()
            .into_text()
            .unwrap_or_else(|_| Text::raw(String::from_utf8_lossy(&bytes).into_owned()));

        if trace_placeholders {
            self.trace_placeholder_rows(&formatted_rows, &text);
        }
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

    /// Diagnostic: for each grid row carrying kitty `U=1` placeholder cells,
    /// record how many placeholder base glyphs survive at the `vt100`
    /// re-serialization stage versus the `ansi-to-tui` stage. A drop between the
    /// two (`vt100_base` > `ansi_base`) means cells are lost in the composited
    /// pipeline, so the host fills fewer cells than the tool reserved and the
    /// image renders smaller than its block. Bounded by `placeholder_trace_budget`.
    fn trace_placeholder_rows(&mut self, formatted_rows: &[Vec<u8>], text: &Text<'static>) {
        for (idx, row) in formatted_rows.iter().enumerate() {
            if self.placeholder_trace_budget == 0 {
                break;
            }
            let row_str = String::from_utf8_lossy(row);
            let vt100_base = row_str.matches(KITTY_PLACEHOLDER_BASE).count();
            if vt100_base == 0 {
                continue;
            }
            let vt100_chars = row_str.chars().count();
            let (ansi_base, ansi_chars) = text
                .lines
                .get(idx)
                .map(|line| {
                    line.spans
                        .iter()
                        .fold((0usize, 0usize), |(base, chars), span| {
                            (
                                base + span.content.matches(KITTY_PLACEHOLDER_BASE).count(),
                                chars + span.content.chars().count(),
                            )
                        })
                })
                .unwrap_or((0, 0));
            self.placeholder_trace_budget = self.placeholder_trace_budget.saturating_sub(1);
            self.log_output_event(
                "placeholder_row",
                &format!(
                    "idx={idx} vt100_base={vt100_base} ansi_base={ansi_base} vt100_chars={vt100_chars} ansi_chars={ansi_chars} raw={}",
                    escape_for_log(row, 240)
                ),
            );
        }
    }

    pub(crate) fn resize(&mut self, rows: u16, cols: u16) {
        if rows == self.pty_rows && cols == self.pty_cols {
            return;
        }
        // Refresh from the host on every real resize so a font/zoom change is
        // reflected in what we advertise to the child.
        let (cell_width_px, cell_height_px) = detect_host_cell_pixels();
        self.cell_width_px = cell_width_px;
        self.cell_height_px = cell_height_px;
        let size = PtySize {
            rows,
            cols,
            // Populate pixel dimensions so nested tools that read ws_xpixel/
            // ws_ypixel via TIOCGWINSZ (instead of CSI 14t/16t) derive the same
            // cell size we advertise. Leaving these at 0 makes tools guess a cell
            // size and mis-size graphics for the pane, wrapping/shifting them.
            pixel_width: cols.saturating_mul(cell_width_px),
            pixel_height: rows.saturating_mul(cell_height_px),
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

    pub(crate) fn drain_graphics_frame_bytes(
        &mut self,
        pane_inner: Rect,
        interactive: bool,
    ) -> Vec<u8> {
        let pending_before = self.graphics_render_state.pending_ops.len();
        let pending_bytes_before = self.graphics_render_state.pending_bytes;
        let reason = if pane_inner.width == 0 || pane_inner.height == 0 {
            "skip_zero_pane"
        } else if self.scroll_offset > 0 {
            "skip_scrolled_history"
        } else if !interactive {
            "skip_inactive_pane"
        } else {
            "flush_attempt"
        };
        let out = self.graphics_render_state.drain_frame_bytes(
            pane_inner,
            self.scroll_offset,
            interactive,
        );
        let pending_after = self.graphics_render_state.pending_ops.len();
        let pending_bytes_after = self.graphics_render_state.pending_bytes;
        if pending_before > 0 || !out.is_empty() {
            self.log_output_event(
                "graphics_flush",
                &format!(
                    "reason={} interactive={} pane={}x{} scroll_offset={} emitted_bytes={} pending_ops_before={} pending_ops_after={} pending_bytes_before={} pending_bytes_after={}",
                    reason,
                    interactive,
                    pane_inner.width,
                    pane_inner.height,
                    self.scroll_offset,
                    out.len(),
                    pending_before,
                    pending_after,
                    pending_bytes_before,
                    pending_bytes_after,
                ),
            );
        }
        out
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

    pub(crate) fn has_active_graphics(&self) -> bool {
        !self.graphics_render_state.cache.is_empty()
            || !self.graphics_render_state.pending_ops.is_empty()
    }

    /// Take the raw graphics bytes queued for verbatim forwarding to the host
    /// terminal this frame. Forwarded immediately (no budget) so nested tools'
    /// images reach the host as fast as in a bare terminal.
    pub(crate) fn take_graphics_passthrough(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.graphics_passthrough)
    }

    /// Take the child's OSC 52 clipboard bytes queued for verbatim forwarding to
    /// the host terminal this frame. Forwarded as-is (no cursor wrapping) so a
    /// nested tool's copy reaches the system clipboard, 1:1 with a bare
    /// terminal.
    pub(crate) fn take_clipboard_passthrough(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.clipboard_passthrough)
    }

    /// Take host-directed OSC bytes (e.g. OSC 9 / OSC 777 notifications) queued
    /// for verbatim forwarding to the host terminal this frame. Forwarded as-is
    /// (no cursor wrapping) since these are position-independent side channels.
    pub(crate) fn take_host_passthrough(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.host_passthrough)
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

/// Expand CSI Cursor-Forward sequences (`\x1b[<n>C`, default n=1) within a
/// single formatted row into `n` spaces. vt100 emits these to advance over
/// blank/default cells, but ansi-to-tui ignores cursor movement, which would
/// collapse those columns and shift the remainder of the row. Within a
/// left-to-right formatted row, cursor-forward only ever skips not-yet-emitted
/// blank cells, so spaces are an exact substitution. Other escape sequences
/// (SGR, etc.) pass through untouched.
pub(crate) fn expand_cursor_forward(row: &[u8]) -> Vec<u8> {
    if !row.windows(2).any(|w| w == b"\x1b[") {
        return row.to_vec();
    }
    let mut out = Vec::with_capacity(row.len());
    let mut i = 0;
    while i < row.len() {
        if row[i] == 0x1b && i + 1 < row.len() && row[i + 1] == b'[' {
            let mut j = i + 2;
            while j < row.len() && (row[j].is_ascii_digit() || row[j] == b';') {
                j += 1;
            }
            if j < row.len() && row[j] == b'C' {
                let count = std::str::from_utf8(&row[i + 2..j])
                    .ok()
                    .map(|raw| {
                        if raw.is_empty() {
                            1
                        } else {
                            raw.parse().unwrap_or(1)
                        }
                    })
                    .unwrap_or(1);
                out.extend(std::iter::repeat_n(b' ', count));
                i = j + 1;
                continue;
            }
        }
        out.push(row[i]);
        i += 1;
    }
    out
}

/// Wrap a pane's verbatim graphics passthrough bytes for forwarding to the host
/// terminal. The transfer is bracketed with DECSC/DECRC (`\x1b7` … `\x1b8`,
/// save/restore cursor) so forwarding never disturbs the cursor position
/// ratatui is about to draw text at — kitty's `U=1` Unicode-placeholder cells in
/// the text grid, not the cursor, drive image placement. Empty input yields
/// empty output so idle frames emit nothing.
pub(crate) fn wrap_graphics_passthrough(bytes: &[u8]) -> Vec<u8> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(bytes.len().saturating_add(4));
    out.extend_from_slice(b"\x1b7");
    out.extend_from_slice(bytes);
    out.extend_from_slice(b"\x1b8");
    out
}

/// Read the controlling terminal's real cell pixel size via the window-size
/// ioctl (TIOCGWINSZ pixel fields). Returns the fallback `CELL_WIDTH_PX` /
/// `CELL_HEIGHT_PX` when there is no tty (tests, pipes) or the terminal reports
/// zero pixels (some terminals/multiplexers do). Advertising the host's real
/// cell size is what lets a nested tool size images 1:1 with a direct run.
pub(crate) fn detect_host_cell_pixels() -> (u16, u16) {
    if let Ok(size) = crossterm::terminal::window_size()
        && size.columns > 0
        && size.rows > 0
        && size.width > 0
        && size.height > 0
    {
        let width = (size.width / size.columns).max(1);
        let height = (size.height / size.rows).max(1);
        return (width, height);
    }
    (CELL_WIDTH_PX, CELL_HEIGHT_PX)
}

fn terminal_query_replies(
    stream: &[u8],
    rows: u16,
    cols: u16,
    cell_width_px: u16,
    cell_height_px: u16,
) -> Vec<u8> {
    let mut replies = Vec::new();
    let window_height_px = rows.saturating_mul(cell_height_px);
    let window_width_px = cols.saturating_mul(cell_width_px);

    if stream.windows(3).any(|segment| segment == b"\x1b[c") {
        // DA response: advertise a broadly-compatible VT feature set.
        replies.extend_from_slice(b"\x1b[?62;4;6;22c");
    }
    if stream.windows(4).any(|segment| segment == b"\x1b[5n") {
        // DSR response: terminal status OK.
        replies.extend_from_slice(b"\x1b[0n");
    }
    if stream.windows(5).any(|segment| segment == b"\x1b[14t") {
        // Report window size in pixels.
        replies
            .extend_from_slice(format!("\x1b[4;{window_height_px};{window_width_px}t").as_bytes());
    }
    if stream.windows(5).any(|segment| segment == b"\x1b[16t") {
        // Report cell size in pixels.
        replies.extend_from_slice(format!("\x1b[6;{cell_height_px};{cell_width_px}t").as_bytes());
    }
    if stream.windows(5).any(|segment| segment == b"\x1b[18t") {
        // Report terminal size in characters.
        replies.extend_from_slice(format!("\x1b[8;{rows};{cols}t").as_bytes());
    }
    if stream.windows(7).any(|segment| segment == b"\x1b]10;?\x07") {
        // Foreground color query (OSC 10).
        replies.extend_from_slice(b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\");
    }
    if stream.windows(7).any(|segment| segment == b"\x1b]11;?\x07") {
        // Background color query (OSC 11).
        replies.extend_from_slice(b"\x1b]11;rgb:0000/0000/0000\x1b\\");
    }
    replies
}

fn kitty_graphics_query_reply(seq: &[u8]) -> Option<Vec<u8>> {
    if !seq.starts_with(b"\x1b_G") || !seq.ends_with(b"\x1b\\") {
        return None;
    }
    let body = &seq[3..seq.len().saturating_sub(2)];
    let separator_idx = body
        .iter()
        .position(|byte| *byte == b';')
        .unwrap_or(body.len());
    let control = &body[..separator_idx];
    if !contains_kitty_control_token(control, b"a=q") {
        return None;
    }
    let image_id = kitty_control_value(control, b"i");
    let mut reply = Vec::new();
    reply.extend_from_slice(b"\x1b_G");
    if let Some(image_id) = image_id {
        reply.extend_from_slice(b"i=");
        reply.extend_from_slice(image_id);
    }
    reply.extend_from_slice(b";OK\x1b\\");
    Some(reply)
}

fn kitty_graphics_query_reply_fallback(seq: &[u8]) -> Option<Vec<u8>> {
    if !seq.starts_with(b"\x1b_G") || !seq.ends_with(b"\x1b\\") {
        return None;
    }
    if !seq.windows(3).any(|window| window == b"a=q") {
        return None;
    }
    let image_id = seq
        .windows(2)
        .position(|window| window == b"i=")
        .and_then(|start| {
            let mut idx = start.saturating_add(2);
            while idx < seq.len() {
                let byte = seq[idx];
                if byte.is_ascii_digit() {
                    idx = idx.saturating_add(1);
                    continue;
                }
                break;
            }
            if idx > start.saturating_add(2) {
                Some(&seq[start.saturating_add(2)..idx])
            } else {
                None
            }
        });
    let mut reply = Vec::new();
    reply.extend_from_slice(b"\x1b_G");
    if let Some(image_id) = image_id {
        reply.extend_from_slice(b"i=");
        reply.extend_from_slice(image_id);
    }
    reply.extend_from_slice(b";OK\x1b\\");
    Some(reply)
}

fn kitty_control_prefix(seq: &[u8]) -> Option<String> {
    if !seq.starts_with(b"\x1b_G") {
        return None;
    }
    let body = &seq[3..];
    let separator_idx = body
        .iter()
        .position(|byte| *byte == b';')
        .unwrap_or(body.len());
    let control = &body[..separator_idx.min(96)];
    Some(String::from_utf8_lossy(control).into_owned())
}

fn kitty_chunk_has_more(seq: &[u8]) -> bool {
    if !seq.starts_with(b"\x1b_G") || !seq.ends_with(b"\x1b\\") {
        return false;
    }
    let body = &seq[3..seq.len().saturating_sub(2)];
    let separator_idx = body
        .iter()
        .position(|byte| *byte == b';')
        .unwrap_or(body.len());
    let control = &body[..separator_idx];
    kitty_control_value(control, b"m").is_some_and(|value| value == b"1")
}

fn kitty_transfer_is_delete(transfer: &[u8]) -> bool {
    kitty_control_prefix(transfer).is_some_and(|control| {
        // Action `a=d`/`a=D` deletes images. It must always pass through and
        // must reset dedup state, otherwise a later identical reload would be
        // suppressed and the image would stay gone.
        control
            .split(',')
            .any(|part| matches!(part.trim(), "a=d" | "a=D"))
    })
}

// Bytes of a transfer fed into the dedup hash. The control prefix (which carries
// the image id, action, format and dimensions) and the start of the payload live
// well within this window, so two transfers that differ at all in identity or
// early pixels hash differently. Hashing only a prefix (plus the exact total
// length below) avoids a full multi-megabyte SipHash pass on every image — pure
// overhead, since a tool that re-IDs each upload (e.g. ratatui-image) never
// dedups anyway, and one that re-sends byte-identical bytes is still caught.
const KITTY_DEDUP_HASH_PREFIX: usize = 4096;

/// Render bytes as a single printable line for the debug log: ASCII graphics
/// pass through, everything else becomes `\xHH`, truncated to `cap` input bytes
/// with a `…+N` suffix so a wide placeholder row stays one readable line.
fn escape_for_log(bytes: &[u8], cap: usize) -> String {
    let shown = bytes.len().min(cap);
    let mut out = String::with_capacity(shown.saturating_mul(2));
    for &byte in &bytes[..shown] {
        match byte {
            0x20..=0x7e => out.push(byte as char),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            0x1b => out.push_str("\\e"),
            _ => out.push_str(&format!("\\x{byte:02x}")),
        }
    }
    if bytes.len() > shown {
        out.push_str(&format!("…+{}", bytes.len() - shown));
    }
    out
}

fn hash_kitty_transfer(transfer: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Length first so transfers sharing a prefix but differing in size (e.g. a
    // larger frame of the same image) never collide.
    transfer.len().hash(&mut hasher);
    let prefix = &transfer[..transfer.len().min(KITTY_DEDUP_HASH_PREFIX)];
    prefix.hash(&mut hasher);
    hasher.finish()
}

/// Decide whether a completed kitty transfer should be forwarded to the host,
/// collapsing runs of byte-identical re-transmits to a single upload.
///
/// Re-uploading the same bytes under the same image id is a no-op for the
/// terminal (the id is already loaded), so a tool that re-transmits its image on
/// every repaint would otherwise flood the host. Returns `(forward, new_last)`:
/// drop when identical to the previously forwarded transfer; deletes always
/// forward and clear the dedup state.
pub(crate) fn dedup_kitty_transfer(transfer: &[u8], last_hash: Option<u64>) -> (bool, Option<u64>) {
    if kitty_transfer_is_delete(transfer) {
        return (true, None);
    }
    let hash = hash_kitty_transfer(transfer);
    if last_hash == Some(hash) {
        (false, last_hash)
    } else {
        (true, Some(hash))
    }
}

fn contains_kitty_control_token(control: &[u8], token: &[u8]) -> bool {
    control.split(|byte| *byte == b',').any(|part| {
        part == token
            || part
                .strip_prefix(b" ")
                .is_some_and(|trimmed| trimmed == token)
    })
}

fn kitty_control_value<'a>(control: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let mut prefix = Vec::with_capacity(key.len().saturating_add(1));
    prefix.extend_from_slice(key);
    prefix.push(b'=');
    for part in control.split(|byte| *byte == b',') {
        let trimmed = if let Some(without_space) = part.strip_prefix(b" ") {
            without_space
        } else {
            part
        };
        if let Some(value) = trimmed.strip_prefix(prefix.as_slice())
            && !value.is_empty()
        {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
pub(crate) fn terminal_query_replies_for_test(stream: &[u8], rows: u16, cols: u16) -> Vec<u8> {
    terminal_query_replies(stream, rows, cols, CELL_WIDTH_PX, CELL_HEIGHT_PX)
}

#[cfg(test)]
pub(crate) fn terminal_query_replies_for_test_with_cell(
    stream: &[u8],
    rows: u16,
    cols: u16,
    cell_width_px: u16,
    cell_height_px: u16,
) -> Vec<u8> {
    terminal_query_replies(stream, rows, cols, cell_width_px, cell_height_px)
}

#[cfg(test)]
pub(crate) fn kitty_graphics_query_reply_for_test(seq: &[u8]) -> Option<Vec<u8>> {
    kitty_graphics_query_reply(seq).or_else(|| kitty_graphics_query_reply_fallback(seq))
}

#[cfg(test)]
pub(crate) fn kitty_multipart_flush_summary_for_test() -> (usize, bool) {
    // Simulate a single multipart kitty image split into 3 chunks
    // (first announces `m=1`, middle `m=1`, final `m=0`).
    let mut state = GraphicsRenderState::default();
    let chunks: [&[u8]; 3] = [
        b"\x1b_Gi=1,a=T,m=1;AAAA\x1b\\",
        b"\x1b_Gm=1;BBBB\x1b\\",
        b"\x1b_Gm=0;CCCC\x1b\\",
    ];
    for chunk in chunks {
        state.queue_event(
            GraphicsEvent {
                protocol: GraphicsProtocol::Kitty,
                bytes: chunk.to_vec(),
            },
            2,
            3,
        );
    }
    let out = state.drain_frame_bytes(Rect::new(0, 0, 80, 24), 0, true);
    let mut cursor_moves = 0usize;
    let mut idx = 0usize;
    while idx + 1 < out.len() {
        if out[idx] == 0x1b && out[idx + 1] == b'[' {
            cursor_moves = cursor_moves.saturating_add(1);
        }
        idx = idx.saturating_add(1);
    }
    let pos = |needle: &[u8]| out.windows(needle.len()).position(|w| w == needle);
    let order_ok = matches!(
        (pos(b"AAAA"), pos(b"BBBB"), pos(b"CCCC")),
        (Some(a), Some(b), Some(c)) if a < b && b < c
    );
    (cursor_moves, order_ok)
}

#[cfg(test)]
/// Mirror of the kitty passthrough staging in `poll_output`: feed chunks in
/// arrival order and report, after each chunk, how many bytes have been released
/// to the host. Released bytes must stay at 0 until the transfer's final chunk
/// (`m=0`) lands, otherwise partial multipart data leaks between ratatui draws
/// and the image tears.
pub(crate) fn kitty_passthrough_staging_for_test(chunks: &[&[u8]]) -> Vec<usize> {
    let mut pending: Vec<u8> = Vec::new();
    let mut ready: Vec<u8> = Vec::new();
    let mut released_after_each = Vec::new();
    for chunk in chunks {
        pending.extend_from_slice(chunk);
        if !kitty_chunk_has_more(chunk) {
            ready.append(&mut pending);
        }
        released_after_each.push(ready.len());
    }
    released_after_each
}

#[cfg(test)]
/// Diagnostic: push a kitty Unicode-placeholder cell (truecolor fg encoding an
/// image id + base char U+10EEEE + two row/col combining diacritics) through the
/// exact text path `render_text_and_metrics` uses, and report whether the base
/// char, the combining diacritics, and the truecolor foreground all survive into
/// the rendered ratatui `Text`. Returns (has_placeholder, has_combining, has_fg).
pub(crate) fn placeholder_render_survival_for_test() -> (bool, bool, bool) {
    use ansi_to_tui::IntoText;
    let mut parser = Parser::new(2, 20, 0);
    parser.process("\x1b[38;2;10;20;30m\u{10EEEE}\u{0305}\u{030D}".as_bytes());
    let cols = 20u16;
    let mut bytes: Vec<u8> = Vec::new();
    for (idx, row) in parser
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
    let mut has_placeholder = false;
    let mut has_combining = false;
    let mut has_fg = false;
    for line in &text.lines {
        for span in &line.spans {
            if span.content.contains('\u{10EEEE}') {
                has_placeholder = true;
                if span.content.contains('\u{0305}') || span.content.contains('\u{030D}') {
                    has_combining = true;
                }
                if span.style.fg == Some(ratatui::style::Color::Rgb(10, 20, 30)) {
                    has_fg = true;
                }
            }
        }
    }
    (has_placeholder, has_combining, has_fg)
}

#[cfg(test)]
/// Diagnostic: render a row of three adjacent kitty placeholder cells (each with
/// distinct combining diacritics) into a real ratatui `Buffer` and report, per
/// column 0..3, whether that cell holds a placeholder grapheme that still carries
/// its combining marks. A correct layout is `[true, true, true]`; anything else
/// means ratatui mis-widths U+10EEEE or drops the diacritics, which shears the
/// placeholder grid and tears the image.
pub(crate) fn placeholder_buffer_layout_for_test() -> [bool; 3] {
    use ansi_to_tui::IntoText;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::widgets::{Paragraph, Widget};

    // Three placeholder cells: base U+10EEEE plus one row diacritic and a
    // distinct column diacritic each (U+0305=0, U+030D=1, U+030E=2).
    let line = "\u{10EEEE}\u{0305}\u{0305}\u{10EEEE}\u{0305}\u{030D}\u{10EEEE}\u{0305}\u{030E}";
    let text = line
        .as_bytes()
        .to_vec()
        .into_text()
        .unwrap_or_else(|_| Text::raw(line));
    let area = Rect::new(0, 0, 6, 1);
    let mut buf = Buffer::empty(area);
    Paragraph::new(text).render(area, &mut buf);
    let mut result = [false; 3];
    for (col, slot) in result.iter_mut().enumerate() {
        if let Some(cell) = buf.cell((col as u16, 0)) {
            let symbol = cell.symbol();
            *slot = symbol.contains('\u{10EEEE}')
                && (symbol.contains('\u{0305}')
                    || symbol.contains('\u{030D}')
                    || symbol.contains('\u{030E}'));
        }
    }
    result
}

#[cfg(test)]
pub(crate) fn parse_stream_events_for_test(chunks: &[&[u8]]) -> Vec<StreamEvent> {
    let mut parser = GraphicsStreamParser::new();
    let mut out = Vec::new();
    for chunk in chunks {
        out.extend(parser.push_chunk(chunk));
    }
    out
}

fn apply_process_environment(command: &mut CommandBuilder) {
    for (key, value) in std::env::vars() {
        if key == "TERM" || key == "TERM_PROGRAM" || key == "TERM_PROGRAM_VERSION" {
            continue;
        }
        command.env(key, value);
    }
    let child_term = resolve_child_term(
        std::env::var("NAVIX_CHILD_TERM").ok().as_deref(),
        std::env::var("TERM").ok().as_deref(),
    );
    command.env("TERM", child_term);
    let term_program = std::env::var("TERM_PROGRAM")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "navix".to_string());
    command.env("TERM_PROGRAM", term_program);
    let term_program_version = std::env::var("TERM_PROGRAM_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    command.env("TERM_PROGRAM_VERSION", term_program_version);
    if std::env::var("COLORTERM").is_err() {
        command.env("COLORTERM", "truecolor");
    }
}

fn resolve_child_term(navix_child_term: Option<&str>, host_term: Option<&str>) -> String {
    if let Some(value) = navix_child_term
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return value.to_string();
    }
    if let Some(value) = host_term.map(str::trim).filter(|value| !value.is_empty()) {
        return value.to_string();
    }
    "xterm-256color".to_string()
}

#[cfg(test)]
pub(crate) fn resolve_child_term_for_test(
    navix_child_term: Option<&str>,
    host_term: Option<&str>,
) -> String {
    resolve_child_term(navix_child_term, host_term)
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
