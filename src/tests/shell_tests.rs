use super::*;
use std::path::Path;

fn kitty_transmit(id: u32, payload: &str) -> Vec<u8> {
    format!("\x1b_Ga=T,i={id},f=32,U=1;{payload}\x1b\\").into_bytes()
}

#[test]
fn dedup_kitty_forwards_first_transfer() {
    let transfer = kitty_transmit(7, "AAAA");
    let (forward, last) = dedup_kitty_transfer(&transfer, None);
    assert!(forward);
    assert!(last.is_some());
}

#[test]
fn dedup_kitty_drops_byte_identical_retransmit() {
    let transfer = kitty_transmit(7, "AAAA");
    let (_, last) = dedup_kitty_transfer(&transfer, None);
    let (forward_again, last_again) = dedup_kitty_transfer(&transfer, last);
    assert!(!forward_again, "identical re-transmit must be dropped");
    assert_eq!(last_again, last, "dedup state unchanged when dropping");
}

#[test]
fn dedup_kitty_forwards_when_payload_changes() {
    let first = kitty_transmit(7, "AAAA");
    let (_, last) = dedup_kitty_transfer(&first, None);
    let second = kitty_transmit(7, "BBBB");
    let (forward, last_after) = dedup_kitty_transfer(&second, last);
    assert!(forward, "changed image data must be forwarded");
    assert_ne!(last_after, last);
}

#[test]
fn dedup_kitty_forwards_when_only_id_changes() {
    // ratatui-image re-uploads its image under a fresh id on each repaint. The id
    // sits in the control prefix, so the prefix hash must distinguish them and
    // forward both — otherwise the placeholder cells referencing the new id would
    // point at an image we suppressed, and nothing would draw.
    let first = kitty_transmit(4_120_076_704, "AAAA");
    let (_, last) = dedup_kitty_transfer(&first, None);
    let second = kitty_transmit(3_984_117_848, "AAAA");
    let (forward, last_after) = dedup_kitty_transfer(&second, last);
    assert!(forward, "re-id'd upload must forward");
    assert_ne!(last_after, last);
}

#[test]
fn dedup_kitty_forwards_when_length_differs_beyond_prefix() {
    // Two transfers that share their first 4KB but differ in total length must
    // not collide: the length is folded into the hash.
    let big_a = kitty_transmit(7, &"A".repeat(8192));
    let big_b = kitty_transmit(7, &"A".repeat(9000));
    let (_, last) = dedup_kitty_transfer(&big_a, None);
    let (forward, _) = dedup_kitty_transfer(&big_b, last);
    assert!(forward, "differing length must forward");
}

#[test]
fn dedup_kitty_drops_large_byte_identical_retransmit() {
    // Cheap (prefix+length) hashing still collapses a genuinely identical large
    // re-upload — the original flood-protection behavior, without a full-buffer
    // hash pass.
    let big = kitty_transmit(7, &"A".repeat(64 * 1024));
    let (_, last) = dedup_kitty_transfer(&big, None);
    let (forward_again, _) = dedup_kitty_transfer(&big, last);
    assert!(!forward_again, "identical large re-transmit must drop");
}

#[test]
fn dedup_kitty_delete_always_forwards_and_resets_state() {
    let transfer = kitty_transmit(7, "AAAA");
    let (_, last) = dedup_kitty_transfer(&transfer, None);
    let delete = b"\x1b_Ga=d,i=7\x1b\\".to_vec();
    let (forward_delete, last_after_delete) = dedup_kitty_transfer(&delete, last);
    assert!(forward_delete, "delete must pass through");
    assert_eq!(last_after_delete, None, "delete resets dedup state");
    // After a delete, an identical reload of the original image must forward.
    let (forward_reload, _) = dedup_kitty_transfer(&transfer, last_after_delete);
    assert!(forward_reload, "reload after delete must not be dropped");
}

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
fn resolve_child_term_prefers_explicit_navix_child_term() {
    assert_eq!(
        resolve_child_term_for_test(Some("xterm-kitty"), Some("xterm-256color")),
        "xterm-kitty"
    );
}

#[test]
fn resolve_child_term_falls_back_to_host_term() {
    assert_eq!(
        resolve_child_term_for_test(None, Some("xterm-kitty")),
        "xterm-kitty"
    );
}

