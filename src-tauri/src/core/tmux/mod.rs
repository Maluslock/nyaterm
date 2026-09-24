//! tmux control-mode (`tmux -CC`) gateway integration.
//!
//! When a session's output stream begins with the 7-byte control-mode marker
//! `ESC P 1 0 0 0 p`, tmux has switched to its machine-readable control
//! protocol. That protocol must never reach xterm.js: an unterminated `ESC P`
//! puts xterm into DCS state and it then swallows all subsequent output
//! permanently (verified by `spike/xterm-dcs-probe`).
//!
//! So the gateway intercepts the session byte stream *before* the output
//! coalescer forwards it to the webview, and replaces it with structured
//! per-pane events.

pub(crate) mod gateway;
pub(crate) mod layout;
pub(crate) mod protocol;
pub(crate) mod types;

#[cfg(test)]
mod integration_tests;

pub(crate) use gateway::{TmuxGateway, TmuxGatewayManager, TmuxPaneOutput};
pub(crate) use types::{TmuxGatewaySnapshot, TmuxLayoutNode, TmuxPane, TmuxSplitDirection, TmuxWindow};
