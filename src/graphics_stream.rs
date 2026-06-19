// navix - incremental graphics/control stream parsing for PTY output.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphicsProtocol {
    Kitty,
    Iterm2Inline,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphicsEvent {
    pub(crate) protocol: GraphicsProtocol,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) enum StreamEvent {
    Text(Vec<u8>),
    Graphics(GraphicsEvent),
    /// A complete OSC 52 clipboard *set* sequence emitted by the child. It is
    /// forwarded verbatim to the host terminal (which owns the system
    /// clipboard) instead of being fed to the local emulator, which would
    /// silently drop it. Clipboard *queries* (`Pd == "?"`) are intentionally not
    /// surfaced here so the host's reply never lands in Navix's key input.
    Clipboard(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    EscapePending,
    Apc { esc_pending: bool },
    Dcs { esc_pending: bool },
    Osc { esc_pending: bool },
}

pub(crate) struct GraphicsStreamParser {
    mode: Mode,
    text_buf: Vec<u8>,
    seq_buf: Vec<u8>,
}

impl GraphicsStreamParser {
    pub(crate) fn new() -> Self {
        Self {
            mode: Mode::Normal,
            text_buf: Vec::new(),
            seq_buf: Vec::new(),
        }
    }

    pub(crate) fn push_chunk(&mut self, chunk: &[u8]) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        let mut idx = 0usize;
        while idx < chunk.len() {
            match self.mode {
                Mode::Normal => {
                    // Bulk-copy the run of plain bytes up to the next ESC instead
                    // of pushing one byte at a time. A multi-megabyte image is the
                    // hot path here; per-byte work made parsing it cost seconds.
                    let rest = &chunk[idx..];
                    match memchr(0x1b, rest) {
                        Some(pos) => {
                            self.text_buf.extend_from_slice(&rest[..pos]);
                            self.flush_text(&mut out);
                            self.mode = Mode::EscapePending;
                            idx = idx.saturating_add(pos).saturating_add(1);
                        },
                        None => {
                            self.text_buf.extend_from_slice(rest);
                            idx = chunk.len();
                        },
                    }
                },
                Mode::EscapePending => {
                    let byte = chunk[idx];
                    match byte {
                        b'_' => {
                            self.seq_buf.clear();
                            self.seq_buf.extend_from_slice(b"\x1b_");
                            self.mode = Mode::Apc { esc_pending: false };
                        },
                        b'P' => {
                            self.seq_buf.clear();
                            self.seq_buf.extend_from_slice(b"\x1bP");
                            self.mode = Mode::Dcs { esc_pending: false };
                        },
                        b']' => {
                            self.seq_buf.clear();
                            self.seq_buf.extend_from_slice(b"\x1b]");
                            self.mode = Mode::Osc { esc_pending: false };
                        },
                        _ => {
                            self.text_buf.push(0x1b);
                            self.text_buf.push(byte);
                            self.mode = Mode::Normal;
                        },
                    }
                    idx = idx.saturating_add(1);
                },
                Mode::Apc { esc_pending } | Mode::Dcs { esc_pending } => {
                    let is_dcs = matches!(self.mode, Mode::Dcs { .. });
                    if esc_pending {
                        // Previous byte was an ESC (already in seq_buf); the ST
                        // terminator is ESC `\`. Anything else (including ESC ESC,
                        // an escaped ESC in tmux passthrough) just clears pending.
                        let byte = chunk[idx];
                        self.seq_buf.push(byte);
                        if byte == b'\\' {
                            self.finish_sequence(&mut out);
                            self.mode = Mode::Normal;
                        } else {
                            self.mode = mode_with_pending(is_dcs, false);
                        }
                        idx = idx.saturating_add(1);
                    } else {
                        let rest = &chunk[idx..];
                        match memchr(0x1b, rest) {
                            Some(pos) => {
                                self.seq_buf.extend_from_slice(&rest[..=pos]);
                                self.mode = mode_with_pending(is_dcs, true);
                                idx = idx.saturating_add(pos).saturating_add(1);
                            },
                            None => {
                                self.seq_buf.extend_from_slice(rest);
                                idx = chunk.len();
                            },
                        }
                    }
                },
                Mode::Osc { esc_pending } => {
                    if esc_pending {
                        let byte = chunk[idx];
                        self.seq_buf.push(byte);
                        if byte == 0x07 || byte == b'\\' {
                            self.finish_sequence(&mut out);
                            self.mode = Mode::Normal;
                        } else {
                            self.mode = Mode::Osc { esc_pending: false };
                        }
                        idx = idx.saturating_add(1);
                    } else {
                        // OSC terminates on BEL or ST (ESC `\`).
                        let rest = &chunk[idx..];
                        match rest.iter().position(|&b| b == 0x07 || b == 0x1b) {
                            Some(pos) => {
                                let terminator = rest[pos];
                                self.seq_buf.extend_from_slice(&rest[..=pos]);
                                idx = idx.saturating_add(pos).saturating_add(1);
                                if terminator == 0x07 {
                                    self.finish_sequence(&mut out);
                                    self.mode = Mode::Normal;
                                } else {
                                    self.mode = Mode::Osc { esc_pending: true };
                                }
                            },
                            None => {
                                self.seq_buf.extend_from_slice(rest);
                                idx = chunk.len();
                            },
                        }
                    }
                },
            }
        }
        self.flush_text(&mut out);
        out
    }

    fn flush_text(&mut self, out: &mut Vec<StreamEvent>) {
        if self.text_buf.is_empty() {
            return;
        }
        let mut bytes = Vec::new();
        std::mem::swap(&mut bytes, &mut self.text_buf);
        out.push(StreamEvent::Text(bytes));
    }

    fn finish_sequence(&mut self, out: &mut Vec<StreamEvent>) {
        if self.seq_buf.is_empty() {
            return;
        }
        let mut seq = Vec::new();
        std::mem::swap(&mut seq, &mut self.seq_buf);
        if is_osc52_clipboard_set(&seq) {
            out.push(StreamEvent::Clipboard(seq));
            return;
        }
        if let Some(protocol) = detect_graphics_protocol(&seq) {
            out.push(StreamEvent::Graphics(GraphicsEvent {
                protocol,
                bytes: seq,
            }));
            return;
        }
        if let Some(decoded_passthrough) = decode_tmux_dcs_passthrough(&seq)
            && let Some(protocol) = detect_graphics_protocol(&decoded_passthrough)
        {
            out.push(StreamEvent::Graphics(GraphicsEvent {
                protocol,
                bytes: decoded_passthrough,
            }));
            return;
        }
        out.push(StreamEvent::Text(seq));
    }
}

