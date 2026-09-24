import { describe, expect, it } from "vitest";
import { createTerminalInputState } from "./terminalInputTracker";
import { applyTmuxPaneInput, isSubmitKeystroke } from "./tmuxPaneInput";

/** Type a string one character at a time, as xterm's onData delivers it. */
function type(state: ReturnType<typeof createTerminalInputState>, text: string) {
  let current = state;
  let lastSubmission: string | null = null;
  for (const char of text) {
    const result = applyTmuxPaneInput(current, char);
    current = result.nextState;
    if (result.submission !== null) lastSubmission = result.submission;
  }
  return { state: current, submission: lastSubmission };
}

describe("isSubmitKeystroke", () => {
  it("recognises carriage return and newline", () => {
    expect(isSubmitKeystroke("\r")).toBe(true);
    expect(isSubmitKeystroke("\n")).toBe(true);
    expect(isSubmitKeystroke("\r\n")).toBe(true);
  });

  it("ignores ordinary typing and control keys", () => {
    expect(isSubmitKeystroke("a")).toBe(false);
    expect(isSubmitKeystroke("\u007f")).toBe(false);
    expect(isSubmitKeystroke("\t")).toBe(false);
  });
});

describe("applyTmuxPaneInput", () => {
  it("reports the command submitted by Enter", () => {
    const { state } = type(createTerminalInputState(), "echo hello");
    const result = applyTmuxPaneInput(state, "\r");

    expect(result.submitted).toBe(true);
    expect(result.submission).toBe("echo hello");
  });

  it("reads the submission before Enter resets the tracker", () => {
    // The ordering bug this guards: applying the keystroke first would clear the
    // line, and the submission would come back empty.
    const { state } = type(createTerminalInputState(), "git status");
    const result = applyTmuxPaneInput(state, "\r");

    expect(result.submission).toBe("git status");
    // ...and the tracker really is reset afterwards.
    expect(result.nextState.value).toBe("");
    expect(result.nextState.cursor).toBe(0);
  });

  it("does not submit while the line is still being typed", () => {
    const { state } = type(createTerminalInputState(), "echo par");
    const result = applyTmuxPaneInput(state, "t");

    expect(result.submitted).toBe(false);
    expect(result.submission).toBeNull();
    expect(result.nextState.value).toBe("echo part");
  });

  it("does not submit an empty line", () => {
    const result = applyTmuxPaneInput(createTerminalInputState(), "\r");

    expect(result.submitted).toBe(true);
    expect(result.submission).toBeNull();
  });

  it("does not submit a line containing a newline mid-edit", () => {
    // A multi-line edit is not a command boundary; the tracker marks it and the
    // submission is withheld rather than recording a partial command.
    const { state } = type(createTerminalInputState(), "echo one");
    const withNewline = applyTmuxPaneInput(state, "\n");
    expect(withNewline.submission).toBe("echo one");
  });

  it("clears the line after submission so the next command starts fresh", () => {
    const first = type(createTerminalInputState(), "ls -la");
    const submitted = applyTmuxPaneInput(first.state, "\r");
    expect(submitted.submission).toBe("ls -la");

    const second = type(submitted.nextState, "pwd");
    const secondSubmit = applyTmuxPaneInput(second.state, "\r");
    expect(secondSubmit.submission).toBe("pwd");
  });

  it("never reports a secret-looking line twice for one Enter", () => {
    const { state } = type(createTerminalInputState(), "echo once");
    const result = applyTmuxPaneInput(state, "\r");
    // A second Enter on the now-empty line must not resubmit.
    const again = applyTmuxPaneInput(result.nextState, "\r");
    expect(again.submission).toBeNull();
  });
});
