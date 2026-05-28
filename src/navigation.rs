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
pub(crate) struct NavEntry {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) is_dir: bool,
    pub(crate) is_symlink: bool,
    pub(crate) file_type_char: char,
    pub(crate) mode: u32,
    pub(crate) nlink: u64,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) size: u64,
    pub(crate) mtime: i64,
}

pub(crate) fn navigation_entries(cwd: &Path) -> io::Result<Vec<NavEntry>> {
    let read_dir = fs::read_dir(cwd)?;
    let mut entries: Vec<NavEntry> = read_dir
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).ok();
            let is_dir = metadata
                .as_ref()
                .map(|m| m.file_type().is_dir())
                .unwrap_or(false);
            let is_symlink = metadata
                .as_ref()
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            let file_type_char = metadata
                .as_ref()
                .map(|m| {
                    let ft = m.file_type();
                    if ft.is_dir() {
                        'd'
                    } else if ft.is_symlink() {
                        'l'
                    } else {
                        '-'
                    }
                })
                .unwrap_or('?');
            let mode = metadata
                .as_ref()
                .map(|m| m.permissions().mode())
                .unwrap_or(0);
            let nlink = metadata.as_ref().map(|m| m.nlink()).unwrap_or(0);
            let uid = metadata.as_ref().map(|m| m.uid()).unwrap_or(0);
            let gid = metadata.as_ref().map(|m| m.gid()).unwrap_or(0);
            let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            let mtime = metadata.as_ref().map(|m| m.mtime()).unwrap_or(0);
            NavEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                path,
                is_dir,
                is_symlink,
                file_type_char,
                mode,
                nlink,
                uid,
                gid,
                size,
                mtime,
            }
        })
        .collect();
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    let parent = cwd.parent().unwrap_or(cwd).to_path_buf();
    let parent_meta = fs::symlink_metadata(&parent).ok();
    let parent_mode = parent_meta
        .as_ref()
        .map(|meta| meta.permissions().mode())
        .unwrap_or(0o755);
    entries.insert(
        0,
        NavEntry {
            name: "..".to_string(),
            path: parent,
            is_dir: true,
            is_symlink: false,
            file_type_char: 'd',
            mode: parent_mode,
            nlink: parent_meta.as_ref().map(|meta| meta.nlink()).unwrap_or(0),
            uid: parent_meta.as_ref().map(|meta| meta.uid()).unwrap_or(0),
            gid: parent_meta.as_ref().map(|meta| meta.gid()).unwrap_or(0),
            size: parent_meta.as_ref().map(|meta| meta.len()).unwrap_or(0),
            mtime: parent_meta.as_ref().map(|meta| meta.mtime()).unwrap_or(0),
        },
    );
    Ok(entries)
}

pub(crate) fn clamp_nav_selection(selected: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        selected.min(len.saturating_sub(1))
    }
}

pub(crate) fn nav_select_prev_wrap(selected: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let selected = clamp_nav_selection(selected, len);
    if selected == 0 { len - 1 } else { selected - 1 }
}

pub(crate) fn nav_select_next_wrap(selected: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let selected = clamp_nav_selection(selected, len);
    if selected + 1 >= len { 0 } else { selected + 1 }
}

pub(crate) fn nav_select_home(_selected: usize, _len: usize) -> usize {
    0
}

pub(crate) fn nav_select_end(_selected: usize, len: usize) -> usize {
    len.saturating_sub(1)
}

pub(crate) fn nav_filter_matches(name: &str, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    if filter.chars().any(|ch| ch.is_ascii_uppercase()) {
        name.contains(filter)
    } else {
        name.to_ascii_lowercase()
            .contains(&filter.to_ascii_lowercase())
    }
}

pub(crate) fn nav_selection_after_filter(
    entries: &[NavEntry],
    selected: usize,
    filter: &str,
) -> usize {
    let selected = clamp_nav_selection(selected, entries.len());
    if filter.is_empty() {
        return selected;
    }
    if entries
        .get(selected)
        .is_some_and(|entry| entry.name == "..")
        && let Some(first_real_entry) = entries.iter().position(|entry| entry.name != "..")
    {
        return first_real_entry;
    }
    selected
}

