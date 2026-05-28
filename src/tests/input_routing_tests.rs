use super::*;
use crossterm::event::ModifierKeyCode;

#[test]
fn escape_prefix_release_update_clears_pending_on_escape_release() {
    let (pending, armed, consumed) =
        escape_prefix_release_update(true, false, KeyCode::Esc, KeyEventKind::Release);
    assert!(!pending);
    assert!(!armed);
    assert!(consumed);
}

#[test]
fn escape_prefix_release_update_keeps_armed_shortcut_on_escape_release() {
    let (pending, armed, consumed) =
        escape_prefix_release_update(true, true, KeyCode::Esc, KeyEventKind::Release);
    assert!(pending);
    assert!(armed);
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
    assert_eq!(
        escape_prefix_shortcut_char(false, KeyCode::Char('c'), KeyModifiers::NONE),
        None
    );
    assert_eq!(
        escape_prefix_shortcut_char(false, KeyCode::Char('0'), KeyModifiers::NONE),
        None
    );
}

#[test]
fn escape_prefix_shortcut_char_accepts_only_supported_shortcuts() {
    assert_eq!(
        escape_prefix_shortcut_char(true, KeyCode::Char('0'), KeyModifiers::NONE),
        Some('0')
    );
    assert_eq!(
        escape_prefix_shortcut_char(true, KeyCode::Char('1'), KeyModifiers::NONE),
        Some('1')
    );
    assert_eq!(
        escape_prefix_shortcut_char(true, KeyCode::Char('2'), KeyModifiers::NONE),
        Some('2')
    );
    assert_eq!(
        escape_prefix_shortcut_char(true, KeyCode::Char('c'), KeyModifiers::NONE),
        Some('c')
    );
    assert_eq!(
        escape_prefix_shortcut_char(true, KeyCode::Char('C'), KeyModifiers::SHIFT),
        Some('c')
    );
    assert_eq!(
        escape_prefix_shortcut_char(true, KeyCode::Char('f'), KeyModifiers::NONE),
        Some('f')
    );
    assert_eq!(
        escape_prefix_shortcut_char(true, KeyCode::Char('r'), KeyModifiers::NONE),
        Some('r')
    );
    assert_eq!(
        escape_prefix_shortcut_char(true, KeyCode::Char('w'), KeyModifiers::NONE),
        Some('w')
    );
    assert_eq!(
        escape_prefix_shortcut_char(true, KeyCode::Char('x'), KeyModifiers::NONE),
        Some('x')
    );
    assert_eq!(
        escape_prefix_shortcut_char(true, KeyCode::Char('?'), KeyModifiers::SHIFT),
        Some('?')
    );
    assert_eq!(
        escape_prefix_shortcut_char(true, KeyCode::Char('/'), KeyModifiers::SHIFT),
        Some('?')
    );
    assert_eq!(
        escape_prefix_shortcut_char(true, KeyCode::Char('q'), KeyModifiers::NONE),
        None
    );
    assert_eq!(
        escape_prefix_shortcut_char(true, KeyCode::Up, KeyModifiers::NONE),
        None
    );
}

#[test]
fn escape_prefix_arm_shortcut_allows_shifted_escape() {
    assert!(escape_prefix_arm_shortcut(KeyCode::Esc, KeyModifiers::NONE,));
    assert!(escape_prefix_arm_shortcut(
        KeyCode::Esc,
        KeyModifiers::SHIFT,
    ));
    assert!(!escape_prefix_arm_shortcut(KeyCode::Esc, KeyModifiers::ALT,));
    assert!(!escape_prefix_arm_shortcut(
        KeyCode::Char('?'),
        KeyModifiers::SHIFT,
    ));
}

#[test]
fn navigation_file_shortcut_char_accepts_only_rw_x_keys() {
    assert_eq!(navigation_file_shortcut_char(KeyCode::Char('r')), Some('r'));
    assert_eq!(navigation_file_shortcut_char(KeyCode::Char('W')), Some('w'));
    assert_eq!(navigation_file_shortcut_char(KeyCode::Char('x')), Some('x'));
    assert_eq!(navigation_file_shortcut_char(KeyCode::Char('q')), None);
    assert_eq!(navigation_file_shortcut_char(KeyCode::Enter), None);
}

