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

impl App {
    fn apply_navigation_filter(&mut self, preferred_path: Option<&Path>) {
        let filter = self.nav_filter.trim();
        self.nav_entries = self
            .nav_all_entries
            .iter()
            .filter(|entry| entry.name == ".." || nav_filter_matches(&entry.name, filter))
            .cloned()
            .collect();
        self.nav_selected = preferred_path
            .and_then(|path| self.nav_entries.iter().position(|entry| entry.path == path))
            .unwrap_or(self.nav_selected);
        self.nav_selected = nav_selection_after_filter(&self.nav_entries, self.nav_selected, filter);
        self.nav_scroll = nav_scroll_for_selection(
            self.nav_scroll,
            self.nav_selected,
            self.nav_entries.len(),
            self.nav_viewport_rows,
        );
        self.nav_meta_cache_path = None;
    }

    pub(super) fn append_navigation_filter_char(&mut self, ch: char) {
        if !ch.is_ascii() {
            return;
        }
        let selected_path = self
            .nav_entries
            .get(self.nav_selected)
            .map(|entry| entry.path.clone());
        self.nav_filter.push(ch);
        self.apply_navigation_filter(selected_path.as_deref());
    }

    pub(super) fn pop_navigation_filter_char(&mut self) {
        if self.nav_filter.pop().is_none() {
            return;
        }
        let selected_path = self
            .nav_entries
            .get(self.nav_selected)
            .map(|entry| entry.path.clone());
        self.apply_navigation_filter(selected_path.as_deref());
    }

    pub(super) fn clear_navigation_filter(&mut self) {
        if self.nav_filter.is_empty() {
            return;
        }
        let selected_path = self
            .nav_entries
            .get(self.nav_selected)
            .map(|entry| entry.path.clone());
        self.nav_filter.clear();
        self.apply_navigation_filter(selected_path.as_deref());
    }

    pub(super) fn new(editor_program: String) -> io::Result<Self> {
        let (config_state, config_status) = ConfigState::load_or_default();
        let mut config_editor = ConfigEditor::default();
        if let Some(status) = config_status {
            config_editor.status_message = status;
        }
        config_editor.ensure_valid(&config_state);
        Ok(Self {
            active: ActivePane::Navigation,
            shell: ShellPane::spawn()?,
            pending_alt: false,
            pending_alt_shortcut_armed: false,
            shell_fullish: false,
            nav_fullish: false,
            config_open: false,
            config_state: config_state.clone(),
            saved_config_state: config_state,
            config_editor,
            nav_colors: LsColorsTheme::from_env(),
            nav_cwd: PathBuf::new(),
            nav_all_entries: Vec::new(),
            nav_entries: Vec::new(),
            nav_filter: String::new(),
            nav_error: None,
            nav_loaded: false,
            nav_selected: 0,
            nav_scroll: 0,
            nav_pending_file_shortcut: None,
            nav_viewport_rows: 1,
            nav_meta_cache_path: None,
            nav_meta_cache: String::new(),
            preview_mode: PreviewMode::Empty,
            preview_depth: 1,
            preview_max_depth: 1,
            preview_dir_enabled: true,
            preview_cached_text: String::new(),
            preview_last_selected_path: None,
            preview_cached_depth: 0,
            preview_command_overlay_active: false,
            preview_command_overlay_command: String::new(),
            preview_command_overlay_mode: None,
            preview_command_overlay_presentation: None,
            preview_command_shell: None,
            effective_identity: EffectiveIdentity::current(),
            editor_program,
            config_shortcut_alert_until: None,
            key_debug_log: open_key_debug_log(),
            key_debug_seq: 0,
            force_terminal_clear: false,
            mouse_selection: None,
            render_snapshot: RenderTextSnapshot::empty(),
        })
    }

    pub(super) fn open_config(&mut self) {
        self.config_open = true;
        self.config_editor.editing = false;
        self.config_editor.clear_input();
        self.config_editor.ensure_valid(&self.config_state);
    }

