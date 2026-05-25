use ansi_to_tui::IntoText;
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
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
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
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};
use vt100::Parser;

const MAX_OUTPUT_CHUNKS_PER_TICK: usize = 512;
const MAX_OUTPUT_BYTES_PER_TICK: usize = 4 * 1024 * 1024;
#[cfg(test)]
const PREVIEW_FILE_MAX_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy)]
struct OutputDrain {
    processed_chunks: usize,
    hit_limit: bool,
}

fn merge_output_drains(primary: OutputDrain, secondary: OutputDrain) -> OutputDrain {
    OutputDrain {
        processed_chunks: primary
            .processed_chunks
            .saturating_add(secondary.processed_chunks),
        hit_limit: primary.hit_limit || secondary.hit_limit,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivePane {
    Shell,
    Navigation,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewMode {
    Empty,
    DirectoryTree,
    FileText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NavigationFileCommandAction {
    RunReadInPreview(String),
    RunWriteInPreview(String),
    PrefillShell(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewCommandMode {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewOverlayPresentation {
    StaticFullscreen,
    InteractiveFullscreenDim,
}

#[derive(Debug, Clone)]
struct EffectiveIdentity {
    euid: u32,
    egid: u32,
    groups: HashSet<u32>,
}

impl EffectiveIdentity {
    fn current() -> Self {
        let euid = unsafe { libc::geteuid() } as u32;
        let egid = unsafe { libc::getegid() } as u32;

        let groups = unsafe {
            let count = libc::getgroups(0, std::ptr::null_mut());
            if count <= 0 {
                let mut fallback = HashSet::new();
                fallback.insert(egid);
                fallback
            } else {
                let mut buf = vec![0 as libc::gid_t; count as usize];
                let written = libc::getgroups(count, buf.as_mut_ptr());
                let mut parsed = HashSet::new();
                if written > 0 {
                    for group in buf.into_iter().take(written as usize) {
                        parsed.insert(group as u32);
                    }
                }
                parsed.insert(egid);
                parsed
            }
        };

        Self { euid, egid, groups }
    }

    fn in_group(&self, gid: u32) -> bool {
        gid == self.egid || self.groups.contains(&gid)
    }
}

#[derive(Debug, Clone, Copy)]
struct EffectiveAccess {
    read: bool,
    write: bool,
    exec: bool,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("navix step1 error: {err}");
    }
}

fn run() -> io::Result<()> {
    let editor_program = ensure_editor_program()?;
    let mut guard = TerminalGuard::enter()?;
    let mut app = App::new(editor_program)?;
    let mut previous_fullish_layout = false;

    loop {
        app.tick_feedback();
        let shell_drain = app.shell.poll_output();
        let preview_drain = app.poll_preview_command_output();
        let drain = merge_output_drains(shell_drain, preview_drain);
        let current_fullish_layout = is_fullish_layout_state(
            app.active,
            app.shell_fullish,
            app.shell.in_alt_screen(),
            app.nav_fullish,
            app.preview_command_overlay_active,
        );
        let left_fullish_layout = previous_fullish_layout && !current_fullish_layout;
        previous_fullish_layout = current_fullish_layout;
        if app.force_terminal_clear || left_fullish_layout {
            guard.terminal.clear()?;
            app.force_terminal_clear = false;
        }

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

            let nav_fullish_mode = app.active == ActivePane::Navigation && app.nav_fullish;
            let preview_fullish_mode = preview_overlay_active;
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(if nav_fullish_mode {
                    vec![Constraint::Min(1), Constraint::Length(12)]
                } else if preview_fullish_mode {
                    vec![Constraint::Length(12), Constraint::Min(1)]
                } else {
                    vec![Constraint::Percentage(30), Constraint::Percentage(70)]
                })
                .split(rows[0]);
            let shell_block_area = rows[1];
            let nav_border = border_style(app.active == ActivePane::Navigation, panel_dim_theme);
            let preview_border = border_style(app.active == ActivePane::Preview, panel_dim_theme);
            let shell_border = border_style(app.active == ActivePane::Shell, panel_dim_theme);

            let nav_block = Block::default()
                .title(tab_title(
                    "─[1]─Navigation",
                    app.active == ActivePane::Navigation,
                ))
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
            let nav_entry_viewport_rows = nav_inner.height.saturating_sub(1) as usize;
            app.nav_viewport_rows = nav_entry_viewport_rows.max(1);
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
            frame.render_widget(Paragraph::new(nav_text), nav_inner);
            app.refresh_preview_panel();
            let preview_hovered_label = app
                .nav_entries
                .get(app.nav_selected)
                .map(|entry| entry.name.as_str());
            let preview_selected_entry = app.nav_entries.get(app.nav_selected);
            let preview_inner = inner(cols[1]);
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
                        Paragraph::new(display).style(nav_style_for_theme(
                            Style::default().fg(Color::White),
                            panel_dim_theme,
                        )),
                        nav_meta_rect,
                    );
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
        })?;

        let input_poll_timeout = poll_timeout_for_drain(drain);
        if !event::poll(input_poll_timeout)? {
            continue;
        }
        let event = event::read()?;
        if let Event::Mouse(mouse) = event {
            if app.config_open {
                continue;
            }
            if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
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

        if app.config_open {
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
            if key.modifiers.is_empty() {
                match key.code {
                    KeyCode::Char(' ') => {
                        if app.handle_preview_space_action() {
                            continue;
                        }
                    }
                    KeyCode::Up => {
                        app.nav_selected = app.nav_selected.saturating_sub(1);
                        app.nav_scroll = nav_scroll_for_selection(
                            app.nav_scroll,
                            app.nav_selected,
                            app.nav_entries.len(),
                            app.nav_viewport_rows,
                        );
                        continue;
                    }
                    KeyCode::Down => {
                        if app.nav_selected + 1 < app.nav_entries.len() {
                            app.nav_selected += 1;
                        }
                        app.nav_scroll = nav_scroll_for_selection(
                            app.nav_scroll,
                            app.nav_selected,
                            app.nav_entries.len(),
                            app.nav_viewport_rows,
                        );
                        continue;
                    }
                    KeyCode::PageUp => {
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
                        if let Some(entry) = app.nav_entries.get(app.nav_selected) {
                            if entry.is_dir {
                                app.shell.cd_to(&entry.path)?;
                                app.nav_loaded = false;
                                app.nav_selected = 0;
                                app.nav_scroll = 0;
                            }
                        }
                        continue;
                    }
                    _ => {}
                }
            } else if key.modifiers == KeyModifiers::CONTROL {
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

fn run_selected_file_command_shortcut(app: &mut App, shortcut: char) -> io::Result<bool> {
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
        }
        NavigationFileCommandAction::RunWriteInPreview(command) => {
            app.run_preview_command_overlay(&command, PreviewCommandMode::Write)?;
        }
        NavigationFileCommandAction::PrefillShell(command) => {
            app.focus_shell_with_prefilled_command(&command)?;
        }
    }
    Ok(true)
}

fn ensure_editor_program() -> io::Result<String> {
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

fn open_key_debug_log() -> Option<File> {
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

fn border_style(focused: bool, fullish_shell_theme: bool) -> Style {
    if fullish_shell_theme {
        let base = Style::default().fg(Color::DarkGray);
        if focused {
            base.add_modifier(Modifier::BOLD)
        } else {
            base
        }
    } else if focused {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Blue)
    }
}

fn tab_title(title: &str, _focused: bool) -> String {
    title.to_string()
}

fn footer_key_style(fullish_shell_theme: bool) -> Style {
    if fullish_shell_theme {
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
    }
}

fn escape_prefix_release_update(
    pending_alt: bool,
    pending_alt_shortcut_armed: bool,
    key_code: KeyCode,
    key_kind: KeyEventKind,
) -> (bool, bool, bool) {
    if key_kind == KeyEventKind::Release && key_code == KeyCode::Esc {
        return (false, false, true);
    }
    (pending_alt, pending_alt_shortcut_armed, false)
}

fn escape_prefix_shortcut_char(shortcut_armed: bool, key_code: KeyCode) -> Option<char> {
    if !shortcut_armed {
        return None;
    }
    let KeyCode::Char(ch) = key_code else {
        return None;
    };
    let lowered = ch.to_ascii_lowercase();
    matches!(lowered, '0' | '1' | '2' | 'c' | 'f' | 'r' | 'w' | 'x').then_some(lowered)
}

fn preview_shortcut_target(selected_entry: Option<&NavEntry>, preview_mode: PreviewMode) -> ActivePane {
    if selected_entry.is_some_and(|entry| entry.is_dir) && preview_mode == PreviewMode::DirectoryTree {
        ActivePane::Navigation
    } else if selected_entry.is_some_and(|entry| !entry.is_dir) && preview_mode == PreviewMode::FileText {
        ActivePane::Navigation
    } else {
        ActivePane::Preview
    }
}

fn panel_click_focus_target(
    clicked_pane: ActivePane,
    preview_overlay_active: bool,
    selected_entry: Option<&NavEntry>,
    preview_mode: PreviewMode,
) -> ActivePane {
    if clicked_pane != ActivePane::Preview || preview_overlay_active {
        return clicked_pane;
    }
    preview_shortcut_target(selected_entry, preview_mode)
}

#[cfg(test)]
fn footer_shortcuts(active: ActivePane, config_open: bool, config_dirty: bool) -> String {
    if config_open {
        return "Close: Esc".to_string();
    }
    let exit_label = if config_dirty { "Exit*" } else { "Exit" };
    match active {
        ActivePane::Shell => format!(
            "Shell: Esc+0 | Navigation: Esc+1 | Preview: Esc+2 | Config: Esc+c | Full: Esc+f | Scroll: PgUp/PgDown, Esc+↑/Esc+↓ | {exit_label}: Ctrl+d"
        ),
        ActivePane::Navigation | ActivePane::Preview => format!(
            "Shell: Esc+0 | Navigation: Esc+1 | Preview: Esc+2 | Config: Esc+c | Full: Esc+f | Scroll: PgUp/PgDown, ↑/↓ | {exit_label}: Ctrl+d"
        ),
    }
}

fn footer_meta() -> String {
    format!("Donate {}", env!("CARGO_PKG_VERSION"))
}

fn footer_shortcuts_line(
    active: ActivePane,
    fullish_shell_theme: bool,
    config_open: bool,
    config_dirty: bool,
    highlight_config_shortcut: bool,
) -> Line<'static> {
    let key_style = footer_key_style(fullish_shell_theme);
    let label_style = if fullish_shell_theme {
        let dimmer = Style::default().fg(Color::DarkGray);
        dimmer
    } else {
        Style::default().fg(Color::White)
    };
    let warning_star_style = Style::default()
        .fg(Color::Indexed(208))
        .add_modifier(Modifier::BOLD);
    let config_alert_style = Style::default()
        .fg(Color::Indexed(208))
        .add_modifier(Modifier::BOLD)
        .add_modifier(Modifier::RAPID_BLINK);
    let exit_label = if config_dirty { " | Exit*: " } else { " | Exit: " };
    let segments: Vec<(String, String, bool)> = if config_open {
        vec![("Close: ".to_string(), "Esc".to_string(), false)]
    } else {
        match active {
            ActivePane::Shell => vec![
                ("Shell: ".to_string(), "Esc+0".to_string(), false),
                (" | Navigation: ".to_string(), "Esc+1".to_string(), false),
                (" | Preview: ".to_string(), "Esc+2".to_string(), false),
                (" | Config: ".to_string(), "Esc+c".to_string(), true),
                (" | Full: ".to_string(), "Esc+f".to_string(), false),
                (
                    " | Scroll: ".to_string(),
                    "PgUp/PgDown, Esc+\u{2191}/Esc+\u{2193}".to_string(),
                    false,
                ),
                (exit_label.to_string(), "Ctrl+d".to_string(), false),
            ],
            ActivePane::Navigation | ActivePane::Preview => vec![
                ("Shell: ".to_string(), "Esc+0".to_string(), false),
                (" | Navigation: ".to_string(), "Esc+1".to_string(), false),
                (" | Preview: ".to_string(), "Esc+2".to_string(), false),
                (" | Config: ".to_string(), "Esc+c".to_string(), true),
                (" | Full: ".to_string(), "Esc+f".to_string(), false),
                (" | Scroll: ".to_string(), "PgUp/PgDown, ↑/↓".to_string(), false),
                (exit_label.to_string(), "Ctrl+d".to_string(), false),
            ],
        }
    };
    let mut spans = Vec::new();
    for (label, key, is_config_segment) in segments {
        let segment_label_style = if is_config_segment && highlight_config_shortcut {
            config_alert_style
        } else {
            label_style
        };
        let segment_key_style = if is_config_segment && highlight_config_shortcut {
            config_alert_style
        } else {
            key_style
        };
        push_shortcut_label(
            &mut spans,
            &label,
            label_style,
            segment_label_style,
            warning_star_style,
        );
        append_key_with_white_slashes(&mut spans, &key, segment_key_style, label_style);
    }
    Line::from(spans)
}

fn push_shortcut_label(
    spans: &mut Vec<Span<'static>>,
    label: &str,
    base_label_style: Style,
    label_style: Style,
    warning_star_style: Style,
) {
    let (prefix, core) = if let Some(core) = label.strip_prefix(" | ") {
        (" | ", core)
    } else {
        ("", label)
    };
    if !prefix.is_empty() {
        spans.push(Span::styled(prefix.to_string(), base_label_style));
    }
    if let Some(star_idx) = core.find('*') {
        let (before_star, star_and_after) = core.split_at(star_idx);
        if !before_star.is_empty() {
            spans.push(Span::styled(before_star.to_string(), label_style));
        }
        spans.push(Span::styled("*".to_string(), warning_star_style));
        let after_star = &star_and_after[1..];
        if !after_star.is_empty() {
            spans.push(Span::styled(after_star.to_string(), label_style));
        }
    } else {
        spans.push(Span::styled(core.to_string(), label_style));
    }
}

fn append_key_with_white_slashes(
    spans: &mut Vec<Span<'static>>,
    key: &str,
    key_style: Style,
    slash_style: Style,
) {
    let mut token = String::new();
    for ch in key.chars() {
        if ch == '/' || ch == ',' || ch == ' ' {
            if !token.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut token), key_style));
            }
            spans.push(Span::styled(ch.to_string(), slash_style));
        } else {
            token.push(ch);
        }
    }
    if !token.is_empty() {
        spans.push(Span::styled(token, key_style));
    }
}

fn footer_meta_line(fullish_shell_theme: bool) -> Line<'static> {
    let (donate_style, key_style, space_style) = if fullish_shell_theme {
        let dimmer = Style::default().fg(Color::DarkGray);
        (
            dimmer.add_modifier(Modifier::CROSSED_OUT),
            dimmer.add_modifier(Modifier::BOLD),
            dimmer,
        )
    } else {
        (
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::CROSSED_OUT),
            Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
            Style::default().fg(Color::White),
        )
    };
    Line::from(vec![
        Span::styled("Donate", donate_style),
        Span::styled(" ", space_style),
        Span::styled(env!("CARGO_PKG_VERSION"), key_style),
    ])
}

fn truncate_to_width(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

#[cfg(test)]
fn compose_footer_line(active: ActivePane, width: u16) -> String {
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return " ".to_string();
    }
    let total_width = width as usize;
    let inner_width = total_width.saturating_sub(2);
    let left = footer_shortcuts(active, false, false);
    let right = footer_meta();
    let right_len = right.chars().count();

    let inner = if right_len >= inner_width {
        truncate_to_width(&right, inner_width)
    } else {
        let space_for_left = inner_width.saturating_sub(right_len + 1);
        let left_display = if left.chars().count() > space_for_left {
            truncate_to_width(&left, space_for_left)
        } else {
            left
        };

        let padding = inner_width.saturating_sub(left_display.chars().count() + right_len);
        format!("{left_display}{}{right}", " ".repeat(padding))
    };

    format!(" {inner} ")
}

struct App {
    active: ActivePane,
    shell: ShellPane,
    pending_alt: bool,
    pending_alt_shortcut_armed: bool,
    shell_fullish: bool,
    nav_fullish: bool,
    config_open: bool,
    config_state: ConfigState,
    saved_config_state: ConfigState,
    config_editor: ConfigEditor,
    nav_colors: LsColorsTheme,
    nav_cwd: PathBuf,
    nav_entries: Vec<NavEntry>,
    nav_error: Option<String>,
    nav_loaded: bool,
    nav_selected: usize,
    nav_scroll: usize,
    nav_viewport_rows: usize,
    nav_meta_cache_path: Option<PathBuf>,
    nav_meta_cache: String,
    preview_mode: PreviewMode,
    preview_depth: usize,
    preview_max_depth: usize,
    preview_dir_enabled: bool,
    preview_cached_text: String,
    preview_last_selected_path: Option<PathBuf>,
    preview_cached_depth: usize,
    preview_command_overlay_active: bool,
    preview_command_overlay_command: String,
    preview_command_overlay_mode: Option<PreviewCommandMode>,
    preview_command_overlay_presentation: Option<PreviewOverlayPresentation>,
    preview_command_shell: Option<ShellPane>,
    effective_identity: EffectiveIdentity,
    editor_program: String,
    config_shortcut_alert_until: Option<Instant>,
    key_debug_log: Option<File>,
    key_debug_seq: u64,
    force_terminal_clear: bool,
}

impl App {
    fn new(editor_program: String) -> io::Result<Self> {
        let (config_state, config_status) = ConfigState::load_or_default();
        let mut config_editor = ConfigEditor::default();
        if let Some(status) = config_status {
            config_editor.status_message = status;
        }
        config_editor.ensure_valid(&config_state);
        Ok(Self {
            active: ActivePane::Navigation,
            shell: ShellPane::spawn()?,
            pending_alt: false,
            pending_alt_shortcut_armed: false,
            shell_fullish: false,
            nav_fullish: false,
            config_open: false,
            config_state: config_state.clone(),
            saved_config_state: config_state,
            config_editor,
            nav_colors: LsColorsTheme::from_env(),
            nav_cwd: PathBuf::new(),
            nav_entries: Vec::new(),
            nav_error: None,
            nav_loaded: false,
            nav_selected: 0,
            nav_scroll: 0,
            nav_viewport_rows: 1,
            nav_meta_cache_path: None,
            nav_meta_cache: String::new(),
            preview_mode: PreviewMode::Empty,
            preview_depth: 1,
            preview_max_depth: 1,
            preview_dir_enabled: true,
            preview_cached_text: String::new(),
            preview_last_selected_path: None,
            preview_cached_depth: 0,
            preview_command_overlay_active: false,
            preview_command_overlay_command: String::new(),
            preview_command_overlay_mode: None,
            preview_command_overlay_presentation: None,
            preview_command_shell: None,
            effective_identity: EffectiveIdentity::current(),
            editor_program,
            config_shortcut_alert_until: None,
            key_debug_log: open_key_debug_log(),
            key_debug_seq: 0,
            force_terminal_clear: false,
        })
    }

    fn open_config(&mut self) {
        self.config_open = true;
        self.config_editor.editing = false;
        self.config_editor.clear_input();
        self.config_editor.ensure_valid(&self.config_state);
    }

    fn has_unsaved_config_changes(&self) -> bool {
        self.config_editor.dirty
    }

    fn block_exit_attempt_if_unsaved(&mut self) -> bool {
        if !self.has_unsaved_config_changes() {
            return false;
        }
        self.config_shortcut_alert_until = Some(Instant::now() + Duration::from_secs(1));
        true
    }

    fn tick_feedback(&mut self) {
        if let Some(until) = self.config_shortcut_alert_until {
            if Instant::now() >= until {
                self.config_shortcut_alert_until = None;
            }
        }
        self.maybe_finish_preview_overlay_session();
    }