#[test]
fn resolve_child_term_uses_safe_default_when_missing() {
    assert_eq!(resolve_child_term_for_test(None, None), "xterm-256color");
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

#[test]
fn terminal_query_replies_include_status_and_geometry_answers() {
    let replies = terminal_query_replies_for_test(b"\x1b[c\x1b[5n\x1b[16t", 40, 120);
    let rendered = String::from_utf8_lossy(&replies);
    assert!(rendered.contains("\x1b[?62;4;6;22c"));
    assert!(rendered.contains("\x1b[0n"));
    assert!(rendered.contains("\x1b[6;16;8t"));
}

#[test]
fn terminal_query_replies_reflect_advertised_cell_size() {
    // CSI 16t (cell size) and 14t (window px) must echo whatever cell size we
    // advertise to the child, so a host with non-8x16 cells is reported 1:1.
    let replies = crate::shell::terminal_query_replies_for_test_with_cell(
        b"\x1b[16t\x1b[14t",
        40,
        100,
        9,
        20,
    );
    let rendered = String::from_utf8_lossy(&replies);
    assert!(
        rendered.contains("\x1b[6;20;9t"),
        "cell size 9x20: {rendered:?}"
    );
    // window px: 40 rows * 20 = 800 high, 100 cols * 9 = 900 wide.
    assert!(
        rendered.contains("\x1b[4;800;900t"),
        "window px: {rendered:?}"
    );
}

#[test]
fn detect_host_cell_pixels_falls_back_without_tty() {
    // In the test harness stdout is not a terminal reporting pixels, so the
    // fallback (8x16) is returned rather than a bogus 0 / divide-by-zero.
    let (width, height) = crate::shell::detect_host_cell_pixels();
    assert_eq!((width, height), (8, 16));
}

#[test]
fn terminal_query_replies_include_osc_color_answers() {
    let replies = terminal_query_replies_for_test(b"\x1b]10;?\x07\x1b]11;?\x07", 24, 80);
    let rendered = String::from_utf8_lossy(&replies);
    assert!(rendered.contains("\x1b]10;rgb:ffff/ffff/ffff\x1b\\"));
    assert!(rendered.contains("\x1b]11;rgb:0000/0000/0000\x1b\\"));
}

#[test]
fn terminal_query_replies_handle_st_terminated_color_queries() {
    // ST-terminated (ESC \) queries must be answered, not just BEL-terminated.
    let replies = terminal_query_replies_for_test(b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\", 24, 80);
    let rendered = String::from_utf8_lossy(&replies);
    assert!(rendered.contains("\x1b]10;rgb:ffff/ffff/ffff\x1b\\"));
    assert!(rendered.contains("\x1b]11;rgb:0000/0000/0000\x1b\\"));
}

#[test]
fn terminal_query_replies_answer_cursor_color_query() {
    let replies = terminal_query_replies_for_test(b"\x1b]12;?\x07", 24, 80);
    let rendered = String::from_utf8_lossy(&replies);
    assert!(rendered.contains("\x1b]12;rgb:ffff/ffff/ffff\x1b\\"));
}

#[test]
fn terminal_query_replies_answer_palette_query_with_xterm_defaults() {
    // Index 1 (ANSI red) = 0x800000, index 196 (cube) = 0xff0000,
    // index 232 (grayscale start) = 0x080808.
    let replies = terminal_query_replies_for_test(b"\x1b]4;1;?\x07\x1b]4;196;?\x1b\\", 24, 80);
    let rendered = String::from_utf8_lossy(&replies);
    assert!(rendered.contains("\x1b]4;1;rgb:8080/0000/0000\x1b\\"));
    assert!(rendered.contains("\x1b]4;196;rgb:ffff/0000/0000\x1b\\"));

    let gray = terminal_query_replies_for_test(b"\x1b]4;232;?\x07", 24, 80);
    assert!(String::from_utf8_lossy(&gray).contains("\x1b]4;232;rgb:0808/0808/0808\x1b\\"));
}

#[test]
fn terminal_query_replies_handle_multiple_palette_pairs() {
    let replies = terminal_query_replies_for_test(b"\x1b]4;0;?;15;?\x07", 24, 80);
    let rendered = String::from_utf8_lossy(&replies);
    assert!(rendered.contains("\x1b]4;0;rgb:0000/0000/0000\x1b\\"));
    assert!(rendered.contains("\x1b]4;15;rgb:ffff/ffff/ffff\x1b\\"));
}

#[test]
fn terminal_query_replies_ignore_incomplete_color_query() {
    // No terminator in the chunk: do not emit a spurious reply.
    let replies = terminal_query_replies_for_test(b"\x1b]10;?", 24, 80);
    assert!(replies.is_empty());
}

#[test]
fn kitty_unicode_placeholder_cell_survives_text_render() {
    let (has_placeholder, has_combining, has_fg) = placeholder_render_survival_for_test();
    // Passthrough placement relies on the placeholder cells reaching the host
    // intact: the base char carries the cell, the truecolor fg carries the image
    // id, and the combining diacritics carry the row/column of the image slice.
    assert!(
        has_placeholder,
        "placeholder base char U+10EEEE was dropped"
    );
    assert!(has_fg, "truecolor foreground (image id) was not preserved");
    assert!(
        has_combining,
        "combining diacritics (image row/col selectors) were dropped"
    );
}

#[test]
fn expand_cursor_forward_replaces_cuf_with_spaces() {
    // `\x1b[2C` (cursor-forward 2) becomes two spaces; SGR passes through.
    let input = b"\x1b[2C\x1b[38;2;1;2;3mX";
    let out = crate::shell::expand_cursor_forward(input);
    assert_eq!(out, b"  \x1b[38;2;1;2;3mX".to_vec());

    // Bare `\x1b[C` defaults to one space.
    assert_eq!(
        crate::shell::expand_cursor_forward(b"\x1b[CX"),
        b" X".to_vec()
    );

    // Rows without CUF are untouched (and avoid reallocation churn).
    let plain = b"   plain text \x1b[0m";
    assert_eq!(crate::shell::expand_cursor_forward(plain), plain.to_vec());
}

#[test]
fn expand_cursor_forward_aligns_indented_rows_for_ansi_to_tui() {
    use ansi_to_tui::IntoText;
    // vt100 emits CUF for a blank indent on one row but literal spaces on the
    // next. Without expansion ansi-to-tui drops the CUF and the two rows end up
    // misaligned. After expansion both rows place their glyph at the same column.
    let row_cuf = crate::shell::expand_cursor_forward(b"\x1b[2C@");
    let row_spaces = crate::shell::expand_cursor_forward(b"  @");
    let mut bytes = row_cuf.clone();
    bytes.extend_from_slice(b"\x1b[0m\n");
    bytes.extend_from_slice(&row_spaces);
    let text = bytes.into_text().expect("into_text");
    let col_of_at = |line: &ratatui::text::Line| -> Option<usize> {
        let mut col = 0usize;
        for span in &line.spans {
            for ch in span.content.chars() {
                if ch == '@' {
                    return Some(col);
                }
                col += 1;
            }
        }
        None
    };
    assert_eq!(text.lines.len(), 2);
    assert_eq!(col_of_at(&text.lines[0]), Some(2));
    assert_eq!(col_of_at(&text.lines[1]), Some(2));
}

#[test]
fn graphics_passthrough_wrap_brackets_with_cursor_save_restore() {
    // A non-empty kitty transfer is forwarded verbatim, bracketed by
    // DECSC/DECRC so the cursor ratatui is about to draw text at is never
    // disturbed — placement is driven by the U=1 placeholder cells, not the
    // cursor position the transfer leaves behind.
    let wrapped = crate::shell::wrap_graphics_passthrough(b"\x1b_Gi=1,a=T;AAAA\x1b\\");
    assert_eq!(wrapped, b"\x1b7\x1b_Gi=1,a=T;AAAA\x1b\\\x1b8".to_vec());

    // Idle frames (empty passthrough) must emit nothing — no per-frame stray
    // save/restore churn against the host terminal.
    assert!(crate::shell::wrap_graphics_passthrough(b"").is_empty());
}

#[test]
fn cuf_rows_render_column_aligned_in_test_backend() {
    use ansi_to_tui::IntoText;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::widgets::Paragraph;

    // Row 0 indents with CUF (`\x1b[2C`), row 1 with literal spaces. After
    // expansion both glyphs must land in the same column when ratatui renders
    // them — otherwise the top of an image block shears (the original tear).
    let mut bytes = crate::shell::expand_cursor_forward(b"\x1b[2C@");
    bytes.extend_from_slice(b"\x1b[0m\n");
    bytes.extend_from_slice(&crate::shell::expand_cursor_forward(b"  @"));
    let text = bytes.into_text().expect("into_text");

    let mut terminal = Terminal::new(TestBackend::new(6, 2)).expect("terminal");
    terminal
        .draw(|frame| frame.render_widget(Paragraph::new(text), frame.area()))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer.cell((2, 0)).map(|c| c.symbol()), Some("@"));
    assert_eq!(buffer.cell((2, 1)).map(|c| c.symbol()), Some("@"));
}

#[test]
fn cuf_rows_render_aligned_snapshot() {
    use ansi_to_tui::IntoText;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::widgets::Paragraph;

    let mut bytes = crate::shell::expand_cursor_forward(b"\x1b[3C#");
    bytes.extend_from_slice(b"\x1b[0m\n");
    bytes.extend_from_slice(&crate::shell::expand_cursor_forward(b"   #"));
    let text = bytes.into_text().expect("into_text");

    let mut terminal = Terminal::new(TestBackend::new(8, 2)).expect("terminal");
    terminal
        .draw(|frame| frame.render_widget(Paragraph::new(text), frame.area()))
        .expect("draw");
    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn kitty_multipart_passthrough_holds_chunks_until_complete() {
    // First chunk (m=1, 4 payload bytes), middle (m=1), final (m=0).
    let chunks: [&[u8]; 3] = [
        b"\x1b_Gi=1,a=T,m=1;AAAA\x1b\\",
        b"\x1b_Gm=1;BBBB\x1b\\",
        b"\x1b_Gm=0;CCCC\x1b\\",
    ];
    let released = kitty_passthrough_staging_for_test(&chunks);
    // Nothing may be released to the host until the final chunk; then the whole
    // transfer is released at once so the host receives every chunk contiguously.
    assert_eq!(released[0], 0, "leaked partial transfer after first chunk");
    assert_eq!(released[1], 0, "leaked partial transfer after middle chunk");
    let total: usize = chunks.iter().map(|c| c.len()).sum();
    assert_eq!(
        released[2], total,
        "final chunk must release the entire contiguous transfer"
    );
}

#[test]
fn kitty_single_shot_passthrough_releases_immediately() {
    // A non-multipart command (no `m` key) is complete on arrival.
    let chunks: [&[u8]; 1] = [b"\x1b_Gi=1,a=T;AAAA\x1b\\"];
    let released = kitty_passthrough_staging_for_test(&chunks);
    assert_eq!(released[0], chunks[0].len());
}

#[test]
fn kitty_placeholder_grid_lays_out_one_cell_per_column() {
    let layout = placeholder_buffer_layout_for_test();
    // Each placeholder must occupy exactly one cell in its own column with its
    // combining marks intact. If U+10EEEE is mis-widthed (0 or 2) the grid
    // shears and the rendered image tears.
    assert_eq!(
        layout,
        [true, true, true],
        "placeholder grid misaligned in ratatui buffer: {layout:?}"
    );
}

#[test]
fn kitty_multipart_transfer_passes_through_without_cursor_move() {
    let (cursor_moves, payload_order_ok) = kitty_multipart_flush_summary_for_test();
    // Kitty placement is owned by the tool (Unicode-placeholder cells or its own
    // cursor handling), so Navix passes the transmission through verbatim with no
    // cursor positioning of its own. Injecting a cursor move both corrupts a
    // multipart transfer and breaks 1:1 placement/clipping behavior.
    assert_eq!(
        cursor_moves, 0,
        "expected no cursor-positioning escape for a passed-through kitty transfer"
    );
    assert!(
        payload_order_ok,
        "expected all kitty chunk payloads to be emitted contiguously in order"
    );
}

#[test]
fn kitty_graphics_query_reply_echoes_image_id() {
    let reply = kitty_graphics_query_reply_for_test(b"\x1b_Gi=31,a=q,t=d;AAAA\x1b\\")
        .expect("query reply expected");
    assert_eq!(reply, b"\x1b_Gi=31;OK\x1b\\");
}

#[test]
fn kitty_graphics_query_reply_ignores_non_query_commands() {
    let reply = kitty_graphics_query_reply_for_test(b"\x1b_Gi=31,a=T,t=d;AAAA\x1b\\");
    assert!(reply.is_none());
}

#[test]
fn kitty_graphics_query_reply_handles_query_without_payload_separator() {
    let reply = kitty_graphics_query_reply_for_test(b"\x1b_Gi=9,a=q,t=d\x1b\\")
        .expect("query reply expected");
    assert_eq!(reply, b"\x1b_Gi=9;OK\x1b\\");
}

#[test]
fn kitty_graphics_query_reply_handles_malformed_but_query_like_probe() {
    let reply = kitty_graphics_query_reply_for_test(b"\x1b_Gi=42;meta,a=q,broken\x1b\\")
        .expect("fallback query reply expected");
    assert_eq!(reply, b"\x1b_Gi=42;OK\x1b\\");
}

#[test]
fn parse_stream_events_detects_kitty_graphics_and_preserves_text_boundaries() {
    let events =
        parse_stream_events_for_test(&[b"before\x1b_Gi=1,a=q,t=d;AAAA", b"BBBB\x1b\\after"]);
    assert_eq!(events.len(), 3);
    match &events[0] {
        StreamEvent::Text(bytes) => assert_eq!(String::from_utf8_lossy(bytes), "before"),
        _ => panic!("expected leading text event"),
    }
    match &events[1] {
        StreamEvent::Graphics(graphics) => {
            assert_eq!(graphics.protocol, GraphicsProtocol::Kitty);
            assert!(graphics.bytes.starts_with(b"\x1b_G"));
        },
        _ => panic!("expected kitty graphics event"),
    }
    match &events[2] {
        StreamEvent::Text(bytes) => assert_eq!(String::from_utf8_lossy(bytes), "after"),
        _ => panic!("expected trailing text event"),
    }
}

#[test]
fn parse_stream_events_detects_iterm2_and_treats_sixel_as_text() {
    let events = parse_stream_events_for_test(&[
        b"\x1b[31mred\x1b[0m\x1bPqDATA",
        b"MORE\x1b\\tail\x1b]1337;File=name=test.png;inline=1:QUJD\x07",
    ]);
    // iTerm2 inline images are still detected as graphics.
    assert!(events.iter().any(|event| {
        matches!(
            event,
            StreamEvent::Graphics(graphics) if graphics.protocol == GraphicsProtocol::Iterm2Inline
        )
    }));
    // Sixel is unsupported: the only graphics events present are iTerm2.
    assert!(events.iter().all(|event| match event {
        StreamEvent::Graphics(graphics) => graphics.protocol == GraphicsProtocol::Iterm2Inline,
        StreamEvent::Text(_) => true,
        StreamEvent::Clipboard(_) => true,
        StreamEvent::HostPassthrough(_) => true,
    }));
    assert!(events.iter().any(|event| {
        matches!(event, StreamEvent::Text(bytes) if String::from_utf8_lossy(bytes).contains("red"))
    }));
}

#[test]
fn parse_stream_events_detects_tmux_wrapped_kitty_passthrough() {
    let events = parse_stream_events_for_test(&[
        b"head\x1bPtmux;\x1b\x1b_Gi=7,a=q,t=d;QUJD\x1b\x1b\\\x1b\\tail",
    ]);
    assert_eq!(events.len(), 3);
    match &events[1] {
        StreamEvent::Graphics(graphics) => {
            assert_eq!(graphics.protocol, GraphicsProtocol::Kitty);
            assert!(graphics.bytes.starts_with(b"\x1b_Gi=7"));
            assert!(graphics.bytes.ends_with(b"\x1b\\"));
        },
        _ => panic!("expected decoded kitty graphics event"),
    }
}

#[test]
fn parse_stream_events_keeps_plain_text_as_single_event() {
    let events = parse_stream_events_for_test(&[b"plain text without escapes"]);
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::Text(bytes) => {
            assert_eq!(String::from_utf8_lossy(bytes), "plain text without escapes")
        },
        _ => panic!("expected plain text event"),
    }
}

#[test]
fn parse_stream_events_routes_osc52_clipboard_set_to_clipboard() {
    // BEL-terminated and ST-terminated OSC 52 writes are both diverted to the
    // host clipboard path instead of the text emulator.
    for terminator in [b"\x07".as_slice(), b"\x1b\\".as_slice()] {
        let mut seq = b"before\x1b]52;c;aGVsbG8=".to_vec();
        seq.extend_from_slice(terminator);
        seq.extend_from_slice(b"after");
        let events = parse_stream_events_for_test(&[&seq]);
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[0], StreamEvent::Text(bytes) if bytes == b"before"));
        match &events[1] {
            StreamEvent::Clipboard(bytes) => {
                assert!(bytes.starts_with(b"\x1b]52;c;aGVsbG8="));
            },
            other => panic!("expected clipboard event, got {other:?}"),
        }
        assert!(matches!(&events[2], StreamEvent::Text(bytes) if bytes == b"after"));
    }
}