    pub(super) fn has_unsaved_config_changes(&self) -> bool {
        self.config_editor.dirty
    }

    pub(super) fn block_exit_attempt_if_unsaved(&mut self) -> bool {
        if !self.has_unsaved_config_changes() {
            return false;
        }
        self.config_shortcut_alert_until = Some(Instant::now() + Duration::from_secs(1));
        true
    }

    pub(super) fn tick_feedback(&mut self) {
        if let Some(until) = self.config_shortcut_alert_until {
            if Instant::now() >= until {
                self.config_shortcut_alert_until = None;
            }
        }
        self.maybe_finish_preview_overlay_session();
    }

    pub(super) fn should_highlight_config_shortcut(&self) -> bool {
        self.config_shortcut_alert_until
            .is_some_and(|until| Instant::now() < until)
            && self.has_unsaved_config_changes()
            && !self.config_open
    }

    pub(super) fn log_key_debug_event(
        &mut self,
        stage: &str,
        key: Option<&crossterm::event::KeyEvent>,
    ) {
        let Some(log) = self.key_debug_log.as_mut() else {
            return;
        };
        self.key_debug_seq = self.key_debug_seq.saturating_add(1);
        if let Some(key) = key {
            let _ = writeln!(
                log,
                "#{:06} stage={} key_code={:?} key_kind={:?} key_mods={:?} pending_alt={} armed={} active={:?} overlay={} overlay_mode={:?}",
                self.key_debug_seq,
                stage,
                key.code,
                key.kind,
                key.modifiers,
                self.pending_alt,
                self.pending_alt_shortcut_armed,
                self.active,
                self.preview_command_overlay_active,
                self.preview_command_overlay_mode
            );
        } else {
            let _ = writeln!(
                log,
                "#{:06} stage={} pending_alt={} armed={} active={:?} overlay={} overlay_mode={:?}",
                self.key_debug_seq,
                stage,
                self.pending_alt,
                self.pending_alt_shortcut_armed,
                self.active,
                self.preview_command_overlay_active,
                self.preview_command_overlay_mode
            );
        }
        let _ = log.flush();
    }

    pub(super) fn refresh_preview_panel(&mut self) {
        let selected_entry = self.nav_entries.get(self.nav_selected);
        let selected_path = selected_entry.map(|entry| entry.path.clone());

        let Some(entry) = selected_entry else {
            self.preview_mode = PreviewMode::Empty;
            self.preview_cached_text.clear();
            self.preview_last_selected_path = None;
            self.preview_cached_depth = 0;
            return;
        };

        let selected_changed = self.preview_last_selected_path.as_ref() != Some(&entry.path);

        if !entry.is_dir {
            self.preview_mode = PreviewMode::FileText;
            self.preview_cached_text.clear();
            self.preview_last_selected_path = selected_path;
            self.preview_cached_depth = 0;
            return;
        }

        self.preview_last_selected_path = selected_path;
        if !self.preview_dir_enabled {
            self.preview_mode = PreviewMode::Empty;
            self.preview_cached_text.clear();
            self.preview_cached_depth = 0;
            return;
        }

        let cache_hit = !selected_changed
            && self.preview_mode == PreviewMode::DirectoryTree
            && self.preview_cached_depth == self.preview_depth;
        if cache_hit {
            return;
        }

        let (mode, text) = preview_content_for_selected_entry(Some(entry), self.preview_depth);
        self.preview_mode = mode;
        self.preview_cached_text = text;
        self.preview_cached_depth = self.preview_depth;
    }

    pub(super) fn toggle_directory_preview(&mut self) {
        if !self
            .nav_entries
            .get(self.nav_selected)
            .is_some_and(|entry| entry.is_dir)
        {
            return;
        }
        self.preview_dir_enabled = !self.preview_dir_enabled;
        self.preview_cached_depth = 0;
        self.refresh_preview_panel();
    }