    fn should_highlight_config_shortcut(&self) -> bool {
        self.config_shortcut_alert_until
            .is_some_and(|until| Instant::now() < until)
            && self.has_unsaved_config_changes()
            && !self.config_open
    }

    fn log_key_debug_event(&mut self, stage: &str, key: Option<&crossterm::event::KeyEvent>) {
        let Some(log) = self.key_debug_log.as_mut() else {
            return;
        };
        self.key_debug_seq = self.key_debug_seq.saturating_add(1);
        if let Some(key) = key {
            let _ = writeln!(
                log,
                "#{:06} stage={} key_code={:?} key_kind={:?} key_mods={:?} pending_alt={} armed={} active={:?} overlay={} overlay_mode={:?}",
                self.key_debug_seq,
                stage,
                key.code,
                key.kind,
                key.modifiers,
                self.pending_alt,
                self.pending_alt_shortcut_armed,
                self.active,
                self.preview_command_overlay_active,
                self.preview_command_overlay_mode
            );
        } else {
            let _ = writeln!(
                log,
                "#{:06} stage={} pending_alt={} armed={} active={:?} overlay={} overlay_mode={:?}",
                self.key_debug_seq,
                stage,
                self.pending_alt,
                self.pending_alt_shortcut_armed,
                self.active,
                self.preview_command_overlay_active,
                self.preview_command_overlay_mode
            );
        }
        let _ = log.flush();
    }

    fn refresh_preview_panel(&mut self) {
        let selected_entry = self.nav_entries.get(self.nav_selected);
        let selected_path = selected_entry.map(|entry| entry.path.clone());

        let Some(entry) = selected_entry else {
            self.preview_mode = PreviewMode::Empty;
            self.preview_cached_text.clear();
            self.preview_last_selected_path = None;
            self.preview_cached_depth = 0;
            return;
        };

        let selected_changed = self.preview_last_selected_path.as_ref() != Some(&entry.path);

        if !entry.is_dir {
            self.preview_mode = PreviewMode::FileText;
            self.preview_cached_text.clear();
            self.preview_last_selected_path = selected_path;
            self.preview_cached_depth = 0;
            return;
        }

        self.preview_last_selected_path = selected_path;
        if !self.preview_dir_enabled {
            self.preview_mode = PreviewMode::Empty;
            self.preview_cached_text.clear();
            self.preview_cached_depth = 0;
            return;
        }

        let cache_hit = !selected_changed
            && self.preview_mode == PreviewMode::DirectoryTree
            && self.preview_cached_depth == self.preview_depth;
        if cache_hit {
            return;
        }

        let (mode, text) = preview_content_for_selected_entry(Some(entry), self.preview_depth);
        self.preview_mode = mode;
        self.preview_cached_text = text;
        self.preview_cached_depth = self.preview_depth;
    }

    fn toggle_directory_preview(&mut self) {
        if !self
            .nav_entries
            .get(self.nav_selected)
            .is_some_and(|entry| entry.is_dir)
        {
            return;
        }
        self.preview_dir_enabled = !self.preview_dir_enabled;
        self.preview_cached_depth = 0;
        self.refresh_preview_panel();
    }

    fn handle_preview_space_action(&mut self) -> bool {
        let selected = self
            .nav_entries
            .get(self.nav_selected)
            .map(|entry| entry.is_dir);
        match selected {
            Some(true) => {
                self.toggle_directory_preview();
                true
            }
            Some(false) => false,
            None => false,
        }
    }

    fn close_preview_command_overlay(&mut self) {
        if let Some(mut session) = self.preview_command_shell.take() {
            session.terminate();
        }
        self.preview_command_overlay_active = false;
        self.preview_command_overlay_command.clear();
        self.preview_command_overlay_mode = None;
        self.preview_command_overlay_presentation = None;
        self.force_terminal_clear = true;
    }

    fn run_preview_command_overlay(&mut self, command: &str, mode: PreviewCommandMode) -> io::Result<()> {
        self.close_preview_command_overlay();
        let cwd = if self.nav_cwd.as_os_str().is_empty() {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
        } else {
            self.nav_cwd.clone()
        };
        let spawned_command = if mode == PreviewCommandMode::Read {
            format!("PAGER=cat BAT_PAGER=cat {command}")
        } else {
            command.to_string()
        };
        let mut session = ShellPane::spawn_command(&spawned_command, &cwd)?;
        if mode == PreviewCommandMode::Read {
            session.scroll_up(usize::MAX);
        }
        self.preview_command_overlay_active = true;
        self.preview_command_overlay_command = command.to_string();
        self.preview_command_overlay_mode = Some(mode);
        self.preview_command_overlay_presentation = Some(PreviewOverlayPresentation::StaticFullscreen);
        self.preview_command_shell = Some(session);
        self.active = ActivePane::Preview;
        self.force_terminal_clear = true;
        Ok(())
    }

    fn focus_shell_with_prefilled_command(&mut self, command: &str) -> io::Result<()> {
        self.close_preview_command_overlay();
        self.active = ActivePane::Shell;
        self.shell.jump_to_bottom();
        self.shell.send_raw(&prefill_shell_input_bytes(command))
    }

    fn poll_preview_command_output(&mut self) -> OutputDrain {
        if let Some(session) = self.preview_command_shell.as_mut() {
            let drain = session.poll_output();
            self.preview_command_overlay_presentation = next_preview_overlay_presentation(
                self.preview_command_overlay_presentation,
                session.in_alt_screen(),
            );
            drain
        } else {
            OutputDrain {
                processed_chunks: 0,
                hit_limit: false,
            }
        }
    }

    fn maybe_finish_preview_overlay_session(&mut self) {
        if !self.preview_command_overlay_active {
            return;
        }
        let Some(session) = self.preview_command_shell.as_mut() else {
            self.close_preview_command_overlay();
            self.active = ActivePane::Navigation;
            return;
        };
        if !should_auto_close_preview_overlay(
            self.preview_command_overlay_presentation,
            session.is_running(),
        ) {
            return;
        }
        self.close_preview_command_overlay();
        self.active = ActivePane::Navigation;
    }

    fn prepare_for_exit(&mut self) {
        self.close_preview_command_overlay();
        self.shell.request_shutdown();
    }

    fn increase_preview_depth(&mut self) {
        if !self
            .nav_entries
            .get(self.nav_selected)
            .is_some_and(|entry| entry.is_dir)
        {
            return;
        }
        self.preview_depth = self
            .preview_depth
            .saturating_add(1);
        self.preview_depth = clamp_preview_depth(self.preview_depth, self.preview_max_depth);
        self.preview_cached_depth = 0;
    }

    fn decrease_preview_depth(&mut self) {
        if !self
            .nav_entries
            .get(self.nav_selected)
            .is_some_and(|entry| entry.is_dir)
        {
            return;
        }
        self.preview_depth = self.preview_depth.saturating_sub(1);
        self.preview_depth = clamp_preview_depth(self.preview_depth, self.preview_max_depth);
        self.preview_cached_depth = 0;
    }

    fn save_config(&mut self) {
        self.config_state.normalize();
        if first_empty_extension(&self.config_state) {
            self.config_editor.status_message =
                "save blocked: extension name cannot be empty".to_string();
            return;
        }
        if let Some(duplicate) = first_duplicate_extension(&self.config_state) {
            self.config_editor.status_message =
                format!("save blocked: duplicate extension '.{duplicate}'");
            return;
        }
        match self.config_state.save() {
            Ok(()) => {
                self.saved_config_state = self.config_state.clone();
                self.config_editor.dirty = false;
                self.config_editor.status_message =
                    format!("saved {}", ConfigState::config_file_path().display());
            }
            Err(err) => {
                self.config_editor.status_message = format!("save failed: {err}");
            }
        }
    }

    fn discard_config_changes(&mut self) {
        self.config_state = self.saved_config_state.clone();
        self.config_editor.dirty = false;
        self.config_editor.editing = false;
        self.config_editor.clear_input();
        self.config_editor.ensure_valid(&self.config_state);
        self.config_editor.status_message = "discarded unsaved changes".to_string();
    }

    fn reload_config(&mut self) {
        match ConfigState::load() {
            Ok(state) => {
                self.config_state = state.clone();
                self.saved_config_state = state;
                self.config_editor.dirty = false;
                self.config_editor.ensure_valid(&self.config_state);
                self.config_editor.status_message =
                    format!("reloaded {}", ConfigState::config_file_path().display());
            }
            Err(err) => {
                self.config_editor.status_message = format!("reload failed: {err}");
            }
        }
    }

    fn delete_selected_extension_rule(&mut self) {
        if self.config_state.extension_rules.is_empty() {
            return;
        }
        self.config_state
            .extension_rules
            .remove(self.config_editor.selected_rule);
        self.config_editor.ensure_valid(&self.config_state);
        self.config_editor.dirty = true;
        self.config_editor.status_message = "deleted extension rule".to_string();
    }

    fn refresh_navigation(&mut self, cwd: &Path) {
        if self.nav_loaded && self.nav_cwd == cwd {
            return;
        }
        self.nav_cwd = cwd.to_path_buf();
        self.nav_loaded = true;
        match navigation_entries(cwd) {
            Ok(entries) => {
                self.nav_entries = entries;
                self.nav_error = None;
            }
            Err(err) => {
                self.nav_entries.clear();
                self.nav_error = Some(err.to_string());
            }
        }
        self.nav_selected = clamp_nav_selection(self.nav_selected, self.nav_entries.len());
        self.nav_scroll = nav_scroll_for_selection(
            self.nav_scroll,
            self.nav_selected,
            self.nav_entries.len(),
            self.nav_viewport_rows,
        );
        self.nav_meta_cache_path = None;
    }

    fn nav_meta_for_selection(&mut self) -> String {
        if let Some(err) = self.nav_error.as_deref() {
            self.nav_meta_cache_path = None;
            self.nav_meta_cache = format!("error: {err}");
            return self.nav_meta_cache.clone();
        }
        let Some(selected) = self.nav_entries.get(self.nav_selected).cloned() else {
            self.nav_meta_cache_path = None;
            self.nav_meta_cache.clear();
            return String::new();
        };
        if self
            .nav_meta_cache_path
            .as_ref()
            .is_some_and(|path| path == &selected.path)
        {
            return self.nav_meta_cache.clone();
        }
        self.nav_meta_cache_path = Some(selected.path.clone());
        self.nav_meta_cache = nav_long_listing(&selected);
        self.nav_meta_cache.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ExtensionCommandRule {
    extension: String,
    read_cmd: String,
    write_cmd: String,
    exec_cmd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConfigState {
    #[serde(default)]
    extension_rules: Vec<ExtensionCommandRule>,
}

impl Default for ConfigState {
    fn default() -> Self {
        Self {
            extension_rules: vec![
                ExtensionCommandRule {
                    extension: "md".to_string(),
                    read_cmd: "bat {file}".to_string(),
                    write_cmd: "$EDITOR {file}".to_string(),
                    exec_cmd: "mdterm {file}".to_string(),
                },
                ExtensionCommandRule {
                    extension: "json".to_string(),
                    read_cmd: "jq . {file}".to_string(),
                    write_cmd: "$EDITOR {file}".to_string(),
                    exec_cmd: "fx {file}".to_string(),
                },
                ExtensionCommandRule {
                    extension: "sh".to_string(),
                    read_cmd: "bat {file}".to_string(),
                    write_cmd: "$EDITOR {file}".to_string(),
                    exec_cmd: "bash {file}".to_string(),
                },
            ],
        }
    }
}

impl ConfigState {
    fn config_file_path() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.trim().is_empty() {
                return PathBuf::from(xdg).join("navix").join("config.toml");
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            if !home.trim().is_empty() {
                return PathBuf::from(home).join(".config").join("navix").join("config.toml");
            }
        }
        PathBuf::from("navix-config.toml")
    }

    fn load() -> io::Result<Self> {
        let path = Self::config_file_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&path)?;
        let mut loaded: Self = toml::from_str(&raw).map_err(to_io)?;
        loaded.normalize();
        Ok(loaded)
    }

    fn load_or_default() -> (Self, Option<String>) {
        match Self::load() {
            Ok(state) => (state, None),
            Err(err) => (
                Self::default(),
                Some(format!("config load failed, using defaults: {err}")),
            ),
        }
    }

    fn save(&self) -> io::Result<()> {
        let path = Self::config_file_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut normalized = self.clone();
        normalized.normalize();
        let serialized = toml::to_string_pretty(&normalized).map_err(to_io)?;
        fs::write(path, serialized)?;
        Ok(())
    }

    fn normalize(&mut self) {
        for rule in &mut self.extension_rules {
            rule.extension = normalize_extension(&rule.extension);
            if rule.read_cmd.trim() == "bat --paging=never {file}" {
                rule.read_cmd = "bat {file}".to_string();
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigField {
    Extension,
    Read,
    Write,
    Exec,
}

impl ConfigField {
    fn next(self) -> Self {
        match self {
            Self::Extension => Self::Read,
            Self::Read => Self::Write,
            Self::Write => Self::Exec,
            Self::Exec => Self::Extension,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Extension => Self::Exec,
            Self::Read => Self::Extension,
            Self::Write => Self::Read,
            Self::Exec => Self::Write,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Extension => "extension",
            Self::Read => "read",
            Self::Write => "write",
            Self::Exec => "exec",
        }
    }
}

#[derive(Debug, Clone)]
struct ConfigEditor {
    selected_rule: usize,
    selected_field: ConfigField,
    editing: bool,
    input_buffer: String,
    input_cursor: usize,
    dirty: bool,
    status_message: String,
}

impl Default for ConfigEditor {
    fn default() -> Self {
        Self {
            selected_rule: 0,
            selected_field: ConfigField::Extension,
            editing: false,
            input_buffer: String::new(),
            input_cursor: 0,
            dirty: false,
            status_message: String::new(),
        }
    }
}

impl ConfigEditor {
    fn ensure_valid(&mut self, config: &ConfigState) {
        self.selected_rule = if config.extension_rules.is_empty() {
            0
        } else {
            self.selected_rule.min(config.extension_rules.len().saturating_sub(1))
        };
    }

    fn clear_input(&mut self) {
        self.input_buffer.clear();
        self.input_cursor = 0;
    }

    fn set_input(&mut self, value: String) {
        self.input_buffer = value;
        self.input_cursor = self.input_buffer.len();
    }

    fn move_cursor_left(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        self.input_cursor = previous_char_boundary(&self.input_buffer, self.input_cursor);
    }

    fn move_cursor_right(&mut self) {
        if self.input_cursor >= self.input_buffer.len() {
            self.input_cursor = self.input_buffer.len();
            return;
        }
        self.input_cursor = next_char_boundary(&self.input_buffer, self.input_cursor);
    }

    fn move_cursor_home(&mut self) {
        self.input_cursor = 0;
    }

    fn move_cursor_end(&mut self) {
        self.input_cursor = self.input_buffer.len();
    }

    fn insert_char(&mut self, ch: char) {
        self.input_buffer.insert(self.input_cursor, ch);
        self.input_cursor += ch.len_utf8();
    }

    fn backspace(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        let start = previous_char_boundary(&self.input_buffer, self.input_cursor);
        self.input_buffer.drain(start..self.input_cursor);
        self.input_cursor = start;
    }

    fn delete(&mut self) {
        if self.input_cursor >= self.input_buffer.len() {
            return;
        }
        let end = next_char_boundary(&self.input_buffer, self.input_cursor);
        self.input_buffer.drain(self.input_cursor..end);
    }
}

struct ShellPane {
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

struct ShellMetrics {
    shown_start: usize,
    shown_end: usize,
    total: usize,
    has_overflow: bool,
}

impl ShellPane {
    fn spawn() -> io::Result<Self> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let shell_path = std::env::var("NAVIX_LAUNCH_SHELL")
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(|| std::env::var("SHELL").ok().filter(|v| !v.is_empty()))
            .unwrap_or_else(|| "/bin/sh".to_string());
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

    fn spawn_command(command_line: &str, cwd: &Path) -> io::Result<Self> {
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

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = tx.send(buf[..n].to_vec());
                    }
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

    fn poll_output(&mut self) -> OutputDrain {
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
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        OutputDrain {
            processed_chunks,
            hit_limit,
        }
    }

    fn render_text_and_metrics(
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
        for (idx, row) in self.parser.screen().rows_formatted(0, cols).into_iter().enumerate() {
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

    fn resize(&mut self, rows: u16, cols: u16) {
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

    fn send_key(&mut self, key: crossterm::event::KeyEvent) -> io::Result<()> {
        let mut bytes: Vec<u8> = Vec::new();
        match key.code {
            KeyCode::Char(ch) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    let lower = ch.to_ascii_lowercase() as u8;
                    bytes.push(lower & 0x1f);
                } else {
                    let mut tmp = [0u8; 4];
                    let encoded = ch.encode_utf8(&mut tmp);
                    bytes.extend_from_slice(encoded.as_bytes());
                }
            }
            KeyCode::Enter => bytes.push(b'\r'),
            KeyCode::Backspace => bytes.push(0x7f),
            KeyCode::Tab => bytes.push(b'\t'),
            KeyCode::Esc => bytes.push(0x1b),
            KeyCode::Up => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    bytes.extend_from_slice(b"\x1b[1;5A");
                } else {
                    bytes.extend_from_slice(b"\x1b[A");
                }
            }
            KeyCode::Down => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    bytes.extend_from_slice(b"\x1b[1;5B");
                } else {
                    bytes.extend_from_slice(b"\x1b[B");
                }
            }
            KeyCode::Left => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    bytes.extend_from_slice(b"\x1b[1;5D");
                } else {
                    bytes.extend_from_slice(b"\x1b[D");
                }
            }
            KeyCode::Right => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    bytes.extend_from_slice(b"\x1b[1;5C");
                } else {
                    bytes.extend_from_slice(b"\x1b[C");
                }
            }
            _ => {}
        }

        if !bytes.is_empty() {
            self.send_raw(&bytes)?;
        }
        Ok(())
    }

    fn send_raw(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    fn cd_to(&mut self, path: &Path) -> io::Result<()> {
        let command = format!("cd -- {}\r", shell_single_quote(path.to_string_lossy().as_ref()));
        self.send_raw(command.as_bytes())?;
        self.last_known_cwd = path.to_path_buf();
        Ok(())
    }

    fn scroll_up(&mut self, rows: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(rows);
    }

    fn scroll_down(&mut self, rows: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(rows);
    }

    fn jump_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    fn page_rows(&self) -> usize {
        self.viewport_rows.max(1)
    }

    fn in_alt_screen(&self) -> bool {
        self.alt_screen_active
    }

    fn is_running(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(_)) => false,
            Ok(None) => true,
            Err(_) => false,
        }
    }

    fn terminate(&mut self) {
        let _ = self.child.kill();
    }

    fn request_shutdown(&mut self) {
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

    fn visible_cursor(&self, viewport_rows: u16, cols: u16) -> Option<(u16, u16)> {
        if self.scroll_offset != 0 || viewport_rows == 0 || cols == 0 || self.parser.screen().hide_cursor() {
            return None;
        }
        let (row, col) = self.parser.screen().cursor_position();
        Some((row.min(viewport_rows.saturating_sub(1)), col.min(cols.saturating_sub(1))))
    }

    fn current_cwd(&mut self) -> PathBuf {
        if let Some(pid) = self.child.process_id() {
            let proc_cwd = format!("/proc/{pid}/cwd");
            if let Ok(path) = fs::read_link(proc_cwd) {
                self.last_known_cwd = path;
            }
        }
        self.last_known_cwd.clone()
    }
}

fn shell_single_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\\''"))
}

