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

#[derive(Debug, Clone, Copy)]
pub(crate) struct OutputDrain {
    pub(crate) processed_chunks: usize,
    pub(crate) hit_limit: bool,
}

pub(crate) fn merge_output_drains(primary: OutputDrain, secondary: OutputDrain) -> OutputDrain {
    OutputDrain {
        processed_chunks: primary
            .processed_chunks
            .saturating_add(secondary.processed_chunks),
        hit_limit: primary.hit_limit || secondary.hit_limit,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivePane {
    Shell,
    Navigation,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PanePoint {
    pub(crate) column: u16,
    pub(crate) row: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PanelSelection {
    pub(crate) pane: ActivePane,
    pub(crate) start: PanePoint,
    pub(crate) end: PanePoint,
    pub(crate) dragged: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct RenderTextSnapshot {
    pub(crate) nav_inner: Rect,
    pub(crate) preview_inner: Rect,
    pub(crate) shell_inner: Rect,
    pub(crate) nav_lines: Vec<String>,
    pub(crate) preview_lines: Vec<String>,
    pub(crate) shell_lines: Vec<String>,
}

impl RenderTextSnapshot {
    pub(crate) fn empty() -> Self {
        Self {
            nav_inner: Rect::default(),
            preview_inner: Rect::default(),
            shell_inner: Rect::default(),
            nav_lines: Vec::new(),
            preview_lines: Vec::new(),
            shell_lines: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviewMode {
    Empty,
    DirectoryTree,
    FileText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NavigationFileCommandAction {
    RunReadInPreview(String),
    RunWriteInPreview(String),
    PrefillShell(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviewCommandMode {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviewOverlayPresentation {
    StaticFullscreen,
    InteractiveFullscreenDim,
}

#[derive(Debug, Clone)]
pub(crate) struct EffectiveIdentity {
    pub(crate) euid: u32,
    pub(crate) egid: u32,
    pub(crate) groups: HashSet<u32>,
}

impl EffectiveIdentity {
    pub(crate) fn current() -> Self {
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

    pub(crate) fn in_group(&self, gid: u32) -> bool {
        gid == self.egid || self.groups.contains(&gid)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EffectiveAccess {
    pub(crate) read: bool,
    pub(crate) write: bool,
    pub(crate) exec: bool,
}

pub(crate) struct App {
    pub(crate) active: ActivePane,
    pub(crate) shell: ShellPane,
    pub(crate) pending_alt: bool,
    pub(crate) pending_alt_shortcut_armed: bool,
    pub(crate) shell_fullish: bool,
    pub(crate) nav_fullish: bool,
    pub(crate) config_open: bool,
    pub(crate) config_state: ConfigState,
    pub(crate) saved_config_state: ConfigState,
    pub(crate) config_editor: ConfigEditor,
    pub(crate) nav_colors: LsColorsTheme,
    pub(crate) nav_cwd: PathBuf,
    pub(crate) nav_all_entries: Vec<NavEntry>,
    pub(crate) nav_entries: Vec<NavEntry>,
    pub(crate) nav_filter: String,
    pub(crate) nav_error: Option<String>,
    pub(crate) nav_loaded: bool,
    pub(crate) nav_selected: usize,
    pub(crate) nav_scroll: usize,
    pub(crate) nav_pending_file_shortcut: Option<char>,
    pub(crate) nav_viewport_rows: usize,
    pub(crate) nav_meta_cache_path: Option<PathBuf>,
    pub(crate) nav_meta_cache: String,
    pub(crate) preview_mode: PreviewMode,
    pub(crate) preview_depth: usize,
    pub(crate) preview_max_depth: usize,
    pub(crate) preview_dir_enabled: bool,
    pub(crate) preview_cached_text: String,
    pub(crate) preview_last_selected_path: Option<PathBuf>,
    pub(crate) preview_cached_depth: usize,
    pub(crate) preview_command_overlay_active: bool,
    pub(crate) preview_command_overlay_command: String,
    pub(crate) preview_command_overlay_mode: Option<PreviewCommandMode>,
    pub(crate) preview_command_overlay_presentation: Option<PreviewOverlayPresentation>,
    pub(crate) preview_command_shell: Option<ShellPane>,
    pub(crate) effective_identity: EffectiveIdentity,
    pub(crate) editor_program: String,
    pub(crate) config_shortcut_alert_until: Option<Instant>,
    pub(crate) key_debug_log: Option<File>,
    pub(crate) key_debug_seq: u64,
    pub(crate) force_terminal_clear: bool,
    pub(crate) mouse_selection: Option<PanelSelection>,
    pub(crate) render_snapshot: RenderTextSnapshot,
}
