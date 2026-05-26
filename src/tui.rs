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

pub(crate) fn border_style(focused: bool, fullish_shell_theme: bool) -> Style {
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

pub(crate) fn tab_title(title: &str, _focused: bool) -> String {
    title.to_string()
}

pub(crate) fn footer_key_style(fullish_shell_theme: bool) -> Style {
    if fullish_shell_theme {
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
    }
}

#[cfg(test)]
pub(crate) fn footer_shortcuts(active: ActivePane, config_open: bool, config_dirty: bool) -> String {
    if config_open {
        return "Close: Esc".to_string();
    }
    let exit_label = if config_dirty { "Exit*" } else { "Exit" };
    match active {
        ActivePane::Shell => format!(
            "Shell: Esc+0 | Navigation: Esc+1 | Preview: Esc+2 | Config: Esc+c | Full: Esc+f | Scroll: PgUp/PgDown, Esc+↑/Esc+↓ | {exit_label}: Ctrl+d"
        ),
        ActivePane::Navigation => format!(
            "Shell: Esc+0 | Navigation: Esc+1 | Preview: Esc+2 | Config: Esc+c | Full: Esc+f | Scroll: PgUp/PgDown, ↑/↓, Home/End, ←/→ | {exit_label}: Ctrl+d"
        ),
        ActivePane::Preview => format!(
            "Shell: Esc+0 | Navigation: Esc+1 | Preview: Esc+2 | Config: Esc+c | Full: Esc+f | Scroll: PgUp/PgDown, ↑/↓ | {exit_label}: Ctrl+d"
        ),
    }
}

pub(crate) fn footer_meta() -> String {
    format!("Donate {}", env!("CARGO_PKG_VERSION"))
}

pub(crate) fn footer_shortcuts_line(
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
            ActivePane::Navigation => vec![
                ("Shell: ".to_string(), "Esc+0".to_string(), false),
                (" | Navigation: ".to_string(), "Esc+1".to_string(), false),
                (" | Preview: ".to_string(), "Esc+2".to_string(), false),
                (" | Config: ".to_string(), "Esc+c".to_string(), true),
                (" | Full: ".to_string(), "Esc+f".to_string(), false),
                (
                    " | Scroll: ".to_string(),
                    "PgUp/PgDown, ↑/↓, Home/End, ←/→".to_string(),
                    false,
                ),
                (exit_label.to_string(), "Ctrl+d".to_string(), false),
            ],
            ActivePane::Preview => vec![
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

pub(crate) fn append_key_with_white_slashes(
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

pub(crate) fn footer_meta_line(fullish_shell_theme: bool) -> Line<'static> {
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

pub(crate) fn truncate_to_width(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

pub(crate) fn nav_meta_compact_lines(meta: &str, width: usize) -> (String, String) {
    if width == 0 {
        return (String::new(), String::new());
    }
    let (top_core, bottom_core) = nav_meta_compact_parts(meta);
    let mut top = String::from("──");
    top.push_str(&top_core);
    if top.chars().count() > width {
        top = truncate_to_width(&top, width);
    } else {
        top.push_str(&alternating_border_fill(width.saturating_sub(top.chars().count())));
    }

    let suffix = if bottom_core.is_empty() {
        String::from("──")
    } else {
        format!("{bottom_core}──")
    };
    let mut bottom = String::from("──");
    let used = bottom.chars().count().saturating_add(suffix.chars().count());
    if used >= width {
        let compact = format!("──{bottom_core}");
        let compact_len = compact.chars().count();
        bottom = compact
            .chars()
            .skip(compact_len.saturating_sub(width))
            .collect();
    } else {
        bottom.push_str(&alternating_border_fill(width.saturating_sub(used)));
        bottom.push_str(&suffix);
    }
    (top, bottom)
}

fn alternating_border_fill(width: usize) -> String {
    "─".repeat(width)
}

fn nav_meta_compact_parts(meta: &str) -> (String, String) {
    let tokens: Vec<&str> = meta.split_whitespace().collect();
    if tokens.len() >= 4 {
        let top = tokens[..tokens.len().saturating_sub(4)].join(" ");
        let bottom = tokens[tokens.len().saturating_sub(4)..].join(" ");
        return (top, bottom);
    }
    (meta.trim().to_string(), String::new())
}

#[cfg(test)]
pub(crate) fn compose_footer_line(active: ActivePane, width: u16) -> String {
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

pub(crate) fn should_use_fullish_theme(active: ActivePane, alt_screen_active: bool) -> bool {
    active == ActivePane::Shell && alt_screen_active
}

pub(crate) fn preview_panel_text(
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

pub(crate) fn preview_directory_panel_text(
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

pub(crate) fn preview_file_commands_panel_text(
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

fn centered_left_padding(content_width: usize, total_width: usize) -> usize {
    total_width.saturating_sub(content_width) / 2
}

fn centered_top_padding(content_lines: usize, total_lines: usize) -> usize {
    total_lines.saturating_sub(content_lines) / 2
}

pub(crate) fn render_panel_status(
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

pub(crate) fn dim_rendered_area(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect) {
    let buf = frame.buffer_mut();
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            buf[(x, y)].set_style(Style::default().fg(Color::DarkGray).bg(Color::Black));
        }
    }
}

pub(crate) fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    area: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
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

pub(crate) fn inner(rect: ratatui::layout::Rect) -> ratatui::layout::Rect {
    ratatui::layout::Rect {
        x: rect.x.saturating_add(1),
        y: rect.y.saturating_add(1),
        width: rect.width.saturating_sub(2),
        height: rect.height.saturating_sub(2),
    }
}