fn shell_program_name(shell_path: &str) -> String {
    Path::new(shell_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(shell_path)
        .to_ascii_lowercase()
}

fn bash_history_sync_prompt_command(existing_prompt_command: Option<&str>) -> String {
    let sync = "history -a; history -n";
    let Some(existing) = existing_prompt_command.map(str::trim).filter(|s| !s.is_empty()) else {
        return sync.to_string();
    };
    if existing.contains("history -n") && existing.contains("history -a") {
        existing.to_string()
    } else {
        format!("{sync}; {existing}")
    }
}

fn default_history_file_for_shell(shell_path: &str) -> Option<String> {
    let home = std::env::var("HOME").ok().filter(|value| !value.is_empty())?;
    let shell_name = shell_program_name(shell_path);
    let mut candidates: Vec<String> = Vec::new();
    match shell_name.as_str() {
        "bash" => candidates.push(format!("{home}/.bash_history")),
        "zsh" => {
            candidates.push(format!("{home}/.zhistory"));
            candidates.push(format!("{home}/.zsh_history"));
            if let Some(state_home) =
                std::env::var("XDG_STATE_HOME").ok().filter(|value| !value.is_empty())
            {
                candidates.push(format!("{state_home}/zsh/history"));
            }
        }
        "fish" => {
            if let Some(data_home) =
                std::env::var("XDG_DATA_HOME").ok().filter(|value| !value.is_empty())
            {
                candidates.push(format!("{data_home}/fish/fish_history"));
            }
            candidates.push(format!("{home}/.local/share/fish/fish_history"));
        }
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
        }
        "zsh" => {
            if std::env::var("HISTSIZE").is_err() {
                command.env("HISTSIZE", "100000");
            }
            if std::env::var("SAVEHIST").is_err() {
                command.env("SAVEHIST", "100000");
            }
        }
        _ => {}
    }
}

fn apply_process_environment(command: &mut CommandBuilder) {
    for (key, value) in std::env::vars() {
        command.env(key, value);
    }
}

#[cfg(test)]
fn window_bounds(content_end: usize, viewport: usize, scroll_offset: usize) -> (usize, usize, usize) {
    let viewport = viewport.max(1);
    let content_end = content_end.max(1).max(viewport);
    let max_offset = content_end.saturating_sub(viewport);
    let clamped_offset = scroll_offset.min(max_offset);
    let end = content_end.saturating_sub(clamped_offset);
    let start = end.saturating_sub(viewport);
    (start, end, max_offset)
}

fn should_show_scrollbar(total: usize, viewport: usize) -> bool {
    total > viewport.max(1)
}

fn visible_range(total: usize, viewport: usize, scroll_offset: usize) -> (usize, usize) {
    let viewport = viewport.max(1);
    let total = total.max(viewport).max(1);
    let max_offset = total.saturating_sub(viewport);
    let clamped_offset = scroll_offset.min(max_offset);
    let shown_end = total.saturating_sub(clamped_offset);
    let shown_start = shown_end
        .saturating_sub(viewport.saturating_sub(1))
        .max(1);
    (shown_start, shown_end.max(shown_start))
}

fn shell_panel_height(total_main_height: u16, active: ActivePane, shell_fullish: bool) -> u16 {
    if active == ActivePane::Shell && shell_fullish {
        return total_main_height.saturating_sub(4).max(1);
    }
    let base = if active == ActivePane::Shell {
        ((total_main_height as f32) * 0.45).round() as u16
    } else {
        7
    };
    base.clamp(7, total_main_height.saturating_sub(4).max(7))
}

fn is_fullish_shell_mode(active: ActivePane, shell_fullish: bool) -> bool {
    active == ActivePane::Shell && shell_fullish
}

fn prefill_shell_input_bytes(command: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(command.len().saturating_add(2));
    // Ctrl+a then Ctrl+k clears current prompt input without executing it.
    out.extend_from_slice(&[0x01, 0x0b]);
    out.extend_from_slice(command.as_bytes());
    out
}

fn is_fullish_layout_state(
    active: ActivePane,
    shell_fullish_toggle: bool,
    shell_alt_screen_active: bool,
    nav_fullish: bool,
    preview_overlay_active: bool,
) -> bool {
    let shell_fullish_mode = is_fullish_shell_mode(active, shell_fullish_toggle || shell_alt_screen_active);
    let nav_fullish_mode = active == ActivePane::Navigation && nav_fullish;
    shell_fullish_mode || nav_fullish_mode || preview_overlay_active
}

fn panel_areas_for_focus_click(
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
    let nav_fullish_mode = active == ActivePane::Navigation && nav_fullish;
    let preview_fullish_mode = preview_overlay_active;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(if nav_fullish_mode {
            vec![Constraint::Min(1), Constraint::Length(12)]
        } else if preview_fullish_mode {
            vec![Constraint::Length(12), Constraint::Min(1)]
        } else {
            vec![Constraint::Percentage(30), Constraint::Percentage(70)]
        })
        .split(rows[0]);
    (cols[0], cols[1], rows[1])
}

fn rect_contains_point(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn pane_from_mouse_position(
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

fn should_use_fullish_theme(active: ActivePane, alt_screen_active: bool) -> bool {
    active == ActivePane::Shell && alt_screen_active
}

fn preview_overlay_is_interactive(
    presentation: Option<PreviewOverlayPresentation>,
) -> bool {
    presentation == Some(PreviewOverlayPresentation::InteractiveFullscreenDim)
}

fn next_preview_overlay_presentation(
    current: Option<PreviewOverlayPresentation>,
    alt_screen_active: bool,
) -> Option<PreviewOverlayPresentation> {
    if alt_screen_active {
        return Some(PreviewOverlayPresentation::InteractiveFullscreenDim);
    }
    current
}

fn should_auto_close_preview_overlay(
    presentation: Option<PreviewOverlayPresentation>,
    session_running: bool,
) -> bool {
    preview_overlay_is_interactive(presentation) && !session_running
}

#[derive(Debug, Clone)]
struct NavEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
    is_symlink: bool,
    file_type_char: char,
    mode: u32,
    nlink: u64,
    uid: u32,
    gid: u32,
    size: u64,
    mtime: i64,
}

fn navigation_entries(cwd: &Path) -> io::Result<Vec<NavEntry>> {
    let read_dir = fs::read_dir(cwd)?;
    let mut entries: Vec<NavEntry> = read_dir
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).ok();
            let is_dir = metadata.as_ref().map(|m| m.file_type().is_dir()).unwrap_or(false);
            let is_symlink = metadata
                .as_ref()
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            let file_type_char = metadata
                .as_ref()
                .map(|m| {
                    let ft = m.file_type();
                    if ft.is_dir() {
                        'd'
                    } else if ft.is_symlink() {
                        'l'
                    } else {
                        '-'
                    }
                })
                .unwrap_or('?');
            let mode = metadata.as_ref().map(|m| m.permissions().mode()).unwrap_or(0);
            let nlink = metadata.as_ref().map(|m| m.nlink()).unwrap_or(0);
            let uid = metadata.as_ref().map(|m| m.uid()).unwrap_or(0);
            let gid = metadata.as_ref().map(|m| m.gid()).unwrap_or(0);
            let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            let mtime = metadata.as_ref().map(|m| m.mtime()).unwrap_or(0);
            NavEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                path,
                is_dir,
                is_symlink,
                file_type_char,
                mode,
                nlink,
                uid,
                gid,
                size,
                mtime,
            }
        })
        .collect();
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    let parent = cwd.parent().unwrap_or(cwd).to_path_buf();
    let parent_meta = fs::symlink_metadata(&parent).ok();
    let parent_mode = parent_meta
        .as_ref()
        .map(|meta| meta.permissions().mode())
        .unwrap_or(0o755);
    entries.insert(
        0,
        NavEntry {
            name: "..".to_string(),
            path: parent,
            is_dir: true,
            is_symlink: false,
            file_type_char: 'd',
            mode: parent_mode,
            nlink: parent_meta.as_ref().map(|meta| meta.nlink()).unwrap_or(0),
            uid: parent_meta.as_ref().map(|meta| meta.uid()).unwrap_or(0),
            gid: parent_meta.as_ref().map(|meta| meta.gid()).unwrap_or(0),
            size: parent_meta.as_ref().map(|meta| meta.len()).unwrap_or(0),
            mtime: parent_meta.as_ref().map(|meta| meta.mtime()).unwrap_or(0),
        },
    );
    Ok(entries)
}

fn clamp_nav_selection(selected: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        selected.min(len.saturating_sub(1))
    }
}

fn nav_window_metrics(
    total_entries: usize,
    viewport_rows: usize,
    scroll_offset: usize,
) -> (usize, usize, usize, usize, usize, bool, usize) {
    if total_entries == 0 || viewport_rows == 0 {
        return (0, 0, 0, 0, total_entries, false, 0);
    }
    let viewport = viewport_rows.max(1);
    let total = total_entries;
    let scroll = scroll_offset.min(nav_max_scroll(total, viewport));
    let start = scroll;
    let end = start.saturating_add(viewport).min(total);
    let shown_start = if total == 0 { 0 } else { start + 1 };
    let shown_end = if total == 0 { 0 } else { end };
    (
        start,
        end,
        shown_start,
        shown_end,
        total,
        should_show_scrollbar(total, viewport),
        scroll,
    )
}

fn nav_max_scroll(total_entries: usize, viewport_rows: usize) -> usize {
    total_entries.saturating_sub(viewport_rows.max(1))
}

fn nav_scroll_for_selection(
    current_scroll: usize,
    selected: usize,
    total_entries: usize,
    viewport_rows: usize,
) -> usize {
    if total_entries == 0 || viewport_rows == 0 {
        return 0;
    }
    let viewport = viewport_rows.max(1);
    let selected = clamp_nav_selection(selected, total_entries);
    let mut scroll = current_scroll.min(nav_max_scroll(total_entries, viewport));
    if selected < scroll {
        scroll = selected;
    } else if selected >= scroll.saturating_add(viewport) {
        scroll = selected.saturating_add(1).saturating_sub(viewport);
    }
    scroll.min(nav_max_scroll(total_entries, viewport))
}

fn navigation_panel_text(
    cwd: &Path,
    entries: &[NavEntry],
    selected: usize,
    window_start: usize,
    window_end: usize,
    colors: &LsColorsTheme,
    fullish_shell_theme: bool,
    load_error: Option<&str>,
) -> Text<'static> {
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        format!("{}", cwd.display()),
        nav_style_for_theme(
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            fullish_shell_theme,
        ),
    )));

    if let Some(err) = load_error {
        lines.push(Line::from(Span::styled(
            format!("└── error: {err}"),
            nav_style_for_theme(Style::default().fg(Color::Red), fullish_shell_theme),
        )));
        return Text::from(lines);
    }

    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "└── (empty)",
            nav_style_for_theme(Style::default().fg(Color::DarkGray), fullish_shell_theme),
        )));
        return Text::from(lines);
    }

    let total = entries.len();
    let selected = clamp_nav_selection(selected, entries.len());
    let end = window_end.min(total);
    let start = window_start.min(end);
    for idx in start..end {
        let entry = &entries[idx];
        let is_last = idx + 1 == total;
        let connector = if is_last { "└──" } else { "├──" };
        let is_selected = idx == selected;
        let perms = simple_permission_bits(entry.file_type_char, entry.mode);
        let mut name = entry.name.clone();
        if entry.is_dir {
            name.push('/');
        }
        let name_style = navigation_name_style(
            colors,
            &entry.name,
            entry.is_dir,
            entry.is_symlink,
            entry.mode,
        );
        let icon = if entry.is_dir { "" } else { "" };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{connector} "),
                nav_row_selected_style(
                    nav_style_for_theme(Style::default().fg(Color::DarkGray), fullish_shell_theme),
                    is_selected,
                ),
            ),
            Span::styled(
                format!("{perms} "),
                nav_row_selected_style(
                    nav_style_for_theme(Style::default().fg(Color::White), fullish_shell_theme),
                    is_selected,
                ),
            ),
            Span::styled(
                format!("{icon} "),
                nav_row_selected_style(nav_style_for_theme(name_style, fullish_shell_theme), is_selected),
            ),
            Span::styled(
                name,
                nav_row_selected_style(nav_style_for_theme(name_style, fullish_shell_theme), is_selected),
            ),
        ]));
    }

    Text::from(lines)
}

#[cfg(test)]
fn navigation_tree_lines(cwd: &Path) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("{}", cwd.display()));

    let entries = match navigation_entries(cwd) {
        Ok(entries) => entries,
        Err(err) => {
            lines.push(format!("└── error: {err}"));
            return lines;
        }
    };

    if entries.is_empty() {
        lines.push("└── (empty)".to_string());
        return lines;
    }

    let total = entries.len();
    for (idx, entry) in entries.iter().enumerate() {
        let is_last = idx + 1 == total;
        let connector = if is_last { "└──" } else { "├──" };
        let perms = simple_permission_bits(entry.file_type_char, entry.mode);
        let mut name = entry.name.clone();
        if entry.is_dir {
            name.push('/');
        }
        let icon = if entry.is_dir { "" } else { "" };
        lines.push(format!("{connector} {perms} {icon} {name}"));
    }

    lines
}

fn preview_content_for_selected_entry(entry: Option<&NavEntry>, depth: usize) -> (PreviewMode, String) {
    let Some(selected) = entry else {
        return (PreviewMode::Empty, String::new());
    };
    if !selected.is_dir {
        return (PreviewMode::Empty, String::new());
    }
    let lines = preview_directory_tree_lines(&selected.path, depth.max(1));
    (PreviewMode::DirectoryTree, lines.join("\n"))
}

fn preview_panel_text(
    mode: PreviewMode,
    cached_text: &str,
    selected_path: Option<&Path>,
    selected_entry: Option<&NavEntry>,
    hovered_label: Option<&str>,
    depth: usize,
    identity: &EffectiveIdentity,
    colors: &LsColorsTheme,
    config: &ConfigState,
    editor_program: &str,
    panel_width: u16,
    panel_height: u16,
    fullish_shell_theme: bool,
) -> Text<'static> {
    if mode == PreviewMode::DirectoryTree {
        if let Some(path) = selected_path {
            return preview_directory_panel_text(path, hovered_label, depth, colors, fullish_shell_theme);
        }
    }
    if mode == PreviewMode::FileText {
        if let Some(entry) = selected_entry {
            return preview_file_commands_panel_text(
                entry,
                config,
                editor_program,
                identity,
                panel_width,
                panel_height,
                fullish_shell_theme,
            );
        }
    }
    Text::from(cached_text.to_string())
}

fn preview_directory_panel_text(
    root: &Path,
    hovered_label: Option<&str>,
    depth: usize,
    colors: &LsColorsTheme,
    fullish_shell_theme: bool,
) -> Text<'static> {
    let header = preview_directory_header_label(root, hovered_label);
    let header_style = nav_style_for_theme(
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        fullish_shell_theme,
    );
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(" ".to_string(), header_style),
        Span::styled(header, header_style),
    ]));
    append_preview_directory_level_text(
        root,
        "",
        clamp_preview_depth(depth, depth.max(1)),
        colors,
        fullish_shell_theme,
        &mut lines,
    );
    Text::from(lines)
}

fn preview_directory_header_label(root: &Path, hovered_label: Option<&str>) -> String {
    if let Some(label) = hovered_label {
        return if label.ends_with('/') {
            label.to_string()
        } else {
            format!("{label}/")
        };
    }
    if let Some(name) = root.file_name().and_then(|name| name.to_str()) {
        return format!("{name}/");
    }
    format!("{}", root.display())
}

fn append_preview_directory_level_text(
    path: &Path,
    prefix: &str,
    remaining_depth: usize,
    colors: &LsColorsTheme,
    fullish_shell_theme: bool,
    lines: &mut Vec<Line<'static>>,
) {
    if remaining_depth == 0 {
        return;
    }

    let connector_style = nav_style_for_theme(Style::default().fg(Color::DarkGray), fullish_shell_theme);
    let perms_style = nav_style_for_theme(Style::default().fg(Color::White), fullish_shell_theme);

    let entries = match preview_directory_entries(path) {
        Ok(entries) => entries,
        Err(err) => {
            lines.push(Line::from(Span::styled(
                format!("{prefix}└── error: {err}"),
                nav_style_for_theme(Style::default().fg(Color::Red), fullish_shell_theme),
            )));
            return;
        }
    };
    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("{prefix}└── (empty)"),
            nav_style_for_theme(Style::default().fg(Color::DarkGray), fullish_shell_theme),
        )));
        return;
    }

    let total = entries.len();
    for (idx, entry) in entries.iter().enumerate() {
        let is_last = idx + 1 == total;
        let connector = if is_last { "└──" } else { "├──" };
        let perms = simple_permission_bits(entry.file_type_char, entry.mode);
        let mut name = entry.name.clone();
        if entry.is_dir {
            name.push('/');
        }
        let name_style = nav_style_for_theme(
            navigation_name_style(
                colors,
                &entry.name,
                entry.is_dir,
                entry.is_symlink,
                entry.mode,
            ),
            fullish_shell_theme,
        );
        let icon = if entry.is_dir { "" } else { "" };
        lines.push(Line::from(vec![
            Span::styled(prefix.to_string(), connector_style),
            Span::styled(format!("{connector} "), connector_style),
            Span::styled(format!("{perms} "), perms_style),
            Span::styled(format!("{icon} "), name_style),
            Span::styled(name, name_style),
        ]));
        if entry.is_dir && remaining_depth > 1 {
            let child_prefix = if is_last {
                format!("{prefix}    ")
            } else {
                format!("{prefix}│   ")
            };
            append_preview_directory_level_text(
                &entry.path,
                &child_prefix,
                remaining_depth - 1,
                colors,
                fullish_shell_theme,
                lines,
            );
        }
    }
}

#[cfg(test)]
fn preview_file_preview_text(path: &Path) -> String {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => return format!("error: {err}"),
    };
    if bytes.is_empty() {
        return "(empty file)".to_string();
    }
    if bytes.contains(&0) {
        return format!("binary file ({} bytes)", bytes.len());
    }
    let limit = PREVIEW_FILE_MAX_BYTES.min(bytes.len());
    let mut text = String::from_utf8_lossy(&bytes[..limit]).into_owned();
    if bytes.len() > limit {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str("... truncated ...");
    }
    text
}

fn preview_file_commands_panel_text(
    entry: &NavEntry,
    config: &ConfigState,
    editor_program: &str,
    identity: &EffectiveIdentity,
    panel_width: u16,
    panel_height: u16,
    fullish_shell_theme: bool,
) -> Text<'static> {
    let lines = available_preview_file_commands(entry, config, editor_program, identity);
    let key_style = footer_key_style(fullish_shell_theme);
    let text_style = nav_style_for_theme(Style::default().fg(Color::White), fullish_shell_theme);
    let muted_style = nav_style_for_theme(Style::default().fg(Color::DarkGray), fullish_shell_theme);
    let compact_commands_mode = panel_width <= 10;

    if lines.is_empty() {
        let message = "no preview commands";
        let left_pad = centered_left_padding(message.chars().count(), panel_width as usize);
        let top_pad = centered_top_padding(1, panel_height as usize);
        let mut out = Vec::new();
        for _ in 0..top_pad {
            out.push(Line::from(""));
        }
        out.push(Line::from(vec![
            Span::raw(" ".repeat(left_pad)),
            Span::styled(message.to_string(), muted_style),
        ]));
        return Text::from(out);
    }

    let content_widths = lines
        .iter()
        .map(|(key, command)| {
            let key_label = format!("Esc+{key}");
            if compact_commands_mode {
                key_label.chars().count()
            } else {
                key_label
                    .chars()
                    .count()
                    .saturating_add(3)
                    .saturating_add(command.chars().count())
            }
        })
        .collect::<Vec<usize>>();
    let block_width = content_widths.iter().copied().max().unwrap_or(0);
    let block_left_pad = centered_left_padding(block_width, panel_width as usize);
    let top_pad = centered_top_padding(lines.len(), panel_height as usize);
    let mut out = Vec::new();
    for _ in 0..top_pad {
        out.push(Line::from(""));
    }
    for (key, command) in lines {
        let key_label = format!("Esc+{key}");
        if compact_commands_mode {
            out.push(Line::from(vec![
                Span::raw(" ".repeat(block_left_pad)),
                Span::styled(key_label, key_style),
            ]));
            continue;
        }
        out.push(Line::from(vec![
            Span::raw(" ".repeat(block_left_pad)),
            Span::styled(key_label, key_style),
            Span::styled(" : ".to_string(), text_style),
            Span::styled(command, text_style),
        ]));
    }
    Text::from(out)
}

