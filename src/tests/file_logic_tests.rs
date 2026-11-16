use super::*;

#[test]
fn permission_bits_renders_unix_style_triplets() {
    assert_eq!(permission_bits('d', 0o755), "drwxr-xr-x");
    assert_eq!(permission_bits('-', 0o644), "-rw-r--r--");
}

#[test]
fn simple_permission_bits_renders_compact_mode() {
    assert_eq!(simple_permission_bits('d', 0o755), "drwx");
    assert_eq!(simple_permission_bits('-', 0o640), "-rw-");
    assert_eq!(simple_permission_bits('l', 0o777), "lrwx");
}

#[test]
fn preview_content_for_directory_selection_is_non_empty() {
    let base = unique_temp_path("navix-preview-dir");
    fs::create_dir_all(base.join("sub/inner")).expect("create dirs");
    fs::write(base.join("sub/file.md"), b"hello").expect("create file");
    fs::write(base.join("sub/inner/deep.txt"), b"deep").expect("create nested file");

    let entry = NavEntry {
        name: "sub".to_string(),
        path: base.join("sub"),
        is_dir: true,
        is_symlink: false,
        file_type_char: 'd',
        mode: 0o755,
        nlink: 1,
        uid: 0,
        gid: 0,
        size: 0,
        mtime: 0,
    };

    let (mode, text) = preview_content_for_selected_entry(Some(&entry), 2);
    assert_eq!(mode, PreviewMode::DirectoryTree);
    assert!(!text.trim().is_empty());
    assert!(text.contains(&format!("{}", base.join("sub").display())));
    assert!(text.contains("inner/"));
    assert!(text.contains("deep.txt"));

    fs::remove_dir_all(base).expect("cleanup temp");
}

#[test]
fn preview_content_for_file_selection_clears_panel() {
    let entry = NavEntry {
        name: "file.md".to_string(),
        path: PathBuf::from("/tmp/file.md"),
        is_dir: false,
        is_symlink: false,
        file_type_char: '-',
        mode: 0o644,
        nlink: 1,
        uid: 0,
        gid: 0,
        size: 0,
        mtime: 0,
    };

    let (mode, text) = preview_content_for_selected_entry(Some(&entry), 2);
    assert_eq!(mode, PreviewMode::Empty);
    assert!(text.is_empty());
}

#[test]
fn preview_file_preview_text_renders_plain_text_files() {
    let base = unique_temp_path("navix-preview-file");
    fs::create_dir_all(&base).expect("create base");
    let path = base.join("notes.txt");
    fs::write(&path, b"hello\npreview\npanel\n").expect("write file");

    let rendered = preview_file_preview_text(&path);
    assert!(rendered.contains("hello"));
    assert!(rendered.contains("preview"));

    fs::remove_dir_all(base).expect("cleanup temp");
}

#[test]
fn preview_file_preview_text_marks_binary_files() {
    let base = unique_temp_path("navix-preview-binary");
    fs::create_dir_all(&base).expect("create base");
    let path = base.join("blob.bin");
    fs::write(&path, [0_u8, 159, 146, 150]).expect("write file");

    let rendered = preview_file_preview_text(&path);
    assert!(rendered.contains("binary file"));
    assert!(rendered.contains("4 bytes"));

    fs::remove_dir_all(base).expect("cleanup temp");
}

#[test]
fn preview_command_template_resolves_editor_and_plain_filename_unquoted() {
    let resolved = resolve_preview_command_template("$EDITOR {file}", "README.md", "nvim");
    assert_eq!(resolved, "nvim README.md");
}

#[test]
fn preview_command_template_escapes_per_quoting_context() {
    let cases = [
        ("bat {file}", "my cv.pdf", "bat my\\ cv.pdf"),
        ("bash {file}", "it's.sh", "bash it\\'s.sh"),
        ("./{file}", "script.sh", "./script.sh"),
        ("bat {file}", "$(id).md", "bat \\$\\(id\\).md"),
        ("bat {file}", "æøå.pdf", "bat æøå.pdf"),
        ("bat {file}", "~home", "bat \\~home"),
        ("bat {file}", "a=b~c", "bat a=b~c"),
        ("bat {file}", "=cmd", "bat \\=cmd"),
        (
            "smbclient -c \"print {file}\"",
            "cover-letter.da.pdf",
            "smbclient -c \"print cover-letter.da.pdf\"",
        ),
        (
            "smbclient -c \"print \\\"{file}\\\"\"",
            "my cv.pdf",
            "smbclient -c \"print \\\"my cv.pdf\\\"\"",
        ),
        (
            "echo \"{file}\"",
            "say \"$HOME\"",
            "echo \"say \\\"\\$HOME\\\"\"",
        ),
        ("sh -c 'cat {file}'", "it's.md", "sh -c 'cat it'\\''s.md'"),
        ("bat {file}", "", "bat ''"),
        ("bat {file}", "a\nb.md", "bat 'a\nb.md'"),
    ];
    for (template, name, expected) in cases {
        assert_eq!(
            resolve_preview_command_template(template, name, "nvim"),
            expected,
            "template {template:?} with name {name:?}"
        );
    }
}

