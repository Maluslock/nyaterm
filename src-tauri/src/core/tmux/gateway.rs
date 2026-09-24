//! The tmux control-mode gateway.
//!
//! Sits in the session output path *before* the output coalescer forwards
//! anything to the webview. It watches for the 7-byte control-mode marker and,
//! once seen, consumes the whole control-protocol stream and republishes it as
//! structured per-pane events.
//!
//! Consuming (rather than forwarding) the protocol is mandatory, not stylistic:
//! an unterminated `ESC P` puts xterm.js into DCS state, after which it swallows
//! every subsequent byte for the life of the terminal. See
//! `spike/xterm-dcs-probe` for the experiment that established this.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::layout::{apply_pane_ids, mark_active, parse_layout};
use super::protocol::{parse_line, TmuxNotification};
use super::types::{TmuxGatewaySnapshot, TmuxWindow};
use crate::core::recording::{InputOrigin, InputSensitivity};
use crate::core::session::{SessionCommand, SessionCommandSender};

/// `tmux -CC` writes exactly these seven bytes, with no ST terminator.
pub const CONTROL_MARKER: &str = "\x1bP1000p";

/// Longest proper prefix of the marker we hold back while awaiting more input.
const MARKER_MAX_HOLD: usize = CONTROL_MARKER.len() - 1;

/// Hard cap on a single buffered protocol line, so a stream that never sends a
/// newline cannot grow without bound.
const MAX_LINE_BYTES: usize = 1 << 20;

/// Per-pane output pushed to the frontend.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TmuxPaneOutput {
    pub pane_id: String,
    pub data: String,
}

/// Response lines are tagged with a literal prefix from our own format string.
///
/// Correlating responses by block order is unreliable: tmux emits an extra empty
/// `%begin`/`%end` block during client setup. Tagging the payload makes each
/// response self-identifying no matter how blocks are batched.
const WINDOW_TAG: &str = "NYATERM-WINDOW:";
const PANE_TAG: &str = "NYATERM-PANE:";

fn windows_command(target: Option<&str>) -> String {
    let target = target.map(|id| format!(" -t {id}")).unwrap_or_default();
    format!(
        "list-windows{target} -F '{WINDOW_TAG}#{{window_id}}\t#{{window_index}}\t#{{window_name}}\t#{{window_active}}\t#{{window_layout}}'"
    )
}

fn panes_command(window_id: Option<&str>) -> String {
    let target = window_id.map(|id| format!(" -t {id}")).unwrap_or_else(|| " -a".to_string());
    format!(
        "list-panes{target} -F '{PANE_TAG}#{{window_id}}\t#{{pane_index}}\t#{{pane_id}}\t#{{pane_active}}'"
    )
}

#[derive(Default)]
struct Inner {
    active: bool,
    /// Bytes held back while the marker may still be split across chunks.
    pre: String,
    /// Partial trailing control-protocol line.
    line: String,
    snapshot: TmuxGatewaySnapshot,
    /// window id -> (pane index -> stable `%N` pane id)
    pane_ids: HashMap<String, HashMap<u32, String>>,
    /// window id -> active pane index reported by tmux
    active_pane_index: HashMap<String, u32>,
    /// Lines captured inside the current `%begin`..`%end` block.
    block_lines: Vec<String>,
    in_block: bool,
    /// Side effects collected under the lock, performed after it is released.
    state_dirty: bool,
    pane_outputs: Vec<(String, String)>,
    outgoing: Vec<String>,
}

impl Inner {
    fn mark_state_dirty(&mut self) {
        self.state_dirty = true;
    }

    fn find_window_mut(&mut self, window_id: &str) -> Option<&mut TmuxWindow> {
        self.snapshot
            .windows
            .iter_mut()
            .find(|window| window.id == window_id)
    }
}

