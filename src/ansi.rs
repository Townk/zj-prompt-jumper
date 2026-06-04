//! Minimal ANSI-escape stripper.
//!
//! Zellij's `get_pane_scrollback` returns each grid row as a `String`. The
//! cell payload is plain text (the VTE parser has already consumed control
//! sequences) but rendered SGR/CSI bytes still occur in some paths, and we
//! also want to be defensive against any stray ESC-prefixed sequences. The
//! helper here strips the patterns we expect to see in scrollback text so
//! the prompt-prefix match in `scan` works against the visible characters
//! alone.
//!
//! This is intentionally a small custom parser — we don't pull in `regex` or
//! `vte` just to discard a few ESC sequences.

/// Return `s` with ANSI escape sequences removed.
///
/// Recognises:
/// * CSI sequences (`ESC [ … <final>`) — final byte is `0x40..=0x7e`.
/// * OSC sequences (`ESC ] … BEL` or `ESC ] … ESC \`).
/// * Charset-designation sequences (`ESC <intermediate> <final>`) where
///   the intermediate is one of `( ) * + - . /` (e.g. `ESC ( B`). These
///   carry a trailing final byte, so they are three bytes wide.
/// * Two-byte ESC sequences (`ESC <byte>`) for everything else.
///
/// Other low-ASCII control characters are passed through unchanged so the
/// caller can still distinguish `\n` / `\t` / `\r` if it cares.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == '\x1b' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            match next {
                '[' => {
                    // CSI: skip until a final byte in 0x40..=0x7e.
                    i += 2;
                    while i < bytes.len() {
                        let ch = bytes[i];
                        i += 1;
                        let cp = ch as u32;
                        if (0x40..=0x7e).contains(&cp) {
                            break;
                        }
                    }
                }
                ']' => {
                    // OSC: skip until BEL or ESC \.
                    i += 2;
                    while i < bytes.len() {
                        let ch = bytes[i];
                        if ch == '\x07' {
                            i += 1;
                            break;
                        }
                        if ch == '\x1b' && i + 1 < bytes.len() && bytes[i + 1] == '\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                '(' | ')' | '*' | '+' | '-' | '.' | '/' => {
                    // Charset designation (`ESC <intermediate> <final>`):
                    // ESC + intermediate + one trailing final byte. Skip all
                    // three so the final byte (e.g. the `B` in `ESC ( B`)
                    // doesn't leak into the output. Guard against truncation.
                    i += if i + 2 < bytes.len() { 3 } else { 2 };
                }
                _ => {
                    // Generic two-byte ESC sequence; skip both.
                    i += 2;
                }
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_sgr_color_codes() {
        let input = "\x1b[31mred\x1b[0m \x1b[1;32mgreen\x1b[0m";
        assert_eq!(strip_ansi(input), "red green");
    }

    #[test]
    fn strips_csi_cursor_moves() {
        let input = "\x1b[2J\x1b[H\x1b[?25lhidden";
        assert_eq!(strip_ansi(input), "hidden");
    }

    #[test]
    fn strips_osc_bel_terminated() {
        let input = "\x1b]0;window-title\x07visible";
        assert_eq!(strip_ansi(input), "visible");
    }

    #[test]
    fn strips_osc_st_terminated() {
        let input = "\x1b]133;A\x1b\\\x1b[32m❯\x1b[0m prompt";
        assert_eq!(strip_ansi(input), "❯ prompt");
    }

    #[test]
    fn strips_charset_designation() {
        // `ESC ( B` selects the US-ASCII charset; the final `B` must not
        // leak into the output ahead of the prompt prefix.
        assert_eq!(strip_ansi("\x1b(B❯ ls"), "❯ ls");
    }

    #[test]
    fn strips_charset_designation_variants() {
        // Other designator intermediates (`)`, `*`, `+`, ...) behave the same.
        assert_eq!(strip_ansi("\x1b)0line"), "line");
        assert_eq!(strip_ansi("a\x1b(Bb\x1b)Ac"), "abc");
    }

    #[test]
    fn truncated_charset_designation_does_not_leak() {
        // `ESC (` with no final byte at the end of the string.
        assert_eq!(strip_ansi("abc\x1b("), "abc");
    }

    #[test]
    fn preserves_unicode() {
        let input = "\x1b[34m❯\x1b[0m  some thing";
        assert_eq!(strip_ansi(input), "❯  some thing");
    }

    #[test]
    fn preserves_tabs_and_newlines() {
        let input = "a\tb\nc";
        assert_eq!(strip_ansi(input), "a\tb\nc");
    }

    #[test]
    fn empty_input() {
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn no_escapes_passes_through() {
        assert_eq!(strip_ansi("plain text"), "plain text");
    }

    #[test]
    fn truncated_csi_does_not_panic() {
        // ESC [ with no final byte at the end of the string — we shouldn't
        // leak the prefix into output or loop forever.
        let input = "abc\x1b[";
        assert_eq!(strip_ansi(input), "abc");
    }

    #[test]
    fn truncated_osc_does_not_panic() {
        let input = "abc\x1b]133;A";
        assert_eq!(strip_ansi(input), "abc");
    }

    #[test]
    fn lone_escape_at_end() {
        let input = "abc\x1b";
        assert_eq!(strip_ansi(input), "abc\x1b");
    }
}
