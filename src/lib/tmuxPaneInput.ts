/**
 * Input handling for one tmux pane.
 *
 * tmux panes get no shell-integration markers: the host session's command
 * capture runs on its own stream before the gateway sees anything, and the
 * per-pane protocol framing means nothing downstream can recover the command.
 * So the pane tracks its own line and decides for itself when a command was
 * submitted.
 *
 * Kept separate from the component because the ordering is the subtle part:
 * `applyTerminalInputData` RESETS the tracked state on Enter, so the submitted
 * command has to be read *before* the keystroke is applied.
 */
import {
  applyTerminalInputData,
  canRegisterCommandFromTracker,
  getTrackedSubmissionCommand,
  type TerminalInputState,
} from "./terminalInputTracker";

export interface TmuxPaneInputResult {
  /** Tracker state after this keystroke. */
  nextState: TerminalInputState;
  /** Command to record in history, when this keystroke submitted one. */
  submission: string | null;
  /** True when this keystroke submitted a line (even an empty one). */
  submitted: boolean;
}

export function isSubmitKeystroke(data: string): boolean {
  return data.includes("\r") || data.includes("\n");
}

export function applyTmuxPaneInput(
  state: TerminalInputState,
  data: string,
): TmuxPaneInputResult {
  const submitted = isSubmitKeystroke(data);

  // Read the command BEFORE applying the keystroke — Enter resets the tracker.
  let submission: string | null = null;
  if (submitted && canRegisterCommandFromTracker(state)) {
    submission = getTrackedSubmissionCommand(state) || null;
  }

  return {
    nextState: applyTerminalInputData(state, data),
    submission,
    submitted,
  };
}