#[test]
fn preview_command_template_handles_quoted_file_placeholder_and_single_quote_filename() {
    let resolved =
        resolve_preview_command_template("bat \"{file}\" && echo '{file}'", "o'hara.md", "nvim");
    assert_eq!(resolved, "bat \"o'hara.md\" && echo 'o'\\''hara.md'");
}

#[test]
fn preview_command_template_tracks_escaped_quotes() {
    // `\"` inside double quotes does not close the string, so `{file}` is still
    // in double-quote context; an escaped `\'` outside quotes opens nothing.
    assert_eq!(
        resolve_preview_command_template("x \"a\\\" {file}\"", "my cv", "nvim"),
        "x \"a\\\" my cv\""
    );
    assert_eq!(
        resolve_preview_command_template("x \\' {file}", "my cv", "nvim"),
        "x \\' my\\ cv"
    );
    // Unterminated quote: the open context applies to the rest of the template.
    assert_eq!(
        resolve_preview_command_template("x \"{file}", "my cv", "nvim"),
        "x \"my cv"
    );
}

#[test]
fn preview_command_template_editor_value_does_not_shift_quote_context() {
    assert_eq!(
        resolve_preview_command_template("$EDITOR {file}", "my cv.md", "code --wait \"x"),
        "code --wait \"x my\\ cv.md"
    );
}

#[test]
fn preview_command_template_does_not_rescan_inserted_file_name() {
    assert_eq!(
        resolve_preview_command_template("bat {file}", "{file}$EDITOR", "nvim"),
        "bat \\{file\\}\\$EDITOR"
    );
}

#[test]
fn preview_command_template_file_raw_is_alias_of_file() {
    let resolved = resolve_preview_command_template(
        "tool --raw {file_raw} --safe {file} -c \"x {file_raw}\"",
        "my file.txt",
        "nvim",
    );
    assert_eq!(
        resolved,
        "tool --raw my\\ file.txt --safe my\\ file.txt -c \"x my file.txt\""
    );
}

/// Names that stress every escaping rule.
const ROUND_TRIP_NAMES: &[&str] = &[
    "plain.md",
    "my cv.pdf",
    "it's.md",
    "say \"hi\".md",
    "back\\slash",
    "$(touch navix-pwned).md",
    "`touch navix-pwned`.md",
    "$HOME",
    "*?[ab].md",
    ";&|<>().md",
    "#hash",
    "!bang",
    "~tilde",
    "=eq",
    "a=b~c",
    "x{file}y$EDITOR",
    "100%",
    "a^b",
    "æøå.pdf",
    "",
    "a\nb.md",
    "tab\there",
    "-dash.md",
];

/// Templates covering every quoting context, plus a mixed one.
const ROUND_TRIP_TEMPLATES: &[&str] = &[
    "printf '%s' {file}",
    "printf '%s' \"{file}\"",
    "printf '%s' '{file}'",
    "printf '%s' \"pre {file} post\" | sed -e 's/^pre //' -e 's/ post$//'",
];

fn available_round_trip_shells() -> Vec<Vec<&'static str>> {
    let candidates: [&[&'static str]; 4] = [
        &["/bin/sh", "-c"],
        &["dash", "-c"],
        &["bash", "--norc", "--noprofile", "-c"],
        &["zsh", "-f", "-c"],
    ];
    candidates
        .into_iter()
        .filter(|argv| {
            std::process::Command::new(argv[0])
                .args(&argv[1..])
                .arg("true")
                .output()
                .is_ok_and(|output| output.status.success())
        })
        .map(<[&str]>::to_vec)
        .collect()
}

