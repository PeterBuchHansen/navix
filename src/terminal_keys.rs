// navix - terminal key encoding helpers for PTY input forwarding.
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

use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

fn csi_modifier_param(modifiers: KeyModifiers) -> Option<u8> {
    let mut value = 1_u8;
    if modifiers.contains(KeyModifiers::SHIFT) {
        value = value.saturating_add(1);
    }
    if modifiers.contains(KeyModifiers::ALT) {
        value = value.saturating_add(2);
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        value = value.saturating_add(4);
    }
    (value > 1).then_some(value)
}

fn csi_letter_sequence(letter: u8, modifiers: KeyModifiers) -> Vec<u8> {
    if let Some(modifier) = csi_modifier_param(modifiers) {
        format!("\x1b[1;{modifier}{}", letter as char).into_bytes()
    } else {
        vec![0x1b, b'[', letter]
    }
}

fn csi_tilde_sequence(code: u16, modifiers: KeyModifiers) -> Vec<u8> {
    if let Some(modifier) = csi_modifier_param(modifiers) {
        format!("\x1b[{code};{modifier}~").into_bytes()
    } else {
        format!("\x1b[{code}~").into_bytes()
    }
}

fn function_key_sequence(number: u8, modifiers: KeyModifiers) -> Option<Vec<u8>> {
    match number {
        1..=4 => {
            let letter = match number {
                1 => b'P',
                2 => b'Q',
                3 => b'R',
                _ => b'S',
            };
            if let Some(modifier) = csi_modifier_param(modifiers) {
                Some(format!("\x1b[1;{modifier}{}", letter as char).into_bytes())
            } else {
                Some(vec![0x1b, b'O', letter])
            }
        },
        5 => Some(csi_tilde_sequence(15, modifiers)),
        6 => Some(csi_tilde_sequence(17, modifiers)),
        7 => Some(csi_tilde_sequence(18, modifiers)),
        8 => Some(csi_tilde_sequence(19, modifiers)),
        9 => Some(csi_tilde_sequence(20, modifiers)),
        10 => Some(csi_tilde_sequence(21, modifiers)),
        11 => Some(csi_tilde_sequence(23, modifiers)),
        12 => Some(csi_tilde_sequence(24, modifiers)),
        _ => None,
    }
}

pub(crate) fn terminal_key_bytes(code: KeyCode, modifiers: KeyModifiers) -> Vec<u8> {
    match code {
        KeyCode::Char(ch) => {
            let mut bytes: Vec<u8> = Vec::new();
            if modifiers.contains(KeyModifiers::ALT) {
                bytes.push(0x1b);
            }
            if modifiers.contains(KeyModifiers::CONTROL) {
                let lower = ch.to_ascii_lowercase() as u8;
                bytes.push(lower & 0x1f);
            } else {
                let mut tmp = [0u8; 4];
                let encoded = ch.encode_utf8(&mut tmp);
                bytes.extend_from_slice(encoded.as_bytes());
            }
            bytes
        },
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => {
            if modifiers.contains(KeyModifiers::ALT) {
                vec![0x1b, 0x7f]
            } else {
                vec![0x7f]
            }
        },
        KeyCode::Tab => {
            if modifiers.contains(KeyModifiers::SHIFT) {
                vec![0x1b, b'[', b'Z']
            } else {
                vec![b'\t']
            }
        },
        KeyCode::BackTab => vec![0x1b, b'[', b'Z'],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => csi_letter_sequence(b'A', modifiers),
        KeyCode::Down => csi_letter_sequence(b'B', modifiers),
        KeyCode::Right => csi_letter_sequence(b'C', modifiers),
        KeyCode::Left => csi_letter_sequence(b'D', modifiers),
        KeyCode::Home => csi_letter_sequence(b'H', modifiers),
        KeyCode::End => csi_letter_sequence(b'F', modifiers),
        KeyCode::Insert => csi_tilde_sequence(2, modifiers),
        KeyCode::Delete => csi_tilde_sequence(3, modifiers),
        KeyCode::PageUp => csi_tilde_sequence(5, modifiers),
        KeyCode::PageDown => csi_tilde_sequence(6, modifiers),
        KeyCode::F(number) => function_key_sequence(number, modifiers).unwrap_or_default(),
        _ => Vec::new(),
    }
}

pub(crate) fn terminal_mouse_bytes(mouse: MouseEvent) -> Vec<u8> {
    let modifier_flags = mouse_modifier_flags(mouse.modifiers);
    let (base_code, suffix) = match mouse.kind {
        MouseEventKind::Down(button) => (mouse_button_code(button).unwrap_or(0), 'M'),
        MouseEventKind::Up(_) => (3, 'm'),
        MouseEventKind::Drag(button) => (32 + mouse_button_code(button).unwrap_or(0), 'M'),
        MouseEventKind::Moved => (35, 'M'),
        MouseEventKind::ScrollUp => (64, 'M'),
        MouseEventKind::ScrollDown => (65, 'M'),
        MouseEventKind::ScrollLeft => (66, 'M'),
        MouseEventKind::ScrollRight => (67, 'M'),
    };
    let code = base_code + modifier_flags;
    let col = mouse.column.saturating_add(1);
    let row = mouse.row.saturating_add(1);
    format!("\x1b[<{code};{col};{row}{suffix}").into_bytes()
}

fn mouse_button_code(button: MouseButton) -> Option<u16> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
    }
}

fn mouse_modifier_flags(modifiers: KeyModifiers) -> u16 {
    let mut value = 0;
    if modifiers.contains(KeyModifiers::SHIFT) {
        value += 4;
    }
    if modifiers.contains(KeyModifiers::ALT) {
        value += 8;
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        value += 16;
    }
    value
}
