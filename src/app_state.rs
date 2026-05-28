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

const PREVIEW_JUMP_HISTORY_LIMIT: usize = 100;
const PREVIEW_JUMP_COMPLETION_LIMIT: usize = 512;

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
        self.nav_selected =
            nav_selection_after_filter(&self.nav_entries, self.nav_selected, filter);
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
        let preview_jump_history = load_preview_jump_history(PREVIEW_JUMP_HISTORY_LIMIT);
        Ok(Self {
            active: ActivePane::Navigation,
            shell: ShellPane::spawn()?,
            shell_pending_input: false,
            pending_alt: false,
            pending_alt_shortcut_armed: false,
            shell_fullish: false,
            nav_fullish: false,
            config_open: false,
            help_open: false,
            help_context: ActivePane::Navigation,
            help_scroll: 0,
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
            nav_backspace_parent_ready: true,
            nav_filter_char_held: None,
            nav_viewport_rows: 1,
            nav_meta_cache_path: None,
            nav_meta_cache: String::new(),
            preview_mode: PreviewMode::Empty,
            preview_depth: 1,
            preview_cached_text: String::new(),
            preview_last_selected_path: None,
            preview_cached_depth: 0,
            preview_jump_input: String::new(),
            preview_jump_user_typed: false,
            preview_jump_history,
            preview_jump_history_index: None,
            preview_jump_completions: Vec::new(),
            preview_jump_status: None,
            preview_jump_cycle_seed: None,
            preview_jump_cycle_index: None,
            nav_preferred_selection_path: None,
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
        self.help_open = false;
        self.config_editor.editing = false;
        self.config_editor.clear_input();
        self.config_editor.ensure_valid(&self.config_state);
    }

    pub(super) fn open_help(&mut self) {
        self.help_context = self.active;
        self.help_open = true;
        self.help_scroll = 0;
    }

    pub(super) fn close_help(&mut self) {
        self.help_open = false;
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

    pub(super) fn preview_jump_push_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        self.preview_jump_input.push(ch);
        self.preview_jump_user_typed = true;
        self.preview_jump_history_index = None;
        self.preview_jump_status = None;
        self.reset_preview_jump_completion_cycle();
        self.update_preview_jump_completions();
    }

    pub(super) fn preview_jump_backspace(&mut self) {
        if self.preview_jump_input.pop().is_none() {
            return;
        }
        self.preview_jump_history_index = None;
        self.preview_jump_status = None;
        if self.preview_jump_input.is_empty() {
            self.preview_jump_user_typed = false;
            self.preview_jump_completions.clear();
        } else {
            self.preview_jump_user_typed = true;
            self.update_preview_jump_completions();
        }
        self.reset_preview_jump_completion_cycle();
    }

    pub(super) fn preview_jump_step(&mut self, direction: i32, panel_width: u16) {
        if self.preview_jump_move_completion(direction, panel_width, false) {
            return;
        }
        let history_wheel_len = self.preview_jump_history.len().saturating_add(1);
        let next = match self.preview_jump_history_index {
            Some(current) => cycle_wrapped_index(current, history_wheel_len, direction),
            None => {
                if direction < 0 {
                    history_wheel_len.saturating_sub(1)
                } else if history_wheel_len > 1 {
                    1
                } else {
                    0
                }
            },
        };
        self.preview_jump_history_index = Some(next);
        if next == 0 {
            self.preview_jump_input.clear();
            self.preview_jump_user_typed = false;
        } else if let Some(entry) = self.preview_jump_history.get(next.saturating_sub(1)) {
            self.preview_jump_input = entry.clone();
            self.preview_jump_user_typed = false;
        }
        self.preview_jump_completions.clear();
        self.reset_preview_jump_completion_cycle();
        self.preview_jump_status = None;
    }

    pub(super) fn preview_jump_step_horizontal(&mut self, direction: i32, panel_width: u16) {
        let _ = self.preview_jump_move_completion(direction, panel_width, true);
    }

    pub(super) fn preview_jump_tab_complete(&mut self) {
        self.preview_jump_tab_complete_with_direction(1);
    }

    pub(super) fn preview_jump_tab_complete_reverse(&mut self) {
        self.preview_jump_tab_complete_with_direction(-1);
    }

    fn preview_jump_tab_complete_with_direction(&mut self, direction: i32) {
        let query = self.preview_jump_input.trim().to_string();
        if query.is_empty() {
            return;
        }
        self.preview_jump_user_typed = true;
        self.preview_jump_history_index = None;
        self.preview_jump_status = None;
        if let Some(seed) = self.preview_jump_cycle_seed.clone() {
            let cycled = preview_jump_completion_candidates(
                &seed,
                &self.preview_jump_base_dir(),
                PREVIEW_JUMP_COMPLETION_LIMIT,
            );
            if !cycled.is_empty() {
                let next = match self.preview_jump_cycle_index {
                    Some(current) => cycle_wrapped_index(current, cycled.len(), direction),
                    None if direction < 0 => cycled.len().saturating_sub(1),
                    None => 0,
                };
                self.preview_jump_cycle_index = Some(next);
                self.preview_jump_completions = cycled;
                self.preview_jump_input =
                    normalize_preview_jump_input_text(&self.preview_jump_completions[next]);
                return;
            }
            self.reset_preview_jump_completion_cycle();
        }
        let matches = preview_jump_completion_candidates(
            &query,
            &self.preview_jump_base_dir(),
            PREVIEW_JUMP_COMPLETION_LIMIT,
        );
        if matches.is_empty() {
            self.preview_jump_completions.clear();
            return;
        }
        if matches.len() == 1 {
            self.preview_jump_input = normalize_preview_jump_input_text(&matches[0]);
            self.preview_jump_completions = matches;
            self.reset_preview_jump_completion_cycle();
            return;
        }
        let common_prefix = longest_common_prefix(&matches);
        if common_prefix.chars().count() > query.chars().count() {
            self.preview_jump_input = normalize_preview_jump_input_text(&common_prefix);
            self.reset_preview_jump_completion_cycle();
            self.update_preview_jump_completions();
            return;
        }
        self.preview_jump_cycle_seed = Some(query);
        let initial_index = if direction < 0 {
            matches.len().saturating_sub(1)
        } else {
            0
        };
        self.preview_jump_cycle_index = Some(initial_index);
        self.preview_jump_completions = matches;
        self.preview_jump_input =
            normalize_preview_jump_input_text(&self.preview_jump_completions[initial_index]);
    }

    pub(super) fn preview_jump_enter_action(&mut self) -> io::Result<bool> {
        if let Some(selected_index) = self.preview_jump_selected_completion_index()
            && let Some(selected) = self.preview_jump_completions.get(selected_index).cloned()
        {
            if selected.ends_with('/') {
                self.preview_jump_input = normalize_preview_jump_input_text(&selected);
                self.preview_jump_user_typed = true;
                self.preview_jump_history_index = None;
                self.preview_jump_status = None;
                self.reset_preview_jump_completion_cycle();
                self.update_preview_jump_completions();
                return Ok(false);
            }
            self.preview_jump_input = normalize_preview_jump_input_text(&selected);
            self.preview_jump_user_typed = true;
            self.preview_jump_history_index = None;
            self.preview_jump_status = None;
            self.reset_preview_jump_completion_cycle();
        }
        self.preview_jump_submit()
    }

    pub(super) fn preview_jump_submit(&mut self) -> io::Result<bool> {
        let query = self.preview_jump_input.trim().to_string();
        if query.trim().is_empty() {
            return Ok(false);
        }
        let base = self.preview_jump_base_dir();
        let target = normalize_path(resolve_preview_jump_query(&query, &base));
        if !target.exists() {
            self.preview_jump_status = Some(format!("path not found: {}", target.display()));
            return Ok(false);
        }
        if target.is_dir() {
            self.nav_filter.clear();
            self.nav_preferred_selection_path = None;
            self.shell.cd_to(&target, self.shell_pending_input)?;
            self.nav_loaded = false;
            self.nav_selected = 0;
            self.nav_scroll = 0;
            self.record_preview_jump_history(&target);
            self.reset_preview_jump_input();
            return Ok(true);
        }
        if target.is_file() {
            let Some(parent) = target.parent() else {
                self.preview_jump_status = Some(format!(
                    "cannot jump to file without parent: {}",
                    target.display()
                ));
                return Ok(false);
            };
            self.nav_filter.clear();
            self.nav_preferred_selection_path = Some(target.clone());
            self.shell.cd_to(parent, self.shell_pending_input)?;
            self.nav_loaded = false;
            self.nav_selected = 0;
            self.nav_scroll = 0;
            self.record_preview_jump_history(&target);
            self.reset_preview_jump_input();
            return Ok(true);
        }
        self.preview_jump_status = Some(format!("unsupported path target: {}", target.display()));
        Ok(false)
    }

    pub(super) fn preview_jump_clear_input(&mut self) {
        self.reset_preview_jump_input();
    }

    pub(super) fn record_preview_jump_history(&mut self, path: &Path) {
        let normalized = normalize_path(path.to_path_buf());
        let entry = normalized.display().to_string();
        if entry.is_empty() {
            return;
        }
        self.preview_jump_history
            .retain(|existing| existing != &entry);
        self.preview_jump_history.insert(0, entry);
        if self.preview_jump_history.len() > PREVIEW_JUMP_HISTORY_LIMIT {
            self.preview_jump_history
                .truncate(PREVIEW_JUMP_HISTORY_LIMIT);
        }
        let _ = save_preview_jump_history(&self.preview_jump_history);
    }

    fn reset_preview_jump_input(&mut self) {
        self.preview_jump_input.clear();
        self.preview_jump_user_typed = false;
        self.preview_jump_history_index = None;
        self.preview_jump_completions.clear();
        self.reset_preview_jump_completion_cycle();
        self.preview_jump_status = None;
    }

    fn preview_jump_base_dir(&self) -> PathBuf {
        if self.nav_cwd.as_os_str().is_empty() {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
        } else {
            self.nav_cwd.clone()
        }
    }

    fn update_preview_jump_completions(&mut self) {
        if !self.preview_jump_user_typed || self.preview_jump_input.trim().is_empty() {
            self.preview_jump_completions.clear();
            return;
        }
        self.preview_jump_completions = preview_jump_completion_candidates(
            &self.preview_jump_input,
            &self.preview_jump_base_dir(),
            PREVIEW_JUMP_COMPLETION_LIMIT,
        );
    }

    fn reset_preview_jump_completion_cycle(&mut self) {
        self.preview_jump_cycle_seed = None;
        self.preview_jump_cycle_index = None;
    }

    fn preview_jump_move_completion(
        &mut self,
        direction: i32,
        panel_width: u16,
        horizontal: bool,
    ) -> bool {
        if self.preview_jump_completions.is_empty() {
            return false;
        }
        if self.preview_jump_cycle_seed.is_none() {
            self.preview_jump_cycle_seed = Some(self.preview_jump_input.clone());
        }
        let len = self.preview_jump_completions.len();
        let columns = self.preview_jump_completion_columns(panel_width).max(1);
        let rows = len.div_ceil(columns);
        let current = self
            .preview_jump_cycle_index
            .or_else(|| {
                self.preview_jump_completions
                    .iter()
                    .position(|candidate| candidate == &self.preview_jump_input)
            })
            .unwrap_or_else(|| {
                if direction < 0 {
                    len.saturating_sub(1)
                } else {
                    0
                }
            });
        let next = if horizontal {
            completion_index_horizontal(current, len, columns, rows, direction)
        } else {
            completion_index_vertical(current, len, columns, rows, direction)
        };
        self.preview_jump_cycle_index = Some(next);
        self.preview_jump_input =
            normalize_preview_jump_input_text(&self.preview_jump_completions[next]);
        self.preview_jump_user_typed = true;
        self.preview_jump_history_index = None;
        self.preview_jump_status = None;
        true
    }

    fn preview_jump_selected_completion_index(&self) -> Option<usize> {
        if self.preview_jump_completions.is_empty() {
            return None;
        }
        if let Some(index) = self.preview_jump_cycle_index
            && index < self.preview_jump_completions.len()
        {
            return Some(index);
        }
        self.preview_jump_completions
            .iter()
            .position(|candidate| candidate == &self.preview_jump_input)
    }

    fn preview_jump_completion_columns(&self, panel_width: u16) -> usize {
        if panel_width == 0 || self.preview_jump_completions.is_empty() {
            return 1;
        }
        let query = self
            .preview_jump_cycle_seed
            .as_deref()
            .unwrap_or(self.preview_jump_input.as_str());
        let max_label_width = self
            .preview_jump_completions
            .iter()
            .map(|candidate| {
                preview_jump_completion_label(query, candidate)
                    .chars()
                    .count()
            })
            .max()
            .unwrap_or(1);
        let col_width = max_label_width.saturating_add(2).max(1);
        (panel_width as usize / col_width).max(1)
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
        self.preview_command_overlay_presentation =
            Some(PreviewOverlayPresentation::StaticFullscreen);
        self.preview_command_shell = Some(session);
        self.active = ActivePane::Preview;
        self.force_terminal_clear = true;
        Ok(())
    }

    pub(super) fn focus_shell_with_prefilled_command(&mut self, command: &str) -> io::Result<()> {
        self.close_preview_command_overlay();
        self.active = ActivePane::Shell;
        self.shell.jump_to_bottom();
        self.shell_pending_input = !command.is_empty();
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

    pub(super) fn save_config(&mut self) {
        self.config_state.normalize();
        if first_empty_extension(&self.config_state) {
            self.config_editor.status_message =
                "save blocked: extension name cannot be empty".to_string();
            return;
        }
        if let Some(duplicate) = first_duplicate_extension(&self.config_state) {
            self.config_editor.status_message =
                format!("save blocked: duplicate extension '.{duplicate}'");
            return;
        }
        match self.config_state.save() {
            Ok(()) => {
                self.saved_config_state = self.config_state.clone();
                self.config_editor.dirty = false;
                self.config_editor.status_message =
                    format!("saved {}", ConfigState::config_file_path().display());
            },
            Err(err) => {
                self.config_editor.status_message = format!("save failed: {err}");
            },
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
            },
            Err(err) => {
                self.config_editor.status_message = format!("reload failed: {err}");
            },
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
        let preferred_selection = self.nav_preferred_selection_path.clone();
        let selected_path = preferred_selection.clone().or_else(|| {
            self.nav_entries
                .get(self.nav_selected)
                .map(|entry| entry.path.clone())
        });
        match navigation_entries(cwd) {
            Ok(entries) => {
                self.nav_all_entries = entries;
                self.nav_error = None;
                self.apply_navigation_filter(selected_path.as_deref());
                self.nav_preferred_selection_path =
                    preferred_selection.and_then(|preferred_path| {
                        if self
                            .nav_entries
                            .iter()
                            .any(|entry| entry.path == preferred_path)
                        {
                            None
                        } else {
                            Some(preferred_path)
                        }
                    });
            },
            Err(err) => {
                self.nav_all_entries.clear();
                self.nav_entries.clear();
                self.nav_error = Some(err.to_string());
                self.nav_preferred_selection_path = preferred_selection;
            },
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

fn cycle_wrapped_index(current: usize, len: usize, direction: i32) -> usize {
    if len == 0 {
        return 0;
    }
    if direction < 0 {
        if current == 0 { len - 1 } else { current - 1 }
    } else {
        (current + 1) % len
    }
}

fn completion_index_horizontal(
    current: usize,
    len: usize,
    columns: usize,
    rows: usize,
    direction: i32,
) -> usize {
    if len == 0 {
        return 0;
    }
    let row = current / columns;
    let col = current % columns;
    if direction < 0 {
        let (next_row, mut next_col) = if col > 0 {
            (row, col - 1)
        } else {
            ((row + rows - 1) % rows, columns.saturating_sub(1))
        };
        while next_row.saturating_mul(columns).saturating_add(next_col) >= len {
            if next_col == 0 {
                break;
            }
            next_col = next_col.saturating_sub(1);
        }
        let idx = next_row.saturating_mul(columns).saturating_add(next_col);
        if idx >= len {
            len.saturating_sub(1)
        } else {
            idx
        }
    } else {
        let mut next_row = row;
        let mut next_col = col.saturating_add(1);
        if next_col >= columns || next_row.saturating_mul(columns).saturating_add(next_col) >= len {
            next_row = (row + 1) % rows;
            next_col = 0;
        }
        let mut idx = next_row.saturating_mul(columns).saturating_add(next_col);
        if idx >= len {
            idx = next_row.saturating_mul(columns);
            if idx >= len {
                idx = 0;
            }
        }
        idx
    }
}

fn completion_index_vertical(
    current: usize,
    len: usize,
    columns: usize,
    rows: usize,
    direction: i32,
) -> usize {
    if len == 0 {
        return 0;
    }
    let row = current / columns;
    let mut col = current % columns;
    let next_row = if direction < 0 {
        (row + rows - 1) % rows
    } else {
        (row + 1) % rows
    };
    let mut idx = next_row.saturating_mul(columns).saturating_add(col);
    while idx >= len && col > 0 {
        col = col.saturating_sub(1);
        idx = next_row.saturating_mul(columns).saturating_add(col);
    }
    if idx >= len {
        next_row.saturating_mul(columns).min(len.saturating_sub(1))
    } else {
        idx
    }
}

fn preview_jump_completion_label(query: &str, candidate: &str) -> String {
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

fn preview_jump_completion_candidates(input: &str, base_dir: &Path, limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }
    let query = input.trim();
    if query.is_empty() {
        return Vec::new();
    }
    if query == ".." || query.ends_with("/..") {
        let prefix = query.strip_suffix("..").unwrap_or_default();
        let candidate = if prefix.is_empty() {
            "../".to_string()
        } else {
            format!("{prefix}../")
        };
        return vec![candidate];
    }
    if query == "~" {
        return vec!["~/".to_string()];
    }
    let query_path = Path::new(query);
    let tilde_prefixed = query.starts_with("~/");
    let absolute = query_path.is_absolute();
    let has_trailing_slash = query.ends_with('/');
    let (search_dir_abs, display_prefix, name_prefix) = if has_trailing_slash {
        let search_dir_abs = if tilde_prefixed {
            resolve_preview_jump_query(query, base_dir)
        } else if absolute {
            PathBuf::from(query)
        } else {
            base_dir.join(query)
        };
        (search_dir_abs, query.to_string(), String::new())
    } else {
        let parent = query_path.parent().unwrap_or(Path::new(""));
        let name_prefix = query_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();
        let search_dir_abs = if tilde_prefixed {
            let parent_text = parent.to_string_lossy();
            if parent_text == "~" || parent_text == "~/" || parent_text.is_empty() {
                home_dir().unwrap_or_else(|| base_dir.to_path_buf())
            } else {
                resolve_preview_jump_query(&parent_text, base_dir)
            }
        } else if absolute {
            if parent.as_os_str().is_empty() {
                PathBuf::from("/")
            } else {
                parent.to_path_buf()
            }
        } else {
            base_dir.join(parent)
        };
        let display_prefix = if tilde_prefixed {
            let parent_text = parent.to_string_lossy();
            if parent_text == "~" || parent_text == "~/" || parent_text.is_empty() {
                "~/".to_string()
            } else {
                format!("{parent_text}/")
            }
        } else if absolute {
            let parent_text = parent.to_string_lossy();
            if parent_text.is_empty() || parent_text == "/" {
                "/".to_string()
            } else {
                format!("{parent_text}/")
            }
        } else {
            let parent_text = parent.to_string_lossy();
            if parent_text.is_empty() {
                String::new()
            } else {
                format!("{parent_text}/")
            }
        };
        (search_dir_abs, display_prefix, name_prefix)
    };
    let read_dir = match fs::read_dir(search_dir_abs) {
        Ok(read_dir) => read_dir,
        Err(_) => return Vec::new(),
    };
    let parent_candidate = if "..".starts_with(&name_prefix) {
        Some(format!("{display_prefix}../"))
    } else {
        None
    };
    let mut items = Vec::new();
    for entry in read_dir.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name_prefix.is_empty() && !name.starts_with(&name_prefix) {
            continue;
        }
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        let mut candidate = format!("{display_prefix}{name}");
        if is_dir {
            candidate.push('/');
        }
        items.push((!is_dir, name.to_ascii_lowercase(), candidate));
    }
    items.sort_by(|left, right| left.cmp(right));
    let reserve_parent_slot = parent_candidate.is_some() && limit > 0;
    let take_count = if reserve_parent_slot {
        limit.saturating_sub(1)
    } else {
        limit
    };
    let mut candidates = items
        .into_iter()
        .take(take_count)
        .map(|(_, _, candidate)| candidate)
        .collect::<Vec<String>>();
    if let Some(parent) = parent_candidate
        && candidates.len() < limit
    {
        candidates.push(parent);
    }
    candidates
}

fn longest_common_prefix(values: &[String]) -> String {
    let Some(first) = values.first() else {
        return String::new();
    };
    let mut prefix = first.clone();
    for value in values.iter().skip(1) {
        while !prefix.is_empty() && !value.starts_with(&prefix) {
            let _ = prefix.pop();
        }
        if prefix.is_empty() {
            break;
        }
    }
    prefix
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {},
            std::path::Component::ParentDir => {
                let _ = normalized.pop();
            },
            _ => normalized.push(component.as_os_str()),
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

fn normalize_preview_jump_input_text(input: &str) -> String {
    let trailing_slash = input.ends_with('/');
    let (mut prefix, remainder) = if let Some(rest) = input.strip_prefix("~/") {
        ("~/", rest)
    } else if let Some(rest) = input.strip_prefix("./") {
        ("./", rest)
    } else if let Some(rest) = input.strip_prefix('/') {
        ("/", rest)
    } else {
        ("", input)
    };
    let mut segments: Vec<&str> = Vec::new();
    for segment in remainder.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            if segments.last().is_some_and(|last| *last != "..") {
                let _ = segments.pop();
                continue;
            }
            if prefix == "/" || prefix == "~/" {
                continue;
            }
            if prefix == "./" && segments.is_empty() {
                prefix = "";
            }
            segments.push("..");
            continue;
        }
        segments.push(segment);
    }
    let mut normalized = String::new();
    normalized.push_str(prefix);
    normalized.push_str(&segments.join("/"));
    if trailing_slash && !normalized.ends_with('/') {
        normalized.push('/');
    }
    if normalized.is_empty() {
        if trailing_slash {
            "./".to_string()
        } else {
            String::new()
        }
    } else {
        normalized
    }
}

#[cfg(test)]
pub(crate) fn preview_jump_completion_candidates_for_test(
    input: &str,
    base_dir: &Path,
    limit: usize,
) -> Vec<String> {
    preview_jump_completion_candidates(input, base_dir, limit)
}

#[cfg(test)]
pub(crate) fn normalize_preview_jump_input_text_for_test(input: &str) -> String {
    normalize_preview_jump_input_text(input)
}

fn resolve_preview_jump_query(query: &str, base_dir: &Path) -> PathBuf {
    if query == "~" {
        return home_dir().unwrap_or_else(|| base_dir.to_path_buf());
    }
    if let Some(rest) = query.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    let path = Path::new(query);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    base_dir.join(path)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn preview_jump_history_file_path() -> PathBuf {
    if let Ok(xdg_state_home) = std::env::var("XDG_STATE_HOME")
        && !xdg_state_home.trim().is_empty()
    {
        return PathBuf::from(xdg_state_home)
            .join("navix")
            .join("preview_jump_history.txt");
    }
    if let Some(home) = home_dir() {
        return home
            .join(".local")
            .join("state")
            .join("navix")
            .join("preview_jump_history.txt");
    }
    PathBuf::from("navix-preview-jump-history.txt")
}

fn load_preview_jump_history(limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }
    let path = preview_jump_history_file_path();
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return Vec::new(),
    };
    let mut loaded = Vec::new();
    for line in raw.lines() {
        let entry = line.trim();
        if entry.is_empty() || loaded.iter().any(|existing| existing == entry) {
            continue;
        }
        loaded.push(entry.to_string());
        if loaded.len() >= limit {
            break;
        }
    }
    loaded
}

fn save_preview_jump_history(entries: &[String]) -> io::Result<()> {
    let path = preview_jump_history_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut contents = entries.join("\n");
    if !contents.is_empty() {
        contents.push('\n');
    }
    fs::write(path, contents)
}
