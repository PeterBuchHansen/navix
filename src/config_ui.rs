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

pub(crate) fn normalize_extension(raw: &str) -> String {
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

pub(crate) fn duplicate_extension_for_rule(
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

pub(crate) fn extension_validation_error_for_rule(
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

pub(crate) fn first_empty_extension(config: &ConfigState) -> bool {
    config
        .extension_rules
        .iter()
        .any(|rule| extension_is_empty(&rule.extension))
}

pub(crate) fn first_duplicate_extension(config: &ConfigState) -> Option<String> {
    let mut seen = HashSet::new();
    for rule in &config.extension_rules {
        let normalized = normalize_extension(&rule.extension);
        if !seen.insert(normalized.clone()) {
            return Some(normalized);
        }
    }
    None
}

pub(crate) fn next_available_extension_name(config: &ConfigState, base: &str) -> String {
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

pub(crate) fn default_extension_rule(extension: &str) -> ExtensionCommandRule {
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

pub(crate) fn previous_char_boundary(text: &str, cursor: usize) -> usize {
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

pub(crate) fn next_char_boundary(text: &str, cursor: usize) -> usize {
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

pub(crate) fn config_field_value<'a>(rule: &'a ExtensionCommandRule, field: ConfigField) -> &'a str {
    match field {
        ConfigField::Extension => &rule.extension,
        ConfigField::Read => &rule.read_cmd,
        ConfigField::Write => &rule.write_cmd,
        ConfigField::Exec => &rule.exec_cmd,
    }
}

pub(crate) fn set_config_field(rule: &mut ExtensionCommandRule, field: ConfigField, value: &str) {
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

pub(crate) fn config_panel_text(
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
