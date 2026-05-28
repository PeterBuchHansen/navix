use super::*;

#[test]
fn completion_candidates_for_dotdot_only_returns_parent_entry() {
    let base = unique_temp_path("navix-preview-dotdot-only");
    fs::create_dir_all(base.join("docs")).expect("create docs");
    fs::create_dir_all(base.join("src")).expect("create src");
    fs::write(base.join("README.md"), b"hello").expect("create file");

    let candidates = preview_jump_completion_candidates_for_test("./docs/..", &base, 64);
    assert_eq!(candidates, vec!["./docs/../".to_string()]);

    fs::remove_dir_all(base).expect("cleanup temp");
}

#[test]
fn completion_candidates_keep_parent_entry_last() {
    let base = unique_temp_path("navix-preview-dotdot-last");
    fs::create_dir_all(base.join("a_dir")).expect("create dir a");
    fs::create_dir_all(base.join("b_dir")).expect("create dir b");
    fs::write(base.join("alpha.txt"), b"a").expect("create alpha");

    let candidates = preview_jump_completion_candidates_for_test("./", &base, 64);
    assert!(!candidates.is_empty());
    assert_eq!(
        candidates.last(),
        Some(&"./../".to_string()),
        "parent completion should be last"
    );

    fs::remove_dir_all(base).expect("cleanup temp");
}

#[test]
fn completion_candidates_reserve_room_for_parent_when_limit_is_one() {
    let base = unique_temp_path("navix-preview-dotdot-limit-one");
    fs::create_dir_all(base.join("folder")).expect("create folder");
    fs::write(base.join("a.txt"), b"a").expect("create file");

    let candidates = preview_jump_completion_candidates_for_test("./", &base, 1);
    assert_eq!(candidates, vec!["./../".to_string()]);

    fs::remove_dir_all(base).expect("cleanup temp");
}

#[test]
fn normalize_preview_jump_input_collapses_parent_sequences() {
    let normalized = normalize_preview_jump_input_text_for_test("./navix/../navix/../navix/docs");
    assert_eq!(normalized, "./navix/docs");
}

#[test]
fn normalize_preview_jump_input_reduces_to_current_dir_for_dotdot() {
    let normalized = normalize_preview_jump_input_text_for_test("./docs/..");
    assert_eq!(normalized, "./");
}
