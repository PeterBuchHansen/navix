use super::*;

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
    assert!(shortcuts.contains("Home/End"));
    assert!(shortcuts.contains("←/→"));
    assert!(shortcuts.contains("Exit: Ctrl+d"));
    assert!(!shortcuts.contains("Ctrl+d/q"));
}

#[test]
fn footer_shortcuts_for_preview_keep_basic_scroll_shortcuts() {
    let shortcuts = footer_shortcuts(ActivePane::Preview, false, false);
    assert!(shortcuts.contains("PgUp/PgDown"));
    assert!(shortcuts.contains("↑/↓"));
    assert!(!shortcuts.contains("Home/End"));
    assert!(!shortcuts.contains("←/→"));
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
fn footer_shortcuts_in_fullish_mode_use_darker_keys() {
    let expected = Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::BOLD);
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
fn nav_meta_compact_lines_split_size_and_time_to_bottom_line() {
    let meta = "drwxr-xr-x  17 637812460 637800513  4096 May 25 21:41";
    let (top, bottom) = nav_meta_compact_lines(meta, 58);
    assert_eq!(top.chars().count(), 58);
    assert_eq!(bottom.chars().count(), 58);
    assert!(top.starts_with("──drwxr-xr-x 17 637812460 637800513"));
    assert!(top.contains("────────"));
    assert!(bottom.starts_with("──"));
    assert!(bottom.contains("────────"));
    assert!(bottom.ends_with("4096 May 25 21:41──"));
}

#[test]
fn nav_meta_compact_lines_handles_small_width_by_prioritizing_tail() {
    let meta = "drwxr-xr-x  17 user group  4096 May 25 21:41";
    let (_top, bottom) = nav_meta_compact_lines(meta, 18);
    assert_eq!(bottom.chars().count(), 18);
    assert!(bottom.contains("May 25 21:41"));
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
    let text = preview_directory_panel_text(&base.join("docs"), Some("docs"), 1, &colors, false);
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
