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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ExtensionCommandRule {
    pub(crate) extension: String,
    pub(crate) read_cmd: String,
    pub(crate) write_cmd: String,
    pub(crate) exec_cmd: String,
    /// Per-command visibility and permission check, written like `ls` bits:
    /// position 0 = read, 1 = write, 2 = exec. Lowercase shows the command and
    /// checks the matching file permission, uppercase (`X`) shows it without the
    /// check, and `-` hides it from Preview and the config editor (the command
    /// text is kept so switching back restores it).
    #[serde(default = "default_command_mask")]
    pub(crate) command_mask: String,
}

pub(crate) const DEFAULT_COMMAND_MASK: &str = "rwx";

fn default_command_mask() -> String {
    DEFAULT_COMMAND_MASK.to_string()
}

/// Allowed characters per mask position, in `Enter` cycle order. Read and
/// write cannot skip their permission check (yet); exec can via `X`.
pub(crate) fn command_mask_options(position: usize) -> &'static [char] {
    match position {
        0 => &['r', '-'],
        1 => &['w', '-'],
        _ => &['x', 'X', '-'],
    }
}

/// Keep valid characters; any missing or invalid position falls back to its
/// default so a typo never hides a command by accident.
pub(crate) fn normalize_command_mask(raw: &str) -> String {
    let chars: Vec<char> = raw.trim().chars().collect();
    (0..3)
        .map(|position| {
            let options = command_mask_options(position);
            chars
                .get(position)
                .copied()
                .filter(|ch| options.contains(ch))
                .unwrap_or(options[0])
        })
        .collect()
}

