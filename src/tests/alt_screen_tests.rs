use super::*;

#[test]
fn alt_screen_event_stream_detects_enter_and_exit() {
    assert_eq!(alt_screen_event_from_stream(b"\x1b[?1049h"), Some(true));
    assert_eq!(alt_screen_event_from_stream(b"\x1b[?1049l"), Some(false));
}

#[test]
fn alt_screen_event_stream_uses_last_event_when_both_present() {
    let bytes = b"\x1b[?1049hhello\x1b[?1049l";
    assert_eq!(alt_screen_event_from_stream(bytes), Some(false));
}

#[test]
fn apply_alt_screen_chunk_handles_split_escape_sequence() {
    let mut tail = Vec::new();
    let state1 = apply_alt_screen_chunk(false, &mut tail, b"\x1b[?10");
    assert!(!state1);
    let state2 = apply_alt_screen_chunk(state1, &mut tail, b"49h");
    assert!(state2);
    let state3 = apply_alt_screen_chunk(state2, &mut tail, b"\x1b[?1049l");
    assert!(!state3);
}