/// Where gateway events go.
///
/// Abstracted so the gateway can be driven end-to-end by tests against a real
/// `tmux -CC` process, without a Tauri app handle.
pub trait TmuxGatewaySink: Send + Sync + 'static {
    fn state(&self, snapshot: &TmuxGatewaySnapshot);
    fn pane_output(&self, output: &TmuxPaneOutput);
}

/// Production sink: emits Tauri events to the webview.
pub struct AppGatewaySink {
    app: AppHandle,
    state_event: String,
    pane_event: String,
}

impl AppGatewaySink {
    pub fn new(app: AppHandle, session_id: &str) -> Self {
        Self {
            app,
            state_event: format!("tmux-state-{session_id}"),
            pane_event: format!("tmux-pane-output-{session_id}"),
        }
    }
}

impl TmuxGatewaySink for AppGatewaySink {
    fn state(&self, snapshot: &TmuxGatewaySnapshot) {
        let _ = self.app.emit(&self.state_event, snapshot);
    }

    fn pane_output(&self, output: &TmuxPaneOutput) {
        let _ = self.app.emit(&self.pane_event, output);
    }
}

pub struct TmuxGateway {
    session_id: String,
    sink: Arc<dyn TmuxGatewaySink>,
    command_tx: SessionCommandSender,
    inner: Mutex<Inner>,
}

impl TmuxGateway {
    pub fn new(
        session_id: String,
        sink: Arc<dyn TmuxGatewaySink>,
        command_tx: SessionCommandSender,
    ) -> Arc<Self> {
        Arc::new(Self {
            session_id,
            sink,
            command_tx,
            inner: Mutex::new(Inner::default()),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::warn!("tmux gateway state mutex was poisoned; recovering");
                let guard = poisoned.into_inner();
                self.inner.clear_poison();
                guard
            }
        }
    }

    pub fn is_active(&self) -> bool {
        self.lock().active
    }

