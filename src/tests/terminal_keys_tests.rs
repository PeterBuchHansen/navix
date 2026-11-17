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
    assert_eq!(
        terminal_key_bytes(KeyCode::Home, KeyModifiers::NONE),
        b"\x1b[H"
    );
    assert_eq!(
        terminal_key_bytes(KeyCode::Home, KeyModifiers::CONTROL),
        b"\x1b[1;5H"
    );
    assert_eq!(
        terminal_key_bytes(KeyCode::Delete, KeyModifiers::SHIFT | KeyModifiers::ALT),
        b"\x1b[3;4~"
    );
    assert_eq!(
        terminal_key_bytes(KeyCode::F(1), KeyModifiers::NONE),
        b"\x1bOP"
    );
    assert_eq!(
        terminal_key_bytes(KeyCode::F(5), KeyModifiers::CONTROL),
        b"\x1b[15;5~"
    );
    assert_eq!(
        terminal_key_bytes(KeyCode::BackTab, KeyModifiers::NONE),
        b"\x1b[Z"
    );
    assert_eq!(
        terminal_key_bytes(KeyCode::Enter, KeyModifiers::NONE),
        b"\r"
    );
    assert_eq!(
        terminal_key_bytes(KeyCode::Enter, KeyModifiers::SHIFT),
        b"\x1b[13;2u"
    );
    assert_eq!(
        terminal_key_bytes(KeyCode::Enter, KeyModifiers::CONTROL),
        b"\x1b[13;5u"
    );
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

fn mouse_at(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn child_mouse_bytes_sends_nothing_when_child_did_not_ask_on_main_screen() {
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::ScrollUp,
        MouseEventKind::Moved,
    ] {
        assert!(
            child_mouse_bytes(
                mouse_at(kind, 4, 2),
                MouseProtocolMode::None,
                MouseProtocolEncoding::Default,
                false,
                false
            )
            .is_empty()
        );
    }
}

#[test]
fn child_mouse_bytes_turns_wheel_into_arrows_on_alt_screen_without_mouse_mode() {
    // Regression for #30: `printf '\e[?1049h'` + wheel must not leak SGR bytes.
    let up = child_mouse_bytes(
        mouse_at(MouseEventKind::ScrollUp, 4, 2),
        MouseProtocolMode::None,
        MouseProtocolEncoding::Default,
        true,
        false,
    );
    assert_eq!(up, b"\x1b[A\x1b[A\x1b[A".to_vec());
    let down_app_cursor = child_mouse_bytes(
        mouse_at(MouseEventKind::ScrollDown, 4, 2),
        MouseProtocolMode::None,
        MouseProtocolEncoding::Default,
        true,
        true,
    );
    assert_eq!(down_app_cursor, b"\x1bOB\x1bOB\x1bOB".to_vec());
    let click = child_mouse_bytes(
        mouse_at(MouseEventKind::Down(MouseButton::Left), 4, 2),
        MouseProtocolMode::None,
        MouseProtocolEncoding::Default,
        true,
        false,
    );
    assert!(click.is_empty());
}

#[test]
fn child_mouse_bytes_reports_only_event_kinds_the_mode_covers() {
    use MouseProtocolMode::*;
    let kinds = [
        ("down", MouseEventKind::Down(MouseButton::Left)),
        ("up", MouseEventKind::Up(MouseButton::Left)),
        ("drag", MouseEventKind::Drag(MouseButton::Left)),
        ("moved", MouseEventKind::Moved),
        ("wheel", MouseEventKind::ScrollDown),
    ];
    let expected = [
        (Press, ["down", "wheel"].as_slice()),
        (PressRelease, ["down", "up", "wheel"].as_slice()),
        (ButtonMotion, ["down", "up", "drag", "wheel"].as_slice()),
        (
            AnyMotion,
            ["down", "up", "drag", "moved", "wheel"].as_slice(),
        ),
    ];
    for (mode, reported) in expected {
        for (name, kind) in kinds {
            let bytes = child_mouse_bytes(
                mouse_at(kind, 4, 2),
                mode,
                MouseProtocolEncoding::Sgr,
                true,
                false,
            );
            assert_eq!(
                !bytes.is_empty(),
                reported.contains(&name),
                "{mode:?} / {name}"
            );
        }
    }
}

#[test]
fn child_mouse_bytes_uses_requested_encoding() {
    let event = mouse_at(MouseEventKind::Down(MouseButton::Left), 4, 2);
    let sgr = child_mouse_bytes(
        event,
        MouseProtocolMode::PressRelease,
        MouseProtocolEncoding::Sgr,
        true,
        false,
    );
    assert_eq!(sgr, b"\x1b[<0;5;3M".to_vec());
    let x10 = child_mouse_bytes(
        event,
        MouseProtocolMode::PressRelease,
        MouseProtocolEncoding::Default,
        true,
        false,
    );
    assert_eq!(x10, vec![0x1b, b'[', b'M', 32, 32 + 5, 32 + 3]);
    let release = child_mouse_bytes(
        mouse_at(MouseEventKind::Up(MouseButton::Left), 4, 2),
        MouseProtocolMode::PressRelease,
        MouseProtocolEncoding::Default,
        true,
        false,
    );
    assert_eq!(release, vec![0x1b, b'[', b'M', 32 + 3, 32 + 5, 32 + 3]);
}

#[test]
fn child_mouse_bytes_legacy_encodings_handle_large_coordinates() {
    let far = mouse_at(MouseEventKind::Down(MouseButton::Left), 299, 2);
    // X10 bytes cap at 255 - 32 = 223 columns: drop rather than send garbage.
    assert!(
        child_mouse_bytes(
            far,
            MouseProtocolMode::PressRelease,
            MouseProtocolEncoding::Default,
            true,
            false
        )
        .is_empty()
    );
    // UTF-8 extension encodes the column (300 + 32 = 332) as a 2-byte char.
    let utf8 = child_mouse_bytes(
        far,
        MouseProtocolMode::PressRelease,
        MouseProtocolEncoding::Utf8,
        true,
        false,
    );
    let mut expected = vec![0x1b, b'[', b'M', 32];
    expected.extend_from_slice("\u{14C}".as_bytes());
    expected.push(32 + 3);
    assert_eq!(utf8, expected);
}
