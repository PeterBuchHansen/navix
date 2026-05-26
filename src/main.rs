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

use ansi_to_tui::IntoText;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use crossterm::{
    cursor::Show,
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        KeyboardEnhancementFlags, MouseButton, MouseEventKind, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Terminal,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

mod panel_layout;
mod app_state;
mod alt_screen;
mod config;
mod config_ui;
mod file_logic;
mod input_routing;
mod navigation;
mod runtime_helpers;
mod shell;
mod state;
mod terminal_runtime;
mod terminal_keys;
mod theme;
mod tui;

pub(crate) use panel_layout::split_navigation_preview_cols;
pub(crate) use alt_screen::apply_alt_screen_chunk;
#[cfg(test)]
pub(crate) use alt_screen::alt_screen_event_from_stream;
pub(crate) use config::{ConfigEditor, ConfigField, ConfigState, ExtensionCommandRule};
pub(crate) use config_ui::{
    config_field_value, config_panel_text, default_extension_rule, extension_validation_error_for_rule,
    first_duplicate_extension, first_empty_extension, next_available_extension_name,
    next_char_boundary, normalize_extension, previous_char_boundary, set_config_field,
};
#[cfg(test)]
pub(crate) use config_ui::duplicate_extension_for_rule;
pub(crate) use file_logic::{
    available_preview_file_commands, clamp_preview_depth, nav_long_listing,
    navigation_file_command_action, preview_content_for_selected_entry,
    preview_directory_entries, simple_permission_bits,
};
#[cfg(test)]
pub(crate) use file_logic::{
    effective_access_from_mode, kernel_effective_access_for_path, permission_bits,
    preview_directory_tree_lines, resolve_preview_command_template,
};
#[cfg(test)]
pub(crate) use file_logic::preview_file_preview_text;
pub(crate) use input_routing::{
    copy_selection_shortcut, escape_prefix_release_update, escape_prefix_shortcut_char,
    navigation_clear_filter_shortcut,
    navigation_enter_file_shortcut, navigation_file_shortcut_char, navigation_filter_char,
    mouse_event_relative_to_panel, pane_from_mouse_position, terminal_prefers_command_copy,
    panel_areas_for_focus_click, panel_click_focus_target, preview_shortcut_target,
    run_selected_file_command_shortcut,
};
#[cfg(test)]
pub(crate) use input_routing::terminal_prefers_command_copy_from_env;
pub(crate) use navigation::{
    clamp_nav_selection, nav_filter_matches, nav_max_scroll, nav_scroll_for_selection,
    nav_selection_after_filter,
    nav_select_end, nav_select_home, nav_select_next_wrap, nav_select_prev_wrap,
    nav_window_metrics, navigation_entries, navigation_panel_text, NavEntry,
};
pub(crate) use runtime_helpers::{
    child_mouse_capture_required, is_fullish_layout_state, next_preview_overlay_presentation,
    poll_timeout_for_drain,
    prefill_shell_input_bytes, preview_overlay_is_interactive,
    scrollbar_thumb_bounds, shell_panel_height, should_auto_close_preview_overlay,
    shell_output_burst_update, should_show_scrollbar, should_throttle_mouse_passthrough_redraw,
    visible_range,
};
#[cfg(test)]
pub(crate) use runtime_helpers::is_fullish_shell_mode;
#[cfg(test)]
pub(crate) use runtime_helpers::window_bounds;
#[cfg(test)]
pub(crate) use navigation::navigation_tree_lines;
#[cfg(test)]
pub(crate) use shell::{parse_scrollback_limit, resolve_scrollback_limit};
pub(crate) use shell::{
    shell_single_quote, ShellMetrics, ShellPane,
};
#[cfg(test)]
pub(crate) use shell::{
    bash_history_sync_prompt_command, default_history_file_for_shell, shell_program_name,
};
pub(crate) use state::{
    merge_output_drains, ActivePane, App, EffectiveAccess, EffectiveIdentity,
    NavigationFileCommandAction, OutputDrain, PanePoint, PanelSelection, PreviewCommandMode,
    PreviewMode, PreviewOverlayPresentation, RenderTextSnapshot,
};
pub(crate) use terminal_runtime::{ensure_editor_program, open_key_debug_log, to_io, TerminalGuard};
pub(crate) use terminal_keys::{terminal_key_bytes, terminal_mouse_bytes};
pub(crate) use theme::{
    nav_row_selected_style, nav_style_for_theme, navigation_name_style, LsColorsTheme,
};
#[cfg(test)]
pub(crate) use theme::sgr_to_style;
pub(crate) use tui::{
    append_key_with_white_slashes, border_style, centered_rect, dim_rendered_area,
    footer_meta, footer_meta_line, footer_shortcuts_line, inner, preview_panel_text, render_panel_status,
    should_use_fullish_theme, tab_title, truncate_to_width, nav_meta_compact_lines,
};
#[cfg(test)]
pub(crate) use tui::{preview_directory_panel_text, preview_file_commands_panel_text};
#[cfg(test)]
pub(crate) use tui::{compose_footer_line, footer_shortcuts};

const MAX_OUTPUT_CHUNKS_PER_TICK: usize = 512;
const MAX_OUTPUT_BYTES_PER_TICK: usize = 4 * 1024 * 1024;
#[cfg(test)]
const PREVIEW_FILE_MAX_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy)]
struct RuntimeOptions {
    navix_mouse_capture: bool,
}

fn parse_runtime_options() -> Result<RuntimeOptions, String> {
    let mut options = RuntimeOptions {
        navix_mouse_capture: true,
    };
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--no-mouse-capture" | "--no.mouse-capture" => {
                options.navix_mouse_capture = false;
            }
            "--help" | "-h" => {
                return Err("Usage: navix [--no-mouse-capture]".to_string());
            }
            _ => {
                return Err(format!(
                    "Unknown option: {arg}\nUsage: navix [--no-mouse-capture]"
                ));
            }
        }
    }
    Ok(options)
}

fn text_lines_plain(text: &Text<'_>) -> Vec<String> {
    text.lines
        .iter()
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
        .collect()
}

fn pane_rect_for_snapshot(snapshot: &RenderTextSnapshot, pane: ActivePane) -> Rect {
    match pane {
        ActivePane::Navigation => snapshot.nav_inner,
        ActivePane::Preview => snapshot.preview_inner,
        ActivePane::Shell => snapshot.shell_inner,
    }
}

fn pane_lines_for_snapshot<'a>(snapshot: &'a RenderTextSnapshot, pane: ActivePane) -> &'a [String] {
    match pane {
        ActivePane::Navigation => &snapshot.nav_lines,
        ActivePane::Preview => &snapshot.preview_lines,
        ActivePane::Shell => &snapshot.shell_lines,
    }
}

