//! End-to-end test of the gateway against a **real** `tmux -CC` process.
//!
//! This is the strongest available verification of the backend: a real tmux
//! server, a real PTY, the real control-mode protocol, and the real gateway.
//! It exercises the full loop — marker detection, `list-windows`/`list-panes`
//! response correlation, layout parsing, per-pane output routing, and input
//! written back with `send-keys -H`.
//!
//! Skipped (with a printed note) when the `tmux` binary is unavailable.

#![cfg(test)]

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use super::gateway::{TmuxGateway, TmuxGatewaySink, TmuxPaneOutput};
use super::types::TmuxGatewaySnapshot;
use crate::core::session::{SessionCommand, session_command_channel};

#[derive(Default)]
struct CollectingSink {
    states: Mutex<Vec<TmuxGatewaySnapshot>>,
    outputs: Mutex<Vec<TmuxPaneOutput>>,
}

impl CollectingSink {
    fn latest_state(&self) -> Option<TmuxGatewaySnapshot> {
        self.states.lock().unwrap().last().cloned()
    }

    fn all_output(&self) -> String {
        self.outputs
            .lock()
            .unwrap()
            .iter()
            .map(|o| o.data.clone())
            .collect()
    }

    fn output_for(&self, pane_id: &str) -> String {
        self.outputs
            .lock()
            .unwrap()
            .iter()
            .filter(|o| o.pane_id == pane_id)
            .map(|o| o.data.clone())
            .collect()
    }
}

impl TmuxGatewaySink for CollectingSink {
    fn state(&self, snapshot: &TmuxGatewaySnapshot) {
        self.states.lock().unwrap().push(snapshot.clone());
    }

    fn pane_output(&self, output: &TmuxPaneOutput) {
        self.outputs.lock().unwrap().push(output.clone());
    }
}

fn tmux_available() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Poll `condition` until it holds or `timeout` elapses.
fn wait_until<F: Fn() -> bool>(condition: F, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    condition()
}