#[test]
fn parse_stream_events_reassembles_osc52_split_across_chunks() {
    let events = parse_stream_events_for_test(&[b"\x1b]52;c;aGVsb", b"G8=\x07tail"]);
    assert_eq!(events.len(), 2);
    match &events[0] {
        StreamEvent::Clipboard(bytes) => assert!(bytes.starts_with(b"\x1b]52;c;aGVsbG8=")),
        other => panic!("expected clipboard event, got {other:?}"),
    }
    assert!(matches!(&events[1], StreamEvent::Text(bytes) if bytes == b"tail"));
}

#[test]
fn parse_stream_events_keeps_osc52_query_as_text() {
    // A read query (`Pd == "?"`) must not be forwarded: the host's reply would
    // otherwise land in Navix's key input. It stays on the text path.
    let events = parse_stream_events_for_test(&[b"\x1b]52;c;?\x07"]);
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::Text(bytes) => assert_eq!(bytes.as_slice(), b"\x1b]52;c;?\x07"),
        other => panic!("expected text event, got {other:?}"),
    }
}

#[test]
fn parse_stream_events_routes_osc9_notification_to_host_passthrough() {
    for terminator in [b"\x07".as_slice(), b"\x1b\\".as_slice()] {
        let mut seq = b"before\x1b]9;Build finished".to_vec();
        seq.extend_from_slice(terminator);
        seq.extend_from_slice(b"after");
        let events = parse_stream_events_for_test(&[&seq]);
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[0], StreamEvent::Text(bytes) if bytes == b"before"));
        match &events[1] {
            StreamEvent::HostPassthrough(bytes) => {
                assert!(bytes.starts_with(b"\x1b]9;Build finished"));
            },
            other => panic!("expected host passthrough event, got {other:?}"),
        }
        assert!(matches!(&events[2], StreamEvent::Text(bytes) if bytes == b"after"));
    }
}

