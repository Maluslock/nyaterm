#[cfg(test)]
mod tests {
    use super::{
        DO, IAC, OPT_NAWS, OPT_SUPPRESS_GO_AHEAD, TelnetEnterMode, TelnetLineEditor,
        TelnetAutoLogin, TelnetAutoLoginAction, TelnetAutoLoginConfig,
        TelnetAutoLoginCredentials, TelnetSessionConfig, WILL, maybe_build_naws,
        await_telnet_connection, escape_telnet_application_data, negotiate_response, normalize_enter_bytes, split_write_chunks,
        strip_telnet_commands,
    };
    use crate::error::AppError;
    use std::time::Instant;
    use tokio::sync::oneshot;

    #[test]
    fn standard_negotiation_responds_by_default() {
        assert_eq!(
            negotiate_response(WILL, OPT_SUPPRESS_GO_AHEAD, true, true),
            vec![IAC, DO, OPT_SUPPRESS_GO_AHEAD]
        );
    }

    #[tokio::test]
    async fn connection_wait_can_be_cancelled_before_connect_finishes() {
        let (cancel_tx, cancel_rx) = oneshot::channel();
        cancel_tx.send(()).expect("send cancellation");

        let result = await_telnet_connection(
            std::future::pending::<std::io::Result<()>>(),
            Some(cancel_rx),
        )
        .await;

        assert!(matches!(
            result,
            Err(AppError::Cancelled(message)) if message == "Session creation cancelled"
        ));
    }

    #[tokio::test]
    async fn connection_wait_returns_connect_result_without_cancellation() {
        let result = await_telnet_connection(
            std::future::ready(Ok::<_, std::io::Error>(42_u8)),
            None,
        )
        .await
        .expect("connect result");

        assert_eq!(result, 42);
    }

    #[test]
    fn send_sga_false_rejects_sga() {
        assert_ne!(
            negotiate_response(WILL, OPT_SUPPRESS_GO_AHEAD, true, false),
            vec![IAC, DO, OPT_SUPPRESS_GO_AHEAD]
        );
    }

    #[test]
    fn send_naws_false_rejects_naws_negotiation() {
        assert_ne!(
            negotiate_response(DO, OPT_NAWS, false, true),
            vec![IAC, WILL, OPT_NAWS]
        );
    }

    #[test]
    fn raw_mode_can_suppress_negotiation_responses() {
        let config = TelnetSessionConfig {
            raw_tcp_cli: true,
            ..Default::default()
        };
        let mut responses = Vec::new();
        if !config.raw_tcp_cli {
            let _ = strip_telnet_commands(&[IAC, WILL, OPT_SUPPRESS_GO_AHEAD], &mut |cmd, opt| {
                responses.push(negotiate_response(
                    cmd,
                    opt,
                    config.send_naws,
                    config.send_sga,
                ));
            });
        }
        assert!(responses.is_empty());
    }

    #[test]
    fn binary_input_escapes_iac_only_for_telnet_transport() {
        let mouse_report = b"\x1b[M \xff\x80";
        assert_eq!(
            escape_telnet_application_data(mouse_report, false),
            b"\x1b[M \xff\xff\x80"
        );
        assert_eq!(
            escape_telnet_application_data(mouse_report, true),
            mouse_report
        );
    }

    #[test]
    fn send_naws_false_prevents_naws_resize_payload() {
        let config = TelnetSessionConfig {
            send_naws: false,
            ..Default::default()
        };
        assert!(maybe_build_naws(80, 24, &config).is_none());
    }

    #[test]
    fn raw_mode_prevents_naws_resize_payload() {
        let config = TelnetSessionConfig {
            raw_tcp_cli: true,
            ..Default::default()
        };
        assert!(maybe_build_naws(80, 24, &config).is_none());
    }

    #[test]
    fn enter_conversion_maps_carriage_return() {
        assert_eq!(
            normalize_enter_bytes(b"show\r", TelnetEnterMode::Crlf),
            b"show\r\n"
        );
        assert_eq!(
            normalize_enter_bytes(b"show\r", TelnetEnterMode::Cr),
            b"show\r"
        );
        assert_eq!(
            normalize_enter_bytes(b"show\r", TelnetEnterMode::Lf),
            b"show\n"
        );
    }