#[test]
fn expanded_file_names_round_trip_through_real_shells() {
    let shells = available_round_trip_shells();
    assert!(!shells.is_empty(), "no POSIX shell available");
    let workdir = unique_temp_path("navix-file-round-trip");
    fs::create_dir_all(&workdir).expect("create workdir");
    for shell in &shells {
        for template in ROUND_TRIP_TEMPLATES {
            for name in ROUND_TRIP_NAMES {
                // The mixed template uses sed line-wise; skip names with newlines there.
                if template.contains("sed") && name.contains('\n') {
                    continue;
                }
                let command = resolve_preview_command_template(template, name, "nvim");
                let output = std::process::Command::new(shell[0])
                    .args(&shell[1..])
                    .arg(&command)
                    .current_dir(&workdir)
                    .output()
                    .expect("run shell");
                assert!(
                    output.status.success(),
                    "{shell:?} failed for {name:?}: {command:?}\n{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let mut printed = output.stdout;
                if template.contains("sed") && printed.last() == Some(&b'\n') {
                    printed.pop();
                }
                assert_eq!(
                    String::from_utf8_lossy(&printed),
                    *name,
                    "{shell:?} mangled {name:?} via {command:?}"
                );
            }
        }
    }
    assert!(
        !workdir.join("navix-pwned").exists(),
        "a file name executed a command"
    );
    fs::remove_dir_all(workdir).expect("cleanup workdir");
}

#[test]
fn encode_file_name_for_context_matches_rules() {
    assert_eq!(
        encode_file_name_for_context("a b'c\"$d", TemplateQuoteContext::Unquoted),
        "a\\ b\\'c\\\"\\$d"
    );
    assert_eq!(
        encode_file_name_for_context("a b'c\"$d", TemplateQuoteContext::Double),
        "a b'c\\\"\\$d"
    );
    assert_eq!(
        encode_file_name_for_context("a b'c\"$d", TemplateQuoteContext::Single),
        "a b'\\''c\"$d"
    );
}

#[test]
fn available_preview_file_commands_fallback_for_unknown_extension() {
    let config = ConfigState::default();
    let identity = test_identity(1000, 1000, &[1000]);
    let base = unique_temp_path("navix-preview-cmds-fallback");
    fs::create_dir_all(&base).expect("create base");
    let path = base.join("editor.html");
    fs::write(&path, b"<h1>test</h1>").expect("write file");
    fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("chmod file");
    let entry = NavEntry {
        name: "editor.html".to_string(),
        path: path.clone(),
        is_dir: false,
        is_symlink: false,
        file_type_char: '-',
        mode: 0o640,
        nlink: 1,
        uid: 1000,
        gid: 1000,
        size: 0,
        mtime: 0,
    };

    let commands = available_preview_file_commands(&entry, &config, "nvim", &identity);
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0], ('r', "bat editor.html".to_string()));
    assert_eq!(commands[1], ('w', "nvim editor.html".to_string()));

    fs::remove_dir_all(base).expect("cleanup temp");
}

#[test]
fn navigation_file_command_action_runs_read_in_preview() {
    let config = ConfigState::default();
    let identity = test_identity(1000, 1000, &[1000]);
    let base = unique_temp_path("navix-nav-read-action");
    fs::create_dir_all(&base).expect("create base");
    let path = base.join("README.md");
    fs::write(&path, b"# test").expect("write file");
    fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("chmod file");
    let entry = NavEntry {
        name: "README.md".to_string(),
        path: path.clone(),
        is_dir: false,
        is_symlink: false,
        file_type_char: '-',
        mode: 0o640,
        nlink: 1,
        uid: 1000,
        gid: 1000,
        size: 0,
        mtime: 0,
    };

    let action = navigation_file_command_action(Some(&entry), 'r', &config, "nvim", &identity);
    assert_eq!(
        action,
        Some(NavigationFileCommandAction::RunReadInPreview(
            "bat README.md".to_string()
        ))
    );

    fs::remove_dir_all(base).expect("cleanup temp");
}

#[test]
fn navigation_file_command_action_runs_write_in_preview() {
    let config = ConfigState::default();
    let identity = test_identity(1000, 1000, &[1000]);
    let base = unique_temp_path("navix-nav-write-action");
    fs::create_dir_all(&base).expect("create base");
    let path = base.join("README.md");
    fs::write(&path, b"# test").expect("write file");
    fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("chmod file");
    let entry = NavEntry {
        name: "README.md".to_string(),
        path: path.clone(),
        is_dir: false,
        is_symlink: false,
        file_type_char: '-',
        mode: 0o640,
        nlink: 1,
        uid: 1000,
        gid: 1000,
        size: 0,
        mtime: 0,
    };

    let action = navigation_file_command_action(Some(&entry), 'w', &config, "nvim", &identity);
    assert_eq!(
        action,
        Some(NavigationFileCommandAction::RunWriteInPreview(
            "nvim README.md".to_string()
        ))
    );

    fs::remove_dir_all(base).expect("cleanup temp");
}

