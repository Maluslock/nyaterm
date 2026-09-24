//! Parser for tmux control-mode notifications.
//!
//! Notification syntax is defined by `man tmux` (CONTROL MODE) and the exact
//! escaping behaviour below was verified against real `tmux 3.2a` output
//! (see `tests::unescapes_real_tmux_output_encoding`):
//!
//! * printable ASCII passes through literally, **except** backslash;
//! * backslash is octal-escaped as `\134`;
//! * control characters (including CR/LF/ESC) are octal-escaped as `\ooo`;
//! * valid UTF-8 multi-byte sequences pass through **raw** (not escaped).

/// A parsed control-mode notification line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TmuxNotification {
    /// `%output %<pane-id> <data>` — pane produced output.
    Output { pane_id: String, data: String },
    /// `%layout-change @<window-id> <layout> <visible-layout> <flags>`
    LayoutChange { window_id: String, layout: String },
    /// `%window-add @<window-id>`
    WindowAdd { window_id: String },
    /// `%window-close @<window-id>`
    WindowClose { window_id: String },
    /// `%window-renamed @<window-id> <name>`
    WindowRenamed { window_id: String, name: String },
    /// `%window-pane-changed @<window-id> %<pane-id>`
    WindowPaneChanged { window_id: String, pane_id: String },
    /// `%session-changed $<session-id> <name>`
    SessionChanged { session_id: String, name: String },
    /// `%session-renamed <name>`
    SessionRenamed { name: String },
    /// `%session-window-changed $<session-id> @<window-id>`
    SessionWindowChanged {
        session_id: String,
        window_id: String,
    },
    /// `%sessions-changed`
    SessionsChanged,
    /// `%unlinked-window-add @<window-id>`
    UnlinkedWindowAdd { window_id: String },
    /// `%unlinked-window-close @<window-id>`
    UnlinkedWindowClose { window_id: String },
    /// `%unlinked-window-renamed @<window-id> <name>`
    UnlinkedWindowRenamed { window_id: String, name: String },
    /// `%exit [reason]`
    Exit { reason: Option<String> },
    /// `%message <text>`
    Message { text: String },
    /// `%begin <time> <command-number> <flags>`
    Begin { command_number: u32 },
    /// `%end <time> <command-number> <flags>`
    End { command_number: u32 },
    /// `%error <time> <command-number> <flags>`
    Error { command_number: u32 },
    /// `%pane-mode-changed %<pane-id>`
    PaneModeChanged { pane_id: String },
    /// `%client-detached <client>`
    ClientDetached,
    /// Any notification this gateway does not act on yet.
    Unhandled { name: String },
}

