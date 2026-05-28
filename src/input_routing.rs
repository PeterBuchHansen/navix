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

pub(crate) fn run_selected_file_command_shortcut(
    app: &mut App,
    shortcut: char,
) -> io::Result<bool> {
    let selected_path = app
        .nav_entries
        .get(app.nav_selected)
        .map(|entry| entry.path.clone());
    let Some(action) = navigation_file_command_action(
        app.nav_entries.get(app.nav_selected),
        shortcut,
        &app.config_state,
        &app.editor_program,
        &app.effective_identity,
    ) else {
        return Ok(false);
    };
    match action {
        NavigationFileCommandAction::RunReadInPreview(command) => {
            app.run_preview_command_overlay(&command, PreviewCommandMode::Read)?;
            if let Some(path) = selected_path.as_deref() {
                app.record_preview_jump_history(path);
            }
        },
        NavigationFileCommandAction::RunWriteInPreview(command) => {
            app.run_preview_command_overlay(&command, PreviewCommandMode::Write)?;
            if let Some(path) = selected_path.as_deref() {
                app.record_preview_jump_history(path);
            }
        },
        NavigationFileCommandAction::PrefillShell(command) => {
            app.focus_shell_with_prefilled_command(&command)?;
            if let Some(path) = selected_path.as_deref() {
                app.record_preview_jump_history(path);
            }
        },
    }
    Ok(true)
}

pub(crate) fn escape_prefix_release_update(
    pending_alt: bool,
    pending_alt_shortcut_armed: bool,
    key_code: KeyCode,
    key_kind: KeyEventKind,
) -> (bool, bool, bool) {
    if key_kind == KeyEventKind::Release && key_code == KeyCode::Esc {
        if pending_alt_shortcut_armed {
            return (pending_alt, pending_alt_shortcut_armed, true);
        }
        return (false, false, true);
    }
    (pending_alt, pending_alt_shortcut_armed, false)
}

pub(crate) fn escape_prefix_shortcut_char(
    shortcut_armed: bool,
    key_code: KeyCode,
    modifiers: KeyModifiers,
) -> Option<char> {
    if !shortcut_armed {
        return None;
    }
    let KeyCode::Char(ch) = key_code else {
        return None;
    };
    let normalized = if ch == '/' && modifiers.contains(KeyModifiers::SHIFT) {
        '?'
    } else {
        ch
    };
    let lowered = normalized.to_ascii_lowercase();
    matches!(lowered, '0' | '1' | '2' | 'c' | 'f' | 'r' | 'w' | 'x' | '?').then_some(lowered)
}

pub(crate) fn escape_prefix_arm_shortcut(key_code: KeyCode, modifiers: KeyModifiers) -> bool {
    key_code == KeyCode::Esc && (modifiers.is_empty() || modifiers == KeyModifiers::SHIFT)
}

pub(crate) fn navigation_file_shortcut_char(key_code: KeyCode) -> Option<char> {
    let KeyCode::Char(ch) = key_code else {
        return None;
    };
    let lowered = ch.to_ascii_lowercase();
    matches!(lowered, 'r' | 'w' | 'x').then_some(lowered)
}

pub(crate) fn navigation_filter_char(key_code: KeyCode, modifiers: KeyModifiers) -> Option<char> {
    if !(modifiers.is_empty() || modifiers == KeyModifiers::SHIFT) {
        return None;
    }
    let KeyCode::Char(ch) = key_code else {
        return None;
    };
    (ch.is_ascii_graphic() && ch != '/').then_some(ch)
}

pub(crate) fn navigation_pending_shortcut_to_filter_char_on_release(
    pending_shortcut: Option<char>,
    key_code: KeyCode,
    key_kind: KeyEventKind,
) -> Option<char> {
    if key_kind != KeyEventKind::Release {
        return None;
    }
    let KeyCode::Char(ch) = key_code else {
        return None;
    };
    pending_shortcut
        .filter(|pending| ch.eq_ignore_ascii_case(pending))
        .map(|_| ch)
}

pub(crate) fn navigation_should_ignore_pending_shortcut_event(
    pending_shortcut: Option<char>,
    key_code: KeyCode,
    key_kind: KeyEventKind,
) -> bool {
    let Some(pending) = pending_shortcut else {
        return false;
    };
    if key_kind == KeyEventKind::Repeat {
        return true;
    }
    matches!(key_code, KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&pending))
}

pub(crate) fn navigation_backspace_parent_ready_after_pop(
    filter_is_empty_after_pop: bool,
    parent_ready: bool,
) -> bool {
    if filter_is_empty_after_pop {
        false
    } else {
        parent_ready
    }
}

pub(crate) fn navigation_clear_filter_shortcut(key_code: KeyCode, modifiers: KeyModifiers) -> bool {
    modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key_code, KeyCode::Backspace | KeyCode::Delete)
}

pub(crate) fn terminal_prefers_command_copy() -> bool {
    let term_program = std::env::var("TERM_PROGRAM").ok();
    let ostype = std::env::var("OSTYPE").ok();
    terminal_prefers_command_copy_from_env(term_program.as_deref(), ostype.as_deref())
}