#[test]
fn navigation_filter_char_accepts_printable_ascii_except_slash() {
    assert_eq!(
        navigation_filter_char(KeyCode::Char('a'), KeyModifiers::NONE),
        Some('a')
    );
    assert_eq!(
        navigation_filter_char(KeyCode::Char('D'), KeyModifiers::SHIFT),
        Some('D')
    );
    assert_eq!(
        navigation_filter_char(KeyCode::Char('.'), KeyModifiers::NONE),
        Some('.')
    );
    assert_eq!(
        navigation_filter_char(KeyCode::Char('/'), KeyModifiers::NONE),
        None
    );
    assert_eq!(
        navigation_filter_char(KeyCode::Char(' '), KeyModifiers::NONE),
        None
    );
    assert_eq!(
        navigation_filter_char(KeyCode::Enter, KeyModifiers::NONE),
        None
    );
    assert_eq!(
        navigation_filter_char(KeyCode::Char('a'), KeyModifiers::CONTROL),
        None
    );
}

#[test]
fn navigation_clear_filter_shortcut_requires_ctrl_backspace_or_delete() {
    assert!(navigation_clear_filter_shortcut(
        KeyCode::Backspace,
        KeyModifiers::CONTROL,
    ));
    assert!(navigation_clear_filter_shortcut(
        KeyCode::Delete,
        KeyModifiers::CONTROL,
    ));
    assert!(!navigation_clear_filter_shortcut(
        KeyCode::Backspace,
        KeyModifiers::NONE,
    ));
    assert!(!navigation_clear_filter_shortcut(
        KeyCode::Delete,
        KeyModifiers::SHIFT,
    ));
    assert!(!navigation_clear_filter_shortcut(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL,
    ));
}

