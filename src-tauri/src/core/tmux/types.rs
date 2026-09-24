//! Serializable state types shared with the frontend.

use serde::{Deserialize, Serialize};

/// Which way a split node arranges its children.
///
/// Derived from tmux's own layout syntax and verified against real tmux output:
/// `{...}` arranges children **side by side**, `[...]` arranges them **stacked**.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TmuxSplitDirection {
    /// Children sit left-to-right (`{...}` in tmux layout strings).
    Columns,
    /// Children sit top-to-bottom (`[...]` in tmux layout strings).
    Rows,
}

/// One leaf pane inside a tmux window.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TmuxPane {
    /// tmux pane **index** within its window, as used by layout strings.
    pub index: u32,
    /// Stable tmux pane id (`%N`), as used by `%output` notifications.
    ///
    /// Resolved from tmux via `list-panes`; `None` until that response lands.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub width: u16,
    pub height: u16,
    pub x: u16,
    pub y: u16,
    pub active: bool,
}

/// Recursive tmux window layout, mirroring the tmux layout string.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TmuxLayoutNode {
    #[serde(rename_all = "camelCase")]
    Leaf {
        width: u16,
        height: u16,
        x: u16,
        y: u16,
        pane: TmuxPane,
    },
    #[serde(rename_all = "camelCase")]
    Split {
        direction: TmuxSplitDirection,
        width: u16,
        height: u16,
        x: u16,
        y: u16,
        children: Vec<TmuxLayoutNode>,
    },
}

impl TmuxLayoutNode {
    /// Collect every leaf pane under this node, left-to-right then top-to-bottom.
    pub fn collect_panes(&self, out: &mut Vec<TmuxPane>) {
        match self {
            TmuxLayoutNode::Leaf { pane, .. } => out.push(pane.clone()),
            TmuxLayoutNode::Split { children, .. } => {
                for child in children {
                    child.collect_panes(out);
                }
            }
        }
    }
}

/// One tmux window, which the frontend renders as one native tab.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TmuxWindow {
    /// tmux window id, e.g. `@0`.
    pub id: String,
    pub index: u32,
    pub name: String,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<TmuxLayoutNode>,
    #[serde(default)]
    pub panes: Vec<TmuxPane>,
}

/// Full gateway state pushed to the frontend on every structural change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TmuxGatewaySnapshot {
    /// tmux session id, e.g. `$0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub session_name: String,
    pub windows: Vec<TmuxWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_window_id: Option<String>,
    /// Client geometry reported back to tmux via `refresh-client -C`.
    pub client_width: u16,
    pub client_height: u16,
    /// Set once the session has detached or tmux exited.
    pub exited: bool,
}

impl Default for TmuxGatewaySnapshot {
    fn default() -> Self {
        Self {
            session_id: None,
            session_name: String::new(),
            windows: Vec::new(),
            active_window_id: None,
            client_width: 80,
            client_height: 24,
            exited: false,
        }
    }
}