    pub fn snapshot(&self) -> TmuxGatewaySnapshot {
        self.lock().snapshot.clone()
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Write one control-mode command line back to tmux.
    fn write_command(&self, command: &str) {
        let mut data = command.as_bytes().to_vec();
        data.push(b'\n');
        let _ = self.command_tx.send(SessionCommand::Write {
            data,
            automated: true,
            origin: InputOrigin::TerminalResponse,
            sensitivity: InputSensitivity::Normal,
        });
    }

    /// Perform collected side effects now that the gateway lock is released.
    fn flush(
        &self,
        outgoing: Vec<String>,
        state: Option<TmuxGatewaySnapshot>,
        pane_outputs: Vec<(String, String)>,
    ) {
        for command in outgoing {
            self.write_command(&command);
        }
        if let Some(snapshot) = state {
            self.sink.state(&snapshot);
        }
        for (pane_id, data) in pane_outputs {
            if data.is_empty() {
                continue;
            }
            self.sink.pane_output(&TmuxPaneOutput { pane_id, data });
        }
    }

    /// Filter session output.
    ///
    /// Returns `Some(text)` for bytes that should still reach xterm, or `None`
    /// when the gateway consumed them.
    pub fn filter(&self, text: &str) -> Option<String> {
        if text.is_empty() {
            return None;
        }

        let mut inner = self.lock();
        let passthrough = if inner.active {
            consume_protocol(&mut inner, text);
            None
        } else {
            self.detect_marker(&mut inner, text)
        };

        let state = if inner.state_dirty {
            inner.state_dirty = false;
            Some(inner.snapshot.clone())
        } else {
            None
        };
        let pane_outputs = std::mem::take(&mut inner.pane_outputs);
        let outgoing = std::mem::take(&mut inner.outgoing);
        drop(inner);

        self.flush(outgoing, state, pane_outputs);
        passthrough
    }

    /// While inactive, watch for the marker without swallowing ordinary output.
    fn detect_marker(&self, inner: &mut Inner, text: &str) -> Option<String> {
        let mut buffer = std::mem::take(&mut inner.pre);
        buffer.push_str(text);

        let Some(index) = buffer.find(CONTROL_MARKER) else {
            let (emit, hold) = split_holdable_tail(&buffer);
            inner.pre = hold;
            return if emit.is_empty() { None } else { Some(emit) };
        };

        let before = buffer[..index].to_string();
        let after = buffer[index + CONTROL_MARKER.len()..].to_string();

        inner.active = true;
        inner.snapshot = TmuxGatewaySnapshot {
            client_width: 80,
            client_height: 24,
            ..Default::default()
        };
        inner.mark_state_dirty();

        tracing::info!(
            session_id = %self.session_id,
            prefix_bytes = before.len(),
            "tmux control mode detected; gateway engaged"
        );

        // Ask tmux for the initial structure. Responses are identified by the
        // tag prefixes rather than by block order.
        inner.outgoing.push(windows_command(None));
        inner.outgoing.push(panes_command(None));

        if !after.is_empty() {
            consume_protocol(inner, &after);
        }

        if before.is_empty() {
            None
        } else {
            Some(before)
        }
    }

    /// Tell tmux the control client's geometry.
    pub fn resize(&self, width: u16, height: u16) {
        let width = width.max(1);
        let height = height.max(1);
        let command = {
            let mut inner = self.lock();
            if !inner.active
                || (inner.snapshot.client_width == width && inner.snapshot.client_height == height)
            {
                return;
            }
            inner.snapshot.client_width = width;
            inner.snapshot.client_height = height;
            format!("refresh-client -C {width}x{height}")
        };
        self.write_command(&command);
    }

    /// Send raw bytes to a tmux pane as if typed.
    ///
    /// Uses `send-keys -H` so every byte (control characters, ESC sequences,
    /// UTF-8) reaches the pane verbatim — verified against real tmux.
    pub fn send_pane_input(&self, pane_id: &str, data: &str) {
        if data.is_empty() {
            return;
        }
        for chunk in data.as_bytes().chunks(240) {
            let mut command = format!("send-keys -H -t {pane_id}");
            for byte in chunk {
                command.push(' ');
                command.push_str(&format!("{byte:02x}"));
            }
            self.write_command(&command);
        }
    }

    /// Run an arbitrary tmux command on behalf of the UI.
    pub fn run_ui_command(&self, command: &str) {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            return;
        }
        self.write_command(trimmed);
    }
}

/// Consume complete protocol lines out of `text`.
fn consume_protocol(inner: &mut Inner, text: &str) {
    inner.line.push_str(text);
    while let Some(index) = inner.line.find('\n') {
        let line: String = inner.line.drain(..=index).collect();
        let line = line.trim_end_matches(['\n', '\r']);
        match parse_line(line) {
            Some(notification) => apply(inner, notification),
            // Inside a `%begin`..`%end` block the response payload is plain
            // text, not notifications — that is where `list-windows` output lands.
            None => {
                if inner.in_block && !line.is_empty() {
                    inner.block_lines.push(line.to_string());
                }
            }
        }
    }
    if inner.line.len() > MAX_LINE_BYTES {
        tracing::warn!("tmux protocol line exceeded the buffer cap; discarding");
        inner.line.clear();
    }
}