    pub(super) fn handle_preview_space_action(&mut self) -> bool {
        let selected = self
            .nav_entries
            .get(self.nav_selected)
            .map(|entry| entry.is_dir);
        match selected {
            Some(true) => {
                self.toggle_directory_preview();
                true
            }
            Some(false) => false,
            None => false,
        }
    }

    pub(super) fn close_preview_command_overlay(&mut self) {
        if let Some(mut session) = self.preview_command_shell.take() {
            session.terminate();
        }
        self.preview_command_overlay_active = false;
        self.preview_command_overlay_command.clear();
        self.preview_command_overlay_mode = None;
        self.preview_command_overlay_presentation = None;
        self.force_terminal_clear = true;
    }

    pub(super) fn run_preview_command_overlay(
        &mut self,
        command: &str,
        mode: PreviewCommandMode,
    ) -> io::Result<()> {
        self.close_preview_command_overlay();
        let cwd = if self.nav_cwd.as_os_str().is_empty() {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
        } else {
            self.nav_cwd.clone()
        };
        let spawned_command = if mode == PreviewCommandMode::Read {
            format!("PAGER=cat BAT_PAGER=cat {command}")
        } else {
            command.to_string()
        };
        let mut session = ShellPane::spawn_command(&spawned_command, &cwd)?;
        if mode == PreviewCommandMode::Read {
            session.scroll_up(usize::MAX);
        }
        self.preview_command_overlay_active = true;
        self.preview_command_overlay_command = command.to_string();
        self.preview_command_overlay_mode = Some(mode);
        self.preview_command_overlay_presentation = Some(PreviewOverlayPresentation::StaticFullscreen);
        self.preview_command_shell = Some(session);
        self.active = ActivePane::Preview;
        self.force_terminal_clear = true;
        Ok(())
    }

    pub(super) fn focus_shell_with_prefilled_command(&mut self, command: &str) -> io::Result<()> {
        self.close_preview_command_overlay();
        self.active = ActivePane::Shell;
        self.shell.jump_to_bottom();
        self.shell.send_raw(&prefill_shell_input_bytes(command))
    }

    pub(super) fn poll_preview_command_output(&mut self) -> OutputDrain {
        if let Some(session) = self.preview_command_shell.as_mut() {
            let drain = session.poll_output();
            self.preview_command_overlay_presentation = next_preview_overlay_presentation(
                self.preview_command_overlay_presentation,
                session.in_alt_screen(),
            );
            drain
        } else {
            OutputDrain {
                processed_chunks: 0,
                hit_limit: false,
            }
        }
    }

    pub(super) fn maybe_finish_preview_overlay_session(&mut self) {
        if !self.preview_command_overlay_active {
            return;
        }
        let Some(session) = self.preview_command_shell.as_mut() else {
            self.close_preview_command_overlay();
            self.active = ActivePane::Navigation;
            return;
        };
        if !should_auto_close_preview_overlay(
            self.preview_command_overlay_presentation,
            session.is_running(),
        ) {
            return;
        }
        self.close_preview_command_overlay();
        self.active = ActivePane::Navigation;
    }

    pub(super) fn prepare_for_exit(&mut self) {
        self.close_preview_command_overlay();
        self.shell.request_shutdown();
    }

    pub(super) fn increase_preview_depth(&mut self) {
        if !self
            .nav_entries
            .get(self.nav_selected)
            .is_some_and(|entry| entry.is_dir)
        {
            return;
        }
        self.preview_depth = self.preview_depth.saturating_add(1);
        self.preview_depth = clamp_preview_depth(self.preview_depth, self.preview_max_depth);
        self.preview_cached_depth = 0;
    }