    #[test]
    fn force_character_at_a_time_preserves_utf8_order() {
        let chunks = split_write_chunks("a中\r".as_bytes(), true);
        assert_eq!(
            chunks,
            vec![b"a".to_vec(), "中".as_bytes().to_vec(), b"\r".to_vec()]
        );
        let joined: Vec<u8> = chunks.into_iter().flatten().collect();
        assert_eq!(joined, "a中\r".as_bytes());
    }

    #[test]
    fn strip_telnet_commands_emits_naws_response_request() {
        let mut seen = Vec::new();
        let visible = strip_telnet_commands(b"hi\xff\xfd\x1f", &mut |cmd, opt| {
            seen.push((cmd, opt));
        });
        assert_eq!(visible, b"hi");
        assert_eq!(seen, vec![(DO, OPT_NAWS)]);
    }

    #[test]
    fn local_line_editor_backspace_updates_buffer() {
        let mut editor = TelnetLineEditor::default();
        let result = editor.process(b"abc\x7f", TelnetEnterMode::Cr);

        assert_eq!(editor.buffer(), "ab");
        assert_eq!(result.display, "abc\x08 \x08");
        assert!(result.writes.is_empty());
    }

    #[test]
    fn local_line_editor_sends_buffer_on_enter() {
        let mut editor = TelnetLineEditor::default();
        let result = editor.process(b"abc\x7fd\r", TelnetEnterMode::Cr);

        assert_eq!(editor.buffer(), "");
        assert_eq!(result.writes, vec![b"abd\r".to_vec()]);
        assert_eq!(result.display, "abc\x08 \x08d\r\n");

        let mut editor = TelnetLineEditor::default();
        let result = editor.process(b"abc\x7fd\r", TelnetEnterMode::Crlf);
        assert_eq!(result.writes, vec![b"abd\r\n".to_vec()]);

        let mut editor = TelnetLineEditor::default();
        let result = editor.process(b"abc\x7fd\r", TelnetEnterMode::Lf);
        assert_eq!(result.writes, vec![b"abd\n".to_vec()]);
    }

    #[test]
    fn local_line_editor_backspace_removes_one_utf8_char() {
        let mut editor = TelnetLineEditor::default();
        let result = editor.process("中a\u{7f}".as_bytes(), TelnetEnterMode::Cr);

        assert_eq!(editor.buffer(), "中");
        assert_eq!(result.display, "中a\x08 \x08");

        let result = editor.process(b"\x7f", TelnetEnterMode::Cr);
        assert_eq!(editor.buffer(), "");
        assert_eq!(result.display, "\x08 \x08");
    }

    #[test]
    fn local_line_editor_passes_controls_without_buffering() {
        let mut editor = TelnetLineEditor::default();
        let result = editor.process(b"a\x03\x04\x1b[A\x1b[3~", TelnetEnterMode::Cr);

        assert_eq!(editor.buffer(), "");
        assert_eq!(
            result.writes,
            vec![vec![0x03], vec![0x04], b"\x1b[A".to_vec()]
        );
        assert_eq!(result.display, "a\x08 \x08");
    }

    fn auto_login() -> TelnetAutoLogin {
        TelnetAutoLogin::new(
            TelnetAutoLoginConfig::default(),
            TelnetAutoLoginCredentials {
                username: "admin".to_string(),
                password: Some("secret".to_string()),
            },
            TelnetEnterMode::Cr,
            Instant::now(),
        )
        .expect("auto login")
    }