fn apply(inner: &mut Inner, notification: TmuxNotification) {
    match notification {
        TmuxNotification::Output { pane_id, data } => {
            inner.pane_outputs.push((pane_id, data));
        }
        TmuxNotification::Begin { .. } => {
            inner.in_block = true;
            inner.block_lines.clear();
        }
        TmuxNotification::End { .. } | TmuxNotification::Error { .. } => {
            if inner.in_block {
                inner.in_block = false;
                let lines = std::mem::take(&mut inner.block_lines);
                process_response_block(inner, &lines);
            }
        }
        TmuxNotification::WindowAdd { window_id } => {
            if !inner.snapshot.windows.iter().any(|w| w.id == window_id) {
                let index = inner
                    .snapshot
                    .windows
                    .iter()
                    .map(|w| w.index + 1)
                    .max()
                    .unwrap_or(0);
                inner.snapshot.windows.push(TmuxWindow {
                    id: window_id.clone(),
                    index,
                    name: String::new(),
                    active: false,
                    layout: None,
                    panes: Vec::new(),
                });
                // A brand new window has neither a layout nor pane ids yet, and
                // `%layout-change` is not guaranteed for a window that has never
                // been displayed — so ask for both explicitly.
                inner.outgoing.push(windows_command(Some(&window_id)));
                inner.outgoing.push(panes_command(Some(&window_id)));
            }
            inner.mark_state_dirty();
        }
        TmuxNotification::WindowClose { window_id }
        | TmuxNotification::UnlinkedWindowClose { window_id } => {
            inner.snapshot.windows.retain(|w| w.id != window_id);
            inner.pane_ids.remove(&window_id);
            inner.active_pane_index.remove(&window_id);
            if inner.snapshot.active_window_id.as_deref() == Some(window_id.as_str()) {
                inner.snapshot.active_window_id =
                    inner.snapshot.windows.first().map(|w| w.id.clone());
            }
            inner.mark_state_dirty();
        }
        TmuxNotification::WindowRenamed { window_id, name }
        | TmuxNotification::UnlinkedWindowRenamed { window_id, name } => {
            if let Some(window) = inner.find_window_mut(&window_id) {
                window.name = name;
            }
            inner.mark_state_dirty();
        }
        TmuxNotification::LayoutChange { window_id, layout } => {
            let ids = inner.pane_ids.get(&window_id).cloned();
            let active = inner.active_pane_index.get(&window_id).copied();
            let mut needs_pane_sync = false;
            if let Some(window) = inner.find_window_mut(&window_id) {
                match parse_layout(&layout) {
                    Ok(mut node) => {
                        if let Some(ids) = &ids {
                            apply_pane_ids(&mut node, ids);
                        }
                        if let Some(active) = active {
                            mark_active(&mut node, active);
                        }
                        let mut panes = Vec::new();
                        node.collect_panes(&mut panes);
                        // `split-window` (from any client) introduces pane indices
                        // we have never resolved to a `%N` id. Without an id the
                        // frontend cannot route input for that pane, so re-query
                        // the window's panes whenever the set moved on.
                        needs_pane_sync = panes
                            .iter()
                            .any(|pane| ids.as_ref().and_then(|m| m.get(&pane.index)).is_none());
                        window.panes = panes;
                        window.layout = Some(node);
                    }
                    Err(error) => {
                        tracing::warn!(
                            window_id = %window_id,
                            ?error,
                            "tmux sent an unparsable layout string"
                        );
                    }
                }
            }
            if needs_pane_sync {
                inner.outgoing.push(panes_command(Some(&window_id)));
            }
            inner.mark_state_dirty();
        }
        TmuxNotification::WindowPaneChanged { window_id, pane_id } => {
            let index = inner
                .pane_ids
                .get(&window_id)
                .and_then(|ids| ids.iter().find(|(_, id)| **id == pane_id).map(|(i, _)| *i));
            if let Some(index) = index {
                inner.active_pane_index.insert(window_id.clone(), index);
            }
            inner.snapshot.active_window_id = Some(window_id.clone());
            if let Some(window) = inner.find_window_mut(&window_id) {
                for pane in window.panes.iter_mut() {
                    pane.active = pane.id.as_deref() == Some(pane_id.as_str());
                }
            }
            inner.mark_state_dirty();
        }
        TmuxNotification::SessionChanged { session_id, name } => {
            inner.snapshot.session_id = Some(session_id);
            inner.snapshot.session_name = name;
            inner.mark_state_dirty();
        }
        TmuxNotification::SessionRenamed { name } => {
            inner.snapshot.session_name = name;
            inner.mark_state_dirty();
        }
        TmuxNotification::SessionWindowChanged { window_id, .. } => {
            inner.snapshot.active_window_id = Some(window_id);
            inner.mark_state_dirty();
        }
        TmuxNotification::SessionsChanged => {
            inner.mark_state_dirty();
        }
        TmuxNotification::Exit { reason } => {
            tracing::info!(
                session_id = %inner.snapshot.session_id.clone().unwrap_or_default(),
                reason = reason.as_deref().unwrap_or(""),
                "tmux control-mode client exited"
            );
            inner.snapshot.exited = true;
            inner.mark_state_dirty();
        }
        // `%message` is a user-visible tmux message, not response payload.
        TmuxNotification::Message { text } => {
            tracing::debug!(message = %text, "tmux gateway message");
        }
        TmuxNotification::UnlinkedWindowAdd { .. }
        | TmuxNotification::PaneModeChanged { .. }
        | TmuxNotification::ClientDetached
        | TmuxNotification::Unhandled { .. } => {}
    }
}

