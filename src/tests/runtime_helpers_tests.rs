use super::*;

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
fn shell_output_burst_update_arms_on_output_and_refreshes_when_quiet() {
    let (active_after_output, refresh_after_output) = shell_output_burst_update(false, 2, false);
    assert!(active_after_output);
    assert!(!refresh_after_output);

    let (active_after_quiet, refresh_after_quiet) = shell_output_burst_update(active_after_output, 0, false);
    assert!(!active_after_quiet);
    assert!(refresh_after_quiet);
}

#[test]
fn shell_output_burst_update_waits_for_alt_screen_to_end_before_refresh() {
    let (active_after_output, refresh_after_output) = shell_output_burst_update(false, 1, true);
    assert!(active_after_output);
    assert!(!refresh_after_output);

    let (active_while_alt_still_on, refresh_while_alt_still_on) =
        shell_output_burst_update(active_after_output, 0, true);
    assert!(active_while_alt_still_on);
    assert!(!refresh_while_alt_still_on);

    let (active_after_alt_off, refresh_after_alt_off) =
        shell_output_burst_update(active_while_alt_still_on, 0, false);
    assert!(!active_after_alt_off);
    assert!(refresh_after_alt_off);
}

#[test]
fn child_mouse_capture_required_is_true_for_interactive_child_sessions() {
    assert!(child_mouse_capture_required(
        ActivePane::Shell,
        true,
        false,
        None,
    ));
    assert!(child_mouse_capture_required(
        ActivePane::Preview,
        false,
        true,
        Some(PreviewOverlayPresentation::InteractiveFullscreenDim),
    ));
    assert!(!child_mouse_capture_required(
        ActivePane::Navigation,
        false,
        false,
        None,
    ));
    assert!(!child_mouse_capture_required(
        ActivePane::Preview,
        false,
        true,
        Some(PreviewOverlayPresentation::StaticFullscreen),
    ));
}

#[test]
fn should_throttle_static_preview_redraw_only_when_passthrough_is_stable_and_idle() {
    assert!(should_throttle_mouse_passthrough_redraw(
        true,
        true,
        OutputDrain {
            processed_chunks: 0,
            hit_limit: false,
        },
        false,
        false,
    ));
    assert!(!should_throttle_mouse_passthrough_redraw(
        true,
        false,
        OutputDrain {
            processed_chunks: 0,
            hit_limit: false,
        },
        false,
        false,
    ));
    assert!(!should_throttle_mouse_passthrough_redraw(
        true,
        true,
        OutputDrain {
            processed_chunks: 1,
            hit_limit: false,
        },
        false,
        false,
    ));
    assert!(!should_throttle_mouse_passthrough_redraw(
        true,
        true,
        OutputDrain {
            processed_chunks: 0,
            hit_limit: true,
        },
        false,
        false,
    ));
    assert!(!should_throttle_mouse_passthrough_redraw(
        true,
        true,
        OutputDrain {
            processed_chunks: 0,
            hit_limit: false,
        },
        true,
        false,
    ));
    assert!(!should_throttle_mouse_passthrough_redraw(
        true,
        true,
        OutputDrain {
            processed_chunks: 0,
            hit_limit: false,
        },
        false,
        true,
    ));
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
    assert!(is_fullish_layout_state(
        ActivePane::Preview,
        false,
        false,
        true,
        false
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
fn prefill_shell_input_bytes_clears_line_before_prefill() {
    let bytes = prefill_shell_input_bytes("bash bootstrap.sh");
    assert_eq!(&bytes[..2], &[0x01, 0x0b]);
    assert_eq!(&bytes[2..], b"bash bootstrap.sh");
}