    pub(super) fn decrease_preview_depth(&mut self) {
        if !self
            .nav_entries
            .get(self.nav_selected)
            .is_some_and(|entry| entry.is_dir)
        {
            return;
        }
        self.preview_depth = self.preview_depth.saturating_sub(1);
        self.preview_depth = clamp_preview_depth(self.preview_depth, self.preview_max_depth);
        self.preview_cached_depth = 0;
    }

    pub(super) fn save_config(&mut self) {
        self.config_state.normalize();
        if first_empty_extension(&self.config_state) {
            self.config_editor.status_message = "save blocked: extension name cannot be empty".to_string();
            return;
        }
        if let Some(duplicate) = first_duplicate_extension(&self.config_state) {
            self.config_editor.status_message = format!("save blocked: duplicate extension '.{duplicate}'");
            return;
        }
        match self.config_state.save() {
            Ok(()) => {
                self.saved_config_state = self.config_state.clone();
                self.config_editor.dirty = false;
                self.config_editor.status_message =
                    format!("saved {}", ConfigState::config_file_path().display());
            }
            Err(err) => {
                self.config_editor.status_message = format!("save failed: {err}");
            }
        }
    }

    pub(super) fn discard_config_changes(&mut self) {
        self.config_state = self.saved_config_state.clone();
        self.config_editor.dirty = false;
        self.config_editor.editing = false;
        self.config_editor.clear_input();
        self.config_editor.ensure_valid(&self.config_state);
        self.config_editor.status_message = "discarded unsaved changes".to_string();
    }

    pub(super) fn reload_config(&mut self) {
        match ConfigState::load() {
            Ok(state) => {
                self.config_state = state.clone();
                self.saved_config_state = state;
                self.config_editor.dirty = false;
                self.config_editor.ensure_valid(&self.config_state);
                self.config_editor.status_message =
                    format!("reloaded {}", ConfigState::config_file_path().display());
            }
            Err(err) => {
                self.config_editor.status_message = format!("reload failed: {err}");
            }
        }
    }

    pub(super) fn delete_selected_extension_rule(&mut self) {
        if self.config_state.extension_rules.is_empty() {
            return;
        }
        self.config_state
            .extension_rules
            .remove(self.config_editor.selected_rule);
        self.config_editor.ensure_valid(&self.config_state);
        self.config_editor.dirty = true;
        self.config_editor.status_message = "deleted extension rule".to_string();
    }

    pub(super) fn refresh_navigation(&mut self, cwd: &Path) {
        if self.nav_loaded && self.nav_cwd == cwd {
            return;
        }
        self.nav_cwd = cwd.to_path_buf();
        self.nav_loaded = true;
        let selected_path = self
            .nav_entries
            .get(self.nav_selected)
            .map(|entry| entry.path.clone());
        match navigation_entries(cwd) {
            Ok(entries) => {
                self.nav_all_entries = entries;
                self.nav_error = None;
                self.apply_navigation_filter(selected_path.as_deref());
            }
            Err(err) => {
                self.nav_all_entries.clear();
                self.nav_entries.clear();
                self.nav_error = Some(err.to_string());
            }
        }
        self.nav_meta_cache_path = None;
    }

    pub(super) fn nav_meta_for_selection(&mut self) -> String {
        if let Some(err) = self.nav_error.as_deref() {
            self.nav_meta_cache_path = None;
            self.nav_meta_cache = format!("error: {err}");
            return self.nav_meta_cache.clone();
        }
        let Some(selected) = self.nav_entries.get(self.nav_selected).cloned() else {
            self.nav_meta_cache_path = None;
            self.nav_meta_cache.clear();
            return String::new();
        };
        if self
            .nav_meta_cache_path
            .as_ref()
            .is_some_and(|path| path == &selected.path)
        {
            return self.nav_meta_cache.clone();
        }
        self.nav_meta_cache_path = Some(selected.path.clone());
        self.nav_meta_cache = nav_long_listing(&selected);
        self.nav_meta_cache.clone()
    }
}