fn navigation_file_command_action(
    selected_entry: Option<&NavEntry>,
    key: char,
    config: &ConfigState,
    editor_program: &str,
    identity: &EffectiveIdentity,
) -> Option<NavigationFileCommandAction> {
    let entry = selected_entry?;
    if entry.is_dir {
        return None;
    }
    let trigger = key.to_ascii_lowercase();
    let command = available_preview_file_commands(entry, config, editor_program, identity)
        .into_iter()
        .find_map(|(shortcut, command)| (shortcut == trigger).then_some(command))?;
    match trigger {
        'r' => Some(NavigationFileCommandAction::RunReadInPreview(command)),
        'w' => Some(NavigationFileCommandAction::RunWriteInPreview(command)),
        'x' => Some(NavigationFileCommandAction::PrefillShell(command)),
        _ => None,
    }
}

fn centered_left_padding(content_width: usize, total_width: usize) -> usize {
    total_width.saturating_sub(content_width) / 2
}

fn centered_top_padding(content_lines: usize, total_lines: usize) -> usize {
    total_lines.saturating_sub(content_lines) / 2
}

fn available_preview_file_commands(
    entry: &NavEntry,
    config: &ConfigState,
    editor_program: &str,
    identity: &EffectiveIdentity,
) -> Vec<(char, String)> {
    if entry.is_dir {
        return Vec::new();
    }
    let extension = Path::new(&entry.name)
        .extension()
        .and_then(|value| value.to_str())
        .map(normalize_extension);
    let matched_rule = extension.as_deref().and_then(|ext| {
        config
            .extension_rules
            .iter()
            .find(|rule| normalize_extension(&rule.extension) == ext)
    });
    let fallback_rule = default_extension_rule("fallback");
    let rule = matched_rule.unwrap_or(&fallback_rule);

    let access = effective_access_for_entry(entry, identity);
    let mut out = Vec::new();
    if command_enabled_for_file(&rule.read_cmd) && access.read {
        out.push((
            'r',
            resolve_preview_command_template(&rule.read_cmd, &entry.name, editor_program),
        ));
    }
    if command_enabled_for_file(&rule.write_cmd) && access.write {
        out.push((
            'w',
            resolve_preview_command_template(&rule.write_cmd, &entry.name, editor_program),
        ));
    }
    if command_enabled_for_file(&rule.exec_cmd) && access.exec {
        out.push((
            'x',
            resolve_preview_command_template(&rule.exec_cmd, &entry.name, editor_program),
        ));
    }
    out
}

fn command_enabled_for_file(template: &str) -> bool {
    let trimmed = template.trim();
    !trimmed.is_empty() && trimmed != "--"
}

fn resolve_preview_command_template(template: &str, file_name: &str, editor_program: &str) -> String {
    template
        .replace("$EDITOR", editor_program)
        .replace("{file}", file_name)
}

fn clamp_preview_depth(depth: usize, max_depth: usize) -> usize {
    depth.max(1).min(max_depth.max(1))
}

fn preview_directory_entries(path: &Path) -> io::Result<Vec<NavEntry>> {
    let mut entries = navigation_entries(path)?;
    entries.retain(|entry| entry.name != "..");
    Ok(entries)
}

fn preview_directory_tree_lines(root: &Path, depth: usize) -> Vec<String> {
    let mut lines = vec![format!("{}", root.display())];
    append_preview_directory_level(root, "", clamp_preview_depth(depth, depth.max(1)), &mut lines);
    lines
}

fn append_preview_directory_level(path: &Path, prefix: &str, remaining_depth: usize, lines: &mut Vec<String>) {
    if remaining_depth == 0 {
        return;
    }
    let entries = match preview_directory_entries(path) {
        Ok(entries) => entries,
        Err(err) => {
            lines.push(format!("{prefix}└── error: {err}"));
            return;
        }
    };
    if entries.is_empty() {
        lines.push(format!("{prefix}└── (empty)"));
        return;
    }
    let total = entries.len();
    for (idx, entry) in entries.iter().enumerate() {
        let is_last = idx + 1 == total;
        let connector = if is_last { "└──" } else { "├──" };
        let perms = simple_permission_bits(entry.file_type_char, entry.mode);
        let mut name = entry.name.clone();
        if entry.is_dir {
            name.push('/');
        }
        let icon = if entry.is_dir { "" } else { "" };
        lines.push(format!("{prefix}{connector} {perms} {icon} {name}"));
        if entry.is_dir && remaining_depth > 1 {
            let child_prefix = if is_last {
                format!("{prefix}    ")
            } else {
                format!("{prefix}│   ")
            };
            append_preview_directory_level(&entry.path, &child_prefix, remaining_depth - 1, lines);
        }
    }
}

fn simple_permission_bits(file_type_char: char, mode: u32) -> String {
    let type_char = match file_type_char {
        'd' => 'd',
        'l' => 'l',
        _ => '-',
    };
    let read = if mode & 0o444 != 0 { 'r' } else { '-' };
    let write = if mode & 0o222 != 0 { 'w' } else { '-' };
    let exec = if mode & 0o111 != 0 { 'x' } else { '-' };
    format!("{type_char}{read}{write}{exec}")
}

fn effective_access_for_entry(entry: &NavEntry, identity: &EffectiveIdentity) -> EffectiveAccess {
    if let Some(access) = kernel_effective_access_for_path(&entry.path) {
        access
    } else {
        effective_access_from_mode(entry.mode, entry.uid, entry.gid, entry.file_type_char, identity)
    }
}

fn kernel_effective_access_for_path(path: &Path) -> Option<EffectiveAccess> {
    Some(EffectiveAccess {
        read: syscall_path_access(path, libc::R_OK).ok()?,
        write: syscall_path_access(path, libc::W_OK).ok()?,
        exec: syscall_path_access(path, libc::X_OK).ok()?,
    })
}

fn syscall_path_access(path: &Path, mode: i32) -> io::Result<bool> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
    let rc = unsafe { libc::faccessat(libc::AT_FDCWD, c_path.as_ptr(), mode, libc::AT_EACCESS) };
    if rc == 0 {
        return Ok(true);
    }

    let err = io::Error::last_os_error();
    match err.raw_os_error() {
        Some(code)
            if code == libc::EACCES
                || code == libc::EPERM
                || code == libc::ENOENT
                || code == libc::ENOTDIR
                || code == libc::ELOOP =>
        {
            Ok(false)
        }
        _ => Err(err),
    }
}

fn effective_access_from_mode(
    mode: u32,
    owner_uid: u32,
    owner_gid: u32,
    file_type_char: char,
    identity: &EffectiveIdentity,
) -> EffectiveAccess {
    if identity.euid == 0 {
        let exec = file_type_char == 'd' || mode & 0o111 != 0;
        return EffectiveAccess {
            read: true,
            write: true,
            exec,
        };
    }

    let (read_bit, write_bit, exec_bit) = if identity.euid == owner_uid {
        (0o400, 0o200, 0o100)
    } else if identity.in_group(owner_gid) {
        (0o040, 0o020, 0o010)
    } else {
        (0o004, 0o002, 0o001)
    };

    EffectiveAccess {
        read: mode & read_bit != 0,
        write: mode & write_bit != 0,
        exec: mode & exec_bit != 0,
    }
}

fn nav_long_listing(entry: &NavEntry) -> String {
    let perms = permission_bits(entry.file_type_char, entry.mode);
    let owner = username_for_uid(entry.uid).unwrap_or_else(|| entry.uid.to_string());
    let group = group_name_for_gid(entry.gid).unwrap_or_else(|| entry.gid.to_string());
    let when = format_epoch_short(entry.mtime).unwrap_or_else(|| "-".to_string());
    format!(
        "{perms}  {} {} {}  {} {}",
        entry.nlink, owner, group, entry.size, when
    )
}

fn username_for_uid(uid: u32) -> Option<String> {
    let content = fs::read_to_string("/etc/passwd").ok()?;
    for line in content.lines() {
        let mut fields = line.split(':');
        let name = fields.next()?;
        let _password = fields.next()?;
        let uid_field = fields.next()?;
        if uid_field.parse::<u32>().ok()? == uid {
            return Some(name.to_string());
        }
    }
    None
}

fn group_name_for_gid(gid: u32) -> Option<String> {
    let content = fs::read_to_string("/etc/group").ok()?;
    for line in content.lines() {
        let mut fields = line.split(':');
        let name = fields.next()?;
        let _password = fields.next()?;
        let gid_field = fields.next()?;
        if gid_field.parse::<u32>().ok()? == gid {
            return Some(name.to_string());
        }
    }
    None
}

fn format_epoch_short(epoch_secs: i64) -> Option<String> {
    if epoch_secs < 0 {
        return None;
    }
    let output = Command::new("date")
        .arg("-d")
        .arg(format!("@{epoch_secs}"))
        .arg("+%b %d %H:%M")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_string())
}

fn permission_bits(file_type_char: char, mode: u32) -> String {
    let mut out = String::with_capacity(10);
    out.push(file_type_char);
    let triplets = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];
    for (bit, ch) in triplets {
        out.push(if mode & bit != 0 { ch } else { '-' });
    }
    out
}

#[derive(Debug, Clone)]
struct LsColorsTheme {
    type_styles: HashMap<String, Style>,
    suffix_styles: Vec<(String, Style)>,
}

impl LsColorsTheme {
    fn from_env() -> Self {
        let mut theme = Self::fallback();
        if let Ok(raw) = std::env::var("LS_COLORS") {
            theme.apply(&raw);
        }
        theme
    }

    fn fallback() -> Self {
        let mut theme = Self {
            type_styles: HashMap::new(),
            suffix_styles: Vec::new(),
        };
        theme.apply("rs=0:no=0:fi=0:di=01;34:ln=01;36:ex=01;32");
        theme
    }

    fn apply(&mut self, raw: &str) {
        for segment in raw.split(':') {
            let Some((raw_key, raw_value)) = segment.split_once('=') else {
                continue;
            };
            let key = raw_key.trim().to_ascii_lowercase();
            if key.is_empty() {
                continue;
            }
            let style = sgr_to_style(raw_value.trim());
            if let Some(raw_suffix) = key.strip_prefix("*.") {
                if raw_suffix.is_empty() {
                    continue;
                }
                let suffix = format!(".{}", raw_suffix.to_ascii_lowercase());
                self.suffix_styles.retain(|(existing, _)| existing != &suffix);
                self.suffix_styles.push((suffix, style));
            } else {
                self.type_styles.insert(key, style);
            }
        }
    }

    fn style_for_entry(&self, name: &str, is_dir: bool, is_symlink: bool, mode: u32) -> Style {
        if is_dir {
            return self.type_style("di");
        }
        if is_symlink {
            return self.type_style("ln");
        }

        let lower_name = name.to_ascii_lowercase();
        if let Some(style) = self
            .suffix_styles
            .iter()
            .filter(|(suffix, _)| lower_name.ends_with(suffix))
            .max_by_key(|(suffix, _)| suffix.len())
            .map(|(_, style)| *style)
        {
            return style;
        }

        if mode & 0o111 != 0 {
            return self.type_style("ex");
        }
        self.type_style("fi")
    }

    fn type_style(&self, key: &str) -> Style {
        self.type_styles
            .get(key)
            .copied()
            .or_else(|| self.type_styles.get("no").copied())
            .unwrap_or_else(|| Style::default().fg(Color::White))
    }
}

fn navigation_name_style(
    colors: &LsColorsTheme,
    name: &str,
    is_dir: bool,
    is_symlink: bool,
    mode: u32,
) -> Style {
    colors.style_for_entry(name, is_dir, is_symlink, mode)
}

fn nav_style_for_theme(style: Style, fullish_shell_theme: bool) -> Style {
    if fullish_shell_theme {
        style.fg(Color::DarkGray)
    } else {
        style
    }
}

fn nav_row_selected_style(style: Style, selected: bool) -> Style {
    if selected {
        style.bg(Color::LightBlue).fg(Color::Black)
    } else {
        style
    }
}

fn render_panel_status(
    frame: &mut ratatui::Frame<'_>,
    panel_area: ratatui::layout::Rect,
    shown_start: usize,
    shown_end: usize,
    total: usize,
    style: Style,
    show: bool,
) {
    if !show || panel_area.width <= 4 || panel_area.height == 0 || total == 0 {
        return;
    }
    let status = format!("{shown_start}-{shown_end} of {total}─╮");
    let status_width = status.chars().count() as u16;
    if status_width == 0 || panel_area.width <= status_width.saturating_add(1) {
        return;
    }
    let status_rect = ratatui::layout::Rect {
        x: panel_area
            .x
            .saturating_add(panel_area.width.saturating_sub(status_width)),
        y: panel_area.y,
        width: status_width,
        height: 1,
    };
    frame.render_widget(Paragraph::new(status).style(style), status_rect);
}

fn dim_rendered_area(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect) {
    let buf = frame.buffer_mut();
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            buf[(x, y)].set_style(Style::default().fg(Color::DarkGray).bg(Color::Black));
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100_u16.saturating_sub(percent_y)) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100_u16.saturating_sub(percent_y)) / 2),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100_u16.saturating_sub(percent_x)) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100_u16.saturating_sub(percent_x)) / 2),
        ])
        .split(vertical[1]);
    horizontal[1]
}

fn normalize_extension(raw: &str) -> String {
    raw.trim().trim_start_matches('.').to_ascii_lowercase()
}

fn extension_exists(config: &ConfigState, extension: &str) -> bool {
    let normalized = normalize_extension(extension);
    config
        .extension_rules
        .iter()
        .any(|rule| rule.extension == normalized)
}

fn extension_is_empty(extension: &str) -> bool {
    normalize_extension(extension).is_empty()
}

fn duplicate_extension_for_rule(
    config: &ConfigState,
    rule_index: usize,
    extension: &str,
) -> Option<String> {
    let normalized = normalize_extension(extension);
    config
        .extension_rules
        .iter()
        .enumerate()
        .find_map(|(idx, rule)| {
            if idx != rule_index && rule.extension == normalized {
                Some(normalized.clone())
            } else {
                None
            }
        })
}

fn extension_validation_error_for_rule(
    config: &ConfigState,
    rule_index: usize,
    extension: &str,
) -> Option<String> {
    if extension_is_empty(extension) {
        return Some("extension name cannot be empty".to_string());
    }
    duplicate_extension_for_rule(config, rule_index, extension)
        .map(|duplicate| format!("duplicate extension '.{duplicate}' not allowed"))
}

fn first_empty_extension(config: &ConfigState) -> bool {
    config
        .extension_rules
        .iter()
        .any(|rule| extension_is_empty(&rule.extension))
}

fn first_duplicate_extension(config: &ConfigState) -> Option<String> {
    let mut seen = HashSet::new();
    for rule in &config.extension_rules {
        let normalized = normalize_extension(&rule.extension);
        if !seen.insert(normalized.clone()) {
            return Some(normalized);
        }
    }
    None
}

fn next_available_extension_name(config: &ConfigState, base: &str) -> String {
    let normalized_base = normalize_extension(base);
    let base = if normalized_base.is_empty() {
        "new".to_string()
    } else {
        normalized_base
    };
    if !extension_exists(config, &base) {
        return base;
    }
    let mut suffix = 2usize;
    loop {
        let candidate = format!("{base}{suffix}");
        if !extension_exists(config, &candidate) {
            return candidate;
        }
        suffix = suffix.saturating_add(1);
    }
}

fn default_extension_rule(extension: &str) -> ExtensionCommandRule {
    ExtensionCommandRule {
        extension: normalize_extension(extension),
        read_cmd: "bat {file}".to_string(),
        write_cmd: "$EDITOR {file}".to_string(),
        exec_cmd: "--".to_string(),
    }
}

fn clamp_char_boundary(text: &str, cursor: usize) -> usize {
    let mut safe_cursor = cursor.min(text.len());
    while safe_cursor > 0 && !text.is_char_boundary(safe_cursor) {
        safe_cursor -= 1;
    }
    safe_cursor
}

fn previous_char_boundary(text: &str, cursor: usize) -> usize {
    let safe_cursor = clamp_char_boundary(text, cursor);
    if safe_cursor == 0 {
        return 0;
    }
    text[..safe_cursor]
        .char_indices()
        .last()
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

fn next_char_boundary(text: &str, cursor: usize) -> usize {
    let safe_cursor = clamp_char_boundary(text, cursor);
    if safe_cursor >= text.len() {
        return text.len();
    }
    let step = text[safe_cursor..]
        .chars()
        .next()
        .map(|ch| ch.len_utf8())
        .unwrap_or(0);
    safe_cursor.saturating_add(step).min(text.len())
}

fn cursor_spans(text: &str, cursor: usize, text_style: Style, cursor_style: Style) -> Vec<Span<'static>> {
    let safe_cursor = clamp_char_boundary(text, cursor);
    let mut spans = Vec::new();
    let (left, right) = text.split_at(safe_cursor);
    if !left.is_empty() {
        spans.push(Span::styled(left.to_string(), text_style));
    }
    if right.is_empty() {
        spans.push(Span::styled(" ".to_string(), cursor_style));
        return spans;
    }
    let mut chars = right.chars();
    let cursor_char = chars.next().unwrap_or(' ');
    spans.push(Span::styled(cursor_char.to_string(), cursor_style));
    let rest: String = chars.collect();
    if !rest.is_empty() {
        spans.push(Span::styled(rest, text_style));
    }
    spans
}

fn config_field_value<'a>(rule: &'a ExtensionCommandRule, field: ConfigField) -> &'a str {
    match field {
        ConfigField::Extension => &rule.extension,
        ConfigField::Read => &rule.read_cmd,
        ConfigField::Write => &rule.write_cmd,
        ConfigField::Exec => &rule.exec_cmd,
    }
}