#[test]
fn copy_selection_shortcut_accepts_ctrl_shift_c_not_plain_ctrl_c() {
    assert!(copy_selection_shortcut(
        KeyCode::Char('C'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        false,
    ));
    assert!(copy_selection_shortcut(
        KeyCode::Char('C'),
        KeyModifiers::CONTROL,
        false,
    ));
    assert!(!copy_selection_shortcut(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
        false,
    ));
}

#[test]
fn copy_selection_shortcut_accepts_super_c_when_command_mode_enabled() {
    assert!(copy_selection_shortcut(
        KeyCode::Char('c'),
        KeyModifiers::SUPER,
        true,
    ));
    assert!(!copy_selection_shortcut(
        KeyCode::Char('c'),
        KeyModifiers::SUPER,
        false,
    ));
}

#[test]
fn should_clear_mouse_selection_for_key_keeps_selection_on_modifier_press() {
    assert!(!should_clear_mouse_selection_for_key(
        KeyCode::Modifier(ModifierKeyCode::LeftShift),
        false,
        false,
    ));
    assert!(should_clear_mouse_selection_for_key(
        KeyCode::Char('q'),
        false,
        false,
    ));
    assert!(!should_clear_mouse_selection_for_key(
        KeyCode::Char('C'),
        true,
        false,
    ));
}

#[test]
fn terminal_prefers_command_copy_from_env_detects_mac_hints() {
    assert!(terminal_prefers_command_copy_from_env(
        Some("Apple_Terminal"),
        None,
    ));
    assert!(terminal_prefers_command_copy_from_env(
        Some("xterm"),
        Some("darwin24"),
    ));
}

#[test]
fn terminal_prefers_command_copy_from_env_defaults_off_without_mac_hints() {
    if cfg!(target_os = "macos") {
        assert!(terminal_prefers_command_copy_from_env(None, None));
    } else {
        assert!(!terminal_prefers_command_copy_from_env(None, None));
    }
}

#[test]
fn navigation_enter_file_shortcut_defaults_to_read() {
    assert_eq!(navigation_enter_file_shortcut(None), 'r');
    assert_eq!(navigation_enter_file_shortcut(Some('w')), 'w');
}

#[test]
fn navigation_pending_shortcut_to_filter_char_on_release_only_matches_same_char() {
    assert_eq!(
        navigation_pending_shortcut_to_filter_char_on_release(
            Some('r'),
            KeyCode::Char('r'),
            KeyEventKind::Release,
        ),
        Some('r')
    );
    assert_eq!(
        navigation_pending_shortcut_to_filter_char_on_release(
            Some('r'),
            KeyCode::Char('w'),
            KeyEventKind::Release,
        ),
        None
    );
    assert_eq!(
        navigation_pending_shortcut_to_filter_char_on_release(
            Some('r'),
            KeyCode::Char('r'),
            KeyEventKind::Press,
        ),
        None
    );
}

#[test]
fn navigation_should_ignore_pending_shortcut_event_blocks_repeat_and_held_key() {
    assert!(navigation_should_ignore_pending_shortcut_event(
        Some('r'),
        KeyCode::Char('r'),
        KeyEventKind::Press,
    ));
    assert!(navigation_should_ignore_pending_shortcut_event(
        Some('r'),
        KeyCode::Char('x'),
        KeyEventKind::Repeat,
    ));
    assert!(!navigation_should_ignore_pending_shortcut_event(
        Some('r'),
        KeyCode::Char('x'),
        KeyEventKind::Press,
    ));
    assert!(!navigation_should_ignore_pending_shortcut_event(
        None,
        KeyCode::Char('r'),
        KeyEventKind::Press,
    ));
}

#[test]
fn navigation_backspace_parent_ready_after_pop_requires_new_press_when_filter_empties() {
    assert!(!navigation_backspace_parent_ready_after_pop(true, true));
    assert!(!navigation_backspace_parent_ready_after_pop(true, false));
    assert!(navigation_backspace_parent_ready_after_pop(false, true));
}

#[test]
fn preview_shortcut_targets_preview_for_directory_tree() {
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
    assert_eq!(target, ActivePane::Preview);
}

#[test]
fn preview_shortcut_targets_preview_for_file_command_list() {
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
        ActivePane::Preview
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
fn panel_click_focus_target_keeps_preview_when_clicked() {
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
        ActivePane::Preview
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
fn mouse_event_relative_to_panel_translates_coordinates_when_inside() {
    let panel = Rect {
        x: 30,
        y: 10,
        width: 40,
        height: 12,
    };
    let event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 33,
        row: 12,
        modifiers: KeyModifiers::SHIFT,
    };
    let translated = mouse_event_relative_to_panel(event, panel).expect("inside panel");
    assert_eq!(translated.column, 3);
    assert_eq!(translated.row, 2);
    assert_eq!(translated.modifiers, KeyModifiers::SHIFT);
}

#[test]
fn mouse_event_relative_to_panel_ignores_events_outside_panel() {
    let panel = Rect {
        x: 30,
        y: 10,
        width: 40,
        height: 12,
    };
    let event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: 2,
        modifiers: KeyModifiers::NONE,
    };
    assert!(mouse_event_relative_to_panel(event, panel).is_none());
}

#[test]
fn panel_areas_for_focus_click_matches_fullish_preview_constraints() {
    let terminal = Rect {
        x: 0,
        y: 0,
        width: 120,
        height: 40,
    };
    let (_, preview_nav_fullish, _) =
        panel_areas_for_focus_click(terminal, ActivePane::Navigation, false, false, true, false);
    assert_eq!(preview_nav_fullish.width, 12);
    let (_, preview_preview_fullish, _) =
        panel_areas_for_focus_click(terminal, ActivePane::Preview, false, false, true, false);
    assert_eq!(preview_preview_fullish.width, 12);

    let (nav_preview_overlay, _, _) =
        panel_areas_for_focus_click(terminal, ActivePane::Navigation, false, false, false, true);
    assert_eq!(nav_preview_overlay.width, 12);
}

#[test]
fn shell_pending_input_after_key_marks_history_and_plain_typing_as_pending() {
    assert!(shell_pending_input_after_key(
        false,
        KeyCode::Up,
        KeyModifiers::NONE,
    ));
    assert!(shell_pending_input_after_key(
        false,
        KeyCode::Char('l'),
        KeyModifiers::NONE,
    ));
}

#[test]
fn shell_pending_input_after_key_clears_pending_on_enter_ctrl_c_and_ctrl_u() {
    assert!(!shell_pending_input_after_key(
        true,
        KeyCode::Enter,
        KeyModifiers::NONE,
    ));
    assert!(!shell_pending_input_after_key(
        true,
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));
    assert!(!shell_pending_input_after_key(
        true,
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
    ));
}
