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

#[derive(Debug, Clone)]
pub(crate) struct LsColorsTheme {
    type_styles: HashMap<String, Style>,
    suffix_styles: Vec<(String, Style)>,
}

impl LsColorsTheme {
    pub(crate) fn from_env() -> Self {
        let mut theme = Self::fallback();
        if let Ok(raw) = std::env::var("LS_COLORS") {
            theme.apply(&raw);
        }
        theme
    }

    pub(crate) fn fallback() -> Self {
        let mut theme = Self {
            type_styles: HashMap::new(),
            suffix_styles: Vec::new(),
        };
        theme.apply("rs=0:no=0:fi=0:di=01;34:ln=01;36:ex=01;32");
        theme
    }

    pub(crate) fn apply(&mut self, raw: &str) {
        for segment in raw.split(':') {
            let Some((raw_key, raw_value)) = segment.split_once('=') else {
                continue;
            };
            let key = raw_key.trim().to_ascii_lowercase();
            if key.is_empty() {
                continue;
            }
            let style = sgr_to_style(raw_value.trim());
            if let Some(raw_suffix) = key.strip_prefix("*.") {
                if raw_suffix.is_empty() {
                    continue;
                }
                let suffix = format!(".{}", raw_suffix.to_ascii_lowercase());
                self.suffix_styles
                    .retain(|(existing, _)| existing != &suffix);
                self.suffix_styles.push((suffix, style));
            } else {
                self.type_styles.insert(key, style);
            }
        }
    }

    pub(crate) fn style_for_entry(
        &self,
        name: &str,
        is_dir: bool,
        is_symlink: bool,
        mode: u32,
    ) -> Style {
        if is_dir {
            return self.type_style("di");
        }
        if is_symlink {
            return self.type_style("ln");
        }

        let lower_name = name.to_ascii_lowercase();
        if let Some(style) = self
            .suffix_styles
            .iter()
            .filter(|(suffix, _)| lower_name.ends_with(suffix))
            .max_by_key(|(suffix, _)| suffix.len())
            .map(|(_, style)| *style)
        {
            return style;
        }

        if mode & 0o111 != 0 {
            return self.type_style("ex");
        }
        self.type_style("fi")
    }

    fn type_style(&self, key: &str) -> Style {
        self.type_styles
            .get(key)
            .copied()
            .or_else(|| self.type_styles.get("no").copied())
            .unwrap_or_else(|| Style::default().fg(Color::White))
    }
}

pub(crate) fn navigation_name_style(
    colors: &LsColorsTheme,
    name: &str,
    is_dir: bool,
    is_symlink: bool,
    mode: u32,
) -> Style {
    colors.style_for_entry(name, is_dir, is_symlink, mode)
}

pub(crate) fn nav_style_for_theme(style: Style, fullish_shell_theme: bool) -> Style {
    if fullish_shell_theme {
        style.fg(Color::DarkGray)
    } else {
        style
    }
}

pub(crate) fn nav_row_selected_style(style: Style, selected: bool) -> Style {
    if selected {
        style.bg(Color::Yellow).fg(Color::Black)
    } else {
        style
    }
}

pub(crate) fn sgr_to_style(raw: &str) -> Style {
    let mut fg: Option<Color> = None;
    let mut bold = false;
    let mut underlined = false;

    let codes: Vec<u16> = raw
        .split(';')
        .filter(|segment| !segment.is_empty())
        .filter_map(|segment| segment.parse::<u16>().ok())
        .collect();

    let mut idx = 0usize;
    while idx < codes.len() {
        let code = codes[idx];
        match code {
            0 => {
                fg = None;
                bold = false;
                underlined = false;
            },
            1 => bold = true,
            4 => underlined = true,
            22 => bold = false,
            24 => underlined = false,
            30..=37 => fg = Some(ansi_basic_color(code)),
            90..=97 => fg = Some(ansi_bright_color(code)),
            39 => fg = None,
            38 => {
                if idx + 2 < codes.len() && codes[idx + 1] == 5 {
                    fg = Some(Color::Indexed(codes[idx + 2] as u8));
                    idx += 2;
                } else if idx + 4 < codes.len() && codes[idx + 1] == 2 {
                    fg = Some(Color::Rgb(
                        codes[idx + 2] as u8,
                        codes[idx + 3] as u8,
                        codes[idx + 4] as u8,
                    ));
                    idx += 4;
                }
            },
            _ => {},
        }
        idx += 1;
    }

    let mut style = Style::default();
    if let Some(color) = fg {
        style = style.fg(color);
    }
    if bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if underlined {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    style
}

fn ansi_basic_color(code: u16) -> Color {
    match code {
        30 => Color::Black,
        31 => Color::Red,
        32 => Color::Green,
        33 => Color::Yellow,
        34 => Color::Blue,
        35 => Color::Magenta,
        36 => Color::Cyan,
        37 => Color::White,
        _ => Color::White,
    }
}

fn ansi_bright_color(code: u16) -> Color {
    match code {
        90 => Color::DarkGray,
        91 => Color::LightRed,
        92 => Color::LightGreen,
        93 => Color::LightYellow,
        94 => Color::LightBlue,
        95 => Color::LightMagenta,
        96 => Color::LightCyan,
        97 => Color::White,
        _ => Color::White,
    }
}
