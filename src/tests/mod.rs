use super::{
    alt_screen_event_from_stream, apply_alt_screen_chunk, available_preview_file_commands,
    bash_history_sync_prompt_command, border_style, clamp_nav_selection, clamp_preview_depth,
    compose_footer_line, config_panel_text, copy_selection_shortcut, default_extension_rule,
    default_history_file_for_shell, duplicate_extension_for_rule, effective_access_from_mode,
    escape_prefix_release_update,
    escape_prefix_shortcut_char, extension_validation_error_for_rule, first_duplicate_extension,
    first_empty_extension, footer_meta, footer_meta_line, footer_shortcuts, footer_shortcuts_line,
    child_mouse_capture_required, is_fullish_layout_state, is_fullish_shell_mode,
    kernel_effective_access_for_path, nav_long_listing, nav_filter_matches, nav_max_scroll,
    nav_meta_compact_lines, nav_row_selected_style, nav_scroll_for_selection, nav_selection_after_filter,
    nav_select_end, nav_select_home, nav_select_next_wrap, nav_select_prev_wrap, nav_style_for_theme,
    navigation_clear_filter_shortcut, navigation_enter_file_shortcut, navigation_file_command_action,
    navigation_file_shortcut_char, navigation_filter_char,
    mouse_event_relative_to_panel,
    navigation_name_style, navigation_tree_lines, next_available_extension_name,
    next_preview_overlay_presentation, normalize_extension,
    pane_from_mouse_position, panel_areas_for_focus_click, panel_click_focus_target,
    parse_scrollback_limit, permission_bits, poll_timeout_for_drain, prefill_shell_input_bytes,
    preview_content_for_selected_entry, preview_directory_panel_text, preview_directory_tree_lines,
    preview_file_commands_panel_text, preview_file_preview_text, preview_overlay_is_interactive,
    preview_shortcut_target, resolve_preview_command_template, resolve_scrollback_limit, sgr_to_style,
    set_config_field, shell_output_burst_update, shell_panel_height, shell_program_name,
    shell_single_quote, should_auto_close_preview_overlay, should_show_scrollbar,
    should_throttle_mouse_passthrough_redraw,
    terminal_prefers_command_copy_from_env,
    should_use_fullish_theme, simple_permission_bits, window_bounds,
    scrollbar_thumb_bounds, terminal_key_bytes, terminal_mouse_bytes, visible_range, ActivePane,
    ConfigEditor, ConfigField, ConfigState, EffectiveIdentity, ExtensionCommandRule, LsColorsTheme, NavEntry,
    NavigationFileCommandAction, OutputDrain, PreviewMode, PreviewOverlayPresentation,
};
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use vt100::Parser;

mod alt_screen_tests;
mod config_tests;
mod file_logic_tests;
mod input_routing_tests;
mod navigation_tests;
mod runtime_helpers_tests;
mod shell_tests;
mod terminal_keys_tests;
mod theme_tests;
mod tui_tests;

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
