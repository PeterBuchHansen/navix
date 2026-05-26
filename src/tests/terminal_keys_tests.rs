use super::*;

#[test]
fn terminal_key_bytes_supports_alt_char() {
    assert_eq!(
        terminal_key_bytes(KeyCode::Char('x'), KeyModifiers::ALT),
        vec![0x1b, b'x']
    );
}

#[test]
fn terminal_key_bytes_supports_home_end_delete_and_function_keys() {
    assert_eq!(terminal_key_bytes(KeyCode::Home, KeyModifiers::NONE), b"\x1b[H");
    assert_eq!(
        terminal_key_bytes(KeyCode::Home, KeyModifiers::CONTROL),
        b"\x1b[1;5H"
    );
    assert_eq!(
        terminal_key_bytes(KeyCode::Delete, KeyModifiers::SHIFT | KeyModifiers::ALT),
        b"\x1b[3;4~"
    );
    assert_eq!(terminal_key_bytes(KeyCode::F(1), KeyModifiers::NONE), b"\x1bOP");
    assert_eq!(
        terminal_key_bytes(KeyCode::F(5), KeyModifiers::CONTROL),
        b"\x1b[15;5~"
    );
    assert_eq!(terminal_key_bytes(KeyCode::BackTab, KeyModifiers::NONE), b"\x1b[Z");
}

#[test]
fn terminal_mouse_bytes_supports_click_drag_release_and_scroll() {
    let down = terminal_mouse_bytes(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 9,
        row: 4,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(down, b"\x1b[<0;10;5M");

    let drag = terminal_mouse_bytes(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 9,
        row: 4,
        modifiers: KeyModifiers::SHIFT,
    });
    assert_eq!(drag, b"\x1b[<36;10;5M");

    let up = terminal_mouse_bytes(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 9,
        row: 4,
        modifiers: KeyModifiers::CONTROL,
    });
    assert_eq!(up, b"\x1b[<19;10;5m");

    let scroll = terminal_mouse_bytes(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 9,
        row: 4,
        modifiers: KeyModifiers::ALT,
    });
    assert_eq!(scroll, b"\x1b[<73;10;5M");
}