#[test]
fn parse_stream_events_routes_osc9_progress_to_host_passthrough() {
    // `OSC 9 ; 4 ; <state> ; <percentage>` taskbar progress is an OSC 9 subform
    // and is forwarded to the host like any other OSC 9 side channel.
    for terminator in [b"\x07".as_slice(), b"\x1b\\".as_slice()] {
        let mut seq = b"before\x1b]9;4;1;42".to_vec();
        seq.extend_from_slice(terminator);
        seq.extend_from_slice(b"after");
        let events = parse_stream_events_for_test(&[&seq]);
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[0], StreamEvent::Text(bytes) if bytes == b"before"));
        match &events[1] {
            StreamEvent::HostPassthrough(bytes) => {
                assert!(bytes.starts_with(b"\x1b]9;4;1;42"));
            },
            other => panic!("expected host passthrough event, got {other:?}"),
        }
        assert!(matches!(&events[2], StreamEvent::Text(bytes) if bytes == b"after"));
    }
}

#[test]
fn parse_stream_events_routes_osc777_notify_to_host_passthrough() {
    let events = parse_stream_events_for_test(&[b"\x1b]777;notify;Title;Body\x1b\\"]);
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::HostPassthrough(bytes) => {
            assert!(bytes.starts_with(b"\x1b]777;notify;Title;Body"));
        },
        other => panic!("expected host passthrough event, got {other:?}"),
    }
}