pub(crate) fn terminal_prefers_command_copy_from_env(
    term_program: Option<&str>,
    ostype: Option<&str>,
) -> bool {
    if cfg!(target_os = "macos") {
        return true;
    }
    if ostype.is_some_and(|value| value.to_ascii_lowercase().contains("darwin")) {
        return true;
    }
    term_program.is_some_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "apple_terminal" | "iterm.app"
        )
    })
}

pub(crate) fn copy_selection_shortcut(
    key_code: KeyCode,
    modifiers: KeyModifiers,
    command_copy_enabled: bool,
) -> bool {
    let KeyCode::Char(ch) = key_code else {
        return false;
    };
    if ch.to_ascii_lowercase() != 'c' {
        return false;
    }
    let ctrl_copy = modifiers.contains(KeyModifiers::CONTROL)
        && !modifiers.contains(KeyModifiers::ALT)
        && !modifiers.contains(KeyModifiers::SUPER)
        && (modifiers.contains(KeyModifiers::SHIFT) || ch == 'C');
    let command_c = command_copy_enabled
        && modifiers.contains(KeyModifiers::SUPER)
        && !modifiers.contains(KeyModifiers::ALT);
    ctrl_copy || command_c
}

pub(crate) fn should_clear_mouse_selection_for_key(
    key_code: KeyCode,
    copy_shortcut: bool,
    preview_ctrl_c_copy: bool,
) -> bool {
    if copy_shortcut || preview_ctrl_c_copy {
        return false;
    }
    !matches!(key_code, KeyCode::Modifier(_))
}

pub(crate) fn navigation_enter_file_shortcut(pending_shortcut: Option<char>) -> char {
    pending_shortcut.unwrap_or('r')
}

pub(crate) fn shell_pending_input_after_key(
    has_pending_input: bool,
    key_code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    if modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(ch) = key_code {
            return match ch.to_ascii_lowercase() {
                // Ctrl+c and Enter both execute/cancel the current line.
                'c' => false,
                // Ctrl+u / Ctrl+k clear all text before/after cursor.
                'u' | 'k' => false,
                _ => has_pending_input,
            };
        }
    }
    match key_code {
        KeyCode::Enter => false,
        // History navigation usually replaces the prompt line with a command.
        KeyCode::Up => true,
        // Keep state on Down: it can move toward empty prompt or another command.
        KeyCode::Down => has_pending_input,
        // Printable chars likely mean in-progress input.
        KeyCode::Char(_) if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT => true,
        // Backspace/Delete may still leave text; keep current state.
        KeyCode::Backspace | KeyCode::Delete => has_pending_input,
        _ => has_pending_input,
    }
}

#[cfg(test)]
pub(crate) fn preview_shortcut_target(
    _selected_entry: Option<&NavEntry>,
    _preview_mode: PreviewMode,
) -> ActivePane {
    ActivePane::Preview
}

pub(crate) fn panel_click_focus_target(
    clicked_pane: ActivePane,
    _preview_overlay_active: bool,
    _selected_entry: Option<&NavEntry>,
    _preview_mode: PreviewMode,
) -> ActivePane {
    clicked_pane
}

pub(crate) fn panel_areas_for_focus_click(
    terminal_area: Rect,
    active: ActivePane,
    shell_fullish_toggle: bool,
    shell_alt_screen_active: bool,
    nav_fullish: bool,
    preview_overlay_active: bool,
) -> (Rect, Rect, Rect) {
    let auto_fullish = active == ActivePane::Shell && shell_alt_screen_active;
    let shell_fullish_mode = shell_fullish_toggle || auto_fullish;
    let frame_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(terminal_area);
    let main_area = frame_rows[0];
    let shell_height = shell_panel_height(main_area.height, active, shell_fullish_mode);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(shell_height)])
        .split(main_area);
    let nav_fullish_mode = !preview_overlay_active
        && nav_fullish
        && matches!(active, ActivePane::Navigation | ActivePane::Preview);
    let preview_fullish_mode = preview_overlay_active;
    let cols = split_navigation_preview_cols(rows[0], nav_fullish_mode, preview_fullish_mode);
    (cols[0], cols[1], rows[1])
}

fn rect_contains_point(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

pub(crate) fn pane_from_mouse_position(
    column: u16,
    row: u16,
    nav_area: Rect,
    preview_area: Rect,
    shell_area: Rect,
) -> Option<ActivePane> {
    if rect_contains_point(nav_area, column, row) {
        Some(ActivePane::Navigation)
    } else if rect_contains_point(preview_area, column, row) {
        Some(ActivePane::Preview)
    } else if rect_contains_point(shell_area, column, row) {
        Some(ActivePane::Shell)
    } else {
        None
    }
}

pub(crate) fn mouse_event_relative_to_panel(
    mouse: crossterm::event::MouseEvent,
    panel_inner: Rect,
) -> Option<crossterm::event::MouseEvent> {
    if !rect_contains_point(panel_inner, mouse.column, mouse.row) {
        return None;
    }
    Some(crossterm::event::MouseEvent {
        kind: mouse.kind,
        column: mouse.column.saturating_sub(panel_inner.x),
        row: mouse.row.saturating_sub(panel_inner.y),
        modifiers: mouse.modifiers,
    })
}