/// Dispatch response payload lines by their tag prefix.
///
/// Tagged lines are self-identifying, so it does not matter how tmux groups them
/// into `%begin`..`%end` blocks (it emits an extra empty block at setup, which
/// makes positional correlation unreliable).
fn process_response_block(inner: &mut Inner, lines: &[String]) {
    let mut saw_windows = false;
    let mut saw_panes = false;

    for line in lines {
        if let Some(rest) = line.strip_prefix(WINDOW_TAG) {
            apply_window_line(inner, rest);
            saw_windows = true;
        } else if let Some(rest) = line.strip_prefix(PANE_TAG) {
            apply_pane_line(inner, rest);
            saw_panes = true;
        }
    }

    if saw_windows || saw_panes {
        reattach_pane_ids(inner);
        inner.mark_state_dirty();
    }
}

fn apply_window_line(inner: &mut Inner, rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    if fields.len() < 5 {
        return;
    }
    let id = fields[0].to_string();
    let index = fields[1].parse().unwrap_or(0);
    let name = fields[2].to_string();
    let active = fields[3] == "1";

    if active {
        inner.snapshot.active_window_id = Some(id.clone());
    }

    let ids = inner.pane_ids.get(&id).cloned();
    let active_pane = inner.active_pane_index.get(&id).copied();

    let mut layout = parse_layout(fields[4]).ok();
    if let Some(node) = layout.as_mut() {
        if let Some(ids) = ids {
            apply_pane_ids(node, &ids);
        }
        if let Some(active_pane) = active_pane {
            mark_active(node, active_pane);
        }
    }
    let mut panes = Vec::new();
    if let Some(node) = layout.as_ref() {
        node.collect_panes(&mut panes);
    }

    match inner.snapshot.windows.iter_mut().find(|w| w.id == id) {
        Some(window) => {
            window.index = index;
            window.name = name;
            window.active = active;
            if layout.is_some() {
                window.layout = layout;
                window.panes = panes;
            }
        }
        None => inner.snapshot.windows.push(TmuxWindow {
            id,
            index,
            name,
            active,
            layout,
            panes,
        }),
    }
}

fn apply_pane_line(inner: &mut Inner, rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    if fields.len() < 4 {
        return;
    }
    let window_id = fields[0].to_string();
    let index: u32 = fields[1].parse().unwrap_or(0);
    inner
        .pane_ids
        .entry(window_id.clone())
        .or_default()
        .insert(index, fields[2].to_string());
    if fields[3] == "1" {
        inner.active_pane_index.insert(window_id, index);
    }
}

