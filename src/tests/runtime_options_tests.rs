use super::*;

#[test]
fn parse_runtime_options_defaults_to_navigation_focus() {
    let options = parse_runtime_options_from_args(Vec::<String>::new()).expect("options");
    assert!(options.navix_mouse_capture);
    assert_eq!(options.startup_focus, ActivePane::Navigation);
    assert_eq!(options.startup_path, None);
}

#[test]
fn parse_runtime_options_supports_focus_flags() {
    let shell_options = parse_runtime_options_from_args(vec!["--shell"]).expect("shell");
    assert_eq!(shell_options.startup_focus, ActivePane::Shell);

    let preview_options = parse_runtime_options_from_args(vec!["--preview"]).expect("preview");
    assert_eq!(preview_options.startup_focus, ActivePane::Preview);

    let nav_options = parse_runtime_options_from_args(vec!["--navigation"]).expect("navigation");
    assert_eq!(nav_options.startup_focus, ActivePane::Navigation);
}

#[test]
fn parse_runtime_options_rejects_conflicting_focus_flags() {
    let error =
        parse_runtime_options_from_args(vec!["--shell", "--preview"]).expect_err("conflict");
    assert!(error.contains("Conflicting focus options"));
    assert!(error.contains("--shell"));
    assert!(error.contains("--preview"));
}

#[test]
fn parse_runtime_options_supports_path_with_space_or_equals() {
    let split =
        parse_runtime_options_from_args(vec!["--path", "./docs/../README.md"]).expect("split");
    assert_eq!(split.startup_path.as_deref(), Some("./docs/../README.md"));

    let equals = parse_runtime_options_from_args(vec!["--path=./src/main.rs"]).expect("equals");
    assert_eq!(equals.startup_path.as_deref(), Some("./src/main.rs"));
}

#[test]
fn parse_runtime_options_rejects_empty_path_value() {
    let missing = parse_runtime_options_from_args(vec!["--path"]).expect_err("missing");
    assert!(missing.contains("Missing value for --path"));

    let empty = parse_runtime_options_from_args(vec!["--path="]).expect_err("empty");
    assert!(empty.contains("Missing value for --path"));
}

#[test]
fn parse_runtime_options_removes_dot_alias_for_mouse_capture() {
    let options = parse_runtime_options_from_args(vec!["--no-mouse-capture"]).expect("valid");
    assert!(!options.navix_mouse_capture);

    let alias_error =
        parse_runtime_options_from_args(vec!["--no.mouse-capture"]).expect_err("alias removed");
    assert!(alias_error.contains("Unknown option: --no.mouse-capture"));
}

#[test]
fn resolve_startup_path_option_uses_directory_as_startup_cwd() {
    let dir = unique_temp_path("runtime-options-startup-dir");
    fs::create_dir_all(&dir).expect("create dir");
    let base = std::env::temp_dir();

    let resolved = resolve_startup_path_option(dir.to_string_lossy().as_ref(), &base)
        .expect("resolve startup dir");

    assert_eq!(resolved.startup_cwd, dir);
    assert_eq!(resolved.preferred_file, None);
    assert_eq!(resolved.history_target, resolved.startup_cwd);

    let _ = fs::remove_dir_all(&resolved.startup_cwd);
}

#[test]
fn resolve_startup_path_option_preselects_file_and_uses_parent_cwd() {
    let root = unique_temp_path("runtime-options-startup-file");
    fs::create_dir_all(&root).expect("create root");
    let file = root.join("note.txt");
    fs::write(&file, "hello").expect("write file");

    let resolved = resolve_startup_path_option(file.to_string_lossy().as_ref(), &root)
        .expect("resolve startup file");

    assert_eq!(resolved.startup_cwd, root);
    assert_eq!(resolved.preferred_file.as_deref(), Some(file.as_path()));
    assert_eq!(resolved.history_target, file);

    let _ = fs::remove_file(&resolved.history_target);
    let _ = fs::remove_dir_all(&resolved.startup_cwd);
}

#[test]
fn resolve_startup_path_option_rejects_missing_path() {
    let missing = unique_temp_path("runtime-options-startup-missing");
    let base = std::env::temp_dir();

    let error = resolve_startup_path_option(missing.to_string_lossy().as_ref(), &base)
        .expect_err("missing path should fail");

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("path not found"));
}