#[test]
fn parse_stream_events_reassembles_osc9_notification_split_across_chunks() {
    let events = parse_stream_events_for_test(&[b"\x1b]9;Done ", b"copying\x07tail"]);
    assert_eq!(events.len(), 2);
    match &events[0] {
        StreamEvent::HostPassthrough(bytes) => assert!(bytes.starts_with(b"\x1b]9;Done copying")),
        other => panic!("expected host passthrough event, got {other:?}"),
    }
    assert!(matches!(&events[1], StreamEvent::Text(bytes) if bytes == b"tail"));
}

#[test]
fn parse_stream_events_reassembles_osc9_progress_split_across_chunks() {
    let events = parse_stream_events_for_test(&[b"\x1b]9;4;1;", b"75\x07tail"]);
    assert_eq!(events.len(), 2);
    match &events[0] {
        StreamEvent::HostPassthrough(bytes) => assert!(bytes.starts_with(b"\x1b]9;4;1;75")),
        other => panic!("expected host passthrough event, got {other:?}"),
    }
    assert!(matches!(&events[1], StreamEvent::Text(bytes) if bytes == b"tail"));
}

#[test]
fn parse_stream_events_handles_st_terminator_split_across_chunks() {
    // The bulk scanner carries `esc_pending` across chunks: chunk 1 ends on the
    // ESC of the ST terminator, chunk 2 supplies the `\`.
    let events = parse_stream_events_for_test(&[b"\x1b_Gi=1,a=T;AAAA\x1b", b"\\tail"]);
    assert_eq!(events.len(), 2);
    match &events[0] {
        StreamEvent::Graphics(graphics) => {
            assert_eq!(graphics.protocol, GraphicsProtocol::Kitty);
            assert!(graphics.bytes.ends_with(b"\x1b\\"));
        },
        _ => panic!("expected kitty graphics event"),
    }
    match &events[1] {
        StreamEvent::Text(bytes) => assert_eq!(String::from_utf8_lossy(bytes), "tail"),
        _ => panic!("expected trailing text event"),
    }
}