fn set_config_field(rule: &mut ExtensionCommandRule, field: ConfigField, value: &str) {
    match field {
        ConfigField::Extension => rule.extension = normalize_extension(value),
        ConfigField::Read => rule.read_cmd = value.to_string(),
        ConfigField::Write => rule.write_cmd = value.to_string(),
        ConfigField::Exec => rule.exec_cmd = value.to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigRuleDiffKind {
    None,
    Added,
    Removed,
    Modified,
}

#[derive(Debug, Clone)]
struct DisplayConfigRule {
    rule: ExtensionCommandRule,
    baseline_rule: Option<ExtensionCommandRule>,
    current_index: Option<usize>,
    diff_kind: ConfigRuleDiffKind,
}

fn config_display_rules(
    config: &ConfigState,
    saved_config: &ConfigState,
    dirty: bool,
) -> Vec<DisplayConfigRule> {
    if !dirty {
        return config
            .extension_rules
            .iter()
            .enumerate()
            .map(|(idx, rule)| DisplayConfigRule {
                rule: rule.clone(),
                baseline_rule: Some(rule.clone()),
                current_index: Some(idx),
                diff_kind: ConfigRuleDiffKind::None,
            })
            .collect();
    }

    let mut display_rules = Vec::new();
    let mut current_indices_by_extension: HashMap<String, Vec<usize>> = HashMap::new();
    for idx in (0..config.extension_rules.len()).rev() {
        let extension = config.extension_rules[idx].extension.clone();
        current_indices_by_extension
            .entry(extension)
            .or_default()
            .push(idx);
    }
    let mut matched_current = vec![false; config.extension_rules.len()];

    for saved_rule in &saved_config.extension_rules {
        let current_idx = current_indices_by_extension
            .get_mut(&saved_rule.extension)
            .and_then(|indices| indices.pop());
        if let Some(idx) = current_idx {
            matched_current[idx] = true;
            let current_rule = config.extension_rules[idx].clone();
            let diff_kind = if current_rule == *saved_rule {
                ConfigRuleDiffKind::None
            } else {
                ConfigRuleDiffKind::Modified
            };
            display_rules.push(DisplayConfigRule {
                rule: current_rule,
                baseline_rule: Some(saved_rule.clone()),
                current_index: Some(idx),
                diff_kind,
            });
        } else {
            display_rules.push(DisplayConfigRule {
                rule: saved_rule.clone(),
                baseline_rule: Some(saved_rule.clone()),
                current_index: None,
                diff_kind: ConfigRuleDiffKind::Removed,
            });
        }
    }

    for (idx, rule) in config.extension_rules.iter().enumerate() {
        if matched_current[idx] {
            continue;
        }
        display_rules.push(DisplayConfigRule {
            rule: rule.clone(),
            baseline_rule: None,
            current_index: Some(idx),
            diff_kind: ConfigRuleDiffKind::Added,
        });
    }

    display_rules
}

fn config_panel_text(
    config: &ConfigState,
    saved_config: &ConfigState,
    editor: &ConfigEditor,
    fullish_shell_theme: bool,
) -> Text<'static> {
    const EXTENSION_KEY_WIDTH: usize = 7;
    let label_style = if fullish_shell_theme {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };
    let hint_key_style = if fullish_shell_theme {
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
    };
    let command_separator_style = if fullish_shell_theme {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };
    let field_style = if fullish_shell_theme {
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::LightBlue).add_modifier(Modifier::BOLD)
    };
    let selected_style = if fullish_shell_theme {
        Style::default().fg(Color::Black).bg(Color::DarkGray)
    } else {
        Style::default().fg(Color::Black).bg(Color::LightBlue)
    };
    let edit_text_style = if fullish_shell_theme {
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    };
    let edit_cursor_style = if fullish_shell_theme {
        Style::default().fg(Color::Black).bg(Color::DarkGray)
    } else {
        Style::default().fg(Color::Black).bg(Color::White)
    };
    let diff_removed_style = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
    let diff_added_style = Style::default().fg(Color::Green).add_modifier(Modifier::BOLD);

    let mut top_spans = Vec::new();
    top_spans.push(Span::styled("Rule: ", label_style));
    append_key_with_white_slashes(
        &mut top_spans,
        "↑/↓",
        hint_key_style,
        command_separator_style,
    );
    top_spans.push(Span::styled(" | Field: ", label_style));
    append_key_with_white_slashes(
        &mut top_spans,
        "←/→",
        hint_key_style,
        command_separator_style,
    );
    top_spans.push(Span::styled(" | Edit: ", label_style));
    append_key_with_white_slashes(
        &mut top_spans,
        "Enter",
        hint_key_style,
        command_separator_style,
    );
    top_spans.push(Span::styled(" | Del-Rule: ", label_style));
    append_key_with_white_slashes(
        &mut top_spans,
        "Ctrl+Del",
        hint_key_style,
        command_separator_style,
    );
    top_spans.push(Span::styled(" | New-Rule: ", label_style));
    append_key_with_white_slashes(
        &mut top_spans,
        "Ctrl+n",
        hint_key_style,
        command_separator_style,
    );
    let mut lines = vec![Line::from(top_spans)];
    lines.push(Line::from(""));
    let display_rules = config_display_rules(config, saved_config, editor.dirty);
    if display_rules.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "(no extension rules yet - press Ctrl+n to add one)",
            label_style,
        )]));
    }

    for display_rule in &display_rules {
        let rule = &display_rule.rule;
        let baseline_rule = display_rule.baseline_rule.as_ref();
        let selected_rule = display_rule
            .current_index
            .is_some_and(|idx| idx == editor.selected_rule);
        let editable_rule = display_rule.current_index.is_some();
        let rule_diff_style = match display_rule.diff_kind {
            ConfigRuleDiffKind::None => None,
            ConfigRuleDiffKind::Added => Some(diff_added_style),
            ConfigRuleDiffKind::Removed => Some(diff_removed_style),
            ConfigRuleDiffKind::Modified => None,
        };
        let extension_changed = baseline_rule.is_some_and(|baseline| baseline.extension != rule.extension);
        let read_changed = baseline_rule.is_some_and(|baseline| baseline.read_cmd != rule.read_cmd);
        let write_changed = baseline_rule.is_some_and(|baseline| baseline.write_cmd != rule.write_cmd);
        let exec_changed = baseline_rule.is_some_and(|baseline| baseline.exec_cmd != rule.exec_cmd);
        let extension_style = if selected_rule && editor.selected_field == ConfigField::Extension {
            selected_style
        } else if display_rule.diff_kind == ConfigRuleDiffKind::Modified && extension_changed {
            diff_added_style
        } else if let Some(style) = rule_diff_style {
            style
        } else {
            field_style
        };
        let read_style = if selected_rule && editor.selected_field == ConfigField::Read {
            selected_style
        } else if display_rule.diff_kind == ConfigRuleDiffKind::Modified && read_changed {
            diff_added_style
        } else if let Some(style) = rule_diff_style {
            style
        } else {
            field_style
        };
        let write_style = if selected_rule && editor.selected_field == ConfigField::Write {
            selected_style
        } else if display_rule.diff_kind == ConfigRuleDiffKind::Modified && write_changed {
            diff_added_style
        } else if let Some(style) = rule_diff_style {
            style
        } else {
            field_style
        };
        let exec_style = if selected_rule && editor.selected_field == ConfigField::Exec {
            selected_style
        } else if display_rule.diff_kind == ConfigRuleDiffKind::Modified && exec_changed {
            diff_added_style
        } else if let Some(style) = rule_diff_style {
            style
        } else {
            field_style
        };
        let marker = if selected_rule { "❯" } else { " " };
        let mut extension_line = vec![Span::styled(format!("{marker} "), label_style)];
        let extension_visible_len = if selected_rule
            && editable_rule
            && editor.selected_field == ConfigField::Extension
            && editor.editing
        {
            1 + editor.input_buffer.chars().count()
        } else {
            1 + rule.extension.chars().count()
        };
        let extension_padding = " ".repeat(
            EXTENSION_KEY_WIDTH
                .saturating_sub(extension_visible_len)
                .max(1),
        );
        if selected_rule
            && editable_rule
            && editor.selected_field == ConfigField::Extension
            && editor.editing
        {
            extension_line.push(Span::styled(".".to_string(), edit_text_style));
            extension_line.extend(cursor_spans(
                &editor.input_buffer,
                editor.input_cursor,
                edit_text_style,
                edit_cursor_style,
            ));
        } else {
            extension_line.push(Span::styled(format!(".{}", rule.extension), extension_style));
        }
        extension_line.push(Span::styled(
            format!("{extension_padding}: extension:"),
            label_style,
        ));
        if display_rule.diff_kind == ConfigRuleDiffKind::Modified
            && extension_changed
            && !(selected_rule
                && editable_rule
                && editor.selected_field == ConfigField::Extension
                && editor.editing)
        {
            if let Some(baseline) = baseline_rule {
                extension_line.push(Span::styled("  ".to_string(), label_style));
                extension_line.push(Span::styled(
                    format!("- .{}", baseline.extension),
                    diff_removed_style,
                ));
                extension_line.push(Span::styled(" ".to_string(), label_style));
                extension_line.push(Span::styled(
                    format!("+ .{}", rule.extension),
                    diff_added_style,
                ));
            }
        }
        lines.push(Line::from(extension_line));

        if selected_rule && editable_rule && editor.selected_field == ConfigField::Read && editor.editing {
            let mut line = vec![Span::styled("    read : ", label_style)];
            line.extend(cursor_spans(
                &editor.input_buffer,
                editor.input_cursor,
                edit_text_style,
                edit_cursor_style,
            ));
            lines.push(Line::from(line));
        } else {
            let mut line = vec![Span::styled("    read : ", label_style)];
            if display_rule.diff_kind == ConfigRuleDiffKind::Modified && read_changed {
                if let Some(baseline) = baseline_rule {
                    line.push(Span::styled(
                        format!("- {}", baseline.read_cmd),
                        diff_removed_style,
                    ));
                    line.push(Span::styled(" ".to_string(), label_style));
                    line.push(Span::styled(format!("+ {}", rule.read_cmd), diff_added_style));
                } else {
                    line.push(Span::styled(rule.read_cmd.clone(), read_style));
                }
            } else {
                line.push(Span::styled(rule.read_cmd.clone(), read_style));
            }
            lines.push(Line::from(line));
        }

        if selected_rule && editable_rule && editor.selected_field == ConfigField::Write && editor.editing {
            let mut line = vec![Span::styled("    write: ", label_style)];
            line.extend(cursor_spans(
                &editor.input_buffer,
                editor.input_cursor,
                edit_text_style,
                edit_cursor_style,
            ));
            lines.push(Line::from(line));
        } else {
            let mut line = vec![Span::styled("    write: ", label_style)];
            if display_rule.diff_kind == ConfigRuleDiffKind::Modified && write_changed {
                if let Some(baseline) = baseline_rule {
                    line.push(Span::styled(
                        format!("- {}", baseline.write_cmd),
                        diff_removed_style,
                    ));
                    line.push(Span::styled(" ".to_string(), label_style));
                    line.push(Span::styled(
                        format!("+ {}", rule.write_cmd),
                        diff_added_style,
                    ));
                } else {
                    line.push(Span::styled(rule.write_cmd.clone(), write_style));
                }
            } else {
                line.push(Span::styled(rule.write_cmd.clone(), write_style));
            }
            lines.push(Line::from(line));
        }

        if selected_rule && editable_rule && editor.selected_field == ConfigField::Exec && editor.editing {
            let mut line = vec![Span::styled("    exec : ", label_style)];
            line.extend(cursor_spans(
                &editor.input_buffer,
                editor.input_cursor,
                edit_text_style,
                edit_cursor_style,
            ));
            lines.push(Line::from(line));
        } else {
            let mut line = vec![Span::styled("    exec : ", label_style)];
            if display_rule.diff_kind == ConfigRuleDiffKind::Modified && exec_changed {
                if let Some(baseline) = baseline_rule {
                    line.push(Span::styled(
                        format!("- {}", baseline.exec_cmd),
                        diff_removed_style,
                    ));
                    line.push(Span::styled(" ".to_string(), label_style));
                    line.push(Span::styled(format!("+ {}", rule.exec_cmd), diff_added_style));
                } else {
                    line.push(Span::styled(rule.exec_cmd.clone(), exec_style));
                }
            } else {
                line.push(Span::styled(rule.exec_cmd.clone(), exec_style));
            }
            lines.push(Line::from(line));
        }
        lines.push(Line::from(""));
    }

    if editor.dirty {
        let unsaved_style = if fullish_shell_theme {
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        };
        let mut unsaved_spans = vec![
            Span::styled("Unsaved changes*", unsaved_style),
            Span::styled(", Save: ", label_style),
        ];
        append_key_with_white_slashes(
            &mut unsaved_spans,
            "Ctrl+s",
            hint_key_style,
            command_separator_style,
        );
        unsaved_spans.push(Span::styled(" | Discard: ", label_style));
        append_key_with_white_slashes(
            &mut unsaved_spans,
            "Ctrl+d",
            hint_key_style,
            command_separator_style,
        );
        lines.push(Line::from(unsaved_spans));
    }

    if !editor.status_message.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            editor.status_message.clone(),
            label_style,
        )]));
    }

    Text::from(lines)
}

