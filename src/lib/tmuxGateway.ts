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
  /** `#{window_zoomed_flag}`: the active pane fills the window. */
  zoomed?: boolean;
  /** `#{window_activity_flag}`: output arrived since the window was last seen. */
  activity?: boolean;
  /** `#{window_bell_flag}`: the window rang the bell. */
  bell?: boolean;
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
  /**
   * Last status-line message tmux showed the control client (`%message`).
   *
   * Control mode draws no status line, so this is where tmux's own feedback —
   * "no next window", a binding's report, a refused resize — arrives.
   */
  message?: TmuxGatewayMessage;
}

export interface TmuxGatewayMessage {
  text: string;
  /** Bumped per message, so an identical repeat still counts as new. */
  sequence: number;
}

export interface TmuxPaneOutput {
  paneId: string;
  data: string;
}

export interface TmuxCommandResponse {
  requestId: string;
  output: string;
  error?: string;
}

export const tmuxStateEvent = (sessionId: string) => `tmux-state-${sessionId}`;
export const tmuxCommandResponseEvent = (sessionId: string) =>
  `tmux-command-response-${sessionId}`;
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

/**
 * Run a tmux command from the UI's command line.
 *
 * The answer (including a tmux `%error`) comes back on
 * `tmux-command-response-<session>`, correlated by `requestId`.
 */
export async function runTmuxCommandWithReply(
  sessionId: string,
  command: string,
  requestId: string,
): Promise<void> {
  await invoke("tmux_gateway_run_command", { sessionId, command, requestId });
}

/** Run a tmux command on behalf of the UI (split-window, kill-pane, ...). */
export async function runTmuxCommand(
  sessionId: string,
  command: string,
): Promise<void> {
  await invoke("tmux_gateway_command", { sessionId, command });
}

/** Flatten every leaf pane in a layout tree. */
export function collectLayoutPanes(
  node: TmuxLayoutNode | undefined,
): TmuxPane[] {
  if (!node) return [];
  if (node.kind === "leaf") return [node.pane];
  return node.children.flatMap(collectLayoutPanes);
}

/** tmux quits with this marker in `%exit`; used to label the detached state. */
export function isGatewayExited(snapshot: TmuxGatewaySnapshot | null): boolean {
  return snapshot?.exited === true;
}

/**
 * Window event asking one pane to open its find bar.
 *
 * The pane menu is rendered by the gateway view while the search state belongs
 * to the pane's own terminal, so the request travels as an event instead of
 * lifting every pane's search state up into the view.
 */
export const TMUX_PANE_FIND_EVENT = "nyaterm:tmux-pane-find";

export function requestTmuxPaneFind(sessionId: string, paneId?: string): void {
  window.dispatchEvent(
    new CustomEvent(TMUX_PANE_FIND_EVENT, { detail: { sessionId, paneId } }),
  );
}
