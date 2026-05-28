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
use std::path::Path;

pub(crate) fn border_style(focused: bool, fullish_shell_theme: bool) -> Style {
    if fullish_shell_theme {
        let base = Style::default().fg(Color::DarkGray);
        if focused {
            base.add_modifier(Modifier::BOLD)
        } else {
            base
        }
    } else if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Blue)
    }
}

pub(crate) fn tab_title(title: &str, _focused: bool) -> String {
    title.to_string()
}

pub(crate) fn footer_key_style(fullish_shell_theme: bool) -> Style {
    if fullish_shell_theme {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD)
    }
}

#[cfg(test)]
pub(crate) fn footer_shortcuts(
    active: ActivePane,
    config_open: bool,
    config_dirty: bool,
) -> String {
    if config_open {
        return "Close: Esc".to_string();
    }
    let exit_label = if config_dirty { "Exit*" } else { "Exit" };
    match active {
        ActivePane::Shell => format!(
            "Help: Esc+? | {exit_label}: Ctrl+d | Config: Esc+c | Shell: Esc+0 | Navigation: Esc+1 | Preview: Esc+2 | Expand view: Esc+f | Scroll: PgUp/PgDown, Esc+↑/Esc+↓"
        ),
        ActivePane::Navigation => format!(
            "Help: Esc+? | {exit_label}: Ctrl+d | Config: Esc+c | Shell: Esc+0 | Navigation: Esc+1 | Preview: Esc+2 | Expand view: Esc+f | Scroll-Select: PgUp/PgDown, ↑/↓, Home/End, ←/→"
        ),
        ActivePane::Preview => format!(
            "Help: Esc+? | {exit_label}: Ctrl+d | Config: Esc+c | Shell: Esc+0 | Navigation: Esc+1 | Preview: Esc+2 | Expand view: Esc+f | Scroll: PgUp/PgDown, ↑/↓"
        ),
    }
}

pub(crate) fn footer_meta() -> String {
    format!("Donate {}", env!("CARGO_PKG_VERSION"))
}