fn sgr_to_style(raw: &str) -> Style {
    let mut fg: Option<Color> = None;
    let mut bold = false;
    let mut underlined = false;

    let codes: Vec<u16> = raw
        .split(';')
        .filter(|segment| !segment.is_empty())
        .filter_map(|segment| segment.parse::<u16>().ok())
        .collect();

    let mut idx = 0usize;
    while idx < codes.len() {
        let code = codes[idx];
        match code {
            0 => {
                fg = None;
                bold = false;
                underlined = false;
            }
            1 => bold = true,
            4 => underlined = true,
            22 => bold = false,
            24 => underlined = false,
            30..=37 => fg = Some(ansi_basic_color(code)),
            90..=97 => fg = Some(ansi_bright_color(code)),
            39 => fg = None,
            38 => {
                if idx + 2 < codes.len() && codes[idx + 1] == 5 {
                    fg = Some(Color::Indexed(codes[idx + 2] as u8));
                    idx += 2;
                } else if idx + 4 < codes.len() && codes[idx + 1] == 2 {
                    fg = Some(Color::Rgb(
                        codes[idx + 2] as u8,
                        codes[idx + 3] as u8,
                        codes[idx + 4] as u8,
                    ));
                    idx += 4;
                }
            }
            _ => {}
        }
        idx += 1;
    }

    let mut style = Style::default();
    if let Some(color) = fg {
        style = style.fg(color);
    }
    if bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if underlined {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    style
}

fn ansi_basic_color(code: u16) -> Color {
    match code {
        30 => Color::Black,
        31 => Color::Red,
        32 => Color::Green,
        33 => Color::Yellow,
        34 => Color::Blue,
        35 => Color::Magenta,
        36 => Color::Cyan,
        37 => Color::White,
        _ => Color::White,
    }
}

fn ansi_bright_color(code: u16) -> Color {
    match code {
        90 => Color::DarkGray,
        91 => Color::LightRed,
        92 => Color::LightGreen,
        93 => Color::LightYellow,
        94 => Color::LightBlue,
        95 => Color::LightMagenta,
        96 => Color::LightCyan,
        97 => Color::White,
        _ => Color::White,
    }
}

fn apply_alt_screen_chunk(current_state: bool, tail: &mut Vec<u8>, chunk: &[u8]) -> bool {
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

fn alt_screen_event_from_stream(stream: &[u8]) -> Option<bool> {
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

fn shell_scrollback_limit() -> usize {
    const DEFAULT_LIMIT: usize = 500_000;
    const PRACTICAL_UNLIMITED: usize = DEFAULT_LIMIT;
    let candidates = ["NAVIX_SHELL_SCROLLBACK_LIMIT", "NAVIX_SHELL_SCROLLBACK"]
        .map(|key| std::env::var(key).ok());
    resolve_scrollback_limit(&candidates, DEFAULT_LIMIT, PRACTICAL_UNLIMITED)
}

fn resolve_scrollback_limit(
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

fn parse_scrollback_limit(raw: &str, practical_unlimited: usize) -> Option<usize> {
    let parsed = raw.trim().parse::<usize>().ok()?;
    if parsed == 0 {
        Some(practical_unlimited)
    } else {
        Some(parsed)
    }
}

fn poll_timeout_for_drain(drain: OutputDrain) -> Duration {
    if drain.hit_limit {
        Duration::from_millis(0)
    } else if drain.processed_chunks > 0 {
        Duration::from_millis(5)
    } else {
        Duration::from_millis(80)
    }
}

fn scrollbar_thumb_bounds(
    total: usize,
    shown_start: usize,
    shown_end: usize,
    bar_height: usize,
) -> (usize, usize) {
    let total = total.max(1);
    let bar_height = bar_height.max(1);
    let shown_start = shown_start.max(1).min(total);
    let shown_end = shown_end.max(shown_start).min(total);
    let top = shown_start.saturating_sub(1).saturating_mul(bar_height) / total;
    let bottom = shown_end
        .saturating_mul(bar_height)
        .saturating_add(total.saturating_sub(1))
        / total;
    let bottom = bottom.max(top.saturating_add(1)).min(bar_height);
    (top, bottom)
}

impl Drop for ShellPane {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
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
        let _ = execute!(stdout, EnableMouseCapture);
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
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

fn inner(rect: ratatui::layout::Rect) -> ratatui::layout::Rect {
    ratatui::layout::Rect {
        x: rect.x.saturating_add(1),
        y: rect.y.saturating_add(1),
        width: rect.width.saturating_sub(2),
        height: rect.height.saturating_sub(2),
    }
}

fn to_io<E: std::fmt::Display>(err: E) -> io::Error {
    io::Error::other(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        alt_screen_event_from_stream, apply_alt_screen_chunk, border_style, compose_footer_line,
        clamp_preview_depth,
        config_panel_text, default_extension_rule, duplicate_extension_for_rule,
        extension_validation_error_for_rule, first_duplicate_extension, first_empty_extension,
        next_available_extension_name, normalize_extension, set_config_field,
        escape_prefix_release_update, escape_prefix_shortcut_char,
        footer_meta, footer_meta_line, footer_shortcuts, footer_shortcuts_line,
        clamp_nav_selection, is_fullish_layout_state, is_fullish_shell_mode, nav_max_scroll, nav_row_selected_style,
        nav_scroll_for_selection, parse_scrollback_limit, poll_timeout_for_drain,
        nav_long_listing, nav_style_for_theme, navigation_name_style, navigation_tree_lines,
        permission_bits, preview_content_for_selected_entry, preview_directory_tree_lines,
        preview_shortcut_target,
        preview_directory_panel_text,
        effective_access_from_mode, kernel_effective_access_for_path,
        next_preview_overlay_presentation, preview_overlay_is_interactive,
        should_auto_close_preview_overlay, PreviewOverlayPresentation,
        preview_file_commands_panel_text,
        navigation_file_command_action, NavigationFileCommandAction,
        available_preview_file_commands, resolve_preview_command_template,
        panel_areas_for_focus_click, panel_click_focus_target, pane_from_mouse_position,
        preview_file_preview_text,
        prefill_shell_input_bytes, sgr_to_style, shell_single_quote, shell_program_name,
        bash_history_sync_prompt_command,
        default_history_file_for_shell, simple_permission_bits,
        resolve_scrollback_limit, scrollbar_thumb_bounds, should_show_scrollbar,
        shell_panel_height, should_use_fullish_theme, visible_range, window_bounds,
        ActivePane, ConfigEditor, ConfigField, ConfigState, ExtensionCommandRule, LsColorsTheme,
        EffectiveIdentity, NavEntry, OutputDrain, PreviewMode,
    };
    use crossterm::event::{KeyCode, KeyEventKind};
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier, Style};
    use std::collections::HashSet;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use vt100::Parser;

    #[test]
    fn prompt_segments_survive_carriage_return_redraws() {
        let mut parser = Parser::new(8, 120, 200);
        parser.process("%\r\x1b[32m   ~/repos/navix    main ?7 \x1b[0m".as_bytes());
        parser.process("\r\x1b[34m❯\x1b[0m ".as_bytes());

        let content = parser.screen().contents();
        assert!(content.contains("repos/navix"));
        assert!(content.contains("❯"));
    }

    #[test]
    fn ls_output_lines_are_preserved() {
        let mut parser = Parser::new(20, 120, 400);
        parser.process(b"ls -lat\r\n");
        parser.process(b"total 16\r\n-rw-r--r-- file_a\r\n-rw-r--r-- file_b\r\n");

        let content = parser.screen().contents();
        assert!(content.contains("total 16"));
        assert!(content.contains("file_a"));
        assert!(content.contains("file_b"));
    }

    #[test]
    fn scrollback_offset_changes_visible_text() {
        let mut parser = Parser::new(3, 40, 100);
        parser.process(b"line1\r\nline2\r\nline3\r\nline4\r\nline5\r\n");
        parser.screen_mut().set_scrollback(0);
        let at_bottom = parser.screen().contents();

        parser.screen_mut().set_scrollback(2);
        let scrolled = parser.screen().contents();

        assert_ne!(at_bottom, scrolled);
        assert!(scrolled.contains("line1") || scrolled.contains("line2"));
    }

    #[test]
    fn window_bounds_puts_bottom_at_last_line() {
        let (start, end, max_offset) = window_bounds(59, 19, 0);
        assert_eq!((start, end, max_offset), (40, 59, 40));
    }

    #[test]
    fn window_bounds_clamps_excessive_scroll_offset() {
        let (start, end, max_offset) = window_bounds(12, 5, 999);
        assert_eq!((start, end, max_offset), (0, 5, 7));
    }

    #[test]
    fn scrollbar_hidden_when_everything_is_visible() {
        assert!(!should_show_scrollbar(19, 19));
        assert!(!should_show_scrollbar(5, 10));
    }

    #[test]
    fn scrollbar_shown_only_when_content_overflows() {
        assert!(should_show_scrollbar(20, 19));
    }

    #[test]
    fn thumb_bounds_fill_to_bottom_at_last_page() {
        let (_, bottom) = scrollbar_thumb_bounds(33, 15, 33, 19);
        assert_eq!(bottom, 19);
    }

    #[test]
    fn thumb_bounds_for_top_page_has_expected_range() {
        let (top, bottom) = scrollbar_thumb_bounds(33, 1, 19, 19);
        assert_eq!(top, 0);
        assert_eq!(bottom, 11);
    }

    #[test]
    fn parse_scrollback_limit_accepts_trimmed_positive_values() {
        assert_eq!(parse_scrollback_limit(" 42 ", 200_000), Some(42));
    }

    #[test]
    fn parse_scrollback_limit_treats_zero_as_practical_unlimited() {
        assert_eq!(parse_scrollback_limit("0", 200_000), Some(200_000));
    }

    #[test]
    fn parse_scrollback_limit_rejects_invalid_values() {
        assert_eq!(parse_scrollback_limit("abc", 200_000), None);
    }

    #[test]
    fn resolve_scrollback_limit_uses_default_when_missing() {
        let candidates = [None, None];
        assert_eq!(resolve_scrollback_limit(&candidates, 500_000, 500_000), 500_000);
    }

    #[test]
    fn resolve_scrollback_limit_prefers_first_candidate() {
        let candidates = [Some("123".to_string()), Some("456".to_string())];
        assert_eq!(resolve_scrollback_limit(&candidates, 500_000, 500_000), 123);
    }

    #[test]
    fn resolve_scrollback_limit_falls_back_to_second_candidate() {
        let candidates = [Some("invalid".to_string()), Some("789".to_string())];
        assert_eq!(resolve_scrollback_limit(&candidates, 500_000, 500_000), 789);
    }

    #[test]
    fn poll_timeout_for_drain_is_zero_when_backlog_hits_limit() {
        let timeout = poll_timeout_for_drain(OutputDrain {
            processed_chunks: 12,
            hit_limit: true,
        });
        assert_eq!(timeout, Duration::from_millis(0));
    }

    #[test]
    fn poll_timeout_for_drain_is_short_when_processing_output() {
        let timeout = poll_timeout_for_drain(OutputDrain {
            processed_chunks: 1,
            hit_limit: false,
        });
        assert_eq!(timeout, Duration::from_millis(5));
    }

    #[test]
    fn poll_timeout_for_drain_is_idle_when_no_output() {
        let timeout = poll_timeout_for_drain(OutputDrain {
            processed_chunks: 0,
            hit_limit: false,
        });
        assert_eq!(timeout, Duration::from_millis(80));
    }

    #[test]
    fn poll_timeout_for_drain_prioritizes_hit_limit() {
        let timeout = poll_timeout_for_drain(OutputDrain {
            processed_chunks: 0,
            hit_limit: true,
        });
        assert_eq!(timeout, Duration::from_millis(0));
    }

    #[test]
    fn visible_range_bottom_page_matches_expected_bounds() {
        let (start, end) = visible_range(33, 19, 0);
        assert_eq!((start, end), (15, 33));
    }

    #[test]
    fn visible_range_top_page_matches_expected_bounds() {
        let (start, end) = visible_range(33, 19, 14);
        assert_eq!((start, end), (1, 19));
    }

    #[test]
    fn visible_range_middle_page_matches_expected_bounds() {
        let (start, end) = visible_range(33, 19, 9);
        assert_eq!((start, end), (6, 24));
    }

    #[test]
    fn visible_range_clamps_excessive_scroll_offset() {
        let (start, end) = visible_range(33, 19, 999);
        assert_eq!((start, end), (1, 19));
    }

    #[test]
    fn visible_range_without_overflow_fills_viewport() {
        let (start, end) = visible_range(19, 19, 7);
        assert_eq!((start, end), (1, 19));
    }

    #[test]
    fn thumb_bounds_always_allocate_at_least_one_row() {
        let (top, bottom) = scrollbar_thumb_bounds(1_000, 500, 500, 10);
        assert_eq!(bottom.saturating_sub(top), 1);
    }

    #[test]
    fn thumb_bounds_stay_within_bar_height() {
        let (_top, bottom) = scrollbar_thumb_bounds(33, 15, 33, 19);
        assert!(bottom <= 19);
    }

    #[test]
    fn footer_shortcuts_for_shell_use_escape_sequences() {
        let shortcuts = footer_shortcuts(ActivePane::Shell, false, false);
        assert!(shortcuts.contains("Esc+0"));
        assert!(shortcuts.contains("Esc+1"));
        assert!(shortcuts.contains("Esc+2"));
        assert!(shortcuts.contains("Config: Esc+c"));
        assert!(shortcuts.contains("Esc+↑/Esc+↓"));
        assert!(shortcuts.contains("Esc+f"));
        assert!(shortcuts.contains("Exit: Ctrl+d"));
        assert!(!shortcuts.contains("Esc+q"));
    }

    #[test]
    fn footer_shortcuts_for_non_shell_use_direct_numbers() {
        let shortcuts = footer_shortcuts(ActivePane::Navigation, false, false);
        assert!(shortcuts.contains("Shell: Esc+0"));
        assert!(shortcuts.contains("Navigation: Esc+1"));
        assert!(shortcuts.contains("Preview: Esc+2"));
        assert!(shortcuts.contains("Config: Esc+c"));
        assert!(shortcuts.contains("Full: Esc+f"));
        assert!(shortcuts.contains("↑/↓"));
        assert!(shortcuts.contains("Exit: Ctrl+d"));
        assert!(!shortcuts.contains("Ctrl+d/q"));
    }

    #[test]
    fn footer_shortcuts_for_config_overlay_show_editor_commands() {
        let shortcuts = footer_shortcuts(ActivePane::Navigation, true, false);
        assert!(shortcuts.contains("Close: Esc"));
        assert!(!shortcuts.contains("Esc/q"));
        assert!(!shortcuts.contains("Exit:"));
        assert!(!shortcuts.contains("Ctrl+d"));
        assert!(!shortcuts.contains("Rule: "));
        assert!(!shortcuts.contains("Field: "));
        assert!(!shortcuts.contains("Config: "));
    }

    #[test]
    fn footer_shortcuts_marks_exit_with_star_when_unsaved() {
        let shell_shortcuts = footer_shortcuts(ActivePane::Shell, false, true);
        assert!(shell_shortcuts.contains("Exit*: Ctrl+d"));
        let nav_shortcuts = footer_shortcuts(ActivePane::Navigation, false, true);
        assert!(nav_shortcuts.contains("Exit*: Ctrl+d"));
    }

    #[test]
    fn footer_shortcuts_line_colors_unsaved_exit_star_orange() {
        let line = footer_shortcuts_line(ActivePane::Shell, false, false, true, false);
        let star = line
            .spans
            .iter()
            .find(|span| span.content.as_ref() == "*")
            .expect("exit star span missing");
        assert_eq!(star.style.fg, Some(Color::Indexed(208)));
    }

    #[test]
    fn footer_shortcuts_line_flashes_orange_config_command_when_exit_blocked() {
        let line = footer_shortcuts_line(ActivePane::Shell, false, false, true, true);
        let config_label = line
            .spans
            .iter()
            .find(|span| span.content.as_ref().contains("Config:"))
            .expect("config label span missing");
        let config_key = line
            .spans
            .iter()
            .find(|span| span.content.as_ref().contains("Esc+c"))
            .expect("config key span missing");
        let separator = line
            .spans
            .iter()
            .find(|span| span.content.as_ref() == " | ")
            .expect("separator span missing");
        assert_eq!(config_label.style.fg, Some(Color::Indexed(208)));
        assert_eq!(config_key.style.fg, Some(Color::Indexed(208)));
        assert!(config_key.style.add_modifier.contains(Modifier::BOLD));
        assert!(config_key.style.add_modifier.contains(Modifier::RAPID_BLINK));
        assert_eq!(separator.style.fg, Some(Color::White));
    }

    #[test]
    fn escape_prefix_release_update_clears_pending_on_escape_release() {
        let (pending, armed, consumed) =
            escape_prefix_release_update(true, true, KeyCode::Esc, KeyEventKind::Release);
        assert!(!pending);
        assert!(!armed);
        assert!(consumed);
    }

    #[test]
    fn escape_prefix_release_update_ignores_non_escape_release() {
        let (pending, armed, consumed) =
            escape_prefix_release_update(true, true, KeyCode::Up, KeyEventKind::Release);
        assert!(pending);
        assert!(armed);
        assert!(!consumed);
    }

    #[test]
    fn escape_prefix_shortcut_char_requires_armed_prefix() {
        assert_eq!(escape_prefix_shortcut_char(false, KeyCode::Char('c')), None);
        assert_eq!(escape_prefix_shortcut_char(false, KeyCode::Char('0')), None);
    }

    #[test]
    fn escape_prefix_shortcut_char_accepts_only_supported_shortcuts() {
        assert_eq!(escape_prefix_shortcut_char(true, KeyCode::Char('0')), Some('0'));
        assert_eq!(escape_prefix_shortcut_char(true, KeyCode::Char('1')), Some('1'));
        assert_eq!(escape_prefix_shortcut_char(true, KeyCode::Char('2')), Some('2'));
        assert_eq!(escape_prefix_shortcut_char(true, KeyCode::Char('c')), Some('c'));
        assert_eq!(escape_prefix_shortcut_char(true, KeyCode::Char('C')), Some('c'));
        assert_eq!(escape_prefix_shortcut_char(true, KeyCode::Char('f')), Some('f'));
        assert_eq!(escape_prefix_shortcut_char(true, KeyCode::Char('r')), Some('r'));
        assert_eq!(escape_prefix_shortcut_char(true, KeyCode::Char('w')), Some('w'));
        assert_eq!(escape_prefix_shortcut_char(true, KeyCode::Char('x')), Some('x'));
        assert_eq!(escape_prefix_shortcut_char(true, KeyCode::Char('q')), None);
        assert_eq!(escape_prefix_shortcut_char(true, KeyCode::Up), None);
    }

    #[test]
    fn config_panel_text_omits_intro_lines() {
        let text = config_panel_text(
            &ConfigState::default(),
            &ConfigState::default(),
            &ConfigEditor::default(),
            false,
        );
        let rendered: String = text
            .lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref().to_string()))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(!rendered.contains("Extension command routing"));
        assert!(!rendered.contains("Keys: c (non-shell)"));
    }

    #[test]
    fn config_panel_text_uses_aligned_extension_block_layout() {
        let text = config_panel_text(
            &ConfigState::default(),
            &ConfigState::default(),
            &ConfigEditor::default(),
            false,
        );
        let first_rule_line = &text.lines[2];
        assert!(
            first_rule_line
                .spans
                .iter()
                .any(|span| span.content.as_ref() == "❯ ")
        );
        assert!(
            first_rule_line
                .spans
                .iter()
                .any(|span| span.content.as_ref() == ".md")
        );
        let extension_label = first_rule_line
            .spans
            .iter()
            .find(|span| span.content.as_ref().ends_with(": extension:"))
            .expect("extension label span missing");
        assert_eq!(extension_label.style.fg, Some(Color::White));
        let rendered: String = text
            .lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref().to_string()))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(rendered.contains("Rule:"));
        assert!(rendered.contains("Field:"));
        assert!(rendered.contains("Edit:"));
        assert!(rendered.contains("Enter"));
        assert!(!rendered.contains("Enter/e"));
        assert!(rendered.contains("Del-Rule:"));
        assert!(rendered.contains("Ctrl+Del"));
        assert!(!rendered.contains("Ctrl+Backspace"));
        assert!(rendered.contains("New-Rule:"));
        assert!(rendered.contains(": extension:"));
        assert!(rendered.contains("    read :"));
        assert!(rendered.contains("    write:"));
        assert!(rendered.contains("    exec :"));
    }

    #[test]
    fn config_panel_text_extension_edit_is_in_place_with_fixed_dot_prefix() {
        let mut editor = ConfigEditor::default();
        editor.selected_rule = 2;
        editor.selected_field = ConfigField::Extension;
        editor.editing = true;
        editor.set_input("sh".to_string());
        let text = config_panel_text(
            &ConfigState::default(),
            &ConfigState::default(),
            &editor,
            false,
        );
        let extension_line = text
            .lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref() == "❯ ")
                    && line
                        .spans
                        .iter()
                        .any(|span| span.content.as_ref().ends_with(": extension:"))
            })
            .expect("selected extension line missing");
        assert!(
            extension_line
                .spans
                .iter()
                .any(|span| span.content.as_ref() == ".")
        );
        assert!(
            extension_line
                .spans
                .iter()
                .any(|span| span.content.as_ref() == "sh")
        );
        assert!(
            !extension_line
                .spans
                .iter()
                .any(|span| span.content.as_ref() == ".sh")
        );
        let rendered: String = text
            .lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref().to_string()))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(!rendered.contains("edit extension:"));
    }

    #[test]
    fn config_top_hint_slashes_are_white() {
        let text = config_panel_text(
            &ConfigState::default(),
            &ConfigState::default(),
            &ConfigEditor::default(),
            false,
        );
        let top = &text.lines[0];
        let mut saw_slash = false;
        for span in &top.spans {
            if span.content.chars().any(|ch| ch == '/') {
                saw_slash = true;
                assert_eq!(span.style.fg, Some(Color::White));
            }
        }
        assert!(saw_slash);
    }

    #[test]
    fn config_unsaved_line_shows_save_shortcut() {
        let mut editor = ConfigEditor::default();
        editor.dirty = true;
        let text = config_panel_text(
            &ConfigState::default(),
            &ConfigState::default(),
            &editor,
            false,
        );
        let rendered: String = text
            .lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref().to_string()))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(rendered.contains("Unsaved changes*"));
        assert!(rendered.contains("Save:"));
        assert!(rendered.contains("Ctrl+s"));
        assert!(rendered.contains("Discard:"));
        assert!(rendered.contains("Ctrl+d"));
    }

    #[test]
    fn config_unsaved_deleted_rule_stays_visible_in_red() {
        let mut config = ConfigState::default();
        config.extension_rules.remove(0);
        let mut editor = ConfigEditor::default();
        editor.dirty = true;
        let saved_config = ConfigState::default();
        let text = config_panel_text(&config, &saved_config, &editor, false);
        let deleted_idx = text
            .lines
            .iter()
            .position(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref() == ".md")
                    && line
                        .spans
                        .iter()
                        .any(|span| span.content.as_ref().ends_with(": extension:"))
            })
            .expect("deleted rule line missing");
        let extension_line = &text.lines[deleted_idx];
        assert!(
            extension_line
                .spans
                .iter()
                .any(|span| span.content.as_ref() == ".md" && span.style.fg == Some(Color::Red))
        );
        let read_line = &text.lines[deleted_idx + 1];
        assert!(
            read_line
                .spans
                .iter()
                .any(|span| span.content.as_ref() == "bat {file}" && span.style.fg == Some(Color::Red))
        );
        let write_line = &text.lines[deleted_idx + 2];
        assert!(
            write_line
                .spans
                .iter()
                .any(|span| {
                    span.content.as_ref() == "$EDITOR {file}" && span.style.fg == Some(Color::Red)
                })
        );
        let exec_line = &text.lines[deleted_idx + 3];
        assert!(
            exec_line
                .spans
                .iter()
                .any(|span| span.content.as_ref() == "mdterm {file}" && span.style.fg == Some(Color::Red))
        );
    }

    #[test]
    fn config_unsaved_added_rule_is_green_on_all_fields() {
        let mut config = ConfigState::default();
        config.extension_rules.push(ExtensionCommandRule {
            extension: "toml".to_string(),
            read_cmd: "bat {file}".to_string(),
            write_cmd: "$EDITOR {file}".to_string(),
            exec_cmd: "taplo fmt {file}".to_string(),
        });
        let mut editor = ConfigEditor::default();
        editor.selected_rule = 0;
        editor.dirty = true;
        let saved_config = ConfigState::default();
        let text = config_panel_text(&config, &saved_config, &editor, false);
        let added_idx = text
            .lines
            .iter()
            .position(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref() == ".toml")
                    && line
                        .spans
                        .iter()
                        .any(|span| span.content.as_ref().ends_with(": extension:"))
            })
            .expect("added rule line missing");
        let extension_line = &text.lines[added_idx];
        assert!(
            extension_line
                .spans
                .iter()
                .any(|span| span.content.as_ref() == ".toml" && span.style.fg == Some(Color::Green))
        );
        let read_line = &text.lines[added_idx + 1];
        assert!(
            read_line
                .spans
                .iter()
                .any(|span| span.content.as_ref() == "bat {file}" && span.style.fg == Some(Color::Green))
        );
        let write_line = &text.lines[added_idx + 2];
        assert!(
            write_line
                .spans
                .iter()
                .any(|span| {
                    span.content.as_ref() == "$EDITOR {file}" && span.style.fg == Some(Color::Green)
                })
        );
        let exec_line = &text.lines[added_idx + 3];
        assert!(
            exec_line
                .spans
                .iter()
                .any(|span| {
                    span.content.as_ref() == "taplo fmt {file}"
                        && span.style.fg == Some(Color::Green)
                })
        );
    }

    #[test]
    fn config_unsaved_readded_extension_shows_partial_field_diff() {
        let mut config = ConfigState::default();
        config.extension_rules.remove(0);
        config.extension_rules.push(ExtensionCommandRule {
            extension: "md".to_string(),
            read_cmd: "bat {file}".to_string(),
            write_cmd: "$EDITOR {file}".to_string(),
            exec_cmd: "--".to_string(),
        });
        let mut editor = ConfigEditor::default();
        editor.selected_rule = 0;
        editor.dirty = true;
        let saved_config = ConfigState::default();
        let text = config_panel_text(&config, &saved_config, &editor, false);
        let md_idx = text
            .lines
            .iter()
            .position(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref() == ".md")
                    && line
                        .spans
                        .iter()
                        .any(|span| span.content.as_ref().ends_with(": extension:"))
            })
            .expect("re-added md line missing");
        let extension_line = &text.lines[md_idx];
        assert!(
            extension_line
                .spans
                .iter()
                .any(|span| span.content.as_ref() == ".md" && span.style.fg == Some(Color::LightBlue))
        );
        let read_line = &text.lines[md_idx + 1];
        assert!(
            read_line
                .spans
                .iter()
                .any(|span| span.content.as_ref() == "bat {file}" && span.style.fg == Some(Color::LightBlue))
        );
        let write_line = &text.lines[md_idx + 2];
        assert!(
            write_line
                .spans
                .iter()
                .any(|span| {
                    span.content.as_ref() == "$EDITOR {file}"
                        && span.style.fg == Some(Color::LightBlue)
                })
        );
        let exec_line = &text.lines[md_idx + 3];
        assert!(
            exec_line
                .spans
                .iter()
                .any(|span| {
                    span.content.as_ref() == "- mdterm {file}"
                        && span.style.fg == Some(Color::Red)
                })
        );
        assert!(
            exec_line
                .spans
                .iter()
                .any(|span| span.content.as_ref() == "+ --" && span.style.fg == Some(Color::Green))
        );
    }

    #[test]
    fn config_unsaved_deleted_rules_render_even_when_current_is_empty() {
        let config = ConfigState {
            extension_rules: Vec::new(),
        };
        let mut editor = ConfigEditor::default();
        editor.dirty = true;
        let saved_config = ConfigState::default();
        let text = config_panel_text(&config, &saved_config, &editor, false);
        let has_deleted_md = text.lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content.as_ref() == ".md" && span.style.fg == Some(Color::Red))
        });
        assert!(has_deleted_md);
        let rendered: String = text
            .lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref().to_string()))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(!rendered.contains("(no extension rules yet - press Ctrl+n to add one)"));
    }

    #[test]
    fn normalize_extension_drops_dot_and_lowercases() {
        assert_eq!(normalize_extension(".MD"), "md");
        assert_eq!(normalize_extension("  txt "), "txt");
    }

    #[test]
    fn config_normalize_migrates_old_bat_paging_flag() {
        let mut config = ConfigState {
            extension_rules: vec![ExtensionCommandRule {
                extension: ".md".to_string(),
                read_cmd: "bat --paging=never {file}".to_string(),
                write_cmd: "$EDITOR {file}".to_string(),
                exec_cmd: "--".to_string(),
            }],
        };
        config.normalize();
        assert_eq!(config.extension_rules[0].extension, "md");
        assert_eq!(config.extension_rules[0].read_cmd, "bat {file}");
    }

    #[test]
    fn default_extension_rule_has_safe_exec_placeholder() {
        let rule = default_extension_rule("NEW");
        assert_eq!(rule.extension, "new");
        assert_eq!(rule.read_cmd, "bat {file}");
        assert_eq!(rule.write_cmd, "$EDITOR {file}");
        assert_eq!(rule.exec_cmd, "--");
    }

    #[test]
    fn set_config_field_normalizes_extension_only() {
        let mut rule = ExtensionCommandRule {
            extension: "md".to_string(),
            read_cmd: "a".to_string(),
            write_cmd: "b".to_string(),
            exec_cmd: "c".to_string(),
        };
        set_config_field(&mut rule, ConfigField::Extension, ".LOG");
        set_config_field(&mut rule, ConfigField::Read, "bat");
        assert_eq!(rule.extension, "log");
        assert_eq!(rule.read_cmd, "bat");
    }

    #[test]
    fn duplicate_extension_for_rule_detects_conflict_on_other_rule() {
        let config = ConfigState::default();
        assert_eq!(
            duplicate_extension_for_rule(&config, 0, ".JSON"),
            Some("json".to_string())
        );
        assert_eq!(duplicate_extension_for_rule(&config, 0, ".md"), None);
    }

    #[test]
    fn first_duplicate_extension_finds_first_repeat() {
        let config = ConfigState {
            extension_rules: vec![default_extension_rule(".md"), default_extension_rule("MD")],
        };
        assert_eq!(first_duplicate_extension(&config), Some("md".to_string()));
    }

    #[test]
    fn extension_validation_error_for_rule_rejects_empty_names() {
        let config = ConfigState::default();
        assert_eq!(
            extension_validation_error_for_rule(&config, 0, " . "),
            Some("extension name cannot be empty".to_string())
        );
    }

    #[test]
    fn first_empty_extension_detects_blank_rule_entries() {
        let mut config = ConfigState::default();
        config.extension_rules.push(default_extension_rule(""));
        assert!(first_empty_extension(&config));
    }

    #[test]
    fn next_available_extension_name_increments_suffix_until_unique() {
        let config = ConfigState {
            extension_rules: vec![
                default_extension_rule("new"),
                default_extension_rule("new2"),
                default_extension_rule("new3"),
            ],
        };
        assert_eq!(next_available_extension_name(&config, "new"), "new4");
    }

    #[test]
    fn config_editor_cursor_allows_middle_rewrite() {
        let mut editor = ConfigEditor::default();
        editor.set_input("bat {file}".to_string());

        editor.move_cursor_home();
        for _ in 0.."bat".chars().count() {
            editor.move_cursor_right();
        }

        editor.insert_char('c');
        editor.insert_char('a');
        editor.insert_char('t');

        assert_eq!(editor.input_buffer, "batcat {file}");
    }

    #[test]
    fn config_editor_backspace_and_delete_work_at_cursor() {
        let mut editor = ConfigEditor::default();
        editor.set_input("abcde".to_string());
        editor.move_cursor_left();
        editor.move_cursor_left();
        editor.backspace();
        assert_eq!(editor.input_buffer, "abde");
        editor.delete();
        assert_eq!(editor.input_buffer, "abe");
    }

    #[test]
    fn footer_meta_uses_package_version() {
        assert_eq!(footer_meta(), format!("Donate {}", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn compose_footer_line_places_meta_on_right() {
        let line = compose_footer_line(ActivePane::Navigation, 120);
        let meta = footer_meta();
        assert_eq!(line.chars().count(), 120);
        assert!(line.starts_with(' '));
        assert!(line.ends_with(' '));
        assert!(line.trim_end().ends_with(&meta));
    }

    #[test]
    fn compose_footer_line_truncates_left_before_meta() {
        let meta = footer_meta();
        let width = (meta.chars().count() + 5) as u16;
        let line = compose_footer_line(ActivePane::Shell, width);
        assert_eq!(line.chars().count(), width as usize);
        assert!(line.starts_with(' '));
        assert!(line.ends_with(' '));
        assert!(line.trim_end().ends_with(&meta));
    }

    #[test]
    fn compose_footer_line_tiny_width_truncates_meta() {
        let line = compose_footer_line(ActivePane::Shell, 4);
        assert_eq!(line.chars().count(), 4);
        assert_eq!(line, " Do ");
    }

    #[test]
    fn compose_footer_line_has_left_and_right_padding_when_possible() {
        let line = compose_footer_line(ActivePane::Shell, 80);
        assert!(line.starts_with(' '));
        assert!(line.ends_with(' '));
    }

    #[test]
    fn compose_footer_line_single_column_is_space() {
        assert_eq!(compose_footer_line(ActivePane::Shell, 1), " ");
    }

    #[test]
    fn footer_shortcuts_slash_separators_are_white_in_both_modes() {
        for mode in [ActivePane::Shell, ActivePane::Navigation] {
            let line = footer_shortcuts_line(mode, false, false, false, false);
            let mut saw_slash = false;
            for span in line.spans {
                if span.content.chars().any(|ch| ch == '/') {
                    saw_slash = true;
                    assert_eq!(span.style.fg, Some(Color::White));
                }
            }
            assert!(saw_slash);
        }
    }

    #[test]
    fn footer_shortcuts_comma_separators_are_white_in_both_modes() {
        for mode in [ActivePane::Shell, ActivePane::Navigation] {
            let line = footer_shortcuts_line(mode, false, false, false, false);
            let mut saw_comma = false;
            for span in line.spans {
                if span.content.chars().any(|ch| ch == ',') {
                    saw_comma = true;
                    assert_eq!(span.style.fg, Some(Color::White));
                }
            }
            assert!(saw_comma);
        }
    }

    #[test]
    fn footer_shortcuts_pgup_pgdown_are_blue_bold() {
        let key_style = Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD);
        for mode in [ActivePane::Shell, ActivePane::Navigation] {
            let line = footer_shortcuts_line(mode, false, false, false, false);
            let mut saw_pgup = false;
            let mut saw_pgdown = false;
            for span in line.spans {
                if span.content.contains("PgUp") {
                    saw_pgup = true;
                    assert_eq!(span.style, key_style);
                }
                if span.content.contains("PgDown") {
                    saw_pgdown = true;
                    assert_eq!(span.style, key_style);
                }
            }
            assert!(saw_pgup);
            assert!(saw_pgdown);
        }
    }

    #[test]
    fn footer_meta_line_has_magenta_strikethrough_donate_label() {
        let line = footer_meta_line(false);
        let donate = &line.spans[0];
        assert_eq!(donate.content, "Donate");
        assert_eq!(donate.style.fg, Some(Color::Magenta));
        assert!(donate.style.add_modifier.contains(Modifier::CROSSED_OUT));
        let gap = &line.spans[1];
        assert_eq!(gap.content, " ");
        assert_eq!(gap.style.fg, Some(Color::White));
        assert!(!gap.style.add_modifier.contains(Modifier::CROSSED_OUT));
    }

    #[test]
    fn fullish_shell_mode_theme_requires_alt_screen() {
        assert!(is_fullish_shell_mode(ActivePane::Shell, true));
        assert!(!is_fullish_shell_mode(ActivePane::Navigation, true));
        assert!(!should_use_fullish_theme(ActivePane::Shell, false));
        assert!(!should_use_fullish_theme(ActivePane::Navigation, true));
        assert!(should_use_fullish_theme(ActivePane::Shell, true));

        let active = border_style(true, true);
        let inactive = border_style(false, true);
        assert_eq!(active.fg, Some(Color::DarkGray));
        assert_eq!(inactive.fg, Some(Color::DarkGray));
        assert!(active.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn is_fullish_layout_state_detects_shell_nav_and_preview_modes() {
        assert!(is_fullish_layout_state(
            ActivePane::Shell,
            true,
            false,
            false,
            false
        ));
        assert!(is_fullish_layout_state(
            ActivePane::Shell,
            false,
            true,
            false,
            false
        ));
        assert!(is_fullish_layout_state(
            ActivePane::Navigation,
            false,
            false,
            true,
            false
        ));
        assert!(is_fullish_layout_state(
            ActivePane::Navigation,
            false,
            false,
            false,
            true
        ));
        assert!(!is_fullish_layout_state(
            ActivePane::Navigation,
            false,
            false,
            false,
            false
        ));
    }

    #[test]
    fn preview_overlay_presentation_promotes_to_interactive_on_alt_screen() {
        let state = next_preview_overlay_presentation(
            Some(PreviewOverlayPresentation::StaticFullscreen),
            true,
        );
        assert_eq!(
            state,
            Some(PreviewOverlayPresentation::InteractiveFullscreenDim)
        );
        assert!(preview_overlay_is_interactive(state));
    }

    #[test]
    fn preview_overlay_auto_close_only_for_interactive_exit() {
        assert!(!should_auto_close_preview_overlay(
            Some(PreviewOverlayPresentation::StaticFullscreen),
            false,
        ));
        assert!(!should_auto_close_preview_overlay(
            Some(PreviewOverlayPresentation::InteractiveFullscreenDim),
            true,
        ));
        assert!(should_auto_close_preview_overlay(
            Some(PreviewOverlayPresentation::InteractiveFullscreenDim),
            false,
        ));
    }

    #[test]
    fn footer_shortcuts_in_fullish_mode_use_darker_keys() {
        let expected = Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD);
        let line = footer_shortcuts_line(ActivePane::Shell, true, false, false, false);
        let mut saw_key = false;
        for span in line.spans {
            if span.content.contains("Esc+f") || span.content.contains("PgUp") {
                saw_key = true;
                assert_eq!(span.style, expected);
            }
        }
        assert!(saw_key);
    }

    #[test]
    fn nav_style_for_theme_dims_foreground_in_fullish_mode() {
        let base = Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD);
        let dimmed = nav_style_for_theme(base, true);
        assert_eq!(dimmed.fg, Some(Color::DarkGray));
        assert!(dimmed.add_modifier.contains(Modifier::BOLD));

        let unchanged = nav_style_for_theme(base, false);
        assert_eq!(unchanged.fg, Some(Color::Blue));
        assert!(unchanged.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn shell_panel_height_fullish_uses_all_but_four_lines() {
        let height = shell_panel_height(40, ActivePane::Shell, true);
        assert_eq!(height, 36);
    }

    #[test]
    fn shell_panel_height_fullish_has_minimum_one_line() {
        let height = shell_panel_height(3, ActivePane::Shell, true);
        assert_eq!(height, 1);
    }

    #[test]
    fn shell_panel_height_normal_shell_mode_uses_percentage_rule() {
        let height = shell_panel_height(40, ActivePane::Shell, false);
        assert_eq!(height, 18);
    }

    #[test]
    fn alt_screen_event_stream_detects_enter_and_exit() {
        assert_eq!(alt_screen_event_from_stream(b"\x1b[?1049h"), Some(true));
        assert_eq!(alt_screen_event_from_stream(b"\x1b[?1049l"), Some(false));
    }

    #[test]
    fn alt_screen_event_stream_uses_last_event_when_both_present() {
        let bytes = b"\x1b[?1049hhello\x1b[?1049l";
        assert_eq!(alt_screen_event_from_stream(bytes), Some(false));
    }

    #[test]
    fn apply_alt_screen_chunk_handles_split_escape_sequence() {
        let mut tail = Vec::new();
        let state1 = apply_alt_screen_chunk(false, &mut tail, b"\x1b[?10");
        assert!(!state1);
        let state2 = apply_alt_screen_chunk(state1, &mut tail, b"49h");
        assert!(state2);
        let state3 = apply_alt_screen_chunk(state2, &mut tail, b"\x1b[?1049l");
        assert!(!state3);
    }

    #[test]
    fn permission_bits_renders_unix_style_triplets() {
        assert_eq!(permission_bits('d', 0o755), "drwxr-xr-x");
        assert_eq!(permission_bits('-', 0o644), "-rw-r--r--");
    }

    #[test]
    fn simple_permission_bits_renders_compact_mode() {
        assert_eq!(simple_permission_bits('d', 0o755), "drwx");
        assert_eq!(simple_permission_bits('-', 0o640), "-rw-");
        assert_eq!(simple_permission_bits('l', 0o777), "lrwx");
    }

    #[test]
    fn clamp_nav_selection_stays_in_bounds() {
        assert_eq!(clamp_nav_selection(3, 0), 0);
        assert_eq!(clamp_nav_selection(4, 2), 1);
        assert_eq!(clamp_nav_selection(1, 5), 1);
    }

    #[test]
    fn nav_max_scroll_respects_viewport() {
        assert_eq!(nav_max_scroll(3, 10), 0);
        assert_eq!(nav_max_scroll(20, 5), 15);
    }

    #[test]
    fn nav_scroll_for_selection_keeps_selection_visible() {
        assert_eq!(nav_scroll_for_selection(0, 8, 20, 5), 4);
        assert_eq!(nav_scroll_for_selection(10, 2, 20, 5), 2);
    }

    #[test]
    fn nav_scroll_for_selection_adjusts_when_viewport_shrinks() {
        assert_eq!(nav_scroll_for_selection(5, 9, 20, 4), 6);
    }

    #[test]
    fn nav_row_selected_style_highlights_with_light_blue_background() {
        let base = Style::default().fg(Color::Blue);
        let selected = nav_row_selected_style(base, true);
        assert_eq!(selected.fg, Some(Color::Black));
        assert_eq!(selected.bg, Some(Color::LightBlue));
    }

    #[test]
    fn shell_single_quote_escapes_single_quotes() {
        assert_eq!(shell_single_quote("/tmp/a'b"), "'/tmp/a'\\''b'");
    }

    #[test]
    fn prefill_shell_input_bytes_clears_line_before_prefill() {
        let bytes = prefill_shell_input_bytes("bash bootstrap.sh");
        assert_eq!(&bytes[..2], &[0x01, 0x0b]);
        assert_eq!(&bytes[2..], b"bash bootstrap.sh");
    }

    #[test]
    fn shell_program_name_extracts_basename() {
        assert_eq!(shell_program_name("/usr/bin/zsh"), "zsh");
        assert_eq!(shell_program_name("bash"), "bash");
    }

    #[test]
    fn bash_history_sync_prompt_command_prepends_sync_steps() {
        assert_eq!(
            bash_history_sync_prompt_command(None),
            "history -a; history -n"
        );
        assert_eq!(
            bash_history_sync_prompt_command(Some("echo hi")),
            "history -a; history -n; echo hi"
        );
        assert_eq!(
            bash_history_sync_prompt_command(Some("history -a; history -n; echo hi")),
            "history -a; history -n; echo hi"
        );
    }

    #[test]
    fn default_history_file_for_shell_uses_standard_paths() {
        let Some(home) = std::env::var("HOME").ok().filter(|value| !value.is_empty()) else {
            return;
        };
        assert_eq!(
            default_history_file_for_shell("/bin/bash"),
            Some(format!("{home}/.bash_history"))
        );
        let zsh_history = default_history_file_for_shell("/usr/bin/zsh");
        assert!(matches!(
            zsh_history.as_deref(),
            Some(path)
                if path == format!("{home}/.zsh_history")
                    || path == format!("{home}/.zhistory")
                    || path.ends_with("/zsh/history")
        ));
    }

    fn unique_temp_path(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ))
    }

    fn test_identity(uid: u32, gid: u32, groups: &[u32]) -> EffectiveIdentity {
        let mut all_groups = HashSet::new();
        all_groups.insert(gid);
        for group in groups {
            all_groups.insert(*group);
        }
        EffectiveIdentity {
            euid: uid,
            egid: gid,
            groups: all_groups,
        }
    }

    #[test]
    fn preview_content_for_directory_selection_is_non_empty() {
        let base = unique_temp_path("navix-preview-dir");
        fs::create_dir_all(base.join("sub/inner")).expect("create dirs");
        fs::write(base.join("sub/file.md"), b"hello").expect("create file");
        fs::write(base.join("sub/inner/deep.txt"), b"deep").expect("create nested file");

        let entry = NavEntry {
            name: "sub".to_string(),
            path: base.join("sub"),
            is_dir: true,
            is_symlink: false,
            file_type_char: 'd',
            mode: 0o755,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 0,
            mtime: 0,
        };

        let (mode, text) = preview_content_for_selected_entry(Some(&entry), 2);
        assert_eq!(mode, PreviewMode::DirectoryTree);
        assert!(!text.trim().is_empty());
        assert!(text.contains(&format!("{}", base.join("sub").display())));
        assert!(text.contains("inner/"));
        assert!(text.contains("deep.txt"));

        fs::remove_dir_all(base).expect("cleanup temp");
    }

    #[test]
    fn preview_content_for_file_selection_clears_panel() {
        let entry = NavEntry {
            name: "file.md".to_string(),
            path: PathBuf::from("/tmp/file.md"),
            is_dir: false,
            is_symlink: false,
            file_type_char: '-',
            mode: 0o644,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 0,
            mtime: 0,
        };

        let (mode, text) = preview_content_for_selected_entry(Some(&entry), 2);
        assert_eq!(mode, PreviewMode::Empty);
        assert!(text.is_empty());
    }

    #[test]
    fn preview_file_preview_text_renders_plain_text_files() {
        let base = unique_temp_path("navix-preview-file");
        fs::create_dir_all(&base).expect("create base");
        let path = base.join("notes.txt");
        fs::write(&path, b"hello\npreview\npanel\n").expect("write file");

        let rendered = preview_file_preview_text(&path);
        assert!(rendered.contains("hello"));
        assert!(rendered.contains("preview"));

        fs::remove_dir_all(base).expect("cleanup temp");
    }

    #[test]
    fn preview_file_preview_text_marks_binary_files() {
        let base = unique_temp_path("navix-preview-binary");
        fs::create_dir_all(&base).expect("create base");
        let path = base.join("blob.bin");
        fs::write(&path, [0_u8, 159, 146, 150]).expect("write file");

        let rendered = preview_file_preview_text(&path);
        assert!(rendered.contains("binary file"));
        assert!(rendered.contains("4 bytes"));

        fs::remove_dir_all(base).expect("cleanup temp");
    }

    #[test]
    fn preview_command_template_resolves_editor_and_filename() {
        let resolved = resolve_preview_command_template("$EDITOR {file}", "README.md", "nvim");
        assert_eq!(resolved, "nvim README.md");
    }

    #[test]
    fn available_preview_file_commands_respects_permission_bits_and_rule_commands() {
        let config = ConfigState::default();
        let identity = test_identity(1000, 1000, &[1000]);
        let base = unique_temp_path("navix-preview-cmds");
        fs::create_dir_all(&base).expect("create base");
        let path = base.join("README.md");
        fs::write(&path, b"# test").expect("write file");
        fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("chmod file");
        let entry = NavEntry {
            name: "README.md".to_string(),
            path: path.clone(),
            is_dir: false,
            is_symlink: false,
            file_type_char: '-',
            mode: 0o640,
            nlink: 1,
            uid: 1000,
            gid: 1000,
            size: 0,
            mtime: 0,
        };

        let commands = available_preview_file_commands(&entry, &config, "nvim", &identity);
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0], ('r', "bat README.md".to_string()));
        assert_eq!(commands[1], ('w', "nvim README.md".to_string()));

        fs::remove_dir_all(base).expect("cleanup temp");
    }

    #[test]
    fn available_preview_file_commands_fallback_for_unknown_extension() {
        let config = ConfigState::default();
        let identity = test_identity(1000, 1000, &[1000]);
        let base = unique_temp_path("navix-preview-cmds-fallback");
        fs::create_dir_all(&base).expect("create base");
        let path = base.join("editor.html");
        fs::write(&path, b"<h1>test</h1>").expect("write file");
        fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("chmod file");
        let entry = NavEntry {
            name: "editor.html".to_string(),
            path: path.clone(),
            is_dir: false,
            is_symlink: false,
            file_type_char: '-',
            mode: 0o640,
            nlink: 1,
            uid: 1000,
            gid: 1000,
            size: 0,
            mtime: 0,
        };

        let commands = available_preview_file_commands(&entry, &config, "nvim", &identity);
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0], ('r', "bat editor.html".to_string()));
        assert_eq!(commands[1], ('w', "nvim editor.html".to_string()));

        fs::remove_dir_all(base).expect("cleanup temp");
    }

    #[test]
    fn preview_file_commands_panel_uses_escape_key_labels() {
        let config = ConfigState::default();
        let identity = test_identity(1000, 1000, &[1000]);
        let base = unique_temp_path("navix-preview-escape-labels");
        fs::create_dir_all(&base).expect("create base");
        let path = base.join("README.md");
        fs::write(&path, b"# test").expect("write file");
        fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("chmod file");
        let entry = NavEntry {
            name: "README.md".to_string(),
            path: path.clone(),
            is_dir: false,
            is_symlink: false,
            file_type_char: '-',
            mode: 0o640,
            nlink: 1,
            uid: 1000,
            gid: 1000,
            size: 0,
            mtime: 0,
        };

        let text = preview_file_commands_panel_text(&entry, &config, "nvim", &identity, 60, 10, false);
        let rendered = text
            .lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref().to_string()))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(rendered.contains("Esc+r"));
        assert!(rendered.contains("Esc+w"));
        assert!(rendered.contains("README.md"));
        assert!(!rendered.contains("Esc+x"));

        fs::remove_dir_all(base).expect("cleanup temp");
    }

    #[test]
    fn preview_file_commands_panel_centers_block_with_left_aligned_lines() {
        let config = ConfigState::default();
        let identity = test_identity(1000, 1000, &[1000]);
        let base = unique_temp_path("navix-preview-align-block");
        fs::create_dir_all(&base).expect("create base");
        let path = base.join("README.md");
        fs::write(&path, b"# test").expect("write file");
        fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("chmod file");
        let entry = NavEntry {
            name: "README.md".to_string(),
            path: path.clone(),
            is_dir: false,
            is_symlink: false,
            file_type_char: '-',
            mode: 0o640,
            nlink: 1,
            uid: 1000,
            gid: 1000,
            size: 0,
            mtime: 0,
        };

        let text = preview_file_commands_panel_text(&entry, &config, "nvim", &identity, 60, 10, false);
        let command_lines = text
            .lines
            .iter()
            .filter(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref().contains("Esc+"))
            })
            .collect::<Vec<_>>();
        assert!(command_lines.len() >= 2);
        let first_left_pad = command_lines[0].spans[0].content.len();
        let second_left_pad = command_lines[1].spans[0].content.len();
        assert_eq!(first_left_pad, second_left_pad);

        fs::remove_dir_all(base).expect("cleanup temp");
    }

    #[test]
    fn preview_file_commands_panel_compact_mode_hides_command_tail() {
        let config = ConfigState::default();
        let identity = test_identity(1000, 1000, &[1000]);
        let base = unique_temp_path("navix-preview-compact-labels");
        fs::create_dir_all(&base).expect("create base");
        let path = base.join("README.md");
        fs::write(&path, b"# test").expect("write file");
        fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("chmod file");
        let entry = NavEntry {
            name: "README.md".to_string(),
            path: path.clone(),
            is_dir: false,
            is_symlink: false,
            file_type_char: '-',
            mode: 0o640,
            nlink: 1,
            uid: 1000,
            gid: 1000,
            size: 0,
            mtime: 0,
        };

        let text = preview_file_commands_panel_text(&entry, &config, "nvim", &identity, 10, 10, false);
        let rendered = text
            .lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref().to_string()))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(rendered.contains("Esc+r"));
        assert!(rendered.contains("Esc+w"));
        assert!(!rendered.contains("README.md"));
        assert!(!rendered.contains("bat"));
        assert!(!rendered.contains("nvim"));

        fs::remove_dir_all(base).expect("cleanup temp");
    }

    #[test]
    fn navigation_file_command_action_runs_read_in_preview() {
        let config = ConfigState::default();
        let identity = test_identity(1000, 1000, &[1000]);
        let base = unique_temp_path("navix-nav-read-action");
        fs::create_dir_all(&base).expect("create base");
        let path = base.join("README.md");
        fs::write(&path, b"# test").expect("write file");
        fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("chmod file");
        let entry = NavEntry {
            name: "README.md".to_string(),
            path: path.clone(),
            is_dir: false,
            is_symlink: false,
            file_type_char: '-',
            mode: 0o640,
            nlink: 1,
            uid: 1000,
            gid: 1000,
            size: 0,
            mtime: 0,
        };

        let action = navigation_file_command_action(Some(&entry), 'r', &config, "nvim", &identity);
        assert_eq!(
            action,
            Some(NavigationFileCommandAction::RunReadInPreview(
                "bat README.md".to_string()
            ))
        );

        fs::remove_dir_all(base).expect("cleanup temp");
    }

    #[test]
    fn navigation_file_command_action_runs_write_in_preview() {
        let config = ConfigState::default();
        let identity = test_identity(1000, 1000, &[1000]);
        let base = unique_temp_path("navix-nav-write-action");
        fs::create_dir_all(&base).expect("create base");
        let path = base.join("README.md");
        fs::write(&path, b"# test").expect("write file");
        fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("chmod file");
        let entry = NavEntry {
            name: "README.md".to_string(),
            path: path.clone(),
            is_dir: false,
            is_symlink: false,
            file_type_char: '-',
            mode: 0o640,
            nlink: 1,
            uid: 1000,
            gid: 1000,
            size: 0,
            mtime: 0,
        };

        let action = navigation_file_command_action(Some(&entry), 'w', &config, "nvim", &identity);
        assert_eq!(
            action,
            Some(NavigationFileCommandAction::RunWriteInPreview(
                "nvim README.md".to_string()
            ))
        );

        fs::remove_dir_all(base).expect("cleanup temp");
    }

    #[test]
    fn navigation_file_command_action_prefills_exec_in_shell() {
        let config = ConfigState::default();
        let identity = test_identity(1000, 1000, &[1000]);
        let base = unique_temp_path("navix-nav-exec-action");
        fs::create_dir_all(&base).expect("create base");
        let path = base.join("script.sh");
        fs::write(&path, b"#!/bin/bash\necho hi\n").expect("write file");
        fs::set_permissions(&path, std::fs::Permissions::from_mode(0o750)).expect("chmod file");
        let entry = NavEntry {
            name: "script.sh".to_string(),
            path: path.clone(),
            is_dir: false,
            is_symlink: false,
            file_type_char: '-',
            mode: 0o750,
            nlink: 1,
            uid: 1000,
            gid: 1000,
            size: 0,
            mtime: 0,
        };

        let action = navigation_file_command_action(Some(&entry), 'x', &config, "nvim", &identity);
        assert_eq!(
            action,
            Some(NavigationFileCommandAction::PrefillShell(
                "bash script.sh".to_string()
            ))
        );

        fs::remove_dir_all(base).expect("cleanup temp");
    }

    #[test]
    fn preview_depth_clamps_within_bounds() {
        assert_eq!(clamp_preview_depth(0, 6), 1);
        assert_eq!(clamp_preview_depth(9, 6), 6);
        assert_eq!(clamp_preview_depth(4, 0), 1);
    }

    #[test]
    fn preview_directory_tree_lines_show_error_when_root_unreadable() {
        let missing = unique_temp_path("navix-preview-missing");
        let lines = preview_directory_tree_lines(&missing, 2);
        let rendered = lines.join("\n");
        assert!(rendered.contains("error:"));
    }

    #[test]
    fn preview_directory_panel_text_uses_navigation_name_colors() {
        let base = unique_temp_path("navix-preview-color");
        fs::create_dir_all(base.join("docs")).expect("create dirs");
        fs::write(base.join("README.md"), b"# hi").expect("create file");

        let mut colors = LsColorsTheme::fallback();
        colors.apply("di=01;34:*.md=01;33");
        let text = preview_directory_panel_text(&base, Some("preview"), 1, &colors, false);

        let dir_span = text
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content.as_ref().contains("docs/"))
            .expect("dir span");
        assert_eq!(dir_span.style.fg, Some(Color::Blue));

        let file_span = text
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content.as_ref().contains("README.md"))
            .expect("file span");
        assert_eq!(file_span.style.fg, Some(Color::Yellow));

        fs::remove_dir_all(base).expect("cleanup temp");
    }

    #[test]
    fn preview_directory_panel_header_uses_hovered_folder_name() {
        let base = unique_temp_path("navix-preview-header");
        fs::create_dir_all(base.join("docs")).expect("create dirs");

        let colors = LsColorsTheme::fallback();
        let text = preview_directory_panel_text(
            &base.join("docs"),
            Some("docs"),
            1,
            &colors,
            false,
        );
        let header = text
            .lines
            .first()
            .expect("header line")
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(header, " docs/");

        fs::remove_dir_all(base).expect("cleanup temp");
    }

    #[test]
    fn nav_long_listing_contains_permission_and_size() {
        let entry = NavEntry {
            name: "tmp".to_string(),
            path: PathBuf::from("/tmp"),
            is_dir: true,
            is_symlink: false,
            file_type_char: 'd',
            mode: 0o755,
            nlink: 3,
            uid: 0,
            gid: 0,
            size: 4096,
            mtime: 0,
        };
        let listing = nav_long_listing(&entry);
        assert!(listing.starts_with("drwxr-xr-x"));
        assert!(listing.contains(" 4096 "));
    }

    #[test]
    fn sgr_to_style_decodes_basic_and_bright_codes() {
        let basic = sgr_to_style("01;34");
        assert_eq!(basic.fg, Some(Color::Blue));
        assert!(basic.add_modifier.contains(Modifier::BOLD));

        let bright = sgr_to_style("90");
        assert_eq!(bright.fg, Some(Color::DarkGray));

        let indexed = sgr_to_style("38;5;214");
        assert_eq!(indexed.fg, Some(Color::Indexed(214)));
    }

    #[test]
    fn ls_colors_parser_ignores_malformed_segments() {
        let mut theme = LsColorsTheme::fallback();
        theme.apply("di=01;35:bad-segment:*=01;31:*.md=01;33");
        let dir = theme.style_for_entry("src", true, false, 0);
        let markdown = theme.style_for_entry("README.md", false, false, 0o644);

        assert_eq!(dir.fg, Some(Color::Magenta));
        assert_eq!(markdown.fg, Some(Color::Yellow));
    }

    #[test]
    fn navigation_name_style_colors_key_entry_types() {
        let colors = LsColorsTheme::fallback();
        let dir = navigation_name_style(&colors, "macros", true, false, 0o755);
        assert_eq!(dir.fg, Some(Color::Blue));
        assert!(dir.add_modifier.contains(Modifier::BOLD));

        let symlink = navigation_name_style(&colors, "link", false, true, 0o777);
        assert_eq!(symlink.fg, Some(Color::Cyan));

        let executable = navigation_name_style(&colors, "run.sh", false, false, 0o755);
        assert_eq!(executable.fg, Some(Color::Green));

        let regular = navigation_name_style(&colors, "README.md", false, false, 0o644);
        assert_eq!(regular.fg, None);
    }

    #[test]
    fn navigation_name_style_prefers_extension_then_executable() {
        let mut colors = LsColorsTheme::fallback();
        colors.apply("*.sh=01;31:ex=01;32");

        let ext_override = navigation_name_style(&colors, "deploy.sh", false, false, 0o755);
        assert_eq!(ext_override.fg, Some(Color::Red));
        assert!(ext_override.add_modifier.contains(Modifier::BOLD));

        let executable = navigation_name_style(&colors, "deploy", false, false, 0o755);
        assert_eq!(executable.fg, Some(Color::Green));
    }

    #[test]
    fn navigation_tree_lines_show_only_level_one_entries() {
        let base = unique_temp_path("navix-nav-test");
        fs::create_dir_all(base.join("sub/inner")).expect("create dirs");
        fs::write(base.join("file.txt"), b"hello").expect("create file");
        fs::write(base.join("sub/inner/deep.txt"), b"hidden").expect("create nested file");

        let lines = navigation_tree_lines(&base);
        let rendered = lines.join("\n");

        assert!(lines.get(1).is_some_and(|line| line.contains("..")));
        assert!(rendered.contains("sub/"));
        assert!(rendered.contains("file.txt"));
        assert!(rendered.contains(""));
        assert!(rendered.contains(""));
        assert!(!rendered.contains('['));
        assert!(!rendered.contains("deep.txt"));

        fs::remove_dir_all(base).expect("cleanup temp");
    }

    #[test]
    fn preview_shortcut_bounces_to_navigation_for_directory_tree() {
        let dir_entry = NavEntry {
            name: "docs".to_string(),
            path: PathBuf::from("/tmp/docs"),
            is_dir: true,
            is_symlink: false,
            file_type_char: 'd',
            mode: 0o755,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 0,
            mtime: 0,
        };
        let target = preview_shortcut_target(Some(&dir_entry), PreviewMode::DirectoryTree);
        assert_eq!(target, ActivePane::Navigation);
    }

    #[test]
    fn preview_shortcut_bounces_to_navigation_for_file_command_list() {
        let file_entry = NavEntry {
            name: "README.md".to_string(),
            path: PathBuf::from("/tmp/README.md"),
            is_dir: false,
            is_symlink: false,
            file_type_char: '-',
            mode: 0o644,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 0,
            mtime: 0,
        };
        assert_eq!(
            preview_shortcut_target(Some(&file_entry), PreviewMode::FileText),
            ActivePane::Navigation
        );
    }

    #[test]
    fn preview_shortcut_allows_preview_for_empty_mode() {
        let file_entry = NavEntry {
            name: "README.md".to_string(),
            path: PathBuf::from("/tmp/README.md"),
            is_dir: false,
            is_symlink: false,
            file_type_char: '-',
            mode: 0o644,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 0,
            mtime: 0,
        };
        assert_eq!(
            preview_shortcut_target(Some(&file_entry), PreviewMode::Empty),
            ActivePane::Preview
        );
    }

    #[test]
    fn panel_click_focus_target_bounces_preview_using_preview_rule() {
        let file_entry = NavEntry {
            name: "README.md".to_string(),
            path: PathBuf::from("/tmp/README.md"),
            is_dir: false,
            is_symlink: false,
            file_type_char: '-',
            mode: 0o644,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 0,
            mtime: 0,
        };
        assert_eq!(
            panel_click_focus_target(
                ActivePane::Preview,
                false,
                Some(&file_entry),
                PreviewMode::FileText,
            ),
            ActivePane::Navigation
        );
    }

    #[test]
    fn panel_click_focus_target_keeps_preview_when_overlay_is_active() {
        let file_entry = NavEntry {
            name: "README.md".to_string(),
            path: PathBuf::from("/tmp/README.md"),
            is_dir: false,
            is_symlink: false,
            file_type_char: '-',
            mode: 0o644,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 0,
            mtime: 0,
        };
        assert_eq!(
            panel_click_focus_target(
                ActivePane::Preview,
                true,
                Some(&file_entry),
                PreviewMode::FileText,
            ),
            ActivePane::Preview
        );
    }

    #[test]
    fn pane_from_mouse_position_maps_coordinates_to_panel() {
        let nav = Rect {
            x: 0,
            y: 0,
            width: 30,
            height: 10,
        };
        let preview = Rect {
            x: 30,
            y: 0,
            width: 70,
            height: 10,
        };
        let shell = Rect {
            x: 0,
            y: 10,
            width: 100,
            height: 5,
        };
        assert_eq!(
            pane_from_mouse_position(5, 2, nav, preview, shell),
            Some(ActivePane::Navigation)
        );
        assert_eq!(
            pane_from_mouse_position(35, 2, nav, preview, shell),
            Some(ActivePane::Preview)
        );
        assert_eq!(
            pane_from_mouse_position(10, 12, nav, preview, shell),
            Some(ActivePane::Shell)
        );
        assert_eq!(pane_from_mouse_position(120, 12, nav, preview, shell), None);
    }

    #[test]
    fn panel_areas_for_focus_click_matches_fullish_preview_constraints() {
        let terminal = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 40,
        };
        let (_, preview_nav_fullish, _) = panel_areas_for_focus_click(
            terminal,
            ActivePane::Navigation,
            false,
            false,
            true,
            false,
        );
        assert_eq!(preview_nav_fullish.width, 12);

        let (nav_preview_overlay, _, _) = panel_areas_for_focus_click(
            terminal,
            ActivePane::Navigation,
            false,
            false,
            false,
            true,
        );
        assert_eq!(nav_preview_overlay.width, 12);
    }

    #[test]
    fn effective_access_prefers_owner_group_and_other_bits() {
        let owner_identity = test_identity(1001, 1001, &[1001]);
        let owner = effective_access_from_mode(0o640, 1001, 2000, '-', &owner_identity);
        assert!(owner.read);
        assert!(owner.write);
        assert!(!owner.exec);

        let group_identity = test_identity(3000, 2000, &[2000]);
        let group = effective_access_from_mode(0o640, 1001, 2000, '-', &group_identity);
        assert!(group.read);
        assert!(!group.write);
        assert!(!group.exec);

        let other_identity = test_identity(3000, 3000, &[3000]);
        let other = effective_access_from_mode(0o640, 1001, 2000, '-', &other_identity);
        assert!(!other.read);
        assert!(!other.write);
        assert!(!other.exec);
    }

    #[test]
    fn kernel_effective_access_for_path_matches_owned_file_mode() {
        let base = unique_temp_path("navix-kernel-access");
        fs::create_dir_all(&base).expect("create base");
        let path = base.join("file.txt");
        fs::write(&path, b"hello").expect("write file");
        fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod file");

        let access = kernel_effective_access_for_path(&path).expect("kernel access");
        assert!(access.read);
        assert!(access.write);
        assert!(!access.exec);

        fs::remove_dir_all(base).expect("cleanup temp");
    }
}

