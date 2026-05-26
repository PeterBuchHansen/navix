use super::*;

#[test]
fn clamp_nav_selection_stays_in_bounds() {
    assert_eq!(clamp_nav_selection(3, 0), 0);
    assert_eq!(clamp_nav_selection(4, 2), 1);
    assert_eq!(clamp_nav_selection(1, 5), 1);
}

#[test]
fn nav_max_scroll_respects_viewport() {
    assert_eq!(nav_max_scroll(3, 10), 0);
    assert_eq!(nav_max_scroll(20, 5), 15);
}

#[test]
fn nav_scroll_for_selection_keeps_selection_visible() {
    assert_eq!(nav_scroll_for_selection(0, 8, 20, 5), 4);
    assert_eq!(nav_scroll_for_selection(10, 2, 20, 5), 2);
}

#[test]
fn nav_scroll_for_selection_adjusts_when_viewport_shrinks() {
    assert_eq!(nav_scroll_for_selection(5, 9, 20, 4), 6);
}

#[test]
fn nav_select_prev_wrap_moves_top_to_last() {
    assert_eq!(nav_select_prev_wrap(0, 5), 4);
    assert_eq!(nav_select_prev_wrap(3, 5), 2);
    assert_eq!(nav_select_prev_wrap(0, 0), 0);
}

#[test]
fn nav_select_next_wrap_moves_last_to_top() {
    assert_eq!(nav_select_next_wrap(4, 5), 0);
    assert_eq!(nav_select_next_wrap(2, 5), 3);
    assert_eq!(nav_select_next_wrap(0, 0), 0);
}

#[test]
fn nav_select_home_always_returns_first_item() {
    assert_eq!(nav_select_home(4, 5), 0);
    assert_eq!(nav_select_home(0, 0), 0);
}

#[test]
fn nav_select_end_returns_last_item_when_available() {
    assert_eq!(nav_select_end(0, 5), 4);
    assert_eq!(nav_select_end(3, 1), 0);
    assert_eq!(nav_select_end(0, 0), 0);
}

#[test]
fn nav_filter_matches_is_case_insensitive_when_filter_is_all_lowercase() {
    assert!(nav_filter_matches("Downloads", "down"));
    assert!(nav_filter_matches("README.MD", "readme"));
}

#[test]
fn nav_filter_matches_becomes_case_sensitive_when_filter_has_uppercase() {
    assert!(nav_filter_matches("Downloads", "Down"));
    assert!(!nav_filter_matches("downloads", "Down"));
}

#[test]
fn nav_selection_after_filter_prefers_first_real_match_over_parent_entry() {
    let entries = vec![
        NavEntry {
            name: "..".to_string(),
            path: PathBuf::from("/tmp"),
            is_dir: true,
            is_symlink: false,
            file_type_char: 'd',
            mode: 0,
            nlink: 0,
            uid: 0,
            gid: 0,
            size: 0,
            mtime: 0,
        },
        NavEntry {
            name: "navix".to_string(),
            path: PathBuf::from("/tmp/navix"),
            is_dir: true,
            is_symlink: false,
            file_type_char: 'd',
            mode: 0,
            nlink: 0,
            uid: 0,
            gid: 0,
            size: 0,
            mtime: 0,
        },
    ];
    assert_eq!(nav_selection_after_filter(&entries, 0, "nav"), 1);
}

#[test]
fn nav_selection_after_filter_keeps_parent_when_no_other_match_exists() {
    let entries = vec![NavEntry {
        name: "..".to_string(),
        path: PathBuf::from("/tmp"),
        is_dir: true,
        is_symlink: false,
        file_type_char: 'd',
        mode: 0,
        nlink: 0,
        uid: 0,
        gid: 0,
        size: 0,
        mtime: 0,
    }];
    assert_eq!(nav_selection_after_filter(&entries, 0, "nav"), 0);
}

#[test]
fn navigation_tree_lines_show_only_level_one_entries() {
    let base = unique_temp_path("navix-nav-test");
    fs::create_dir_all(base.join("sub/inner")).expect("create dirs");
    fs::write(base.join("file.txt"), b"hello").expect("create file");
    fs::write(base.join("sub/inner/deep.txt"), b"hidden").expect("create nested file");

    let lines = navigation_tree_lines(&base);
    let rendered = lines.join("\n");

    assert!(lines.get(1).is_some_and(|line| line.contains("..")));
    assert!(rendered.contains("sub/"));
    assert!(rendered.contains("file.txt"));
    assert!(rendered.contains(""));
    assert!(rendered.contains(""));
    assert!(!rendered.contains('['));
    assert!(!rendered.contains("deep.txt"));

    fs::remove_dir_all(base).expect("cleanup temp");
}
