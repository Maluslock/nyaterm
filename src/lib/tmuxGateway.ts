/**
 * Frontend bindings for the tmux control-mode (`tmux -CC`) gateway.
 *
 * The backend intercepts the control protocol before it reaches the webview
 * (an unterminated `ESC P` would put xterm.js into a permanent DCS state), and
 * republishes it as these structured events.
 */
import { invoke } from "./invoke";

export interface TmuxPane {
  index: number;
  /** Stable tmux pane id (`%N`), used to route output and input. */
  id?: string;
  width: number;
  height: number;
  x: number;
  y: number;
  active: boolean;
}

export type TmuxSplitDirection = "columns" | "rows";

export type TmuxLayoutNode =
  | {
      kind: "leaf";
      width: number;
      height: number;
      x: number;
      y: number;
      pane: TmuxPane;
    }
  | {
      kind: "split";
      direction: TmuxSplitDirection;
      width: number;
      height: number;
      x: number;
      y: number;
      children: TmuxLayoutNode[];
    };

export interface TmuxWindow {
  id: string;
  index: number;
  name: string;
  active: boolean;
  layout?: TmuxLayoutNode;
  panes: TmuxPane[];
}

export interface TmuxGatewaySnapshot {
  sessionId?: string;
  sessionName: string;
  windows: TmuxWindow[];
  activeWindowId?: string;
  clientWidth: number;
  clientHeight: number;
  exited: boolean;
}

export interface TmuxPaneOutput {
  paneId: string;
  data: string;
}

export const tmuxStateEvent = (sessionId: string) => `tmux-state-${sessionId}`;
export const tmuxPaneOutputEvent = (sessionId: string) =>
  `tmux-pane-output-${sessionId}`;

/** Current gateway state, or `null` when the session is not in control mode. */
export async function fetchTmuxSnapshot(
  sessionId: string,
): Promise<TmuxGatewaySnapshot | null> {
  return invoke<TmuxGatewaySnapshot | null>("tmux_gateway_snapshot", {
    sessionId,
  });
}

/** Send raw keyboard bytes to one tmux pane. */
export async function sendTmuxPaneInput(
  sessionId: string,
  paneId: string,
  data: string,
): Promise<void> {
  await invoke("tmux_gateway_input", { sessionId, paneId, data });
}

/** Report the control client's geometry so tmux can size its panes. */
export async function resizeTmuxClient(
  sessionId: string,
  width: number,
  height: number,
): Promise<void> {
  await invoke("tmux_gateway_resize", { sessionId, width, height });
}

/**
 * Replay a pane's current screen into its view.
 *
 * Control mode only streams *new* pane output: attaching to a session that
 * already had content, or mounting a pane view after its prompt was printed,
 * would otherwise leave the pane blank. The replay arrives on the normal pane
 * output event, prefixed with a clear-screen sequence.
 */
export async function requestTmuxPaneCapture(
  sessionId: string,
  paneId: string,
): Promise<void> {
  await invoke("tmux_gateway_capture_pane", { sessionId, paneId });
}

/** Run a tmux command on behalf of the UI (split-window, kill-pane, ...). */
export async function runTmuxCommand(
  sessionId: string,
  command: string,
): Promise<void> {
  await invoke("tmux_gateway_command", { sessionId, command });
}

/** Flatten every leaf pane in a layout tree. */
export function collectLayoutPanes(node: TmuxLayoutNode | undefined): TmuxPane[] {
  if (!node) return [];
  if (node.kind === "leaf") return [node.pane];
  return node.children.flatMap(collectLayoutPanes);
}

/** tmux quits with this marker in `%exit`; used to label the detached state. */
export function isGatewayExited(snapshot: TmuxGatewaySnapshot | null): boolean {
  return snapshot?.exited === true;
}