pub(crate) fn help_panel_text(context: ActivePane, nav_colors: &LsColorsTheme) -> Text<'static> {
    let heading_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let section_heading_style = heading_style.add_modifier(Modifier::UNDERLINED);
    let body_style = Style::default().fg(Color::White);
    let input_heading_style = body_style.add_modifier(Modifier::BOLD);
    let global_cmd_style = Style::default()
        .fg(Color::Blue)
        .add_modifier(Modifier::BOLD);
    let panel_cmd_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let directory_icon_style = navigation_name_style(nav_colors, "help", true, false, 0o755);
    let shortcut_len = |commands: &[&str]| -> usize {
        commands
            .iter()
            .enumerate()
            .map(|(idx, command)| command.chars().count() + if idx > 0 { 3 } else { 0 })
            .sum()
    };
    let shortcut_line = |commands: &[&str],
                         desc: &str,
                         command_style: Style,
                         label_width: usize|
     -> Line<'static> {
        let mut spans = vec![Span::styled("• ".to_string(), body_style)];
        let label_len = shortcut_len(commands);
        for (idx, command) in commands.iter().enumerate() {
            if idx > 0 {
                spans.push(Span::styled(" / ".to_string(), body_style));
            }
            spans.push(Span::styled((*command).to_string(), command_style));
        }
        spans.push(Span::styled(
            " ".repeat(label_width.saturating_sub(label_len)),
            body_style,
        ));
        spans.push(Span::styled(format!(" : {desc}"), body_style));
        Line::from(spans)
    };
    let global_label_width = [
        shortcut_len(&["Esc+?"]),
        shortcut_len(&["Ctrl+Shift+C"]),
        shortcut_len(&["Esc"]),
        shortcut_len(&["Ctrl+d"]),
        shortcut_len(&["Esc+c"]),
        shortcut_len(&["Esc+0"]),
        shortcut_len(&["Esc+1"]),
        shortcut_len(&["Esc+2"]),
        shortcut_len(&["Esc+f"]),
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    let shell_label_width = [
        shortcut_len(&["PgUp", "PgDown"]),
        shortcut_len(&["Esc+↑", "Esc+↓"]),
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    let navigation_label_width = [
        shortcut_len(&["↑", "↓"]),
        shortcut_len(&["PgUp", "PgDown"]),
        shortcut_len(&["Ctrl+↑", "Ctrl+↓"]),
        shortcut_len(&["Home", "End", "←", "→"]),
        shortcut_len(&["Backspace"]),
        shortcut_len(&["Ctrl+Backspace", "Ctrl+Delete"]),
        "d---  Enter".chars().count(),
        "-r--  r+Enter / Esc+r / Enter".chars().count(),
        "--w-  w+Enter / Esc+w".chars().count(),
        "---x  x+Enter / Esc+x".chars().count(),
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    let preview_label_width = [
        shortcut_len(&["Tab", "Shift+Tab"]),
        shortcut_len(&["↑", "↓"]),
        shortcut_len(&["←", "→"]),
        shortcut_len(&["Ctrl+c"]),
        shortcut_len(&["Enter"]),
        shortcut_len(&["Shift+Enter"]),
        shortcut_len(&["Esc"]),
    ]
    .into_iter()
    .max()
    .unwrap_or(0);

    let global_section = vec![
        Line::from(Span::styled("Global shortcuts", section_heading_style)),
        shortcut_line(
            &["Esc+?"],
            "Open navix Help Context (this)",
            global_cmd_style,
            global_label_width,
        ),
        shortcut_line(
            &["Ctrl+Shift+C"],
            "Copy selected text",
            global_cmd_style,
            global_label_width,
        ),
        shortcut_line(
            &["Esc"],
            "Close Help/config overlay",
            global_cmd_style,
            global_label_width,
        ),
        shortcut_line(
            &["Ctrl+d"],
            "Exit navix",
            global_cmd_style,
            global_label_width,
        ),
        shortcut_line(
            &["Esc+c"],
            "Open navix Extension Config",
            global_cmd_style,
            global_label_width,
        ),
        shortcut_line(
            &["Esc+0"],
            "Focus Shell",
            global_cmd_style,
            global_label_width,
        ),
        shortcut_line(
            &["Esc+1"],
            "Focus Navigation",
            global_cmd_style,
            global_label_width,
        ),
        shortcut_line(
            &["Esc+2"],
            "Focus Preview",
            global_cmd_style,
            global_label_width,
        ),
        shortcut_line(
            &["Esc+f"],
            "Toggle expanded view",
            global_cmd_style,
            global_label_width,
        ),
    ];

    let shell_section = vec![
        Line::from(Span::styled("Shell Panel shortcuts", section_heading_style)),
        shortcut_line(
            &["PgUp", "PgDown"],
            "Scroll shell output",
            panel_cmd_style,
            shell_label_width,
        ),
        shortcut_line(
            &["Esc+↑", "Esc+↓"],
            "Scroll shell output by one line",
            panel_cmd_style,
            shell_label_width,
        ),
    ];

    let navigation_section = vec![
        Line::from(Span::styled(
            "Navigation Panel shortcuts",
            section_heading_style,
        )),
        shortcut_line(
            &["↑", "↓"],
            "Move selection",
            panel_cmd_style,
            navigation_label_width,
        ),
        shortcut_line(
            &["PgUp", "PgDown"],
            "Move selection by page",
            panel_cmd_style,
            navigation_label_width,
        ),
        shortcut_line(
            &["Ctrl+↑", "Ctrl+↓"],
            "Scroll viewport without changing selection",
            panel_cmd_style,
            navigation_label_width,
        ),
        shortcut_line(
            &["Home", "End", "←", "→"],
            "Move selection to first or last entry",
            panel_cmd_style,
            navigation_label_width,
        ),
        shortcut_line(
            &["Backspace"],
            "cd ../ (folder up) or delete filter char",
            panel_cmd_style,
            navigation_label_width,
        ),
        shortcut_line(
            &["Ctrl+Backspace", "Ctrl+Delete"],
            "Clear filter",
            panel_cmd_style,
            navigation_label_width,
        ),
        Line::from(vec![
            Span::styled("• ".to_string(), body_style),
            Span::styled("d--- ".to_string(), body_style),
            Span::styled("".to_string(), directory_icon_style),
            Span::styled(" ".to_string(), body_style),
            Span::styled("Enter".to_string(), panel_cmd_style),
            Span::styled(
                " ".repeat(navigation_label_width.saturating_sub("d---  Enter".chars().count())),
                body_style,
            ),
            Span::styled(" : Directory cd".to_string(), body_style),
        ]),
        Line::from(vec![
            Span::styled("• ".to_string(), body_style),
            Span::styled("-r-- ".to_string(), body_style),
            Span::styled("".to_string(), body_style),
            Span::styled(" ".to_string(), body_style),
            Span::styled("r+Enter".to_string(), panel_cmd_style),
            Span::styled(" / ".to_string(), body_style),
            Span::styled("Esc+r".to_string(), panel_cmd_style),
            Span::styled(" / ".to_string(), body_style),
            Span::styled("Enter".to_string(), panel_cmd_style),
            Span::styled(
                " ".repeat(
                    navigation_label_width
                        .saturating_sub("-r--  r+Enter / Esc+r / Enter".chars().count()),
                ),
                body_style,
            ),
            Span::styled(" : File read command shortcut".to_string(), body_style),
        ]),
        Line::from(vec![
            Span::styled("• ".to_string(), body_style),
            Span::styled("--w- ".to_string(), body_style),
            Span::styled("".to_string(), body_style),
            Span::styled(" ".to_string(), body_style),
            Span::styled("w+Enter".to_string(), panel_cmd_style),
            Span::styled(" / ".to_string(), body_style),
            Span::styled("Esc+w".to_string(), panel_cmd_style),
            Span::styled(
                " ".repeat(
                    navigation_label_width.saturating_sub("--w-  w+Enter / Esc+w".chars().count()),
                ),
                body_style,
            ),
            Span::styled(" : File write command shortcut".to_string(), body_style),
        ]),
        Line::from(vec![
            Span::styled("• ".to_string(), body_style),
            Span::styled("---x ".to_string(), body_style),
            Span::styled("".to_string(), body_style),
            Span::styled(" ".to_string(), body_style),
            Span::styled("x+Enter".to_string(), panel_cmd_style),
            Span::styled(" / ".to_string(), body_style),
            Span::styled("Esc+x".to_string(), panel_cmd_style),
            Span::styled(
                " ".repeat(
                    navigation_label_width.saturating_sub("---x  x+Enter / Esc+x".chars().count()),
                ),
                body_style,
            ),
            Span::styled(" : File execute command shortcut".to_string(), body_style),
        ]),
    ];

    let mut preview_section = vec![
        Line::from(Span::styled(
            "Preview Panel shortcuts",
            section_heading_style,
        )),
        Line::from(Span::styled(
            "<path typing>".to_string(),
            input_heading_style,
        )),
        shortcut_line(
            &["Tab", "Shift+Tab"],
            "Cycle completion forward/reverse",
            panel_cmd_style,
            preview_label_width,
        ),
        shortcut_line(
            &["←", "→"],
            "Move completion columns",
            panel_cmd_style,
            preview_label_width,
        ),
        shortcut_line(
            &["Ctrl+c"],
            "Clear input field and reset completion",
            panel_cmd_style,
            preview_label_width,
        ),
        Line::from(Span::styled(
            "<history selection>".to_string(),
            input_heading_style,
        )),
        shortcut_line(
            &["↑", "↓"],
            "Rotate history",
            panel_cmd_style,
            preview_label_width,
        ),
        Line::from(vec![
            Span::styled("• ".to_string(), body_style),
            Span::styled("Enter".to_string(), panel_cmd_style),
            Span::styled(
                " ".repeat(preview_label_width.saturating_sub("Enter".chars().count())),
                body_style,
            ),
            Span::styled(" : Jump to ".to_string(), body_style),
            Span::styled("".to_string(), directory_icon_style),
            Span::styled("/".to_string(), body_style),
            Span::styled("".to_string(), body_style),
            Span::styled(" in Navigation".to_string(), body_style),
        ]),
        shortcut_line(
            &["Shift+Enter"],
            "Jump but keep focus in Preview",
            panel_cmd_style,
            preview_label_width,
        ),
    ];

    if context == ActivePane::Preview {
        preview_section.push(shortcut_line(
            &["Esc"],
            "Close preview command overlay (when open)",
            panel_cmd_style,
            preview_label_width,
        ));
    }

    let mut panel_sections = vec![
        (ActivePane::Shell, shell_section),
        (ActivePane::Navigation, navigation_section),
        (ActivePane::Preview, preview_section),
    ];
    let active_section =
        if let Some(active_index) = panel_sections.iter().position(|(pane, _)| *pane == context) {
            panel_sections.remove(active_index).1
        } else {
            Vec::new()
        };

    let mut lines = Vec::new();
    if !active_section.is_empty() {
        lines.extend(active_section);
        lines.push(Line::from(""));
    }
    lines.extend(global_section);
    for (_, section_lines) in panel_sections {
        lines.push(Line::from(""));
        lines.extend(section_lines);
    }

    Text::from(lines)
}

pub(crate) fn footer_shortcuts_line(
    active: ActivePane,
    fullish_shell_theme: bool,
    config_open: bool,
    config_dirty: bool,
    highlight_config_shortcut: bool,
) -> Line<'static> {
    let key_style = footer_key_style(fullish_shell_theme);
    let panel_key_style = border_style(true, fullish_shell_theme);
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
    let exit_label = if config_dirty {
        " | Exit*: "
    } else {
        " | Exit: "
    };
    let segments: Vec<(String, String, bool, bool)> = if config_open {
        vec![("Close: ".to_string(), "Esc".to_string(), false, false)]
    } else {
        match active {
            ActivePane::Shell => vec![
                ("Help: ".to_string(), "Esc+?".to_string(), false, false),
                (exit_label.to_string(), "Ctrl+d".to_string(), false, false),
                (" | Config: ".to_string(), "Esc+c".to_string(), true, false),
                (" | Shell: ".to_string(), "Esc+0".to_string(), false, false),
                (
                    " | Navigation: ".to_string(),
                    "Esc+1".to_string(),
                    false,
                    false,
                ),
                (
                    " | Preview: ".to_string(),
                    "Esc+2".to_string(),
                    false,
                    false,
                ),
                (
                    " | Expand view: ".to_string(),
                    "Esc+f".to_string(),
                    false,
                    false,
                ),
                (
                    " | Scroll: ".to_string(),
                    "PgUp/PgDown, Esc+\u{2191}/Esc+\u{2193}".to_string(),
                    false,
                    true,
                ),
            ],
            ActivePane::Navigation => vec![
                ("Help: ".to_string(), "Esc+?".to_string(), false, false),
                (exit_label.to_string(), "Ctrl+d".to_string(), false, false),
                (" | Config: ".to_string(), "Esc+c".to_string(), true, false),
                (" | Shell: ".to_string(), "Esc+0".to_string(), false, false),
                (
                    " | Navigation: ".to_string(),
                    "Esc+1".to_string(),
                    false,
                    false,
                ),
                (
                    " | Preview: ".to_string(),
                    "Esc+2".to_string(),
                    false,
                    false,
                ),
                (
                    " | Expand view: ".to_string(),
                    "Esc+f".to_string(),
                    false,
                    false,
                ),
                (
                    " | Scroll-Select: ".to_string(),
                    "PgUp/PgDown, ↑/↓, Home/End, ←/→".to_string(),
                    false,
                    true,
                ),
            ],
            ActivePane::Preview => vec![
                ("Help: ".to_string(), "Esc+?".to_string(), false, false),
                (exit_label.to_string(), "Ctrl+d".to_string(), false, false),
                (" | Config: ".to_string(), "Esc+c".to_string(), true, false),
                (" | Shell: ".to_string(), "Esc+0".to_string(), false, false),
                (
                    " | Navigation: ".to_string(),
                    "Esc+1".to_string(),
                    false,
                    false,
                ),
                (
                    " | Preview: ".to_string(),
                    "Esc+2".to_string(),
                    false,
                    false,
                ),
                (
                    " | Expand view: ".to_string(),
                    "Esc+f".to_string(),
                    false,
                    false,
                ),
                (
                    " | Scroll: ".to_string(),
                    "PgUp/PgDown, ↑/↓".to_string(),
                    false,
                    true,
                ),
            ],
        }
    };
    let mut spans = Vec::new();
    for (label, key, is_config_segment, is_panel_segment) in segments {
        let segment_label_style = if is_config_segment && highlight_config_shortcut {
            config_alert_style
        } else {
            label_style
        };
        let segment_key_style = if is_config_segment && highlight_config_shortcut {
            config_alert_style
        } else if is_panel_segment {
            panel_key_style
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
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
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

pub(crate) fn nav_meta_line_for_width(meta: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let normalized = normalize_nav_meta_spacing(meta);
    if normalized.chars().count() <= width {
        return normalized;
    }

    let without_owner_group = nav_meta_without_owner_group(&normalized);
    if without_owner_group.chars().count() <= width {
        return without_owner_group;
    }

    truncate_to_width(&without_owner_group, width)
}

fn normalize_nav_meta_spacing(meta: &str) -> String {
    meta.split_whitespace().collect::<Vec<&str>>().join(" ")
}

fn nav_meta_without_owner_group(meta: &str) -> String {
    let tokens: Vec<&str> = meta.split_whitespace().collect();
    if tokens.len() < 6 {
        return meta.to_string();
    }

    let size_idx = if tokens.len() >= 8 {
        tokens.len().saturating_sub(4)
    } else {
        tokens.len().saturating_sub(2)
    };
    if size_idx < 4 || size_idx + 1 >= tokens.len() {
        return meta.to_string();
    }

    let when = tokens[size_idx + 1..].join(" ");
    format!("{} {} {} {}", tokens[0], tokens[1], tokens[size_idx], when)
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
            return preview_directory_panel_text(
                path,
                hovered_label,
                depth,
                colors,
                fullish_shell_theme,
            );
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

pub(crate) fn preview_jump_panel_text(
    input: &str,
    completion_query: &str,
    user_typed: bool,
    history: &[String],
    history_index: Option<usize>,
    completions: &[String],
    completion_index: Option<usize>,
    status: Option<&str>,
    colors: &LsColorsTheme,
    panel_width: u16,
    panel_height: u16,
    fullish_shell_theme: bool,
) -> Text<'static> {
    let base_style = nav_style_for_theme(Style::default().fg(Color::White), fullish_shell_theme);
    let placeholder_style =
        nav_style_for_theme(Style::default().fg(Color::DarkGray), fullish_shell_theme);
    let highlight_style = border_style(true, fullish_shell_theme);
    let input_border_style = border_style(true, fullish_shell_theme);
    let file_icon_style = nav_style_for_theme(
        navigation_name_style(colors, "preview.txt", false, false, 0o644),
        fullish_shell_theme,
    );
    let folder_icon_style = nav_style_for_theme(
        navigation_name_style(colors, "preview", true, false, 0o755),
        fullish_shell_theme,
    );
    let placeholder = if history.is_empty() {
        "...reletive/abselut path of  or  to jump.".to_string()
    } else {
        "...reletive/abselut path of  or  to jump. Or history ↑/↓".to_string()
    };
    let show_completions = user_typed && !input.is_empty();
    let inner_width = panel_width.saturating_sub(2) as usize;
    let prompt = "❯ ";
    let available_input_width = inner_width.saturating_sub(prompt.chars().count());
    let input_display = clamp_display_tail(input, available_input_width);
    let placeholder_display = clamp_display_head(&placeholder, available_input_width);
    let input_text = if input.is_empty() {
        placeholder_display
    } else {
        input_display
    };
    let input_text = pad_to_width(&input_text, available_input_width);
    let border_line = if inner_width == 0 {
        String::new()
    } else {
        "─".repeat(inner_width)
    };
    let mut lines = Vec::new();
    lines.push(Line::default());
    lines.push(Line::from(vec![
        Span::styled("Select ".to_string(), base_style),
        Span::styled("".to_string(), file_icon_style),
        Span::styled(" or ".to_string(), base_style),
        Span::styled("".to_string(), folder_icon_style),
        Span::styled(
            "  in Navigation panel to Preview it. Or type...".to_string(),
            base_style,
        ),
    ]));
    lines.push(Line::from(Span::styled(
        format!("╭{border_line}╮"),
        input_border_style,
    )));
    lines.push(Line::from(vec![
        Span::styled("│".to_string(), input_border_style),
        Span::styled(prompt.to_string(), base_style),
        Span::styled(
            input_text,
            if input.is_empty() {
                placeholder_style
            } else {
                highlight_style
            },
        ),
        Span::styled("│".to_string(), input_border_style),
    ]));
    lines.push(Line::from(Span::styled(
        format!("╰{border_line}╯"),
        input_border_style,
    )));

    let reserved_status_rows = usize::from(status.is_some());
    let visible_rows = (panel_height as usize)
        .saturating_sub(lines.len())
        .saturating_sub(reserved_status_rows);
    if show_completions {
        let rows_for_entries = if visible_rows == 0 {
            0
        } else {
            visible_rows.saturating_sub(1)
        };
        let completion_entries = completions
            .iter()
            .enumerate()
            .map(|(idx, candidate)| CompletionEntry {
                label: completion_display_label(completion_query, candidate),
                style: completion_style_for_candidate(candidate, colors, fullish_shell_theme),
                selected: completion_index == Some(idx),
            })
            .collect::<Vec<CompletionEntry>>();
        let (packed_rows, rendered_count) = compact_completion_rows(
            &completion_entries,
            panel_width as usize,
            rows_for_entries,
            2,
        );
        for row in packed_rows {
            lines.push(row);
        }
        let hidden_count = completion_entries.len().saturating_sub(rendered_count);
        if hidden_count > 0 && visible_rows > 0 {
            lines.push(Line::from(Span::styled(
                format!("... +{hidden_count} more"),
                placeholder_style,
            )));
        }
    } else {
        let wheel_len = history.len().saturating_add(1);
        if let Some(selected_idx) = history_index {
            let selected_idx = selected_idx.min(history.len());
            let max_unique_rows = wheel_len.saturating_sub(1);
            let rows_to_render = visible_rows.min(max_unique_rows);
            for step in 1..=rows_to_render {
                let idx = (selected_idx + step) % wheel_len;
                let style = base_style;
                lines.push(history_wheel_line(
                    history,
                    idx,
                    prompt.chars().count().saturating_add(1),
                    style,
                ));
            }
        } else {
            let rows_to_render = visible_rows.min(history.len());
            for idx in 1..=rows_to_render {
                let style = base_style;
                lines.push(history_wheel_line(
                    history,
                    idx,
                    prompt.chars().count().saturating_add(1),
                    style,
                ));
            }
        }
    }

    if let Some(status_line) = status {
        lines.push(Line::from(Span::styled(
            status_line.to_string(),
            nav_style_for_theme(Style::default().fg(Color::Red), fullish_shell_theme),
        )));
    }
    Text::from(lines)
}

pub(crate) fn preview_jump_cursor_position(input: &str, panel_width: u16) -> Option<(u16, u16)> {
    if panel_width < 4 {
        return None;
    }
    let inner_width = panel_width.saturating_sub(2) as usize;
    let prompt_len = "❯ ".chars().count();
    if prompt_len >= inner_width {
        return None;
    }
    let max_input_chars = inner_width.saturating_sub(prompt_len);
    let input_len = input.chars().count().min(max_input_chars);
    let cursor_x = 1usize.saturating_add(prompt_len).saturating_add(input_len);
    Some((3, cursor_x as u16))
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
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
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

    let connector_style =
        nav_style_for_theme(Style::default().fg(Color::DarkGray), fullish_shell_theme);
    let perms_style = nav_style_for_theme(Style::default().fg(Color::White), fullish_shell_theme);

    let entries = match preview_directory_entries(path) {
        Ok(entries) => entries,
        Err(err) => {
            lines.push(Line::from(Span::styled(
                format!("{prefix}└── error: {err}"),
                nav_style_for_theme(Style::default().fg(Color::Red), fullish_shell_theme),
            )));
            return;
        },
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
    let lines = preview_file_command_entries(entry, config, editor_program, identity);
    let key_style = border_style(true, fullish_shell_theme);
    let text_style = nav_style_for_theme(Style::default().fg(Color::White), fullish_shell_theme);
    let muted_style =
        nav_style_for_theme(Style::default().fg(Color::DarkGray), fullish_shell_theme);
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
        .map(|(key, command, _enabled)| {
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
    for (key, command, enabled) in lines {
        let key_label = format!("Esc+{key}");
        let segment_key_style = if enabled { key_style } else { muted_style };
        let segment_text_style = if enabled { text_style } else { muted_style };
        if compact_commands_mode {
            out.push(Line::from(vec![
                Span::raw(" ".repeat(block_left_pad)),
                Span::styled(key_label, segment_key_style),
            ]));
            continue;
        }
        out.push(Line::from(vec![
            Span::raw(" ".repeat(block_left_pad)),
            Span::styled(key_label, segment_key_style),
            Span::styled(" : ".to_string(), segment_text_style),
            Span::styled(command, segment_text_style),
        ]));
    }
    Text::from(out)
}

fn centered_left_padding(content_width: usize, total_width: usize) -> usize {
    total_width.saturating_sub(content_width) / 2
}

fn clamp_display_tail(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let char_count = input.chars().count();
    if char_count <= max_chars {
        return input.to_string();
    }
    input
        .chars()
        .skip(char_count.saturating_sub(max_chars))
        .collect()
}

fn clamp_display_head(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    input.chars().take(max_chars).collect()
}

fn pad_to_width(input: &str, width: usize) -> String {
    let char_count = input.chars().count();
    if char_count >= width {
        return input.chars().take(width).collect();
    }
    format!("{input}{}", " ".repeat(width.saturating_sub(char_count)))
}

fn history_wheel_line(
    history: &[String],
    index: usize,
    input_alignment_padding: usize,
    style: Style,
) -> Line<'static> {
    let entry = if index == 0 {
        "...".to_string()
    } else {
        history
            .get(index.saturating_sub(1))
            .cloned()
            .unwrap_or_default()
    };
    Line::from(vec![
        Span::styled(" ".repeat(input_alignment_padding), style),
        Span::styled(entry, style),
    ])
}

fn completion_display_label(query: &str, candidate: &str) -> String {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return candidate.to_string();
    }
    let query_path = Path::new(trimmed);
    let display_prefix = if trimmed.ends_with('/') {
        trimmed.to_string()
    } else if trimmed.starts_with("~/") {
        let parent = query_path.parent().unwrap_or(Path::new(""));
        let parent_text = parent.to_string_lossy();
        if parent_text == "~" || parent_text == "~/" || parent_text.is_empty() {
            "~/".to_string()
        } else {
            format!("{parent_text}/")
        }
    } else if query_path.is_absolute() {
        let parent = query_path.parent().unwrap_or(Path::new(""));
        let parent_text = parent.to_string_lossy();
        if parent_text.is_empty() || parent_text == "/" {
            "/".to_string()
        } else {
            format!("{parent_text}/")
        }
    } else {
        let parent = query_path.parent().unwrap_or(Path::new(""));
        let parent_text = parent.to_string_lossy();
        if parent_text.is_empty() {
            String::new()
        } else {
            format!("{parent_text}/")
        }
    };
    if display_prefix.is_empty() {
        return candidate.to_string();
    }
    candidate
        .strip_prefix(&display_prefix)
        .filter(|suffix| !suffix.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| candidate.to_string())
}

#[derive(Clone)]
struct CompletionEntry {
    label: String,
    style: Style,
    selected: bool,
}

fn completion_style_for_candidate(
    candidate: &str,
    colors: &LsColorsTheme,
    fullish_shell_theme: bool,
) -> Style {
    let is_dir = candidate.ends_with('/');
    let trimmed = candidate.trim_end_matches('/');
    let name = trimmed
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or(trimmed);
    let fallback_name = if name.is_empty() {
        if is_dir { "folder" } else { "file" }
    } else {
        name
    };
    let mode = if is_dir { 0o755 } else { 0o644 };
    nav_style_for_theme(
        navigation_name_style(colors, fallback_name, is_dir, false, mode),
        fullish_shell_theme,
    )
}

fn compact_completion_rows(
    entries: &[CompletionEntry],
    total_width: usize,
    max_rows: usize,
    gap_width: usize,
) -> (Vec<Line<'static>>, usize) {
    if entries.is_empty() || total_width == 0 || max_rows == 0 {
        return (Vec::new(), 0);
    }
    let max_label_width = entries
        .iter()
        .map(|entry| entry.label.chars().count())
        .max()
        .unwrap_or(1);
    let col_width = max_label_width.saturating_add(gap_width).max(1);
    let columns = (total_width / col_width).max(1);
    let total_rows = entries.len().div_ceil(columns);
    let rows_to_render = total_rows.min(max_rows);
    let mut rows = Vec::with_capacity(rows_to_render);
    let mut rendered = 0usize;
    for row in 0..rows_to_render {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for col in 0..columns {
            let idx = row.saturating_mul(columns).saturating_add(col);
            if idx >= entries.len() {
                break;
            }
            rendered = rendered.saturating_add(1);
            let entry = &entries[idx];
            let is_last_column = col + 1 == columns || idx + 1 >= entries.len();
            let selected_style = nav_row_selected_style(entry.style, entry.selected);
            if is_last_column {
                spans.push(Span::styled(entry.label.clone(), selected_style));
            } else {
                let label_len = entry.label.chars().count();
                if entry.selected {
                    spans.push(Span::styled(entry.label.clone(), selected_style));
                    let padding = col_width.saturating_sub(label_len);
                    if padding > 0 {
                        spans.push(Span::styled(" ".repeat(padding), entry.style));
                    }
                } else {
                    spans.push(Span::styled(
                        pad_to_width(&entry.label, col_width),
                        entry.style,
                    ));
                }
            }
        }
        rows.push(Line::from(spans));
    }
    (rows, rendered)
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
