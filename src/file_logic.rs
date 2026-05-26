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

pub(crate) fn preview_content_for_selected_entry(entry: Option<&NavEntry>, depth: usize) -> (PreviewMode, String) {
    let Some(selected) = entry else {
        return (PreviewMode::Empty, String::new());
    };
    if !selected.is_dir {
        return (PreviewMode::Empty, String::new());
    }
    let lines = preview_directory_tree_lines(&selected.path, depth.max(1));
    (PreviewMode::DirectoryTree, lines.join("\n"))
}

#[cfg(test)]
pub(crate) fn preview_file_preview_text(path: &Path) -> String {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => return format!("error: {err}"),
    };
    if bytes.is_empty() {
        return "(empty file)".to_string();
    }
    if bytes.contains(&0) {
        return format!("binary file ({} bytes)", bytes.len());
    }
    let limit = PREVIEW_FILE_MAX_BYTES.min(bytes.len());
    let mut text = String::from_utf8_lossy(&bytes[..limit]).into_owned();
    if bytes.len() > limit {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str("... truncated ...");
    }
    text
}

pub(crate) fn navigation_file_command_action(
    selected_entry: Option<&NavEntry>,
    key: char,
    config: &ConfigState,
    editor_program: &str,
    identity: &EffectiveIdentity,
) -> Option<NavigationFileCommandAction> {
    let entry = selected_entry?;
    if entry.is_dir {
        return None;
    }
    let trigger = key.to_ascii_lowercase();
    let command = available_preview_file_commands(entry, config, editor_program, identity)
        .into_iter()
        .find_map(|(shortcut, command)| (shortcut == trigger).then_some(command))?;
    match trigger {
        'r' => Some(NavigationFileCommandAction::RunReadInPreview(command)),
        'w' => Some(NavigationFileCommandAction::RunWriteInPreview(command)),
        'x' => Some(NavigationFileCommandAction::PrefillShell(command)),
        _ => None,
    }
}

pub(crate) fn available_preview_file_commands(
    entry: &NavEntry,
    config: &ConfigState,
    editor_program: &str,
    identity: &EffectiveIdentity,
) -> Vec<(char, String)> {
    if entry.is_dir {
        return Vec::new();
    }
    let extension = Path::new(&entry.name)
        .extension()
        .and_then(|value| value.to_str())
        .map(normalize_extension);
    let matched_rule = extension.as_deref().and_then(|ext| {
        config
            .extension_rules
            .iter()
            .find(|rule| normalize_extension(&rule.extension) == ext)
    });
    let fallback_rule = default_extension_rule("fallback");
    let rule = matched_rule.unwrap_or(&fallback_rule);

    let access = effective_access_for_entry(entry, identity);
    let mut out = Vec::new();
    if command_enabled_for_file(&rule.read_cmd) && access.read {
        out.push((
            'r',
            resolve_preview_command_template(&rule.read_cmd, &entry.name, editor_program),
        ));
    }
    if command_enabled_for_file(&rule.write_cmd) && access.write {
        out.push((
            'w',
            resolve_preview_command_template(&rule.write_cmd, &entry.name, editor_program),
        ));
    }
    if command_enabled_for_file(&rule.exec_cmd) && access.exec {
        out.push((
            'x',
            resolve_preview_command_template(&rule.exec_cmd, &entry.name, editor_program),
        ));
    }
    out
}

fn command_enabled_for_file(template: &str) -> bool {
    let trimmed = template.trim();
    !trimmed.is_empty() && trimmed != "--"
}

pub(crate) fn resolve_preview_command_template(
    template: &str,
    file_name: &str,
    editor_program: &str,
) -> String {
    let escaped_file = shell_single_quote(file_name);
    let raw_marker = "\u{0}NAVIX_FILE_RAW\u{0}";
    template
        .replace("$EDITOR", editor_program)
        .replace("{file_raw}", raw_marker)
        // Preserve common user forms like "{file}" and '{file}' while keeping shell-safe output.
        .replace("\"{file}\"", &escaped_file)
        .replace("'{file}'", &escaped_file)
        .replace("{file}", &escaped_file)
        .replace(raw_marker, file_name)
}

pub(crate) fn clamp_preview_depth(depth: usize, max_depth: usize) -> usize {
    depth.max(1).min(max_depth.max(1))
}

pub(crate) fn preview_directory_entries(path: &Path) -> io::Result<Vec<NavEntry>> {
    let mut entries = navigation_entries(path)?;
    entries.retain(|entry| entry.name != "..");
    Ok(entries)
}

pub(crate) fn preview_directory_tree_lines(root: &Path, depth: usize) -> Vec<String> {
    let mut lines = vec![format!("{}", root.display())];
    append_preview_directory_level(root, "", clamp_preview_depth(depth, depth.max(1)), &mut lines);
    lines
}