/// Re-apply pane ids and active markers onto parsed layouts, then refresh panes.
fn reattach_pane_ids(inner: &mut Inner) {
    for position in 0..inner.snapshot.windows.len() {
        let window_id = inner.snapshot.windows[position].id.clone();
        let ids = inner.pane_ids.get(&window_id).cloned();
        let active = inner.active_pane_index.get(&window_id).copied();
        let window = &mut inner.snapshot.windows[position];
        if let Some(layout) = window.layout.as_mut() {
            if let Some(ids) = ids {
                apply_pane_ids(layout, &ids);
            }
            if let Some(active) = active {
                mark_active(layout, active);
            }
        }
        if let Some(layout) = window.layout.as_ref() {
            let mut panes = Vec::new();
            layout.collect_panes(&mut panes);
            window.panes = panes;
        }
    }
}

/// Split `buffer` into (emit-now, hold-back).
///
/// Only a tail that is a proper prefix of the marker **of length >= 2** is held,
/// so ordinary output ending in a lone `ESC` is never delayed.
fn split_holdable_tail(buffer: &str) -> (String, String) {
    let bytes = buffer.as_bytes();
    let marker = CONTROL_MARKER.as_bytes();
    for len in (2..=MARKER_MAX_HOLD).rev() {
        if bytes.len() >= len && marker.starts_with(&bytes[bytes.len() - len..]) {
            let cut = bytes.len() - len;
            // Retained bytes all match ASCII marker bytes, so `cut` is a valid
            // char boundary on both sides.
            return (buffer[..cut].to_string(), buffer[cut..].to_string());
        }
    }
    (buffer.to_string(), String::new())
}

/// Owns one gateway per session id.
#[derive(Default)]
pub struct TmuxGatewayManager {
    gateways: Mutex<HashMap<String, Arc<TmuxGateway>>>,
}

impl TmuxGatewayManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Arc<TmuxGateway>>> {
        match self.gateways.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                let guard = poisoned.into_inner();
                self.gateways.clear_poison();
                guard
            }
        }
    }

    /// Fetch (or lazily create) the gateway for a session.
    pub fn gateway_for(
        &self,
        session_id: &str,
        app: &AppHandle,
        command_tx: &SessionCommandSender,
    ) -> Arc<TmuxGateway> {
        let mut gateways = self.lock();
        gateways
            .entry(session_id.to_string())
            .or_insert_with(|| {
                let sink = Arc::new(AppGatewaySink::new(app.clone(), session_id));
                TmuxGateway::new(session_id.to_string(), sink, command_tx.clone())
            })
            .clone()
    }

    pub fn get(&self, session_id: &str) -> Option<Arc<TmuxGateway>> {
        self.lock().get(session_id).cloned()
    }

    pub fn forget(&self, session_id: &str) {
        self.lock().remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_only_marker_prefixes_of_two_or_more_bytes() {
        // A lone trailing ESC must NOT be held: ordinary escape sequences end
        // chunks constantly and holding them would add latency to every session.
        let (emit, hold) = split_holdable_tail("text\x1b");
        assert_eq!(emit, "text\x1b");
        assert_eq!(hold, "");

        // Two or more matching bytes are a plausible marker prefix.
        let (emit, hold) = split_holdable_tail("text\x1bP");
        assert_eq!(emit, "text");
        assert_eq!(hold, "\x1bP");

        let (emit, hold) = split_holdable_tail("text\x1bP100");
        assert_eq!(emit, "text");
        assert_eq!(hold, "\x1bP100");

        // A complete marker is never held back as a "prefix".
        let (emit, hold) = split_holdable_tail(CONTROL_MARKER);
        assert_eq!(emit, CONTROL_MARKER);
        assert_eq!(hold, "");
    }

    #[test]
    fn does_not_hold_unrelated_escape_sequences() {
        let (emit, hold) = split_holdable_tail("prompt\x1b]0;title\x07");
        assert_eq!(hold, "");
        assert_eq!(emit, "prompt\x1b]0;title\x07");
    }

    #[test]
    fn holds_marker_split_across_chunks() {
        // Worst realistic split: 6 bytes then the 7th.
        let (emit, hold) = split_holdable_tail("\x1bP1000");
        assert_eq!(emit, "");
        assert_eq!(hold, "\x1bP1000");
    }
}
