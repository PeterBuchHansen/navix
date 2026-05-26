use super::*;

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
            line.spans.iter().any(|span| span.content.as_ref() == "❯ ")
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
            line.spans.iter().any(|span| span.content.as_ref() == ".md")
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
    assert!(write_line.spans.iter().any(|span| {
        span.content.as_ref() == "$EDITOR {file}" && span.style.fg == Some(Color::Red)
    }));
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
            line.spans.iter().any(|span| span.content.as_ref() == ".toml")
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
    assert!(write_line.spans.iter().any(|span| {
        span.content.as_ref() == "$EDITOR {file}" && span.style.fg == Some(Color::Green)
    }));
    let exec_line = &text.lines[added_idx + 3];
    assert!(exec_line.spans.iter().any(|span| {
        span.content.as_ref() == "taplo fmt {file}" && span.style.fg == Some(Color::Green)
    }));
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
            line.spans.iter().any(|span| span.content.as_ref() == ".md")
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
        read_line.spans.iter().any(|span| {
            span.content.as_ref() == "bat {file}" && span.style.fg == Some(Color::LightBlue)
        })
    );
    let write_line = &text.lines[md_idx + 2];
    assert!(write_line.spans.iter().any(|span| {
        span.content.as_ref() == "$EDITOR {file}" && span.style.fg == Some(Color::LightBlue)
    }));
    let exec_line = &text.lines[md_idx + 3];
    assert!(exec_line.spans.iter().any(|span| {
        span.content.as_ref() == "- mdterm {file}" && span.style.fg == Some(Color::Red)
    }));
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