/// Index of the first `needle` byte in `haystack`. A plain `position` scan that
/// the compiler vectorizes in release builds — fast enough for the per-chunk hot
/// path without pulling in an external dependency.
fn memchr(needle: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(|&byte| byte == needle)
}

fn mode_with_pending(is_dcs: bool, esc_pending: bool) -> Mode {
    if is_dcs {
        Mode::Dcs { esc_pending }
    } else {
        Mode::Apc { esc_pending }
    }
}

/// True when `seq` is a complete OSC 52 clipboard *set* sequence
/// (`ESC ] 52 ; Pc ; Pd` terminated by BEL or ST) whose data payload `Pd` is
/// real base64 rather than the `?` read query. Such sequences must be forwarded
/// to the host so a nested tool's "copy" reaches the system clipboard, exactly
/// as it would in a bare terminal. Queries return false: forwarding them would
/// route the host's clipboard reply into Navix's stdin.
fn is_osc52_clipboard_set(seq: &[u8]) -> bool {
    let Some(body) = seq.strip_prefix(b"\x1b]52;") else {
        return false;
    };
    let body = body
        .strip_suffix(b"\x07")
        .or_else(|| body.strip_suffix(b"\x1b\\"))
        .unwrap_or(body);
    // Body is `Pc ; Pd`; the data payload is everything after the first `;`.
    let Some(sep) = body.iter().position(|&byte| byte == b';') else {
        return false;
    };
    let data = &body[sep + 1..];
    !data.is_empty() && data != b"?"
}

fn detect_graphics_protocol(seq: &[u8]) -> Option<GraphicsProtocol> {
    if seq.starts_with(b"\x1b_G") {
        return Some(GraphicsProtocol::Kitty);
    }
    if seq.starts_with(b"\x1b]1337;File=") {
        return Some(GraphicsProtocol::Iterm2Inline);
    }
    None
}

fn decode_tmux_dcs_passthrough(seq: &[u8]) -> Option<Vec<u8>> {
    if !seq.starts_with(b"\x1bPtmux;") || !seq.ends_with(b"\x1b\\") {
        return None;
    }
    let mut idx = b"\x1bPtmux;".len();
    let end = seq.len().saturating_sub(2);
    let mut decoded = Vec::with_capacity(end.saturating_sub(idx));
    while idx < end {
        let byte = seq[idx];
        if byte == 0x1b && idx + 1 < end && seq[idx + 1] == 0x1b {
            decoded.push(0x1b);
            idx = idx.saturating_add(2);
            continue;
        }
        decoded.push(byte);
        idx = idx.saturating_add(1);
    }
    Some(decoded)
}