pub(crate) fn nav_window_metrics(
    total_entries: usize,
    viewport_rows: usize,
    scroll_offset: usize,
) -> (usize, usize, usize, usize, usize, bool, usize) {
    if total_entries == 0 || viewport_rows == 0 {
        return (0, 0, 0, 0, total_entries, false, 0);
    }
    let viewport = viewport_rows.max(1);
    let total = total_entries;
    let scroll = scroll_offset.min(nav_max_scroll(total, viewport));
    let start = scroll;
    let end = start.saturating_add(viewport).min(total);
    let shown_start = if total == 0 { 0 } else { start + 1 };
    let shown_end = if total == 0 { 0 } else { end };
    (
        start,
        end,
        shown_start,
        shown_end,
        total,
        should_show_scrollbar(total, viewport),
        scroll,
    )
}

pub(crate) fn nav_max_scroll(total_entries: usize, viewport_rows: usize) -> usize {
    total_entries.saturating_sub(viewport_rows.max(1))
}

pub(crate) fn nav_scroll_for_selection(
    current_scroll: usize,
    selected: usize,
    total_entries: usize,
    viewport_rows: usize,
) -> usize {
    if total_entries == 0 || viewport_rows == 0 {
        return 0;
    }
    let viewport = viewport_rows.max(1);
    let selected = clamp_nav_selection(selected, total_entries);
    let mut scroll = current_scroll.min(nav_max_scroll(total_entries, viewport));
    if selected < scroll {
        scroll = selected;
    } else if selected >= scroll.saturating_add(viewport) {
        scroll = selected.saturating_add(1).saturating_sub(viewport);
    }
    scroll.min(nav_max_scroll(total_entries, viewport))
}

pub(crate) fn navigation_panel_text(
    cwd: &Path,
    entries: &[NavEntry],
    selected: usize,
    window_start: usize,
    window_end: usize,
    colors: &LsColorsTheme,
    fullish_shell_theme: bool,
    load_error: Option<&str>,
) -> Text<'static> {
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        format!("{}", cwd.display()),
        nav_style_for_theme(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            fullish_shell_theme,
        ),
    )));

    if let Some(err) = load_error {
        lines.push(Line::from(Span::styled(
            format!("└── error: {err}"),
            nav_style_for_theme(Style::default().fg(Color::Red), fullish_shell_theme),
        )));
        return Text::from(lines);
    }

    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "└── (empty)",
            nav_style_for_theme(Style::default().fg(Color::DarkGray), fullish_shell_theme),
        )));
        return Text::from(lines);
    }

    let total = entries.len();
    let selected = clamp_nav_selection(selected, entries.len());
    let end = window_end.min(total);
    let start = window_start.min(end);
    for idx in start..end {
        let entry = &entries[idx];
        let is_last = idx + 1 == total;
        let connector = if is_last { "└──" } else { "├──" };
        let is_selected = idx == selected;
        let perms = simple_permission_bits(entry.file_type_char, entry.mode);
        let mut name = entry.name.clone();
        if entry.is_dir {
            name.push('/');
        }
        let name_style = navigation_name_style(
            colors,
            &entry.name,
            entry.is_dir,
            entry.is_symlink,
            entry.mode,
        );
        let icon = if entry.is_dir { "" } else { "" };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{connector} "),
                nav_row_selected_style(
                    nav_style_for_theme(Style::default().fg(Color::DarkGray), fullish_shell_theme),
                    is_selected,
                ),
            ),
            Span::styled(
                format!("{perms} "),
                nav_row_selected_style(
                    nav_style_for_theme(Style::default().fg(Color::White), fullish_shell_theme),
                    is_selected,
                ),
            ),
            Span::styled(
                format!("{icon} "),
                nav_row_selected_style(
                    nav_style_for_theme(name_style, fullish_shell_theme),
                    is_selected,
                ),
            ),
            Span::styled(
                name,
                nav_row_selected_style(
                    nav_style_for_theme(name_style, fullish_shell_theme),
                    is_selected,
                ),
            ),
        ]));
    }

    Text::from(lines)
}

#[cfg(test)]
pub(crate) fn navigation_tree_lines(cwd: &Path) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("{}", cwd.display()));

    let entries = match navigation_entries(cwd) {
        Ok(entries) => entries,
        Err(err) => {
            lines.push(format!("└── error: {err}"));
            return lines;
        },
    };

    if entries.is_empty() {
        lines.push("└── (empty)".to_string());
        return lines;
    }

    let total = entries.len();
    for (idx, entry) in entries.iter().enumerate() {
        let is_last = idx + 1 == total;
        let connector = if is_last { "└──" } else { "├──" };
        let perms = simple_permission_bits(entry.file_type_char, entry.mode);
        let mut name = entry.name.clone();
        if entry.is_dir {
            name.push('/');
        }
        let icon = if entry.is_dir { "" } else { "" };
        lines.push(format!("{connector} {perms} {icon} {name}"));
    }

    lines
}