#[test]
fn parse_stream_events_reassembles_multi_chunk_payload() {
    // A payload split into many small chunks (as a multipart image arrives) must
    // reassemble into exactly one graphics event with every byte preserved.
    let mut chunks: Vec<Vec<u8>> = vec![b"\x1b_Gi=9,a=T;".to_vec()];
    for _ in 0..500 {
        chunks.push(b"ABCD".to_vec());
    }
    chunks.push(b"\x1b\\".to_vec());
    let refs: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_slice()).collect();
    let events = parse_stream_events_for_test(&refs);
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::Graphics(graphics) => {
            assert_eq!(graphics.protocol, GraphicsProtocol::Kitty);
            assert_eq!(graphics.bytes.len(), b"\x1b_Gi=9,a=T;".len() + 500 * 4 + 2);
            assert!(graphics.bytes.ends_with(b"ABCD\x1b\\"));
        },
        _ => panic!("expected single reassembled kitty graphics event"),
    }
}

#[test]
fn resolve_launch_shell_prefers_explicit_navix_launch_shell() {
    let resolved = resolve_launch_shell_path_with(
        Some("/opt/custom/zsh"),
        Some("zsh"),
        Some("/bin/bash"),
        |_| Some("/usr/bin/zsh".to_string()),
    );
    assert_eq!(resolved, "/opt/custom/zsh");
}

#[test]
fn resolve_launch_shell_prefers_parent_shell_over_shell_env_when_detected() {
    let resolved =
        resolve_launch_shell_path_with(None, Some("zsh"), Some("/bin/bash"), |command| {
            if command == "zsh" {
                Some("/usr/bin/zsh".to_string())
            } else {
                None
            }
        });
    assert_eq!(resolved, "/usr/bin/zsh");
}

#[test]
fn resolve_launch_shell_ignores_non_shell_parent_and_uses_shell_env() {
    let resolved = resolve_launch_shell_path_with(None, Some("tmux"), Some("/bin/bash"), |_| {
        Some("/usr/bin/tmux".to_string())
    });
    assert_eq!(resolved, "/bin/bash");
}

#[test]
fn resolve_launch_shell_falls_back_to_bin_sh_when_no_candidates_exist() {
    let resolved = resolve_launch_shell_path_with(None, None, None, |_| None);
    assert_eq!(resolved, "/bin/sh");
}
