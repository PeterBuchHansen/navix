use super::*;

#[test]
fn nav_row_selected_style_highlights_with_light_blue_background() {
    let base = Style::default().fg(Color::Blue);
    let selected = nav_row_selected_style(base, true);
    assert_eq!(selected.fg, Some(Color::Black));
    assert_eq!(selected.bg, Some(Color::LightBlue));
}

#[test]
fn nav_style_for_theme_dims_foreground_in_fullish_mode() {
    let base = Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD);
    let dimmed = nav_style_for_theme(base, true);
    assert_eq!(dimmed.fg, Some(Color::DarkGray));
    assert!(dimmed.add_modifier.contains(Modifier::BOLD));

    let unchanged = nav_style_for_theme(base, false);
    assert_eq!(unchanged.fg, Some(Color::Blue));
    assert!(unchanged.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn sgr_to_style_decodes_basic_and_bright_codes() {
    let basic = sgr_to_style("01;34");
    assert_eq!(basic.fg, Some(Color::Blue));
    assert!(basic.add_modifier.contains(Modifier::BOLD));

    let bright = sgr_to_style("90");
    assert_eq!(bright.fg, Some(Color::DarkGray));

    let indexed = sgr_to_style("38;5;214");
    assert_eq!(indexed.fg, Some(Color::Indexed(214)));
}

#[test]
fn ls_colors_parser_ignores_malformed_segments() {
    let mut theme = LsColorsTheme::fallback();
    theme.apply("di=01;35:bad-segment:*=01;31:*.md=01;33");
    let dir = theme.style_for_entry("src", true, false, 0);
    let markdown = theme.style_for_entry("README.md", false, false, 0o644);

    assert_eq!(dir.fg, Some(Color::Magenta));
    assert_eq!(markdown.fg, Some(Color::Yellow));
}

#[test]
fn navigation_name_style_colors_key_entry_types() {
    let colors = LsColorsTheme::fallback();
    let dir = navigation_name_style(&colors, "macros", true, false, 0o755);
    assert_eq!(dir.fg, Some(Color::Blue));
    assert!(dir.add_modifier.contains(Modifier::BOLD));

    let symlink = navigation_name_style(&colors, "link", false, true, 0o777);
    assert_eq!(symlink.fg, Some(Color::Cyan));

    let executable = navigation_name_style(&colors, "run.sh", false, false, 0o755);
    assert_eq!(executable.fg, Some(Color::Green));

    let regular = navigation_name_style(&colors, "README.md", false, false, 0o644);
    assert_eq!(regular.fg, None);
}

#[test]
fn navigation_name_style_prefers_extension_then_executable() {
    let mut colors = LsColorsTheme::fallback();
    colors.apply("*.sh=01;31:ex=01;32");

    let ext_override = navigation_name_style(&colors, "deploy.sh", false, false, 0o755);
    assert_eq!(ext_override.fg, Some(Color::Red));
    assert!(ext_override.add_modifier.contains(Modifier::BOLD));

    let executable = navigation_name_style(&colors, "deploy", false, false, 0o755);
    assert_eq!(executable.fg, Some(Color::Green));
}
