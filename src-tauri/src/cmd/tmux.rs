//! Tauri commands for the tmux control-mode gateway.

use std::sync::Arc;

use tauri::State;

use crate::core::tmux::{TmuxGatewayManager, TmuxGatewaySnapshot};
use crate::error::{AppError, AppResult};

fn gateway_for(
    manager: &TmuxGatewayManager,
    session_id: &str,
) -> AppResult<Arc<crate::core::tmux::TmuxGateway>> {
    manager
        .get(session_id)
        .ok_or_else(|| AppError::SessionNotFound(format!("no tmux gateway for session {session_id}")))
}

/// Current gateway state, or `None` when the session is not in tmux control mode.
#[tauri::command]
pub async fn tmux_gateway_snapshot(
    manager: State<'_, Arc<TmuxGatewayManager>>,
    session_id: String,
) -> AppResult<Option<TmuxGatewaySnapshot>> {
    Ok(manager.get(&session_id).map(|gateway| gateway.snapshot()))
}

/// Send raw keyboard bytes to one tmux pane.
#[tauri::command]
pub async fn tmux_gateway_input(
    manager: State<'_, Arc<TmuxGatewayManager>>,
    session_id: String,
    pane_id: String,
    data: String,
) -> AppResult<()> {
    let gateway = gateway_for(&manager, &session_id)?;
    gateway.send_pane_input(&pane_id, &data);
    Ok(())
}

/// Report the control client's geometry so tmux can size its panes.
#[tauri::command]
pub async fn tmux_gateway_resize(
    manager: State<'_, Arc<TmuxGatewayManager>>,
    session_id: String,
    width: u16,
    height: u16,
) -> AppResult<()> {
    let gateway = gateway_for(&manager, &session_id)?;
    gateway.resize(width, height);
    Ok(())
}

/// Replay a pane's current screen, for a pane view that has just mounted.
///
/// Control mode only streams new output, so without this a pane that already had
/// content (or whose prompt was printed before its view mounted) stays blank.
#[tauri::command]
pub async fn tmux_gateway_capture_pane(
    manager: State<'_, Arc<TmuxGatewayManager>>,
    session_id: String,
    pane_id: String,
) -> AppResult<()> {
    let gateway = gateway_for(&manager, &session_id)?;
    gateway.capture_pane(&pane_id);
    Ok(())
}

/// Run a tmux command on behalf of the UI (split-window, kill-pane, ...).
#[tauri::command]
pub async fn tmux_gateway_command(
    manager: State<'_, Arc<TmuxGatewayManager>>,
    session_id: String,
    command: String,
) -> AppResult<()> {
    let gateway = gateway_for(&manager, &session_id)?;
    gateway.run_ui_command(&command);
    Ok(())
}