fn pane_inner_from_areas(pane: ActivePane, nav_area: Rect, preview_area: Rect, shell_area: Rect) -> Rect {
    match pane {
        ActivePane::Navigation => inner(nav_area),
        ActivePane::Preview => inner(preview_area),
        ActivePane::Shell => inner(shell_area),
    }
}

fn clamp_point_to_rect(column: u16, row: u16, rect: Rect) -> PanePoint {
    if rect.width == 0 || rect.height == 0 {
        return PanePoint { column: 0, row: 0 };
    }
    let min_col = rect.x;
    let max_col = rect.x.saturating_add(rect.width.saturating_sub(1));
    let min_row = rect.y;
    let max_row = rect.y.saturating_add(rect.height.saturating_sub(1));
    PanePoint {
        column: column.clamp(min_col, max_col),
        row: row.clamp(min_row, max_row),
    }
}

fn normalized_selection_points(selection: PanelSelection) -> (PanePoint, PanePoint) {
    if selection.start.row < selection.end.row
        || (selection.start.row == selection.end.row && selection.start.column <= selection.end.column)
    {
        (selection.start, selection.end)
    } else {
        (selection.end, selection.start)
    }
}

fn slice_chars(line: &str, start: usize, end: usize) -> String {
    if start >= end {
        return String::new();
    }
    line.chars().skip(start).take(end.saturating_sub(start)).collect()
}

fn selected_text_from_snapshot(selection: PanelSelection, snapshot: &RenderTextSnapshot) -> String {
    let rect = pane_rect_for_snapshot(snapshot, selection.pane);
    if rect.width == 0 || rect.height == 0 {
        return String::new();
    }
    let lines = pane_lines_for_snapshot(snapshot, selection.pane);
    if lines.is_empty() {
        return String::new();
    }
    let (start, end) = normalized_selection_points(selection);
    let rel_start_row = start.row.saturating_sub(rect.y) as usize;
    let rel_end_row = end.row.saturating_sub(rect.y) as usize;
    let rel_start_col = start.column.saturating_sub(rect.x) as usize;
    let rel_end_col = end.column.saturating_sub(rect.x) as usize;
    let mut out = Vec::new();
    for row in rel_start_row..=rel_end_row {
        let line = lines.get(row).map(String::as_str).unwrap_or("");
        let line_len = line.chars().count();
        let start_col = if row == rel_start_row {
            rel_start_col.min(line_len)
        } else {
            0
        };
        let end_col_exclusive = if row == rel_end_row {
            rel_end_col.saturating_add(1).min(line_len)
        } else {
            line_len
        };
        out.push(slice_chars(line, start_col, end_col_exclusive));
    }
    out.join("\n")
}

fn copy_text_to_clipboard(text: &str) -> io::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    let encoded = BASE64_STANDARD.encode(text.as_bytes());
    let mut stdout = io::stdout();
    write!(stdout, "\x1b]52;c;{encoded}\x07")?;
    stdout.flush()
}

fn highlight_selection(frame: &mut ratatui::Frame<'_>, selection: PanelSelection, snapshot: &RenderTextSnapshot) {
    let rect = pane_rect_for_snapshot(snapshot, selection.pane);
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let (start, end) = normalized_selection_points(selection);
    let start = clamp_point_to_rect(start.column, start.row, rect);
    let end = clamp_point_to_rect(end.column, end.row, rect);
    let selection_style = Style::default().add_modifier(Modifier::REVERSED);
    let buf = frame.buffer_mut();
    for row in start.row..=end.row {
        let row_start_col = if row == start.row { start.column } else { rect.x };
        let row_end_col = if row == end.row {
            end.column
        } else {
            rect.x.saturating_add(rect.width.saturating_sub(1))
        };
        for col in row_start_col..=row_end_col {
            buf[(col, row)].set_style(selection_style);
        }
    }
}

fn main() {
    let options = match parse_runtime_options() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return;
        }
    };
    if let Err(err) = run(options) {
        eprintln!("navix step1 error: {err}");
    }
}