#[test]
fn navigation_file_command_action_prefills_exec_in_shell() {
    let config = ConfigState::default();
    let identity = test_identity(1000, 1000, &[1000]);
    let base = unique_temp_path("navix-nav-exec-action");
    fs::create_dir_all(&base).expect("create base");
    let path = base.join("script.sh");
    fs::write(&path, b"#!/bin/bash\necho hi\n").expect("write file");
    fs::set_permissions(&path, std::fs::Permissions::from_mode(0o750)).expect("chmod file");
    let entry = NavEntry {
        name: "script.sh".to_string(),
        path: path.clone(),
        is_dir: false,
        is_symlink: false,
        file_type_char: '-',
        mode: 0o750,
        nlink: 1,
        uid: 1000,
        gid: 1000,
        size: 0,
        mtime: 0,
    };

    let action = navigation_file_command_action(Some(&entry), 'x', &config, "nvim", &identity);
    assert_eq!(
        action,
        Some(NavigationFileCommandAction::PrefillShell(
            "bash script.sh".to_string()
        ))
    );

    fs::remove_dir_all(base).expect("cleanup temp");
}

#[test]
fn navigation_file_command_action_follows_effective_read_access() {
    let config = ConfigState::default();
    let identity = test_identity(1000, 1000, &[1000]);
    let base = unique_temp_path("navix-nav-read-no-perm");
    fs::create_dir_all(&base).expect("create base");
    let path = base.join("README.md");
    fs::write(&path, b"# test").expect("write file");
    fs::set_permissions(&path, std::fs::Permissions::from_mode(0o200)).expect("chmod file");
    let entry = NavEntry {
        name: "README.md".to_string(),
        path: path.clone(),
        is_dir: false,
        is_symlink: false,
        file_type_char: '-',
        mode: 0o200,
        nlink: 1,
        uid: 1000,
        gid: 1000,
        size: 0,
        mtime: 0,
    };

    let access = kernel_effective_access_for_path(&path).expect("kernel access");
    let action = navigation_file_command_action(Some(&entry), 'r', &config, "nvim", &identity);
    if access.read {
        assert!(matches!(
            action,
            Some(NavigationFileCommandAction::RunReadInPreview(_))
        ));
    } else {
        assert_eq!(action, None);
    }

    fs::remove_dir_all(base).expect("cleanup temp");
}

#[test]
fn navigation_file_command_action_disables_exec_without_exec_permission() {
    let config = ConfigState::default();
    let identity = test_identity(1000, 1000, &[1000]);
    let base = unique_temp_path("navix-nav-exec-no-perm");
    fs::create_dir_all(&base).expect("create base");
    let path = base.join("script.sh");
    fs::write(&path, b"#!/bin/bash\necho hi\n").expect("write file");
    fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("chmod file");
    let entry = NavEntry {
        name: "script.sh".to_string(),
        path: path.clone(),
        is_dir: false,
        is_symlink: false,
        file_type_char: '-',
        mode: 0o640,
        nlink: 1,
        uid: 1000,
        gid: 1000,
        size: 0,
        mtime: 0,
    };

    let action = navigation_file_command_action(Some(&entry), 'x', &config, "nvim", &identity);
    assert_eq!(action, None);

    fs::remove_dir_all(base).expect("cleanup temp");
}

fn md_entry(path: &Path, mode: u32) -> NavEntry {
    NavEntry {
        name: "notes.md".to_string(),
        path: path.to_path_buf(),
        is_dir: false,
        is_symlink: false,
        file_type_char: '-',
        mode,
        nlink: 1,
        uid: 1000,
        gid: 1000,
        size: 0,
        mtime: 0,
    }
}

fn config_with_md_mask(mask: &str) -> ConfigState {
    let mut config = ConfigState::default();
    let md_rule = config
        .extension_rules
        .iter_mut()
        .find(|rule| rule.extension == "md")
        .expect("default md rule");
    md_rule.exec_cmd = "xclip -selection clipboard < {file}".to_string();
    md_rule.command_mask = mask.to_string();
    config
}

#[test]
fn uppercase_x_mask_skips_exec_permission_check() {
    let identity = test_identity(1000, 1000, &[1000]);
    let base = unique_temp_path("navix-mask-upper-x");
    fs::create_dir_all(&base).expect("create base");
    let path = base.join("notes.md");
    fs::write(&path, b"# notes\n").expect("write file");
    fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod file");
    let entry = md_entry(&path, 0o644);

    let action = navigation_file_command_action(
        Some(&entry),
        'x',
        &config_with_md_mask("rwX"),
        "nvim",
        &identity,
    );
    assert_eq!(
        action,
        Some(NavigationFileCommandAction::PrefillShell(
            "xclip -selection clipboard < notes.md".to_string()
        ))
    );

    // Lowercase keeps the check: the same non-executable file stays disabled.
    let action = navigation_file_command_action(
        Some(&entry),
        'x',
        &config_with_md_mask("rwx"),
        "nvim",
        &identity,
    );
    assert_eq!(action, None);

    fs::remove_dir_all(base).expect("cleanup temp");
}

