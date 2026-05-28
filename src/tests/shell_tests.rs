use super::*;
use std::path::Path;

#[test]
fn prompt_segments_survive_carriage_return_redraws() {
    let mut parser = Parser::new(8, 120, 200);
    parser.process("%\r\x1b[32m   ~/repos/navix    main ?7 \x1b[0m".as_bytes());
    parser.process("\r\x1b[34m❯\x1b[0m ".as_bytes());

    let content = parser.screen().contents();
    assert!(content.contains("repos/navix"));
    assert!(content.contains("❯"));
}

#[test]
fn ls_output_lines_are_preserved() {
    let mut parser = Parser::new(20, 120, 400);
    parser.process(b"ls -lat\r\n");
    parser.process(b"total 16\r\n-rw-r--r-- file_a\r\n-rw-r--r-- file_b\r\n");

    let content = parser.screen().contents();
    assert!(content.contains("total 16"));
    assert!(content.contains("file_a"));
    assert!(content.contains("file_b"));
}

#[test]
fn scrollback_offset_changes_visible_text() {
    let mut parser = Parser::new(3, 40, 100);
    parser.process(b"line1\r\nline2\r\nline3\r\nline4\r\nline5\r\n");
    parser.screen_mut().set_scrollback(0);
    let at_bottom = parser.screen().contents();

    parser.screen_mut().set_scrollback(2);
    let scrolled = parser.screen().contents();

    assert_ne!(at_bottom, scrolled);
    assert!(scrolled.contains("line1") || scrolled.contains("line2"));
}

#[test]
fn parse_scrollback_limit_accepts_trimmed_positive_values() {
    assert_eq!(parse_scrollback_limit(" 42 ", 200_000), Some(42));
}

#[test]
fn parse_scrollback_limit_treats_zero_as_practical_unlimited() {
    assert_eq!(parse_scrollback_limit("0", 200_000), Some(200_000));
}

#[test]
fn parse_scrollback_limit_rejects_invalid_values() {
    assert_eq!(parse_scrollback_limit("abc", 200_000), None);
}

#[test]
fn resolve_scrollback_limit_uses_default_when_missing() {
    let candidates = [None, None];
    assert_eq!(
        resolve_scrollback_limit(&candidates, 500_000, 500_000),
        500_000
    );
}

#[test]
fn resolve_scrollback_limit_prefers_first_candidate() {
    let candidates = [Some("123".to_string()), Some("456".to_string())];
    assert_eq!(resolve_scrollback_limit(&candidates, 500_000, 500_000), 123);
}

#[test]
fn resolve_scrollback_limit_falls_back_to_second_candidate() {
    let candidates = [Some("invalid".to_string()), Some("789".to_string())];
    assert_eq!(resolve_scrollback_limit(&candidates, 500_000, 500_000), 789);
}

#[test]
fn shell_single_quote_escapes_single_quotes() {
    assert_eq!(shell_single_quote("/tmp/a'b"), "'/tmp/a'\\''b'");
}

#[test]
fn shell_program_name_extracts_basename() {
    assert_eq!(shell_program_name("/usr/bin/zsh"), "zsh");
    assert_eq!(shell_program_name("bash"), "bash");
}

#[test]
fn bash_history_sync_prompt_command_prepends_sync_steps() {
    assert_eq!(
        bash_history_sync_prompt_command(None),
        "history -a; history -n"
    );
    assert_eq!(
        bash_history_sync_prompt_command(Some("echo hi")),
        "history -a; history -n; echo hi"
    );
    assert_eq!(
        bash_history_sync_prompt_command(Some("history -a; history -n; echo hi")),
        "history -a; history -n; echo hi"
    );
}

#[test]
fn default_history_file_for_shell_uses_standard_paths() {
    let Some(home) = std::env::var("HOME").ok().filter(|value| !value.is_empty()) else {
        return;
    };
    assert_eq!(
        default_history_file_for_shell("/bin/bash"),
        Some(format!("{home}/.bash_history"))
    );
    let zsh_history = default_history_file_for_shell("/usr/bin/zsh");
    assert!(matches!(
        zsh_history.as_deref(),
        Some(path)
            if path == format!("{home}/.zsh_history")
                || path == format!("{home}/.zhistory")
                || path.ends_with("/zsh/history")
    ));
}

#[test]
fn cd_to_bytes_clears_prompt_before_cd_command() {
    let bytes = cd_to_bytes(Path::new("/tmp/demo"), false);
    assert!(bytes.starts_with(&[0x01, 0x0b]));
    let rendered = String::from_utf8(bytes[2..].to_vec()).expect("utf8");
    assert_eq!(rendered, "cd -- '/tmp/demo'\r");
}

#[test]
fn cd_to_bytes_restores_pending_input_when_requested() {
    let bytes = cd_to_bytes(Path::new("/tmp/demo"), true);
    assert_eq!(bytes.last().copied(), Some(0x19));
}
