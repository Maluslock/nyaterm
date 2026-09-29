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

use super::gateway::{TmuxCommandResponse, TmuxGateway, TmuxGatewaySink, TmuxPaneOutput};
use super::types::TmuxGatewaySnapshot;
use crate::core::session::{SessionCommand, session_command_channel};

#[derive(Default)]
struct CollectingSink {
    states: Mutex<Vec<TmuxGatewaySnapshot>>,
    outputs: Mutex<Vec<TmuxPaneOutput>>,
    responses: Mutex<Vec<TmuxCommandResponse>>,
}

impl CollectingSink {
    fn response_for(&self, request_id: &str) -> Option<TmuxCommandResponse> {
        self.responses
            .lock()
            .unwrap()
            .iter()
            .find(|response| response.request_id == request_id)
            .cloned()
    }

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

    fn command_response(&self, response: &TmuxCommandResponse) {
        self.responses.lock().unwrap().push(response.clone());
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

    // ---- 2b2. Injected input and UI commands go through the pane ----------
    // App-level writes must land in the pane: tmux parses session writes as its
    // own command lines, so a quick command like `ls -la` would otherwise be a
    // parse error. `active_pane_id` is what the SSH write path consults.
    let active_pane = gateway.active_pane_id().expect("an active pane");
    let expected_active = sink
        .latest_state()
        .unwrap_or_default()
        .windows
        .iter()
        .flat_map(|window| window.panes.iter())
        .find(|pane| pane.active)
        .and_then(|pane| pane.id.clone())
        .expect("a pane tmux marked active");
    assert_eq!(
        active_pane, expected_active,
        "active pane should be the pane tmux reports as active"
    );

    // UI commands report their answer (and their error) back to the caller.
    gateway.run_command_with_reply("list-windows -F '#{window_id}'", "req-ok");
    let answered = wait_until(
        || {
            sink.response_for("req-ok")
                .is_some_and(|response| response.error.is_none() && !response.output.is_empty())
        },
        Duration::from_secs(10),
    );
    let response = sink.response_for("req-ok").expect("a response");
    assert!(answered, "no answer to a valid command: {response:?}");
    assert!(
        response.output.contains('@'),
        "unexpected reply payload: {response:?}"
    );

    gateway.run_command_with_reply("nyaterm-not-a-command", "req-bad");
    let rejected = wait_until(
        || {
            sink.response_for("req-bad")
                .is_some_and(|response| response.error.is_some())
        },
        Duration::from_secs(10),
    );
    let response = sink.response_for("req-bad").expect("an error response");
    assert!(rejected, "a bad command should report %error: {response:?}");

    // ---- 2c. A window created later has pane index 0 but a fresh `%N` id --
    // The layout's leaf number is the pane *id* number, not `#{pane_index}`:
    // every window created after the first one reports pane_index 0 while its
    // pane id is %1 or higher. Keying ids by index therefore leaves such panes
    // without an id, which is what rendered an empty (black) pane in the app.
    let existing_window_ids: Vec<String> = sink
        .latest_state()
        .unwrap_or_default()
        .windows
        .iter()
        .map(|window| window.id.clone())
        .collect();
    gateway.run_ui_command("new-window -t itest");
    let second_window_ready = wait_until(
        || {
            sink.latest_state().is_some_and(|state| {
                state.windows.iter().any(|window| {
                    !existing_window_ids.contains(&window.id)
                        && !window.panes.is_empty()
                        && window.panes.iter().all(|pane| pane.id.is_some())
                })
            })
        },
        Duration::from_secs(20),
    );
    assert!(
        second_window_ready,
        "a window created after startup must resolve its pane ids; state={:?}",
        sink.latest_state().unwrap_or_default()
    );

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
    let before_kill = sink.latest_state().unwrap_or_default();
    let victim = before_kill
        .windows
        .iter()
        .find(|w| before_kill.active_window_id.as_deref() != Some(w.id.as_str()))
        .or_else(|| before_kill.windows.first())
        .map(|w| w.id.clone())
        .expect("a window to kill");
    gateway.run_ui_command(&format!("kill-window -t {victim}"));
    // Assert on the victim's absence rather than on a window count: other
    // windows created by earlier stages can still be arriving, which makes a
    // count-based check race the snapshot.
    let removed = wait_until(
        || {
            sink.latest_state().is_some_and(|state| {
                !state.windows.iter().any(|window| window.id == victim)
            })
        },
        Duration::from_secs(20),
    );
    let state = sink.latest_state().unwrap_or_default();
    assert!(
        removed,
        "kill-window should have removed window {victim}; windows_before={:?}; state={state:?}",
        before_kill.windows.iter().map(|w| w.id.clone()).collect::<Vec<_>>()
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
