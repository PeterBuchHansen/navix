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

pub(crate) fn apply_alt_screen_chunk(current_state: bool, tail: &mut Vec<u8>, chunk: &[u8]) -> bool {
    const ALT_SEQ_MAX_LEN: usize = 8;
    let mut merged = Vec::with_capacity(tail.len() + chunk.len());
    merged.extend_from_slice(tail);
    merged.extend_from_slice(chunk);
    let next_state = alt_screen_event_from_stream(&merged).unwrap_or(current_state);
    let keep = ALT_SEQ_MAX_LEN.saturating_sub(1);
    if merged.len() > keep {
        *tail = merged[merged.len() - keep..].to_vec();
    } else {
        *tail = merged;
    }
    next_state
}

pub(crate) fn alt_screen_event_from_stream(stream: &[u8]) -> Option<bool> {
    const ENTER_SEQS: [&[u8]; 3] = [b"\x1b[?1049h", b"\x1b[?1047h", b"\x1b[?47h"];
    const EXIT_SEQS: [&[u8]; 3] = [b"\x1b[?1049l", b"\x1b[?1047l", b"\x1b[?47l"];
    let mut last: Option<(usize, bool)> = None;
    for idx in 0..stream.len() {
        for seq in ENTER_SEQS {
            if stream[idx..].starts_with(seq) {
                last = Some((idx, true));
            }
        }
        for seq in EXIT_SEQS {
            if stream[idx..].starts_with(seq) {
                last = Some((idx, false));
            }
        }
    }
    last.map(|(_, state)| state)
}
