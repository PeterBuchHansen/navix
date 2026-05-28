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
fn preview_command_template_resolves_editor_and_filename() {
    let resolved = resolve_preview_command_template("$EDITOR {file}", "README.md", "nvim");
    assert_eq!(resolved, "nvim 'README.md'");
}

#[test]
fn preview_command_template_supports_raw_placeholder() {
    let resolved = resolve_preview_command_template(
        "tool --raw {file_raw} --safe {file}",
        "my file.txt",
        "nvim",
    );
    assert_eq!(resolved, "tool --raw my file.txt --safe 'my file.txt'");
}

#[test]
fn preview_command_template_keeps_raw_placeholder_when_filename_contains_file_token() {
    let resolved =
        resolve_preview_command_template("tool --raw {file_raw} --safe {file}", "x{file}y", "nvim");
    assert_eq!(resolved, "tool --raw x{file}y --safe 'x{file}y'");
}

#[test]
fn preview_command_template_handles_quoted_file_placeholder_and_single_quote_filename() {
    let resolved =
        resolve_preview_command_template("bat \"{file}\" && echo '{file}'", "o'hara.md", "nvim");
    assert_eq!(resolved, "bat 'o'\\''hara.md' && echo 'o'\\''hara.md'");
}

#[test]
fn available_preview_file_commands_respects_permission_bits_and_rule_commands() {
    let config = ConfigState::default();
    let identity = test_identity(1000, 1000, &[1000]);
    let base = unique_temp_path("navix-preview-cmds");
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

    let commands = available_preview_file_commands(&entry, &config, "nvim", &identity);
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0], ('r', "bat 'README.md'".to_string()));
    assert_eq!(commands[1], ('w', "nvim 'README.md'".to_string()));

    fs::remove_dir_all(base).expect("cleanup temp");
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
    assert_eq!(commands[0], ('r', "bat 'editor.html'".to_string()));
    assert_eq!(commands[1], ('w', "nvim 'editor.html'".to_string()));

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
            "bat 'README.md'".to_string()
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
            "nvim 'README.md'".to_string()
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
            "bash 'script.sh'".to_string()
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