/// Decode `%output` escaping back into the raw byte stream, then to text.
///
/// Returns a `String` because downstream consumers are Rust `String`s and
/// xterm.js takes text; invalid UTF-8 is replaced rather than dropped.
pub fn unescape_output(data: &str) -> String {
    let bytes = data.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            // `\ooo` — exactly three octal digits.
            let digits = &bytes[i + 1..bytes.len().min(i + 4)];
            if digits.len() == 3 && digits.iter().all(|b| (b'0'..=b'7').contains(b)) {
                let value = (digits[0] - b'0') * 64 + (digits[1] - b'0') * 8 + (digits[2] - b'0');
                out.push(value);
                i += 4;
                continue;
            }
            // `\\` — a literal backslash, in case it is not octal-escaped.
            if bytes[i + 1] == b'\\' {
                out.push(b'\\');
                i += 2;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn tokens(line: &str) -> Vec<&str> {
    line.split(' ').filter(|t| !t.is_empty()).collect()
}

/// Parse one control-mode line (without its trailing newline).
///
/// Returns `None` for blank lines and for lines that are not notifications.
pub fn parse_line(line: &str) -> Option<TmuxNotification> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    if line.is_empty() {
        return None;
    }
    if !line.starts_with('%') {
        return None;
    }

    // `%output` and `%extended-output` carry arbitrary trailing payload, so
    // they are split off before generic tokenising.
    if let Some(rest) = line.strip_prefix("%output ") {
        let (pane_id, data) = match rest.split_once(' ') {
            Some((pane, data)) => (pane, data),
            None => (rest, ""),
        };
        return Some(TmuxNotification::Output {
            pane_id: pane_id.to_string(),
            data: unescape_output(data),
        });
    }
    if let Some(rest) = line.strip_prefix("%extended-output ") {
        // `%extended-output %<pane> <age> ... : <data>`
        let (head, data) = rest.split_once(" : ").unwrap_or((rest, ""));
        let pane_id = head.split(' ').next().unwrap_or_default();
        return Some(TmuxNotification::Output {
            pane_id: pane_id.to_string(),
            data: unescape_output(data),
        });
    }

    let parts = tokens(line);
    let name = parts.first().copied().unwrap_or_default();
    let arg = |n: usize| parts.get(n).copied().unwrap_or_default().to_string();

    Some(match name {
        "%begin" => TmuxNotification::Begin {
            command_number: parts.get(2).and_then(|n| n.parse().ok()).unwrap_or(0),
        },
        "%end" => TmuxNotification::End {
            command_number: parts.get(2).and_then(|n| n.parse().ok()).unwrap_or(0),
        },
        "%error" => TmuxNotification::Error {
            command_number: parts.get(2).and_then(|n| n.parse().ok()).unwrap_or(0),
        },
        "%layout-change" => TmuxNotification::LayoutChange {
            window_id: arg(1),
            layout: arg(2),
        },
        "%window-add" => TmuxNotification::WindowAdd {
            window_id: arg(1),
        },
        "%window-close" => TmuxNotification::WindowClose {
            window_id: arg(1),
        },
        "%window-renamed" => TmuxNotification::WindowRenamed {
            window_id: arg(1),
            name: parts.get(2..).unwrap_or_default().join(" "),
        },
        "%window-pane-changed" => TmuxNotification::WindowPaneChanged {
            window_id: arg(1),
            pane_id: arg(2),
        },
        "%session-changed" => TmuxNotification::SessionChanged {
            session_id: arg(1),
            name: parts.get(2..).unwrap_or_default().join(" "),
        },
        "%session-renamed" => TmuxNotification::SessionRenamed {
            name: parts.get(1..).unwrap_or_default().join(" "),
        },
        "%session-window-changed" => TmuxNotification::SessionWindowChanged {
            session_id: arg(1),
            window_id: arg(2),
        },
        "%sessions-changed" => TmuxNotification::SessionsChanged,
        "%unlinked-window-add" => TmuxNotification::UnlinkedWindowAdd {
            window_id: arg(1),
        },
        "%unlinked-window-close" => TmuxNotification::UnlinkedWindowClose {
            window_id: arg(1),
        },
        "%unlinked-window-renamed" => TmuxNotification::UnlinkedWindowRenamed {
            window_id: arg(1),
            name: parts.get(2..).unwrap_or_default().join(" "),
        },
        "%exit" => TmuxNotification::Exit {
            reason: parts
                .get(1..)
                .filter(|rest| !rest.is_empty())
                .map(|rest| rest.join(" ")),
        },
        "%message" => TmuxNotification::Message {
            text: parts.get(1..).unwrap_or_default().join(" "),
        },
        "%pane-mode-changed" => TmuxNotification::PaneModeChanged { pane_id: arg(1) },
        "%client-detached" => TmuxNotification::ClientDetached,
        other => TmuxNotification::Unhandled {
            name: other.to_string(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescapes_real_tmux_output_encoding() {
        // Byte-for-byte from a real tmux 3.2a capture. The pane printed
        // "PLAIN" + 0x01 0x02 + "BACK\SLASH" + ESC [ D + "é中" + CR LF.
        // Note the multi-byte characters arrive RAW, not octal-escaped.
        let raw = "PLAIN\\001\\002BACK\\134SLASH\\033[D\u{e9}\u{4e2d}\\015\\012";
        let decoded = unescape_output(raw);
        assert_eq!(
            decoded,
            "PLAIN\u{1}\u{2}BACK\\SLASH\u{1b}[D\u{e9}\u{4e2d}\r\n"
        );
    }

    #[test]
    fn passes_utf8_through_untouched() {
        // tmux only escapes control characters and backslash; valid UTF-8 is raw.
        assert_eq!(unescape_output("héllo 中"), "héllo 中");
    }

    #[test]
    fn handles_literal_double_backslash() {
        assert_eq!(unescape_output("a\\\\b"), "a\\b");
    }

    #[test]
    fn parses_output_notification() {
        let n = parse_line("%output %0 hello\r").expect("parses");
        assert_eq!(
            n,
            TmuxNotification::Output {
                pane_id: "%0".into(),
                data: "hello".into()
            }
        );
    }

    #[test]
    fn parses_output_with_empty_payload() {
        let n = parse_line("%output %3 ").expect("parses");
        assert_eq!(
            n,
            TmuxNotification::Output {
                pane_id: "%3".into(),
                data: String::new()
            }
        );
    }

    #[test]
    fn parses_output_payload_containing_spaces() {
        let n = parse_line("%output %1 a b  c").expect("parses");
        assert_eq!(
            n,
            TmuxNotification::Output {
                pane_id: "%1".into(),
                data: "a b  c".into()
            }
        );
    }

    #[test]
    fn parses_extended_output() {
        let n = parse_line("%extended-output %2 5 0 : hello there").expect("parses");
        assert_eq!(
            n,
            TmuxNotification::Output {
                pane_id: "%2".into(),
                data: "hello there".into()
            }
        );
    }

    #[test]
    fn parses_real_capture_lines() {
        // Taken verbatim from a real `tmux -CC` attach.
        assert_eq!(
            parse_line("%begin 1790220521 279 0"),
            Some(TmuxNotification::Begin {
                command_number: 279
            })
        );
        assert_eq!(
            parse_line("%end 1790220521 279 0"),
            Some(TmuxNotification::End {
                command_number: 279
            })
        );
        assert_eq!(
            parse_line("%session-changed $0 spike"),
            Some(TmuxNotification::SessionChanged {
                session_id: "$0".into(),
                name: "spike".into()
            })
        );
    }

    #[test]
    fn parses_layout_change_with_real_layout() {
        let n = parse_line(
            "%layout-change @0 d67e,80x24,0,0{40x24,0,0,0,39x24,41,0[39x12,41,0,1,39x11,41,13,2]} d67e,80x24,0,0{40x24,0,0,0,39x24,41,0[39x12,41,0,1,39x11,41,13,2]} *",
        )
        .expect("parses");
        match n {
            TmuxNotification::LayoutChange { window_id, layout } => {
                assert_eq!(window_id, "@0");
                assert!(layout.starts_with("d67e,80x24,0,0{"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parses_window_and_exit_notifications() {
        assert_eq!(
            parse_line("%window-add @1"),
            Some(TmuxNotification::WindowAdd {
                window_id: "@1".into()
            })
        );
        assert_eq!(
            parse_line("%window-renamed @1 my window"),
            Some(TmuxNotification::WindowRenamed {
                window_id: "@1".into(),
                name: "my window".into()
            })
        );
        assert_eq!(
            parse_line("%exit"),
            Some(TmuxNotification::Exit { reason: None })
        );
        assert_eq!(
            parse_line("%exit server exited"),
            Some(TmuxNotification::Exit {
                reason: Some("server exited".into())
            })
        );
    }

    #[test]
    fn ignores_blank_and_non_notification_lines() {
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("\r"), None);
        assert_eq!(parse_line("plain shell output"), None);
    }

    #[test]
    fn unknown_notification_is_reported_not_dropped() {
        assert_eq!(
            parse_line("%paste-buffer-changed buf0"),
            Some(TmuxNotification::Unhandled {
                name: "%paste-buffer-changed".into()
            })
        );
    }
}
