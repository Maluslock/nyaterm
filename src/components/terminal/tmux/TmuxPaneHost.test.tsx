import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TmuxGatewaySnapshot } from "@/lib/tmuxGateway";
import { TmuxPaneHost } from "./TmuxPaneHost";

const gateway = vi.hoisted(() => ({ snapshot: null as TmuxGatewaySnapshot | null }));

vi.mock("@/hooks/useTmuxGateway", () => ({
  useTmuxGateway: () => gateway.snapshot,
}));

vi.mock("./TmuxGatewayView", () => ({
  TmuxGatewayView: () => <div data-testid="tmux-view" />,
}));

const snapshot = (overrides: Partial<TmuxGatewaySnapshot>): TmuxGatewaySnapshot => ({
  sessionName: "itest",
  windows: [],
  clientWidth: 80,
  clientHeight: 24,
  exited: false,
  ...overrides,
});

describe("TmuxPaneHost", () => {
  beforeEach(() => {
    gateway.snapshot = null;
  });

  it("shows the ordinary terminal while no tmux session is detected", () => {
    render(
      <TmuxPaneHost sessionId="s1">
        <div data-testid="terminal" />
      </TmuxPaneHost>,
    );

    expect(screen.getByTestId("terminal")).toBeTruthy();
    expect(screen.queryByTestId("tmux-view")).toBeNull();
  });

  it("swaps to the tmux view once the gateway reports a window", () => {
    gateway.snapshot = snapshot({
      windows: [{ id: "@0", index: 0, name: "bash", active: true, panes: [] }],
    });

    render(
      <TmuxPaneHost sessionId="s1">
        <div data-testid="terminal" />
      </TmuxPaneHost>,
    );

    expect(screen.getByTestId("tmux-view")).toBeTruthy();
    // The ordinary terminal stays mounted underneath so it keeps buffering the
    // session's output for the moment tmux exits.
    expect(screen.getByTestId("terminal")).toBeTruthy();
  });

  it("returns to the ordinary terminal when the tmux client exits", () => {
    // Detaching or quitting tmux must not strand the pane in a dead tmux view.
    gateway.snapshot = snapshot({ exited: true });

    render(
      <TmuxPaneHost sessionId="s1">
        <div data-testid="terminal" />
      </TmuxPaneHost>,
    );

    expect(screen.getByTestId("terminal")).toBeTruthy();
    expect(screen.queryByTestId("tmux-view")).toBeNull();
  });
  it("hands the keyboard back to the terminal when the tmux view goes away", () => {
    // `inert` drops focus when the tmux view appears, so detaching must put the
    // caret back into the terminal underneath instead of making the user click.
    gateway.snapshot = snapshot({
      windows: [{ id: "@0", index: 0, name: "bash", active: true, panes: [] }],
    });

    const { rerender } = render(
      <TmuxPaneHost sessionId="s1">
        <textarea className="xterm-helper-textarea" data-testid="terminal-input" />
      </TmuxPaneHost>,
    );
    expect(screen.queryByTestId("tmux-view")).toBeTruthy();

    gateway.snapshot = snapshot({ exited: true });
    rerender(
      <TmuxPaneHost sessionId="s1">
        <textarea className="xterm-helper-textarea" data-testid="terminal-input" />
      </TmuxPaneHost>,
    );

    expect(document.activeElement).toBe(screen.getByTestId("terminal-input"));
  });
});