#[test]
fn dash_mask_hides_commands_from_preview_list_and_shortcuts() {
    let identity = test_identity(1000, 1000, &[1000]);
    let base = unique_temp_path("navix-mask-dash");
    fs::create_dir_all(&base).expect("create base");
    let path = base.join("notes.md");
    fs::write(&path, b"# notes\n").expect("write file");
    fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod file");
    let entry = md_entry(&path, 0o755);
    let config = config_with_md_mask("r-X");

    let entries = preview_file_command_entries(&entry, &config, "nvim", &identity);
    let shortcuts: Vec<char> = entries.iter().map(|(key, _, _)| *key).collect();
    assert_eq!(shortcuts, vec!['r', 'x']);
    // Hidden even though the file is writable.
    assert_eq!(
        navigation_file_command_action(Some(&entry), 'w', &config, "nvim", &identity),
        None
    );

    let all_hidden = config_with_md_mask("---");
    assert!(preview_file_command_entries(&entry, &all_hidden, "nvim", &identity).is_empty());

    fs::remove_dir_all(base).expect("cleanup temp");
}

#[test]
fn none_command_stays_listed_and_dimmed_when_not_masked() {
    let identity = test_identity(1000, 1000, &[1000]);
    let base = unique_temp_path("navix-mask-none");
    fs::create_dir_all(&base).expect("create base");
    let path = base.join("notes.md");
    fs::write(&path, b"# notes\n").expect("write file");
    fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod file");
    let entry = md_entry(&path, 0o755);
    let mut config = config_with_md_mask("rwX");
    config.extension_rules[0].exec_cmd = "None".to_string();

    let entries = preview_file_command_entries(&entry, &config, "nvim", &identity);
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[2], ('x', "None".to_string(), false));

    fs::remove_dir_all(base).expect("cleanup temp");
}

#[test]
fn preview_depth_clamps_within_bounds() {
    assert_eq!(clamp_preview_depth(0, 6), 1);
    assert_eq!(clamp_preview_depth(9, 6), 6);
    assert_eq!(clamp_preview_depth(4, 0), 1);
}

#[test]
fn preview_directory_tree_lines_show_error_when_root_unreadable() {
    let missing = unique_temp_path("navix-preview-missing");
    let lines = preview_directory_tree_lines(&missing, 2);
    let rendered = lines.join("\n");
    assert!(rendered.contains("error:"));
}

#[test]
fn nav_long_listing_contains_permission_and_size() {
    let entry = NavEntry {
        name: "tmp".to_string(),
        path: PathBuf::from("/tmp"),
        is_dir: true,
        is_symlink: false,
        file_type_char: 'd',
        mode: 0o755,
        nlink: 3,
        uid: 0,
        gid: 0,
        size: 4096,
        mtime: 0,
    };
    let listing = nav_long_listing(&entry);
    assert!(listing.starts_with("drwxr-xr-x"));
    assert!(listing.contains(" 4096 "));
}

#[test]
fn effective_access_prefers_owner_group_and_other_bits() {
    let owner_identity = test_identity(1001, 1001, &[1001]);
    let owner = effective_access_from_mode(0o640, 1001, 2000, '-', &owner_identity);
    assert!(owner.read);
    assert!(owner.write);
    assert!(!owner.exec);

    let group_identity = test_identity(3000, 2000, &[2000]);
    let group = effective_access_from_mode(0o640, 1001, 2000, '-', &group_identity);
    assert!(group.read);
    assert!(!group.write);
    assert!(!group.exec);

    let other_identity = test_identity(3000, 3000, &[3000]);
    let other = effective_access_from_mode(0o640, 1001, 2000, '-', &other_identity);
    assert!(!other.read);
    assert!(!other.write);
    assert!(!other.exec);
}

#[test]
fn kernel_effective_access_for_path_matches_owned_file_mode() {
    let base = unique_temp_path("navix-kernel-access");
    fs::create_dir_all(&base).expect("create base");
    let path = base.join("file.txt");
    fs::write(&path, b"hello").expect("write file");
    fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod file");

    let access = kernel_effective_access_for_path(&path).expect("kernel access");
    assert!(access.read);
    assert!(access.write);
    assert!(!access.exec);

    fs::remove_dir_all(base).expect("cleanup temp");
}