#[test]
fn gateway_drives_a_real_tmux_control_client() {
    if !tmux_available() {
        eprintln!("skipping: tmux binary not available");
        return;
    }

    let socket = std::env::temp_dir().join(format!(
        "nyaterm-tmux-itest-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    let socket_path = socket.to_string_lossy().to_string();

    // A fresh server per test run keeps this hermetic.
    let _ = std::process::Command::new("tmux")
        .args(["-S", &socket_path, "kill-server"])
        .output();

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open pty");

    let mut command = CommandBuilder::new("tmux");
    command.args(["-S", &socket_path, "-CC", "new-session", "-A", "-s", "itest"]);
    command.env("TERM", "xterm-256color");
    let mut child = pair.slave.spawn_command(command).expect("spawn tmux -CC");
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("pty reader");
    let mut writer = pair.master.take_writer().expect("pty writer");

    let (command_tx, mut command_rx) = session_command_channel("itest");

    let sink = Arc::new(CollectingSink::default());
    let gateway = TmuxGateway::new(
        "itest".to_string(),
        sink.clone(),
        command_tx,
    );

    // Reader: real tmux bytes -> gateway.
    let reader_gateway = gateway.clone();
    let reader_thread = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                    reader_gateway.filter(&text);
                }
            }
        }
    });

    // Command sink: gateway output -> real tmux stdin.
    let command_thread = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            match command_rx.try_recv() {
                Ok(SessionCommand::Write { data, .. }) => {
                    if writer.write_all(&data).is_err() || writer.flush().is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => std::thread::sleep(Duration::from_millis(2)),
            }
        }
    });

    // ---- 1. Marker detection + structural discovery -----------------------
    let discovered = wait_until(
        || {
            sink.latest_state()
                .is_some_and(|state| !state.windows.is_empty() && state.windows.iter().any(|w| !w.panes.is_empty()))
        },
        Duration::from_secs(20),
    );
    let state = sink.latest_state().unwrap_or_default();
    assert!(
        discovered,
        "gateway never discovered a window with panes; state={state:?}"
    );
    assert!(
        gateway.is_active(),
        "gateway should be active after seeing the marker"
    );

    // tmux must have reported a session name via %session-changed.
    assert_eq!(state.session_name, "itest", "state={state:?}");

    let window = state.windows.first().expect("a window");
    assert!(window.id.starts_with('@'), "window id looks wrong: {window:?}");

    let pane = window
        .panes
        .first()
        .expect("at least one pane")
        .clone();
    let pane_id = pane
        .id
        .clone()
        .expect("list-panes response should have attached a %N pane id");
    assert!(pane_id.starts_with('%'), "pane id looks wrong: {pane_id}");

    // Layout must have been parsed into real geometry.
    assert!(
        window.layout.is_some(),
        "window layout should have been parsed: {window:?}"
    );
    assert!(pane.width > 0 && pane.height > 0, "pane geometry: {pane:?}");

    // ---- 2. Input round-trip through send-keys -H -------------------------
    let token = "NYATERM_TMUX_ROUNDTRIP_OK";
    gateway.send_pane_input(&pane_id, &format!("echo {token}\r"));

    let echoed = wait_until(
        || sink.output_for(&pane_id).contains(token),
        Duration::from_secs(20),
    );
    assert!(
        echoed,
        "pane never echoed the token; captured pane output was: {:?}",
        sink.output_for(&pane_id)
    );

    // ---- 2b. Panes created after startup must resolve to a `%N` id --------
    // `split-window` introduces a pane index the gateway has never seen. If the
    // id is not resolved, the frontend renders the pane but cannot route input
    // to it — the pane looks live and silently ignores every click and key.
    gateway.run_ui_command("split-window -h -t itest");
    let split_ready = wait_until(
        || {
            sink.latest_state().is_some_and(|state| {
                state
                    .windows
                    .iter()
                    .any(|window| window.panes.len() >= 2)
            })
        },
        Duration::from_secs(20),
    );
    let state = sink.latest_state().unwrap_or_default();
    assert!(split_ready, "split-window should add a pane; state={state:?}");

    for window in &state.windows {
        for pane in &window.panes {
            assert!(
                pane.id.is_some(),
                "pane {} in window {} has no resolved id, so the frontend \
                 cannot route input to it: {window:?}",
                pane.index,
                window.id
            );
        }
    }

    // ---- 3. The protocol must not leak to xterm ---------------------------
    // The gateway reports xterm-bound bytes through filter()'s return value, so
    // anything it consumed is invisible here by construction. What we can assert
    // is that the collected *pane* output carried the shell text rather than
    // protocol framing.
    let pane_output = sink.output_for(&pane_id);
    assert!(
        !pane_output.contains("%output"),
        "pane output must not contain protocol framing: {pane_output:?}"
    );
    assert!(
        !pane_output.contains("\u{1b}P1000p"),
        "pane output must not contain the control marker: {pane_output:?}"
    );

    // ---- 4. A new window shows up as a second entry -----------------------
    // `-d` creates the window without displaying it. tmux does not reliably
    // emit `%layout-change` in that case, so the gateway must query the layout
    // for the new window itself — otherwise the frontend renders a tab with no
    // panes. This assertion is the regression guard for that.
    gateway.run_ui_command("new-window -d -t itest");
    let second_window_ready = wait_until(
        || {
            sink.latest_state().is_some_and(|state| {
                state
                    .windows
                    .iter()
                    .filter(|window| window.layout.is_some() && !window.panes.is_empty())
                    .count()
                    >= 2
            })
        },
        Duration::from_secs(20),
    );
    let state = sink.latest_state().unwrap_or_default();
    assert!(
        second_window_ready,
        "both windows should have a layout and panes; state={state:?}"
    );
    assert!(
        state.windows.len() >= 2,
        "new-window should have produced a second window; state={state:?}"
    );

    // Every window the gateway reports must be renderable by the frontend.
    for window in &state.windows {
        assert!(
            window.layout.is_some(),
            "window {} has no parsed layout: {window:?}",
            window.id
        );
        assert!(
            !window.panes.is_empty(),
            "window {} has no panes: {window:?}",
            window.id
        );
    }

    // ---- 5. Killing a window removes it -----------------------------------
    let victim = state
        .windows
        .iter()
        .find(|w| state.active_window_id.as_deref() != Some(w.id.as_str()))
        .or_else(|| state.windows.first())
        .map(|w| w.id.clone())
        .expect("a window to kill");
    gateway.run_ui_command(&format!("kill-window -t {victim}"));
    let removed = wait_until(
        || {
            sink.latest_state()
                .is_some_and(|state| state.windows.len() < 2)
        },
        Duration::from_secs(20),
    );
    let state = sink.latest_state().unwrap_or_default();
    assert!(
        removed,
        "kill-window should have removed a window; state={state:?}"
    );

    // ---- teardown ---------------------------------------------------------
    gateway.run_ui_command("kill-server");
    std::thread::sleep(Duration::from_millis(200));
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::process::Command::new("tmux")
        .args(["-S", &socket_path, "kill-server"])
        .output();
    let _ = std::fs::remove_file(&socket);

    drop(gateway);
    let _ = reader_thread.join();
    let _ = command_thread.join();
    let _ = sink.all_output();
}
