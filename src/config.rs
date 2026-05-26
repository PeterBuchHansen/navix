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
                },
                ExtensionCommandRule {
                    extension: "json".to_string(),
                    read_cmd: "jq . {file}".to_string(),
                    write_cmd: "$EDITOR {file}".to_string(),
                    exec_cmd: "fx {file}".to_string(),
                },
                ExtensionCommandRule {
                    extension: "sh".to_string(),
                    read_cmd: "bat {file}".to_string(),
                    write_cmd: "$EDITOR {file}".to_string(),
                    exec_cmd: "bash {file}".to_string(),
                },
            ],
        }
    }
}

impl ConfigState {
    pub(crate) fn config_file_path() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.trim().is_empty() {
                return PathBuf::from(xdg).join("navix").join("config.toml");
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            if !home.trim().is_empty() {
                return PathBuf::from(home).join(".config").join("navix").join("config.toml");
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
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigField {
    Extension,
    Read,
    Write,
    Exec,
}

impl ConfigField {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Extension => Self::Read,
            Self::Read => Self::Write,
            Self::Write => Self::Exec,
            Self::Exec => Self::Extension,
        }
    }

    pub(crate) fn prev(self) -> Self {
        match self {
            Self::Extension => Self::Exec,
            Self::Read => Self::Extension,
            Self::Write => Self::Read,
            Self::Exec => Self::Write,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Extension => "extension",
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
            self.selected_rule.min(config.extension_rules.len().saturating_sub(1))
        };
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