fn append_preview_directory_level(
    path: &Path,
    prefix: &str,
    remaining_depth: usize,
    lines: &mut Vec<String>,
) {
    if remaining_depth == 0 {
        return;
    }
    let entries = match preview_directory_entries(path) {
        Ok(entries) => entries,
        Err(err) => {
            lines.push(format!("{prefix}└── error: {err}"));
            return;
        }
    };
    if entries.is_empty() {
        lines.push(format!("{prefix}└── (empty)"));
        return;
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
        lines.push(format!("{prefix}{connector} {perms} {icon} {name}"));
        if entry.is_dir && remaining_depth > 1 {
            let child_prefix = if is_last {
                format!("{prefix}    ")
            } else {
                format!("{prefix}│   ")
            };
            append_preview_directory_level(&entry.path, &child_prefix, remaining_depth - 1, lines);
        }
    }
}

pub(crate) fn simple_permission_bits(file_type_char: char, mode: u32) -> String {
    let type_char = match file_type_char {
        'd' => 'd',
        'l' => 'l',
        _ => '-',
    };
    let read = if mode & 0o444 != 0 { 'r' } else { '-' };
    let write = if mode & 0o222 != 0 { 'w' } else { '-' };
    let exec = if mode & 0o111 != 0 { 'x' } else { '-' };
    format!("{type_char}{read}{write}{exec}")
}

fn effective_access_for_entry(entry: &NavEntry, identity: &EffectiveIdentity) -> EffectiveAccess {
    if let Some(access) = kernel_effective_access_for_path(&entry.path) {
        access
    } else {
        effective_access_from_mode(entry.mode, entry.uid, entry.gid, entry.file_type_char, identity)
    }
}

pub(crate) fn kernel_effective_access_for_path(path: &Path) -> Option<EffectiveAccess> {
    Some(EffectiveAccess {
        read: syscall_path_access(path, libc::R_OK).ok()?,
        write: syscall_path_access(path, libc::W_OK).ok()?,
        exec: syscall_path_access(path, libc::X_OK).ok()?,
    })
}

fn syscall_path_access(path: &Path, mode: i32) -> io::Result<bool> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
    let rc = unsafe { libc::faccessat(libc::AT_FDCWD, c_path.as_ptr(), mode, libc::AT_EACCESS) };
    if rc == 0 {
        return Ok(true);
    }

    let err = io::Error::last_os_error();
    match err.raw_os_error() {
        Some(code)
            if code == libc::EACCES
                || code == libc::EPERM
                || code == libc::ENOENT
                || code == libc::ENOTDIR
                || code == libc::ELOOP =>
        {
            Ok(false)
        }
        _ => Err(err),
    }
}

pub(crate) fn effective_access_from_mode(
    mode: u32,
    owner_uid: u32,
    owner_gid: u32,
    file_type_char: char,
    identity: &EffectiveIdentity,
) -> EffectiveAccess {
    if identity.euid == 0 {
        let exec = file_type_char == 'd' || mode & 0o111 != 0;
        return EffectiveAccess {
            read: true,
            write: true,
            exec,
        };
    }

    let (read_bit, write_bit, exec_bit) = if identity.euid == owner_uid {
        (0o400, 0o200, 0o100)
    } else if identity.in_group(owner_gid) {
        (0o040, 0o020, 0o010)
    } else {
        (0o004, 0o002, 0o001)
    };

    EffectiveAccess {
        read: mode & read_bit != 0,
        write: mode & write_bit != 0,
        exec: mode & exec_bit != 0,
    }
}

pub(crate) fn nav_long_listing(entry: &NavEntry) -> String {
    let perms = permission_bits(entry.file_type_char, entry.mode);
    let owner = username_for_uid(entry.uid).unwrap_or_else(|| entry.uid.to_string());
    let group = group_name_for_gid(entry.gid).unwrap_or_else(|| entry.gid.to_string());
    let when = format_epoch_short(entry.mtime).unwrap_or_else(|| "-".to_string());
    format!(
        "{perms}  {} {} {}  {} {}",
        entry.nlink, owner, group, entry.size, when
    )
}

fn username_for_uid(uid: u32) -> Option<String> {
    let content = fs::read_to_string("/etc/passwd").ok()?;
    for line in content.lines() {
        let mut fields = line.split(':');
        let name = fields.next()?;
        let _password = fields.next()?;
        let uid_field = fields.next()?;
        if uid_field.parse::<u32>().ok()? == uid {
            return Some(name.to_string());
        }
    }
    None
}

fn group_name_for_gid(gid: u32) -> Option<String> {
    let content = fs::read_to_string("/etc/group").ok()?;
    for line in content.lines() {
        let mut fields = line.split(':');
        let name = fields.next()?;
        let _password = fields.next()?;
        let gid_field = fields.next()?;
        if gid_field.parse::<u32>().ok()? == gid {
            return Some(name.to_string());
        }
    }
    None
}

fn format_epoch_short(epoch_secs: i64) -> Option<String> {
    if epoch_secs < 0 {
        return None;
    }
    let output = Command::new("date")
        .arg("-d")
        .arg(format!("@{epoch_secs}"))
        .arg("+%b %d %H:%M")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_string())
}

pub(crate) fn permission_bits(file_type_char: char, mode: u32) -> String {
    let mut out = String::with_capacity(10);
    out.push(file_type_char);
    let triplets = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];
    for (bit, ch) in triplets {
        out.push(if mode & bit != 0 { ch } else { '-' });
    }
    out
}