    fn sent_payloads(actions: Vec<TelnetAutoLoginAction>) -> Vec<Vec<u8>> {
        actions
            .into_iter()
            .filter_map(|action| match action {
                TelnetAutoLoginAction::Send(data) => Some(data),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn auto_login_detects_split_username_and_password_prompts() {
        let mut login = auto_login();

        assert!(login.handle_text("User", Instant::now()).is_empty());
        assert_eq!(
            sent_payloads(login.handle_text("name:", Instant::now())),
            vec![b"admin\r".to_vec()]
        );
        assert!(login.handle_text("Pass", Instant::now()).is_empty());
        assert_eq!(
            sent_payloads(login.handle_text("word:", Instant::now())),
            vec![b"secret\r".to_vec()]
        );
    }

    #[test]
    fn auto_login_detects_chinese_prompts() {
        let mut login = auto_login();

        assert_eq!(
            sent_payloads(login.handle_text("用户名：", Instant::now())),
            vec![b"admin\r".to_vec()]
        );
        assert_eq!(
            sent_payloads(login.handle_text("密码：", Instant::now())),
            vec![b"secret\r".to_vec()]
        );
    }

    #[test]
    fn auto_login_detects_linux_getty_host_login_prompt() {
        let mut login = auto_login();

        assert_eq!(
            sent_payloads(login.handle_text(
                "\r\nLinux 6.8.0-107-generic (kaikai) (pts/3)\r\n\r\nkaikai login: ",
                Instant::now()
            )),
            vec![b"admin\r".to_vec()]
        );
    }

    #[test]
    fn auto_login_detects_password_prompt_without_colon() {
        let mut login = auto_login();

        assert_eq!(
            sent_payloads(login.handle_text("Input Password", Instant::now())),
            vec![b"secret\r".to_vec()]
        );
    }

    #[test]
    fn auto_login_sends_wake_enter_once() {
        let mut login = auto_login();

        assert_eq!(
            sent_payloads(login.handle_text("Press RETURN to get started.", Instant::now())),
            vec![b"\r".to_vec()]
        );
        assert!(login
            .handle_text("Press RETURN to get started.", Instant::now())
            .is_empty());
    }

    #[test]
    fn auto_login_does_not_treat_last_login_as_username_prompt() {
        let mut login = auto_login();

        assert!(login
            .handle_text("Last login: Mon Jul 13 12:00:00", Instant::now())
            .is_empty());
        assert!(login
            .handle_text("Previous login: Mon Jul 13 12:00:00", Instant::now())
            .is_empty());
    }

    #[test]
    fn auto_login_sends_username_before_password_in_same_chunk() {
        let mut login = auto_login();

        assert_eq!(
            sent_payloads(login.handle_text("Username:\nPassword:", Instant::now())),
            vec![b"admin\r".to_vec(), b"secret\r".to_vec()]
        );
    }

    #[test]
    fn auto_login_completes_on_shell_prompt_after_credentials() {
        let mut login = auto_login();
        let _ = login.handle_text("Username:", Instant::now());
        let _ = login.handle_text("Password:", Instant::now());

        assert!(login
            .handle_text("router#", Instant::now())
            .contains(&TelnetAutoLoginAction::Complete));
    }

    #[test]
    fn auto_login_failure_retries_then_disables() {
        let mut login = TelnetAutoLogin::new(
            TelnetAutoLoginConfig {
                max_retries: 1,
                ..TelnetAutoLoginConfig::default()
            },
            TelnetAutoLoginCredentials {
                username: String::new(),
                password: Some("secret".to_string()),
            },
            TelnetEnterMode::Cr,
            Instant::now(),
        )
        .expect("auto login");

        assert_eq!(
            sent_payloads(login.handle_text("Password:", Instant::now())),
            vec![b"secret\r".to_vec()]
        );
        assert!(login
            .handle_text("Authentication failed", Instant::now())
            .is_empty());
        assert_eq!(
            sent_payloads(login.handle_text("Password:", Instant::now())),
            vec![b"secret\r".to_vec()]
        );
        assert!(login
            .handle_text("Authentication failed", Instant::now())
            .contains(&TelnetAutoLoginAction::Disable));
    }

    #[test]
    fn auto_login_user_input_disables_but_automated_input_does_not() {
        let mut login = auto_login();

        assert_eq!(login.handle_user_input(true), None);
        assert_eq!(
            login.handle_user_input(false),
            Some(TelnetAutoLoginAction::Disable)
        );
        assert!(login.handle_text("Username:", Instant::now()).is_empty());
    }
}
