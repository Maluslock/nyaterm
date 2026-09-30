fn negotiate_response(command: u8, option: u8, send_naws: bool, send_sga: bool) -> Vec<u8> {
    match command {
        WILL => {
            if option == OPT_ECHO || (send_sga && option == OPT_SUPPRESS_GO_AHEAD) {
                vec![IAC, DO, option]
            } else {
                vec![IAC, DONT, option]
            }
        }
        DO => {
            if send_naws && option == OPT_NAWS {
                vec![IAC, WILL, option]
            } else {
                vec![IAC, WONT, option]
            }
        }
        WONT => vec![IAC, DONT, option],
        DONT => vec![IAC, WONT, option],
        _ => vec![],
    }
}

/// Build a NAWS (Negotiate About Window Size) subnegotiation sequence.
fn build_naws(cols: u16, rows: u16) -> Vec<u8> {
    vec![
        IAC,
        SB,
        OPT_NAWS,
        (cols >> 8) as u8,
        (cols & 0xff) as u8,
        (rows >> 8) as u8,
        (rows & 0xff) as u8,
        IAC,
        SE,
    ]
}

fn maybe_build_naws(cols: u16, rows: u16, config: &TelnetSessionConfig) -> Option<Vec<u8>> {
    if config.raw_tcp_cli || !config.send_naws {
        None
    } else {
        Some(build_naws(cols, rows))
    }
}

fn unescape_iac_iac(data: &[u8]) -> Vec<u8> {
    let mut visible = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == IAC && i + 1 < data.len() && data[i + 1] == IAC {
            visible.push(IAC);
            i += 2;
        } else {
            visible.push(data[i]);
            i += 1;
        }
    }
    visible
}

fn escape_telnet_application_data(data: &[u8], raw_tcp_cli: bool) -> Vec<u8> {
    if raw_tcp_cli {
        return data.to_vec();
    }

    let mut escaped = Vec::with_capacity(data.len());
    for &byte in data {
        escaped.push(byte);
        if byte == IAC {
            escaped.push(IAC);
        }
    }
    escaped
}

/// Strip IAC sequences from raw data, returning only user-visible bytes.
/// Calls `on_negotiate` for each IAC command/option pair encountered.
fn strip_telnet_commands(data: &[u8], on_negotiate: &mut impl FnMut(u8, u8)) -> Vec<u8> {
    let mut visible = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == IAC && i + 1 < data.len() {
            let cmd = data[i + 1];
            match cmd {
                IAC => {
                    visible.push(IAC);
                    i += 2;
                }
                WILL | WONT | DO | DONT => {
                    if i + 2 < data.len() {
                        on_negotiate(cmd, data[i + 2]);
                        i += 3;
                    } else {
                        i += 2;
                    }
                }
                SB => {
                    // Skip subnegotiation until IAC SE
                    i += 2;
                    while i < data.len() {
                        if data[i] == IAC && i + 1 < data.len() && data[i + 1] == SE {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    i += 2;
                }
            }
        } else {
            visible.push(data[i]);
            i += 1;
        }
    }
    visible
}

fn normalize_enter_bytes(data: &[u8], enter_mode: TelnetEnterMode) -> Vec<u8> {
    let replacement: &[u8] = match enter_mode {
        TelnetEnterMode::Crlf => b"\r\n",
        TelnetEnterMode::Cr => b"\r",
        TelnetEnterMode::Lf => b"\n",
    };
    let mut normalized = Vec::with_capacity(data.len());
    for byte in data {
        if *byte == b'\r' {
            normalized.extend_from_slice(replacement);
        } else {
            normalized.push(*byte);
        }
    }
    normalized
}

fn enter_bytes(enter_mode: TelnetEnterMode) -> &'static [u8] {
    match enter_mode {
        TelnetEnterMode::Crlf => b"\r\n",
        TelnetEnterMode::Cr => b"\r",
        TelnetEnterMode::Lf => b"\n",
    }
}

fn split_write_chunks(data: &[u8], force_character_at_a_time: bool) -> Vec<Vec<u8>> {
    if !force_character_at_a_time {
        return vec![data.to_vec()];
    }

    String::from_utf8_lossy(data)
        .chars()
        .map(|ch| {
            let mut buf = [0u8; 4];
            ch.encode_utf8(&mut buf).as_bytes().to_vec()
        })
        .collect()
}
