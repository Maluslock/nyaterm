import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { invoke } from "@/lib/invoke";
import type { TmuxGatewaySnapshot, TmuxLayoutNode } from "@/lib/tmuxGateway";
import { TmuxGatewayView } from "./TmuxGatewayView";

const eventHandlers = vi.hoisted(
  () => new Map<string, (event: { payload: unknown }) => void>(),
);

vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (event: { payload: unknown }) => void) => {
    eventHandlers.set(name, handler);
    return Promise.resolve(() => eventHandlers.delete(name));
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/lib/invoke", () => ({
  invoke: vi.fn(async () => null),
}));

// A real xterm needs canvas/layout that jsdom does not provide; the view's job
// is the split structure, so stand in for the terminal surface.
vi.mock("./TmuxPaneTerminal", () => ({
  TmuxPaneTerminal: ({ pane }: { pane: { id?: string; index: number } }) => (
    <div data-testid={`pane-${pane.id ?? pane.index}`} />
  ),
}));

const paneAt = (index: number, x: number, y: number, w: number, h: number) => ({
  index,
  id: `%${index}`,
  width: w,
  height: h,
  x,
  y,
  active: index === 2,
});

// Mirrors the layout captured from real tmux:
//   d67e,80x24,0,0{40x24,0,0,0,39x24,41,0[39x12,41,0,1,39x11,41,13,2]}
const nestedLayout: TmuxLayoutNode = {
  kind: "split",
  direction: "columns",
  width: 80,
  height: 24,
  x: 0,
  y: 0,
  children: [
    { kind: "leaf", width: 40, height: 24, x: 0, y: 0, pane: paneAt(0, 0, 0, 40, 24) },
    {
      kind: "split",
      direction: "rows",
      width: 39,
      height: 24,
      x: 41,
      y: 0,
      children: [
        { kind: "leaf", width: 39, height: 12, x: 41, y: 0, pane: paneAt(1, 41, 0, 39, 12) },
        { kind: "leaf", width: 39, height: 11, x: 41, y: 13, pane: paneAt(2, 41, 13, 39, 11) },
      ],
    },
  ],
};

const snapshot: TmuxGatewaySnapshot = {
  sessionId: "$0",
  sessionName: "work",
  clientWidth: 80,
  clientHeight: 24,
  exited: false,
  activeWindowId: "@1",
  windows: [
    {
      id: "@0",
      index: 0,
      name: "editor",
      active: false,
      layout: { kind: "leaf", width: 80, height: 24, x: 0, y: 0, pane: paneAt(9, 0, 0, 80, 24) },
      panes: [paneAt(9, 0, 0, 80, 24)],
    },
    {
      id: "@1",
      index: 1,
      name: "bash",
      active: true,
      layout: nestedLayout,
      panes: [paneAt(0, 0, 0, 40, 24), paneAt(1, 41, 0, 39, 12), paneAt(2, 41, 13, 39, 11)],
    },
  ],
};

