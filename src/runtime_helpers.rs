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

#[cfg(test)]
pub(crate) fn window_bounds(content_end: usize, viewport: usize, scroll_offset: usize) -> (usize, usize, usize) {
    let viewport = viewport.max(1);
    let content_end = content_end.max(1).max(viewport);
    let max_offset = content_end.saturating_sub(viewport);
    let clamped_offset = scroll_offset.min(max_offset);
    let end = content_end.saturating_sub(clamped_offset);
    let start = end.saturating_sub(viewport);
    (start, end, max_offset)
}

pub(crate) fn should_show_scrollbar(total: usize, viewport: usize) -> bool {
    total > viewport.max(1)
}

pub(crate) fn visible_range(total: usize, viewport: usize, scroll_offset: usize) -> (usize, usize) {
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

pub(crate) fn shell_panel_height(total_main_height: u16, active: ActivePane, shell_fullish: bool) -> u16 {
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

pub(crate) fn is_fullish_shell_mode(active: ActivePane, shell_fullish: bool) -> bool {
    active == ActivePane::Shell && shell_fullish
}

pub(crate) fn prefill_shell_input_bytes(command: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(command.len().saturating_add(2));
    // Ctrl+a then Ctrl+k clears current prompt input without executing it.
    out.extend_from_slice(&[0x01, 0x0b]);
    out.extend_from_slice(command.as_bytes());
    out
}

pub(crate) fn is_fullish_layout_state(
    active: ActivePane,
    shell_fullish_toggle: bool,
    shell_alt_screen_active: bool,
    nav_fullish: bool,
    preview_overlay_active: bool,
) -> bool {
    let shell_fullish_mode = is_fullish_shell_mode(active, shell_fullish_toggle || shell_alt_screen_active);
    let nav_fullish_mode = matches!(active, ActivePane::Navigation | ActivePane::Preview) && nav_fullish;
    shell_fullish_mode || nav_fullish_mode || preview_overlay_active
}

pub(crate) fn preview_overlay_is_interactive(
    presentation: Option<PreviewOverlayPresentation>,
) -> bool {
    presentation == Some(PreviewOverlayPresentation::InteractiveFullscreenDim)
}

pub(crate) fn next_preview_overlay_presentation(
    current: Option<PreviewOverlayPresentation>,
    alt_screen_active: bool,
) -> Option<PreviewOverlayPresentation> {
    if alt_screen_active {
        return Some(PreviewOverlayPresentation::InteractiveFullscreenDim);
    }
    current
}

pub(crate) fn should_auto_close_preview_overlay(
    presentation: Option<PreviewOverlayPresentation>,
    session_running: bool,
) -> bool {
    preview_overlay_is_interactive(presentation) && !session_running
}

pub(crate) fn poll_timeout_for_drain(drain: OutputDrain) -> Duration {
    if drain.hit_limit {
        Duration::from_millis(0)
    } else if drain.processed_chunks > 0 {
        Duration::from_millis(5)
    } else {
        Duration::from_millis(80)
    }
}

pub(crate) fn shell_output_burst_update(
    burst_active: bool,
    processed_chunks: usize,
    alt_screen_active: bool,
) -> (bool, bool) {
    if processed_chunks > 0 {
        return (true, false);
    }
    if burst_active && !alt_screen_active {
        return (false, true);
    }
    (burst_active, false)
}

pub(crate) fn child_mouse_capture_required(
    active: ActivePane,
    shell_alt_screen_active: bool,
    preview_overlay_active: bool,
    preview_presentation: Option<PreviewOverlayPresentation>,
) -> bool {
    (active == ActivePane::Shell && shell_alt_screen_active)
        || (active == ActivePane::Preview
            && preview_overlay_active
            && preview_overlay_is_interactive(preview_presentation))
}

pub(crate) fn should_throttle_mouse_passthrough_redraw(
    passthrough_active: bool,
    was_passthrough_active: bool,
    drain: OutputDrain,
    force_terminal_clear: bool,
    force_draw_next_iteration: bool,
) -> bool {
    passthrough_active
        && was_passthrough_active
        && drain.processed_chunks == 0
        && !drain.hit_limit
        && !force_terminal_clear
        && !force_draw_next_iteration
}

pub(crate) fn scrollbar_thumb_bounds(
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