fn run(options: RuntimeOptions) -> io::Result<()> {
    let editor_program = ensure_editor_program()?;
    let mut guard = TerminalGuard::enter(options.navix_mouse_capture)?;
    let mut app = App::new(editor_program)?;
    let command_copy_enabled = terminal_prefers_command_copy();
    let mut previous_fullish_layout = false;
    let mut shell_output_burst_active = false;
    let mut mouse_capture_enabled = options.navix_mouse_capture;
    let mut was_copy_first_mouse_passthrough = false;
    let mut force_draw_next_iteration = false;
    let mut last_navigation_click: Option<(usize, Instant)> = None;

    loop {
        app.tick_feedback();
        let shell_drain = app.shell.poll_output();
        let (next_shell_output_burst_active, refresh_navigation_now) = shell_output_burst_update(
            shell_output_burst_active,
            shell_drain.processed_chunks,
            app.shell.in_alt_screen(),
        );
        shell_output_burst_active = next_shell_output_burst_active;
        if refresh_navigation_now {
            app.nav_loaded = false;
        }
        let preview_drain = app.poll_preview_command_output();
        let child_mouse_mode = child_mouse_capture_required(
            app.active,
            app.shell.in_alt_screen(),
            app.preview_command_overlay_active,
            app.preview_command_overlay_presentation,
        );
        let copy_first_mouse_passthrough = !options.navix_mouse_capture && !child_mouse_mode;
        let desired_mouse_capture = child_mouse_mode || options.navix_mouse_capture;
        if desired_mouse_capture != mouse_capture_enabled {
            guard.set_mouse_capture(desired_mouse_capture)?;
            mouse_capture_enabled = desired_mouse_capture;
        }
        let drain = merge_output_drains(shell_drain, preview_drain);
        let throttle_copy_first_redraw = should_throttle_mouse_passthrough_redraw(
            copy_first_mouse_passthrough,
            was_copy_first_mouse_passthrough,
            drain,
            app.force_terminal_clear,
            force_draw_next_iteration,
        );
        let throttle_redraw = throttle_copy_first_redraw;
        was_copy_first_mouse_passthrough = copy_first_mouse_passthrough;
        let current_fullish_layout = is_fullish_layout_state(
            app.active,
            app.shell_fullish,
            app.shell.in_alt_screen(),
            app.nav_fullish,
            app.preview_command_overlay_active,
        );
        let left_fullish_layout = previous_fullish_layout && !current_fullish_layout;
        previous_fullish_layout = current_fullish_layout;
        if !throttle_redraw && (app.force_terminal_clear || left_fullish_layout) {
            guard.terminal.clear()?;
            app.force_terminal_clear = false;
        }

        if !throttle_redraw {
            guard.terminal.draw(|frame| {
            let size = frame.area();
            let auto_fullish = app.active == ActivePane::Shell && app.shell.in_alt_screen();
            let preview_overlay_active = app.preview_command_overlay_active;
            let preview_overlay_interactive =
                preview_overlay_is_interactive(app.preview_command_overlay_presentation);
            let shell_fullish_mode = app.shell_fullish || auto_fullish;
            let fullish_shell_theme = should_use_fullish_theme(app.active, auto_fullish);
            let panel_dim_theme =
                fullish_shell_theme || app.config_open || preview_overlay_interactive;
            let footer_dim_theme = fullish_shell_theme || preview_overlay_interactive;
            let frame_rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(size);
            let main_area = frame_rows[0];
            let footer_area = frame_rows[1];
            // Full-frame clear prevents stale glyph bleed across dynamic layout changes.
            frame.render_widget(Clear, main_area);
            frame.render_widget(Clear, footer_area);
            let shell_height = shell_panel_height(main_area.height, app.active, shell_fullish_mode);
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(shell_height)])
                .split(main_area);

            let preview_fullish_mode = preview_overlay_active;
            let nav_fullish_mode = !preview_fullish_mode
                && app.nav_fullish
                && matches!(app.active, ActivePane::Navigation | ActivePane::Preview);
            let cols = split_navigation_preview_cols(rows[0], nav_fullish_mode, preview_fullish_mode);
            let shell_block_area = rows[1];
            let nav_border = border_style(app.active == ActivePane::Navigation, panel_dim_theme);
            let preview_border = border_style(app.active == ActivePane::Preview, panel_dim_theme);
            let shell_border = border_style(app.active == ActivePane::Shell, panel_dim_theme);

            let nav_title = if app.nav_filter.is_empty() {
                "─[1]─Navigation".to_string()
            } else {
                format!("─[1]─Navigation─[Filter:{}]─", app.nav_filter)
            };
            let nav_block = Block::default()
                .title(tab_title(&nav_title, app.active == ActivePane::Navigation))
                .borders(Borders::ALL)
                .border_set(ratatui::symbols::border::ROUNDED)
                .border_style(nav_border);
            let preview_block = Block::default()
                .title(tab_title("─[2]─Preview", app.active == ActivePane::Preview))
                .borders(Borders::ALL)
                .border_set(ratatui::symbols::border::ROUNDED)
                .border_style(preview_border);
            let shell_inner = inner(shell_block_area);
            let shell_block = Block::default()
                .title(tab_title("─[0]─Shell", app.active == ActivePane::Shell))
                .borders(Borders::ALL)
                .border_set(ratatui::symbols::border::ROUNDED)
                .border_style(shell_border);
            let (shell_text, metrics) = app
                .shell
                .render_text_and_metrics(shell_inner.height.max(1), shell_inner.width.max(1));
            let shell_lines_plain = text_lines_plain(&shell_text);

            frame.render_widget(Clear, cols[0]);
            frame.render_widget(Clear, cols[1]);
            frame.render_widget(Clear, shell_block_area);
            frame.render_widget(nav_block, cols[0]);
            frame.render_widget(preview_block, cols[1]);
            frame.render_widget(shell_block, shell_block_area);

            let nav_inner = inner(cols[0]);
            let nav_cwd = app.shell.current_cwd();
            app.refresh_navigation(&nav_cwd);
            app.nav_selected = clamp_nav_selection(app.nav_selected, app.nav_entries.len());
            let nav_compact_meta_row_reserved = !nav_fullish_mode && cols[0].height >= 3;
            let nav_entry_viewport_rows = nav_inner.height.saturating_sub(2) as usize;
            app.nav_viewport_rows = nav_entry_viewport_rows;
            app.nav_scroll = nav_scroll_for_selection(
                app.nav_scroll,
                app.nav_selected,
                app.nav_entries.len(),
                app.nav_viewport_rows,
            );
            let (
                nav_window_start,
                nav_window_end,
                nav_shown_start,
                nav_shown_end,
                nav_total,
                nav_has_overflow,
                nav_scroll,
            ) = nav_window_metrics(
                app.nav_entries.len(),
                nav_entry_viewport_rows,
                app.nav_scroll,
            );
            app.nav_scroll = nav_scroll;
            let nav_text = navigation_panel_text(
                &nav_cwd,
                &app.nav_entries,
                app.nav_selected,
                nav_window_start,
                nav_window_end,
                &app.nav_colors,
                panel_dim_theme,
                app.nav_error.as_deref(),
            );
            let nav_lines_plain = text_lines_plain(&nav_text);
            frame.render_widget(Paragraph::new(nav_text), nav_inner);
            app.refresh_preview_panel();
            let preview_hovered_label = app
                .nav_entries
                .get(app.nav_selected)
                .map(|entry| entry.name.as_str());
            let preview_selected_entry = app.nav_entries.get(app.nav_selected);
            let preview_inner = inner(cols[1]);
            let mut preview_lines_plain = Vec::new();
            let preview_metrics = if preview_overlay_active {
                frame.render_widget(Clear, cols[1]);
                let overlay_preview_block = Block::default()
                    .title(tab_title("─[2]─Preview", app.active == ActivePane::Preview))
                    .borders(Borders::ALL)
                    .border_set(ratatui::symbols::border::ROUNDED)
                    .border_style(preview_border);
                frame.render_widget(overlay_preview_block, cols[1]);
                if let Some(session) = app.preview_command_shell.as_mut() {
                    let (preview_shell_text, metrics) = session
                        .render_text_and_metrics(preview_inner.height.max(1), preview_inner.width.max(1));
                    preview_lines_plain = text_lines_plain(&preview_shell_text);
                    // Avoid stale edge glyphs while terminal app redraws.
                    frame.render_widget(Clear, preview_inner);
                    frame.render_widget(Paragraph::new(preview_shell_text), preview_inner);
                    if let Some((cursor_row, cursor_col)) = session
                        .visible_cursor(preview_inner.height.max(1), preview_inner.width.max(1))
                    {
                        frame.set_cursor_position((
                            preview_inner.x.saturating_add(cursor_col),
                            preview_inner.y.saturating_add(cursor_row),
                        ));
                    }
                    metrics
                } else {
                    ShellMetrics {
                        shown_start: 1,
                        shown_end: 1,
                        total: 1,
                        has_overflow: false,
                    }
                }
            } else {
                let preview_text = preview_panel_text(
                    app.preview_mode,
                    &app.preview_cached_text,
                    app.preview_last_selected_path.as_deref(),
                    preview_selected_entry,
                    preview_hovered_label,
                    app.preview_depth,
                    &app.effective_identity,
                    &app.nav_colors,
                    &app.config_state,
                    &app.editor_program,
                    preview_inner.width,
                    preview_inner.height,
                    panel_dim_theme,
                );
                preview_lines_plain = text_lines_plain(&preview_text);
                let no_wrap_preview_fullish = app.active == ActivePane::Navigation
                    && app.nav_fullish
                    && app.preview_mode == PreviewMode::DirectoryTree;
                if no_wrap_preview_fullish {
                    frame.render_widget(Paragraph::new(preview_text), preview_inner);
                } else {
                    frame.render_widget(
                        Paragraph::new(preview_text).wrap(Wrap { trim: false }),
                        preview_inner,
                    );
                }
                ShellMetrics {
                    shown_start: 1,
                    shown_end: 1,
                    total: 1,
                    has_overflow: false,
                }
            };
            if preview_overlay_active
                && !preview_overlay_interactive
                && preview_metrics.has_overflow
                && cols[1].width > 0
                && cols[1].height > 2
            {
                let (thumb_top, thumb_bottom) = scrollbar_thumb_bounds(
                    preview_metrics.total,
                    preview_metrics.shown_start,
                    preview_metrics.shown_end,
                    cols[1].height.saturating_sub(2) as usize,
                );
                let border_x = cols[1].x.saturating_add(cols[1].width.saturating_sub(1));
                let bar_top_y = cols[1].y.saturating_add(1);
                let bar_bottom_y = cols[1].y.saturating_add(cols[1].height.saturating_sub(1));
                let buf = frame.buffer_mut();
                for row in thumb_top..thumb_bottom {
                    let y = bar_top_y.saturating_add(row as u16);
                    if y < bar_bottom_y {
                        buf[(border_x, y)].set_symbol("▐");
                    }
                }
            }
            let nav_meta = app.nav_meta_for_selection();
            if !nav_meta.is_empty() && cols[0].width > 4 && cols[0].height > 0 {
                let single_line_meta_style =
                    nav_style_for_theme(Style::default().fg(Color::White), panel_dim_theme);
                if !nav_compact_meta_row_reserved {
                    let max_chars = cols[0].width.saturating_sub(4) as usize;
                    let display = truncate_to_width(&nav_meta, max_chars);
                    let width = display.chars().count() as u16;
                    if width > 0 {
                        let nav_meta_rect = ratatui::layout::Rect {
                            x: cols[0].x.saturating_add(2),
                            y: cols[0].y.saturating_add(cols[0].height.saturating_sub(1)),
                            width,
                            height: 1,
                        };
                        frame.render_widget(
                            Paragraph::new(display).style(single_line_meta_style),
                            nav_meta_rect,
                        );
                    }
                } else {
                    let inner_width = cols[0].width.saturating_sub(2) as usize;
                    let (top_line, bottom_line) = nav_meta_compact_lines(&nav_meta, inner_width);
                    let top_rect = ratatui::layout::Rect {
                        x: cols[0].x.saturating_add(1),
                        y: cols[0].y.saturating_add(cols[0].height.saturating_sub(2)),
                        width: inner_width as u16,
                        height: 1,
                    };
                    let bottom_rect = ratatui::layout::Rect {
                        x: cols[0].x.saturating_add(1),
                        y: cols[0].y.saturating_add(cols[0].height.saturating_sub(1)),
                        width: inner_width as u16,
                        height: 1,
                    };
                    frame.render_widget(Paragraph::new(top_line).style(nav_border), top_rect);
                    frame.render_widget(Paragraph::new(bottom_line).style(nav_border), bottom_rect);
                    if cols[0].width > 1 && cols[0].height > 2 {
                        let tee_y = cols[0].y.saturating_add(cols[0].height.saturating_sub(2));
                        let left_x = cols[0].x;
                        let right_x = cols[0].x.saturating_add(cols[0].width.saturating_sub(1));
                        let buf = frame.buffer_mut();
                        buf[(left_x, tee_y)].set_symbol("├");
                        buf[(left_x, tee_y)].set_style(nav_border);
                        buf[(right_x, tee_y)].set_symbol("┤");
                        buf[(right_x, tee_y)].set_style(nav_border);
                    }
                }
            }
            if nav_has_overflow && cols[0].width > 0 && cols[0].height > 2 {
                let (thumb_top, thumb_bottom) = scrollbar_thumb_bounds(
                    nav_total,
                    nav_shown_start,
                    nav_shown_end,
                    cols[0].height.saturating_sub(2) as usize,
                );
                let border_x = cols[0].x.saturating_add(cols[0].width.saturating_sub(1));
                let bar_top_y = cols[0].y.saturating_add(1);
                let bar_bottom_y = cols[0].y.saturating_add(cols[0].height.saturating_sub(1));
                let buf = frame.buffer_mut();
                for row in thumb_top..thumb_bottom {
                    let y = bar_top_y.saturating_add(row as u16);
                    if y < bar_bottom_y {
                        buf[(border_x, y)].set_symbol("▐");
                    }
                }
            }
            render_panel_status(
                frame,
                cols[0],
                nav_shown_start,
                nav_shown_end,
                nav_total,
                nav_border,
                nav_has_overflow,
            );

            let shell_view = Paragraph::new(shell_text);
            frame.render_widget(shell_view, shell_inner);
            if app.active == ActivePane::Shell {
                if let Some((cursor_row, cursor_col)) =
                    app.shell.visible_cursor(shell_inner.height.max(1), shell_inner.width.max(1))
                {
                    frame.set_cursor_position((
                        shell_inner.x.saturating_add(cursor_col),
                        shell_inner.y.saturating_add(cursor_row),
                    ));
                }
            }

            if metrics.has_overflow && shell_block_area.width > 0 && shell_block_area.height > 2 {
                let (thumb_top, thumb_bottom) = scrollbar_thumb_bounds(
                    metrics.total,
                    metrics.shown_start,
                    metrics.shown_end,
                    shell_block_area.height.saturating_sub(2) as usize,
                );
                let border_x = shell_block_area
                    .x
                    .saturating_add(shell_block_area.width.saturating_sub(1));
                let bar_top_y = shell_block_area.y.saturating_add(1);
                let bar_bottom_y = shell_block_area
                    .y
                    .saturating_add(shell_block_area.height.saturating_sub(1));
                let buf = frame.buffer_mut();
                for row in thumb_top..thumb_bottom {
                    let y = bar_top_y.saturating_add(row as u16);
                    if y < bar_bottom_y {
                        buf[(border_x, y)].set_symbol("▐");
                    }
                }
            }
            render_panel_status(
                frame,
                shell_block_area,
                metrics.shown_start,
                metrics.shown_end,
                metrics.total,
                shell_border,
                metrics.has_overflow,
            );
            render_panel_status(
                frame,
                cols[1],
                preview_metrics.shown_start,
                preview_metrics.shown_end,
                preview_metrics.total,
                preview_border,
                preview_metrics.has_overflow,
            );

            frame.render_widget(Paragraph::new(" ".repeat(footer_area.width as usize)), footer_area);
            if footer_area.width > 2 {
                let inner_width = footer_area.width.saturating_sub(2);
                let right_text = footer_meta();
                let right_len = right_text.chars().count() as u16;
                if right_len >= inner_width {
                    let right_rect = ratatui::layout::Rect {
                        x: footer_area.x.saturating_add(1),
                        y: footer_area.y,
                        width: inner_width,
                        height: 1,
                    };
                    frame.render_widget(Paragraph::new(truncate_to_width(&right_text, inner_width as usize)), right_rect);
                } else {
                    let left_width = inner_width.saturating_sub(right_len.saturating_add(1));
                    if left_width > 0 {
                        let left_rect = ratatui::layout::Rect {
                            x: footer_area.x.saturating_add(1),
                            y: footer_area.y,
                            width: left_width,
                            height: 1,
                        };
                        frame.render_widget(
                            Paragraph::new(footer_shortcuts_line(
                                app.active,
                                footer_dim_theme,
                                app.config_open,
                                app.has_unsaved_config_changes(),
                                app.should_highlight_config_shortcut(),
                            )),
                            left_rect,
                        );
                    }
                    let right_rect = ratatui::layout::Rect {
                        x: footer_area
                            .x
                            .saturating_add(1)
                            .saturating_add(inner_width.saturating_sub(right_len)),
                        y: footer_area.y,
                        width: right_len,
                        height: 1,
                    };
                    frame.render_widget(Paragraph::new(footer_meta_line(footer_dim_theme)), right_rect);
                }
            }

            if app.config_open {
                dim_rendered_area(frame, main_area);
                let overlay = centered_rect(78, 70, main_area);
                frame.render_widget(Clear, overlay);
                let overlay_block = Block::default()
                    .title("─[c]─Config─extension─routing─(read/write/execute)────")
                    .borders(Borders::ALL)
                    .border_set(ratatui::symbols::border::ROUNDED)
                    .border_style(border_style(true, false));
                frame.render_widget(overlay_block, overlay);
                let overlay_inner = inner(overlay);
                let overlay_text = config_panel_text(
                    &app.config_state,
                    &app.saved_config_state,
                    &app.config_editor,
                    false,
                );
                frame.render_widget(
                    Paragraph::new(overlay_text).wrap(Wrap { trim: false }),
                    overlay_inner,
                );
            }
            let snapshot = RenderTextSnapshot {
                nav_inner,
                preview_inner,
                shell_inner,
                nav_lines: nav_lines_plain,
                preview_lines: preview_lines_plain,
                shell_lines: shell_lines_plain,
            };
            if let Some(selection) = app.mouse_selection
                && selection.dragged
            {
                highlight_selection(frame, selection, &snapshot);
            }
            app.render_snapshot = snapshot;
            })?;
            force_draw_next_iteration = false;
        }

        let input_poll_timeout = poll_timeout_for_drain(drain);
        if !event::poll(input_poll_timeout)? {
            continue;
        }
        let event = event::read()?;
        force_draw_next_iteration = true;
        if let Event::Mouse(mouse) = event {
            // Mouse interaction always cancels pending Esc-prefix state.
            app.pending_alt = false;
            app.pending_alt_shortcut_armed = false;
            app.nav_pending_file_shortcut = None;
            if app.config_open {
                continue;
            }
            let terminal_area: Rect = guard.terminal.size()?.into();
            let (nav_area, preview_area, shell_area) = panel_areas_for_focus_click(
                terminal_area,
                app.active,
                app.shell_fullish,
                app.shell.in_alt_screen(),
                app.nav_fullish,
                app.preview_command_overlay_active,
            );
            if child_mouse_mode {
                if app.active == ActivePane::Shell
                    && app.shell.in_alt_screen()
                    && let Some(local_mouse) = mouse_event_relative_to_panel(mouse, inner(shell_area))
                {
                    app.shell.send_mouse(local_mouse)?;
                    continue;
                }
                if app.active == ActivePane::Preview
                    && app.preview_command_overlay_active
                    && preview_overlay_is_interactive(app.preview_command_overlay_presentation)
                    && let Some(local_mouse) = mouse_event_relative_to_panel(mouse, inner(preview_area))
                    && let Some(session) = app.preview_command_shell.as_mut()
                {
                    session.send_mouse(local_mouse)?;
                    continue;
                }
            }
            if !options.navix_mouse_capture {
                continue;
            }
            match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => {
                    if let Some(selection) = app.mouse_selection.as_mut() {
                        let panel_inner = pane_inner_from_areas(
                            selection.pane,
                            nav_area,
                            preview_area,
                            shell_area,
                        );
                        if panel_inner.width > 0 && panel_inner.height > 0 {
                            let point = clamp_point_to_rect(mouse.column, mouse.row, panel_inner);
                            selection.end = point;
                            if point != selection.start {
                                selection.dragged = true;
                            }
                        }
                    }
                    continue;
                }
                MouseEventKind::Down(MouseButton::Left) => {}
                _ => {
                    continue;
                }
            }
            let Some(clicked_pane) = pane_from_mouse_position(
                mouse.column,
                mouse.row,
                nav_area,
                preview_area,
                shell_area,
            ) else {
                continue;
            };
            if clicked_pane == ActivePane::Preview && !app.preview_command_overlay_active {
                app.refresh_preview_panel();
            }
            let target = panel_click_focus_target(
                clicked_pane,
                app.preview_command_overlay_active,
                app.nav_entries.get(app.nav_selected),
                app.preview_mode,
            );
            if target != ActivePane::Preview {
                app.close_preview_command_overlay();
            }
            app.active = target;
            let target_inner = pane_inner_from_areas(target, nav_area, preview_area, shell_area);
            if target_inner.width > 0 && target_inner.height > 0 {
                let start = clamp_point_to_rect(mouse.column, mouse.row, target_inner);
                app.mouse_selection = Some(PanelSelection {
                    pane: target,
                    start,
                    end: start,
                    dragged: false,
                });
            } else {
                app.mouse_selection = None;
            }
            if clicked_pane != ActivePane::Navigation {
                last_navigation_click = None;
                continue;
            }
            let nav_inner = inner(nav_area);
            let nav_entry_viewport_rows = nav_inner.height.saturating_sub(2) as usize;
            let (window_start, window_end, _, _, _, _, _) =
                nav_window_metrics(app.nav_entries.len(), nav_entry_viewport_rows, app.nav_scroll);
            let first_entry_y = nav_inner.y.saturating_add(1);
            if mouse.row < first_entry_y {
                last_navigation_click = None;
                continue;
            }
            let line_idx = mouse.row.saturating_sub(first_entry_y) as usize;
            let clicked_index = window_start.saturating_add(line_idx);
            if clicked_index >= window_end {
                last_navigation_click = None;
                continue;
            }
            app.nav_selected = clicked_index;
            app.nav_scroll = nav_scroll_for_selection(
                app.nav_scroll,
                app.nav_selected,
                app.nav_entries.len(),
                app.nav_viewport_rows,
            );
            let now = Instant::now();
            let is_double_click = last_navigation_click
                .is_some_and(|(idx, at)| idx == clicked_index && now.duration_since(at) <= Duration::from_millis(350));
            if is_double_click {
                if let Some(entry) = app.nav_entries.get(app.nav_selected) {
                    if entry.is_dir {
                        app.nav_filter.clear();
                        app.shell.cd_to(&entry.path)?;
                        app.nav_loaded = false;
                        app.nav_selected = 0;
                        app.nav_scroll = 0;
                    } else {
                        let _ = run_selected_file_command_shortcut(&mut app, 'r')?;
                    }
                }
                last_navigation_click = None;
            } else {
                last_navigation_click = Some((clicked_index, now));
            }
            continue;
        }
        let Event::Key(key) = event else {
            continue;
        };
        app.log_key_debug_event("recv", Some(&key));
        let (
            next_pending_alt,
            next_pending_alt_shortcut_armed,
            consumed_by_release,
        ) =
            escape_prefix_release_update(
                app.pending_alt,
                app.pending_alt_shortcut_armed,
                key.code,
                key.kind,
            );
        app.pending_alt = next_pending_alt;
        app.pending_alt_shortcut_armed = next_pending_alt_shortcut_armed;
        app.log_key_debug_event("after_release_update", Some(&key));
        if consumed_by_release {
            app.log_key_debug_event("consumed_release", Some(&key));
            continue;
        }
        if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
            app.log_key_debug_event("ignored_non_press_repeat", Some(&key));
            continue;
        }
        if copy_selection_shortcut(key.code, key.modifiers, command_copy_enabled) {
            if let Some(selection) = app.mouse_selection.filter(|selection| selection.dragged) {
                let selected_text = selected_text_from_snapshot(selection, &app.render_snapshot);
                copy_text_to_clipboard(&selected_text)?;
                app.mouse_selection = None;
                continue;
            }
        } else {
            app.mouse_selection = None;
        }
        if app.active != ActivePane::Navigation {
            app.nav_pending_file_shortcut = None;
        }

        if app.config_open {
            app.nav_pending_file_shortcut = None;
            if app.config_editor.editing {
                match key.code {
                    KeyCode::Esc => {
                        app.config_editor.editing = false;
                        app.config_editor.clear_input();
                        app.config_editor.status_message = "edit canceled".to_string();
                        continue;
                    }
                    KeyCode::Enter => {
                        if app.config_editor.selected_field == ConfigField::Extension {
                            if let Some(error_message) = extension_validation_error_for_rule(
                                &app.config_state,
                                app.config_editor.selected_rule,
                                &app.config_editor.input_buffer,
                            ) {
                                app.config_editor.status_message = error_message;
                                continue;
                            }
                        }
                        if let Some(rule) =
                            app.config_state.extension_rules.get_mut(app.config_editor.selected_rule)
                        {
                            let value = app.config_editor.input_buffer.clone();
                            set_config_field(rule, app.config_editor.selected_field, &value);
                            app.config_editor.dirty = true;
                            app.config_editor.status_message = format!(
                                "updated {}",
                                app.config_editor.selected_field.label()
                            );
                        }
                        app.config_editor.editing = false;
                        app.config_editor.clear_input();
                        continue;
                    }
                    KeyCode::Backspace => {
                        app.config_editor.backspace();
                        continue;
                    }
                    KeyCode::Delete => {
                        app.config_editor.delete();
                        continue;
                    }
                    KeyCode::Left => {
                        app.config_editor.move_cursor_left();
                        continue;
                    }
                    KeyCode::Right => {
                        app.config_editor.move_cursor_right();
                        continue;
                    }
                    KeyCode::Home => {
                        app.config_editor.move_cursor_home();
                        continue;
                    }
                    KeyCode::End => {
                        app.config_editor.move_cursor_end();
                        continue;
                    }
                    KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.config_editor.move_cursor_home();
                        continue;
                    }
                    KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.config_editor.move_cursor_end();
                        continue;
                    }
                    KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.config_editor.insert_char(ch);
                        continue;
                    }
                    _ => continue,
                }
            }

            if key.modifiers.contains(KeyModifiers::CONTROL) {
                match key.code {
                    KeyCode::Char('s') => {
                        app.save_config();
                        continue;
                    }
                    KeyCode::Char('r') => {
                        app.reload_config();
                        continue;
                    }
                    KeyCode::Char('d') => {
                        app.discard_config_changes();
                        continue;
                    }
                    KeyCode::Char('n') => {
                        let extension = next_available_extension_name(&app.config_state, "new");
                        app.config_state
                            .extension_rules
                            .push(default_extension_rule(&extension));
                        app.config_editor.selected_rule =
                            app.config_state.extension_rules.len().saturating_sub(1);
                        app.config_editor.selected_field = ConfigField::Extension;
                        if let Some(rule) =
                            app.config_state.extension_rules.get(app.config_editor.selected_rule)
                        {
                            app.config_editor.set_input(rule.extension.clone());
                            app.config_editor.editing = true;
                        }
                        app.config_editor.dirty = true;
                        app.config_editor.status_message.clear();
                        continue;
                    }
                    KeyCode::Delete => {
                        app.delete_selected_extension_rule();
                        continue;
                    }
                    KeyCode::Backspace => {
                        app.delete_selected_extension_rule();
                        continue;
                    }
                    // Some terminals report Ctrl+Backspace as Ctrl+h.
                    KeyCode::Char('h') => {
                        app.delete_selected_extension_rule();
                        continue;
                    }
                    _ => {}
                }
            }

            match key.code {
                KeyCode::Esc => {
                    app.config_open = false;
                    app.config_editor.editing = false;
                    app.config_editor.clear_input();
                    app.pending_alt = false;
                    app.pending_alt_shortcut_armed = false;
                    continue;
                }
                KeyCode::Up if key.modifiers.is_empty() => {
                    app.config_editor.selected_rule = app.config_editor.selected_rule.saturating_sub(1);
                    app.config_editor.ensure_valid(&app.config_state);
                    continue;
                }
                KeyCode::Down if key.modifiers.is_empty() => {
                    if app.config_editor.selected_rule + 1 < app.config_state.extension_rules.len() {
                        app.config_editor.selected_rule += 1;
                    }
                    app.config_editor.ensure_valid(&app.config_state);
                    continue;
                }
                KeyCode::Left if key.modifiers.is_empty() => {
                    app.config_editor.selected_field = app.config_editor.selected_field.prev();
                    continue;
                }
                KeyCode::Right if key.modifiers.is_empty() => {
                    app.config_editor.selected_field = app.config_editor.selected_field.next();
                    continue;
                }
                KeyCode::Enter if key.modifiers.is_empty() => {
                    if app.config_state.extension_rules.is_empty() {
                        let extension = next_available_extension_name(&app.config_state, "new");
                        app.config_state
                            .extension_rules
                            .push(default_extension_rule(&extension));
                        app.config_editor.selected_rule = 0;
                        app.config_editor.selected_field = ConfigField::Extension;
                    }
                    if let Some(rule) =
                        app.config_state.extension_rules.get(app.config_editor.selected_rule)
                    {
                        app.config_editor.set_input(
                            config_field_value(rule, app.config_editor.selected_field).to_string(),
                        );
                        app.config_editor.editing = true;
                    }
                    continue;
                }
                _ => continue,
            }
        }

        if app.pending_alt {
            app.log_key_debug_event("pending_alt_enter", Some(&key));
            if app.active == ActivePane::Shell && !key.modifiers.contains(KeyModifiers::CONTROL) {
                match key.code {
                    KeyCode::Up => {
                        app.shell.scroll_up(1);
                        app.pending_alt = true;
                        app.pending_alt_shortcut_armed = false;
                        app.log_key_debug_event("pending_alt_shell_scroll_up", Some(&key));
                        continue;
                    }
                    KeyCode::Down => {
                        app.shell.scroll_down(1);
                        app.pending_alt = true;
                        app.pending_alt_shortcut_armed = false;
                        app.log_key_debug_event("pending_alt_shell_scroll_down", Some(&key));
                        continue;
                    }
                    _ => {}
                }
            }
            let armed = app.pending_alt_shortcut_armed;
            app.pending_alt = false;
            app.pending_alt_shortcut_armed = false;
            if let Some(shortcut) = escape_prefix_shortcut_char(armed, key.code) {
                match shortcut {
                    '0' => {
                        app.close_preview_command_overlay();
                        app.active = ActivePane::Shell;
                        app.log_key_debug_event("pending_alt_shortcut_0", Some(&key));
                        continue;
                    }
                    '1' => {
                        app.close_preview_command_overlay();
                        app.active = ActivePane::Navigation;
                        app.log_key_debug_event("pending_alt_shortcut_1", Some(&key));
                        continue;
                    }
                    '2' => {
                        app.refresh_preview_panel();
                        app.active = preview_shortcut_target(
                            app.nav_entries.get(app.nav_selected),
                            app.preview_mode,
                        );
                        if app.active != ActivePane::Preview {
                            app.close_preview_command_overlay();
                        }
                        app.log_key_debug_event("pending_alt_shortcut_2", Some(&key));
                        continue;
                    }
                    'c' => {
                        app.close_preview_command_overlay();
                        app.open_config();
                        app.log_key_debug_event("pending_alt_shortcut_c", Some(&key));
                        continue;
                    }
                    'f' => {
                        match app.active {
                            ActivePane::Shell => {
                                app.shell_fullish = !app.shell_fullish;
                            }
                            ActivePane::Navigation | ActivePane::Preview => {
                                app.nav_fullish = !app.nav_fullish;
                            }
                        }
                        app.log_key_debug_event("pending_alt_shortcut_f", Some(&key));
                        continue;
                    }
                    'r' | 'w' | 'x' => {
                        if run_selected_file_command_shortcut(&mut app, shortcut)? {
                            app.log_key_debug_event("pending_alt_shortcut_file_command", Some(&key));
                            continue;
                        }
                        if app.active == ActivePane::Shell {
                            app.shell.send_raw(&[0x1b])?;
                            app.log_key_debug_event(
                                "pending_alt_sent_literal_esc_shell_file_shortcut_fallback",
                                Some(&key),
                            );
                        }
                    }
                    _ => {}
                }
            } else if app.active == ActivePane::Shell {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    app.log_key_debug_event("pending_alt_cleared_before_ctrl_shell", Some(&key));
                } else {
                    app.shell.send_raw(&[0x1b])?;
                    app.log_key_debug_event("pending_alt_sent_literal_esc_shell", Some(&key));
                }
            } else if app.preview_command_overlay_active {
                if let Some(session) = app.preview_command_shell.as_mut() {
                    session.send_raw(&[0x1b])?;
                }
                app.log_key_debug_event("pending_alt_sent_literal_esc_preview", Some(&key));
            }
        }

        if app.preview_command_overlay_active {
            app.nav_pending_file_shortcut = None;
            if app.active != ActivePane::Preview {
                app.close_preview_command_overlay();
                app.active = ActivePane::Navigation;
                continue;
            }
            if app.preview_command_overlay_mode.is_none() {
                app.close_preview_command_overlay();
                app.active = ActivePane::Navigation;
                continue;
            }
            if let Some(session) = app.preview_command_shell.as_mut() {
                if preview_overlay_is_interactive(app.preview_command_overlay_presentation) {
                    session.send_key(key)?;
                    continue;
                }
                if key.code == KeyCode::Esc && key.modifiers.is_empty() {
                    app.close_preview_command_overlay();
                    app.active = ActivePane::Navigation;
                    continue;
                }
                if key.modifiers.is_empty() {
                    match key.code {
                        KeyCode::Up => {
                            session.scroll_up(1);
                            continue;
                        }
                        KeyCode::Down => {
                            session.scroll_down(1);
                            continue;
                        }
                        KeyCode::PageUp => {
                            session.scroll_up(session.page_rows());
                            continue;
                        }
                        KeyCode::PageDown => {
                            session.scroll_down(session.page_rows());
                            continue;
                        }
                        _ => {}
                    }
                }
            }
            continue;
        }

        if app.active == ActivePane::Shell {
            app.nav_pending_file_shortcut = None;
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('d') {
                if app.block_exit_attempt_if_unsaved() {
                    continue;
                }
                app.prepare_for_exit();
                break;
            }
            match key.code {
                KeyCode::PageUp => app.shell.scroll_up(app.shell.page_rows()),
                KeyCode::PageDown => app.shell.scroll_down(app.shell.page_rows()),
                KeyCode::Esc if key.modifiers.is_empty() => {
                    app.pending_alt = true;
                    app.pending_alt_shortcut_armed = true;
                    app.log_key_debug_event("arm_escape_prefix_shell", Some(&key));
                    continue;
                }
                KeyCode::Up | KeyCode::Down if key.modifiers.is_empty() => {
                    app.shell.jump_to_bottom();
                    app.shell.send_key(key)?;
                }
                _ => {
                    // Match terminal behavior: any input key returns to the live prompt view.
                    app.shell.jump_to_bottom();
                    app.shell.send_key(key)?;
                }
            }
            continue;
        }

        if app.active == ActivePane::Preview && key.modifiers.is_empty() {
            app.nav_pending_file_shortcut = None;
            match key.code {
                KeyCode::Char(' ') => {
                    if app.handle_preview_space_action() {
                        continue;
                    }
                }
                KeyCode::Left => {
                    app.decrease_preview_depth();
                    app.refresh_preview_panel();
                    continue;
                }
                KeyCode::Right => {
                    app.increase_preview_depth();
                    app.refresh_preview_panel();
                    continue;
                }
                _ => {}
            }
        }

        if key.code == KeyCode::Esc && key.modifiers.is_empty() {
            app.pending_alt = true;
            app.pending_alt_shortcut_armed = true;
            app.log_key_debug_event("arm_escape_prefix_global", Some(&key));
            continue;
        }

        if app.active == ActivePane::Navigation {
            app.nav_selected = clamp_nav_selection(app.nav_selected, app.nav_entries.len());
            if app.nav_pending_file_shortcut.is_some() && key.code != KeyCode::Enter {
                if let Some(pending) = app.nav_pending_file_shortcut.take()
                    && let Some(_) = navigation_filter_char(key.code, key.modifiers)
                {
                    app.append_navigation_filter_char(pending);
                } else {
                    app.nav_pending_file_shortcut = None;
                }
            }
            if navigation_clear_filter_shortcut(key.code, key.modifiers) {
                app.nav_pending_file_shortcut = None;
                app.clear_navigation_filter();
                continue;
            }
            if key.code == KeyCode::Backspace && key.modifiers.is_empty() {
                app.nav_pending_file_shortcut = None;
                app.pop_navigation_filter_char();
                continue;
            }
            if let Some(ch) = navigation_filter_char(key.code, key.modifiers) {
                let can_arm_shortcut = key.modifiers.is_empty()
                    && app.nav_filter.is_empty()
                    && navigation_file_shortcut_char(key.code)
                        .is_some_and(|shortcut| {
                            navigation_file_command_action(
                                app.nav_entries.get(app.nav_selected),
                                shortcut,
                                &app.config_state,
                                &app.editor_program,
                                &app.effective_identity,
                            )
                            .is_some()
                        });
                if can_arm_shortcut {
                    app.nav_pending_file_shortcut = navigation_file_shortcut_char(key.code);
                    continue;
                }
                app.nav_pending_file_shortcut = None;
                app.append_navigation_filter_char(ch);
                continue;
            }
            if key.modifiers.is_empty() {
                match key.code {
                    KeyCode::Char(' ') => {
                        app.nav_pending_file_shortcut = None;
                        if app.handle_preview_space_action() {
                            continue;
                        }
                    }
                    KeyCode::Up => {
                        app.nav_pending_file_shortcut = None;
                        app.nav_selected = nav_select_prev_wrap(app.nav_selected, app.nav_entries.len());
                        app.nav_scroll = nav_scroll_for_selection(
                            app.nav_scroll,
                            app.nav_selected,
                            app.nav_entries.len(),
                            app.nav_viewport_rows,
                        );
                        continue;
                    }
                    KeyCode::Down => {
                        app.nav_pending_file_shortcut = None;
                        app.nav_selected = nav_select_next_wrap(app.nav_selected, app.nav_entries.len());
                        app.nav_scroll = nav_scroll_for_selection(
                            app.nav_scroll,
                            app.nav_selected,
                            app.nav_entries.len(),
                            app.nav_viewport_rows,
                        );
                        continue;
                    }
                    KeyCode::Left | KeyCode::Home => {
                        app.nav_pending_file_shortcut = None;
                        app.nav_selected = nav_select_home(app.nav_selected, app.nav_entries.len());
                        app.nav_scroll = nav_scroll_for_selection(
                            app.nav_scroll,
                            app.nav_selected,
                            app.nav_entries.len(),
                            app.nav_viewport_rows,
                        );
                        continue;
                    }
                    KeyCode::Right | KeyCode::End => {
                        app.nav_pending_file_shortcut = None;
                        app.nav_selected = nav_select_end(app.nav_selected, app.nav_entries.len());
                        app.nav_scroll = nav_scroll_for_selection(
                            app.nav_scroll,
                            app.nav_selected,
                            app.nav_entries.len(),
                            app.nav_viewport_rows,
                        );
                        continue;
                    }
                    KeyCode::PageUp => {
                        app.nav_pending_file_shortcut = None;
                        let page = app.nav_viewport_rows.max(1);
                        app.nav_selected = app.nav_selected.saturating_sub(page);
                        app.nav_scroll = nav_scroll_for_selection(
                            app.nav_scroll,
                            app.nav_selected,
                            app.nav_entries.len(),
                            app.nav_viewport_rows,
                        );
                        continue;
                    }
                    KeyCode::PageDown => {
                        app.nav_pending_file_shortcut = None;
                        let page = app.nav_viewport_rows.max(1);
                        let max_selected = app.nav_entries.len().saturating_sub(1);
                        app.nav_selected = app.nav_selected.saturating_add(page).min(max_selected);
                        app.nav_scroll = nav_scroll_for_selection(
                            app.nav_scroll,
                            app.nav_selected,
                            app.nav_entries.len(),
                            app.nav_viewport_rows,
                        );
                        continue;
                    }
                    KeyCode::Enter => {
                        let pending_shortcut = app.nav_pending_file_shortcut.take();
                        if let Some(entry) = app.nav_entries.get(app.nav_selected) {
                            if entry.is_dir {
                                app.nav_filter.clear();
                                app.shell.cd_to(&entry.path)?;
                                app.nav_loaded = false;
                                app.nav_selected = 0;
                                app.nav_scroll = 0;
                            } else {
                                let shortcut = navigation_enter_file_shortcut(pending_shortcut);
                                let _ = run_selected_file_command_shortcut(&mut app, shortcut)?;
                            }
                        }
                        continue;
                    }
                    _ => {
                        app.nav_pending_file_shortcut = None;
                    }
                }
            } else if key.modifiers == KeyModifiers::CONTROL {
                app.nav_pending_file_shortcut = None;
                match key.code {
                    KeyCode::Up => {
                        app.nav_scroll = app.nav_scroll.saturating_sub(1);
                        continue;
                    }
                    KeyCode::Down => {
                        let max_scroll = nav_max_scroll(app.nav_entries.len(), app.nav_viewport_rows);
                        app.nav_scroll = app.nav_scroll.saturating_add(1).min(max_scroll);
                        continue;
                    }
                    _ => {}
                }
            } else {
                app.nav_pending_file_shortcut = None;
            }
        }

        if let KeyCode::Char(ch) = key.code {
            if ch == 'd' && key.modifiers.contains(KeyModifiers::CONTROL) {
                if app.block_exit_attempt_if_unsaved() {
                    continue;
                }
                app.prepare_for_exit();
                break;
            }
            match ch {
                _ => {}
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests;