/// Advance one mask position to its next allowed character.
pub(crate) fn cycle_command_mask(mask: &str, position: usize) -> String {
    let mut chars: Vec<char> = normalize_command_mask(mask).chars().collect();
    let options = command_mask_options(position);
    let current = options
        .iter()
        .position(|option| *option == chars[position])
        .unwrap_or(0);
    chars[position] = options[(current + 1) % options.len()];
    chars.into_iter().collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandSlotMode {
    /// Shown; enabled only with the matching effective file permission.
    Checked,
    /// Shown; permission check skipped.
    Unchecked,
    /// Not shown anywhere and cannot be triggered.
    Hidden,
}

impl ExtensionCommandRule {
    /// Mode of the command at `position` (0 = read, 1 = write, 2 = exec).
    pub(crate) fn command_slot_mode(&self, position: usize) -> CommandSlotMode {
        match normalize_command_mask(&self.command_mask)
            .chars()
            .nth(position)
        {
            Some('-') => CommandSlotMode::Hidden,
            Some(ch) if ch.is_ascii_uppercase() => CommandSlotMode::Unchecked,
            _ => CommandSlotMode::Checked,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ConfigState {
    #[serde(default)]
    pub(crate) extension_rules: Vec<ExtensionCommandRule>,
}

impl Default for ConfigState {
    fn default() -> Self {
        Self {
            extension_rules: vec![
                ExtensionCommandRule {
                    extension: "md".to_string(),
                    read_cmd: "bat {file}".to_string(),
                    write_cmd: "$EDITOR {file}".to_string(),
                    exec_cmd: "mdterm {file}".to_string(),
                    command_mask: default_command_mask(),
                },
                ExtensionCommandRule {
                    extension: "json".to_string(),
                    read_cmd: "jq . {file}".to_string(),
                    write_cmd: "$EDITOR {file}".to_string(),
                    exec_cmd: "fx {file}".to_string(),
                    command_mask: default_command_mask(),
                },
                ExtensionCommandRule {
                    extension: "sh".to_string(),
                    read_cmd: "bat {file}".to_string(),
                    write_cmd: "$EDITOR {file}".to_string(),
                    exec_cmd: "bash {file}".to_string(),
                    command_mask: default_command_mask(),
                },
            ],
        }
    }
}

impl ConfigState {
    fn normalize_command_template(template: &str) -> String {
        let trimmed = template.trim();
        if trimmed == "--" || trimmed.eq_ignore_ascii_case("none") {
            "None".to_string()
        } else {
            template.to_string()
        }
    }

    pub(crate) fn config_file_path() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.trim().is_empty() {
                return PathBuf::from(xdg).join("navix").join("config.toml");
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            if !home.trim().is_empty() {
                return PathBuf::from(home)
                    .join(".config")
                    .join("navix")
                    .join("config.toml");
            }
        }
        PathBuf::from("navix-config.toml")
    }

    pub(crate) fn load() -> io::Result<Self> {
        let path = Self::config_file_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&path)?;
        let mut loaded: Self = toml::from_str(&raw).map_err(to_io)?;
        loaded.normalize();
        Ok(loaded)
    }

    pub(crate) fn load_or_default() -> (Self, Option<String>) {
        match Self::load() {
            Ok(state) => (state, None),
            Err(err) => (
                Self::default(),
                Some(format!("config load failed, using defaults: {err}")),
            ),
        }
    }

    pub(crate) fn save(&self) -> io::Result<()> {
        let path = Self::config_file_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut normalized = self.clone();
        normalized.normalize();
        let serialized = toml::to_string_pretty(&normalized).map_err(to_io)?;
        fs::write(path, serialized)?;
        Ok(())
    }

    pub(crate) fn normalize(&mut self) {
        for rule in &mut self.extension_rules {
            rule.extension = normalize_extension(&rule.extension);
            if rule.read_cmd.trim() == "bat --paging=never {file}" {
                rule.read_cmd = "bat {file}".to_string();
            }
            rule.read_cmd = Self::normalize_command_template(&rule.read_cmd);
            rule.write_cmd = Self::normalize_command_template(&rule.write_cmd);
            rule.exec_cmd = Self::normalize_command_template(&rule.exec_cmd);
            rule.command_mask = normalize_command_mask(&rule.command_mask);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigField {
    Extension,
    /// One character of `command_mask`: 0 = read, 1 = write, 2 = exec.
    CommandMask(usize),
    Read,
    Write,
    Exec,
}

impl ConfigField {
    /// Raw field order, matching the on-screen layout.
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Extension => Self::CommandMask(0),
            Self::CommandMask(position) if position < 2 => Self::CommandMask(position + 1),
            Self::CommandMask(_) => Self::Read,
            Self::Read => Self::Write,
            Self::Write => Self::Exec,
            Self::Exec => Self::Extension,
        }
    }

    pub(crate) fn prev(self) -> Self {
        match self {
            Self::Extension => Self::Exec,
            Self::CommandMask(0) => Self::Extension,
            Self::CommandMask(position) => Self::CommandMask(position - 1),
            Self::Read => Self::CommandMask(2),
            Self::Write => Self::Read,
            Self::Exec => Self::Write,
        }
    }

    /// Whether this field is on screen for `rule` (commands hidden by the mask
    /// are skipped by navigation).
    pub(crate) fn visible_in(self, rule: &ExtensionCommandRule) -> bool {
        let position = match self {
            Self::Read => 0,
            Self::Write => 1,
            Self::Exec => 2,
            Self::Extension | Self::CommandMask(_) => return true,
        };
        rule.command_slot_mode(position) != CommandSlotMode::Hidden
    }

    /// Next field that is visible for `rule`. Always terminates because the
    /// extension and mask fields are never hidden.
    pub(crate) fn next_in(self, rule: &ExtensionCommandRule) -> Self {
        let mut field = self.next();
        while !field.visible_in(rule) {
            field = field.next();
        }
        field
    }

    pub(crate) fn prev_in(self, rule: &ExtensionCommandRule) -> Self {
        let mut field = self.prev();
        while !field.visible_in(rule) {
            field = field.prev();
        }
        field
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Extension => "extension",
            Self::CommandMask(_) => "command-mask",
            Self::Read => "read",
            Self::Write => "write",
            Self::Exec => "exec",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ConfigEditor {
    pub(crate) selected_rule: usize,
    pub(crate) selected_field: ConfigField,
    pub(crate) editing: bool,
    pub(crate) input_buffer: String,
    pub(crate) input_cursor: usize,
    pub(crate) dirty: bool,
    pub(crate) status_message: String,
}

impl Default for ConfigEditor {
    fn default() -> Self {
        Self {
            selected_rule: 0,
            selected_field: ConfigField::Extension,
            editing: false,
            input_buffer: String::new(),
            input_cursor: 0,
            dirty: false,
            status_message: String::new(),
        }
    }
}

impl ConfigEditor {
    pub(crate) fn ensure_valid(&mut self, config: &ConfigState) {
        self.selected_rule = if config.extension_rules.is_empty() {
            0
        } else {
            self.selected_rule
                .min(config.extension_rules.len().saturating_sub(1))
        };
        // Moving to a rule that hides the selected command lands on the next
        // visible field instead of an invisible one.
        if let Some(rule) = config.extension_rules.get(self.selected_rule)
            && !self.selected_field.visible_in(rule)
        {
            self.selected_field = self.selected_field.next_in(rule);
        }
    }

    pub(crate) fn clear_input(&mut self) {
        self.input_buffer.clear();
        self.input_cursor = 0;
    }

    pub(crate) fn set_input(&mut self, value: String) {
        self.input_buffer = value;
        self.input_cursor = self.input_buffer.len();
    }

    pub(crate) fn move_cursor_left(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        self.input_cursor = previous_char_boundary(&self.input_buffer, self.input_cursor);
    }

    pub(crate) fn move_cursor_right(&mut self) {
        if self.input_cursor >= self.input_buffer.len() {
            self.input_cursor = self.input_buffer.len();
            return;
        }
        self.input_cursor = next_char_boundary(&self.input_buffer, self.input_cursor);
    }

    pub(crate) fn move_cursor_home(&mut self) {
        self.input_cursor = 0;
    }

    pub(crate) fn move_cursor_end(&mut self) {
        self.input_cursor = self.input_buffer.len();
    }

    pub(crate) fn insert_char(&mut self, ch: char) {
        self.input_buffer.insert(self.input_cursor, ch);
        self.input_cursor += ch.len_utf8();
    }

    pub(crate) fn backspace(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        let start = previous_char_boundary(&self.input_buffer, self.input_cursor);
        self.input_buffer.drain(start..self.input_cursor);
        self.input_cursor = start;
    }

    pub(crate) fn delete(&mut self) {
        if self.input_cursor >= self.input_buffer.len() {
            return;
        }
        let end = next_char_boundary(&self.input_buffer, self.input_cursor);
        self.input_buffer.drain(self.input_cursor..end);
    }
}