describe("TmuxGatewayView", () => {
  it("renders one terminal surface per pane of the active window", () => {
    const { queryByTestId } = render(
      <TmuxGatewayView sessionId="s1" snapshot={snapshot} />,
    );

    expect(queryByTestId("pane-%0")).toBeTruthy();
    expect(queryByTestId("pane-%1")).toBeTruthy();
    expect(queryByTestId("pane-%2")).toBeTruthy();
    // Panes belong to the inactive window only in that window's layout.
    expect(queryByTestId("pane-%9")).toBeNull();
  });

  it("mirrors tmux split directions as row and column flex containers", () => {
    const { container } = render(
      <TmuxGatewayView sessionId="s1" snapshot={snapshot} />,
    );

    // The captured layout is a columns split containing a rows split.
    expect(container.querySelectorAll(".flex-row").length).toBeGreaterThan(0);
    expect(container.querySelectorAll(".flex-col").length).toBeGreaterThan(0);
  });

  it("creates a tab per tmux window and marks the active one", () => {
    const { container } = render(
      <TmuxGatewayView sessionId="s1" snapshot={snapshot} />,
    );

    const tabButtons = Array.from(
      container.querySelectorAll("button"),
    ).filter((button) => button.textContent?.includes("bash") || button.textContent?.includes("editor"));

    expect(tabButtons.map((button) => button.textContent)).toEqual([
      "0: editor",
      "1: bash",
    ]);
  });

  it("renders nothing when tmux has not reported any window yet", () => {
    const empty: TmuxGatewaySnapshot = { ...snapshot, windows: [], activeWindowId: undefined };
    const { container } = render(
      <TmuxGatewayView sessionId="s1" snapshot={empty} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("puts the window strip below the panes, not above them", () => {
    const { container } = render(
      <TmuxGatewayView sessionId="s1" snapshot={snapshot} />,
    );

    const root = container.firstElementChild as HTMLElement;
    const children = Array.from(root.children) as HTMLElement[];
    const paneArea = children[0];
    const strip = children[children.length - 1];

    // Panes take the flexible space; the strip is the trailing fixed row.
    expect(paneArea.className).toContain("flex-1");
    expect(strip.className).toContain("border-t");
    expect(strip.textContent).toContain("0: editor");
    expect(strip.textContent).toContain("1: bash");
  });

  it("switches tmux windows with Alt+[ and Alt+]", () => {
    const mockedInvoke = vi.mocked(invoke);
    mockedInvoke.mockClear();
    render(<TmuxGatewayView sessionId="s1" snapshot={snapshot} />);

    fireEvent.keyDown(window, { key: "]", altKey: true });
    expect(mockedInvoke).toHaveBeenCalledWith("tmux_gateway_command", {
      sessionId: "s1",
      command: "next-window",
    });

    mockedInvoke.mockClear();
    fireEvent.keyDown(window, { key: "[", altKey: true });
    expect(mockedInvoke).toHaveBeenCalledWith("tmux_gateway_command", {
      sessionId: "s1",
      command: "previous-window",
    });
  });

  it("selects panes directionally with Alt+arrows", () => {
    const mockedInvoke = vi.mocked(invoke);
    mockedInvoke.mockClear();
    render(<TmuxGatewayView sessionId="s1" snapshot={snapshot} />);

    fireEvent.keyDown(window, { key: "ArrowRight", altKey: true });
    expect(mockedInvoke).toHaveBeenCalledWith("tmux_gateway_command", {
      sessionId: "s1",
      command: "select-pane -R",
    });
  });
  it("detaches from tmux with Ctrl-b d", () => {
    const mockedInvoke = vi.mocked(invoke);
    mockedInvoke.mockClear();
    render(<TmuxGatewayView sessionId="s1" snapshot={snapshot} />);

    fireEvent.keyDown(window, { key: "b", ctrlKey: true });
    fireEvent.keyDown(window, { key: "d" });

    expect(mockedInvoke).toHaveBeenCalledWith("tmux_gateway_command", {
      sessionId: "s1",
      command: "detach-client",
    });
  });

  it("forwards a prefixed key it does not emulate to the pane", () => {
    const mockedInvoke = vi.mocked(invoke);
    mockedInvoke.mockClear();
    render(<TmuxGatewayView sessionId="s1" snapshot={snapshot} />);

    fireEvent.keyDown(window, { key: "b", ctrlKey: true });
    fireEvent.keyDown(window, { key: "z" });

    expect(mockedInvoke).toHaveBeenCalledWith("tmux_gateway_input", {
      sessionId: "s1",
      paneId: "%2",
      data: "\u0002z",
    });
  });

  it("offers a detach button in the window strip", () => {
    const mockedInvoke = vi.mocked(invoke);
    mockedInvoke.mockClear();
    const { getByLabelText } = render(<TmuxGatewayView sessionId="s1" snapshot={snapshot} />);

    fireEvent.click(getByLabelText("tmux.detach"));

    expect(mockedInvoke).toHaveBeenCalledWith("tmux_gateway_command", {
      sessionId: "s1",
      command: "detach-client",
    });
  });
  it("opens the tmux command line with Ctrl-b : and runs the command", () => {
    const mockedInvoke = vi.mocked(invoke);
    mockedInvoke.mockClear();
    render(<TmuxGatewayView sessionId="s1" snapshot={snapshot} />);

    fireEvent.keyDown(window, { key: "b", ctrlKey: true });
    // A real keyboard sends Shift down before the colon: that must not be eaten
    // as the prefixed key.
    fireEvent.keyDown(window, { key: "Shift", shiftKey: true });
    fireEvent.keyDown(window, { key: ":", shiftKey: true });

    // The strip button carries the same label, so target the input by its
    // placeholder.
    const input = screen.getByPlaceholderText("tmux.commandPlaceholder");
    fireEvent.change(input, { target: { value: "rename-window demo" } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(mockedInvoke).toHaveBeenCalledWith("tmux_gateway_run_command", {
      sessionId: "s1",
      command: "rename-window demo",
      requestId: expect.any(String),
    });
  });

  it("shows the answer tmux gives, including a rejected command", async () => {
    const mockedInvoke = vi.mocked(invoke);
    mockedInvoke.mockClear();
    render(<TmuxGatewayView sessionId="s1" snapshot={snapshot} />);

    fireEvent.keyDown(window, { key: "b", ctrlKey: true });
    fireEvent.keyDown(window, { key: ":" });
    const input = screen.getByPlaceholderText("tmux.commandPlaceholder");
    fireEvent.change(input, { target: { value: "nonsense-command" } });
    fireEvent.keyDown(input, { key: "Enter" });

    const calls = mockedInvoke.mock.calls;
    const requestId = calls[calls.length - 1]?.[1] as { requestId: string };
    const handler = eventHandlers.get("tmux-command-response-s1");
    expect(handler).toBeTruthy();

    await act(async () => {
      handler?.({
        payload: {
          requestId: requestId.requestId,
          output: "",
          error: "parse error: unknown command: nonsense-command",
        },
      });
    });

    expect(
      screen.getByText("parse error: unknown command: nonsense-command"),
    ).toBeTruthy();
  });
});
