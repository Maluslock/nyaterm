import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { TmuxGatewaySnapshot, TmuxLayoutNode } from "@/lib/tmuxGateway";
import { TmuxWindowList } from "./TmuxWindowList";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const leaf = (
  paneIndex: number,
  active: boolean,
  width = 80,
  height = 24,
  x = 0,
  y = 0,
): TmuxLayoutNode => ({
  kind: "leaf",
  width,
  height,
  x,
  y,
  pane: { index: paneIndex, id: `%${paneIndex}`, width, height, x, y, active },
});

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
      bell: true,
      layout: leaf(7, true),
      panes: [],
    },
    {
      id: "@1",
      index: 1,
      name: "bash",
      active: true,
      zoomed: true,
      layout: {
        kind: "split",
        direction: "rows",
        width: 80,
        height: 24,
        x: 0,
        y: 0,
        children: [leaf(0, false, 80, 11, 0, 0), leaf(1, true, 80, 12, 0, 12)],
      },
      panes: [],
    },
  ],
};

describe("TmuxWindowList", () => {
  it("lists every window with its panes nested underneath", () => {
    render(
      <TmuxWindowList
        snapshot={snapshot}
        onSelectWindow={vi.fn()}
        onSelectPane={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByText("0: editor")).toBeTruthy();
    expect(screen.getByText("1: bash")).toBeTruthy();
    // Windows that are not the active one still show their panes.
    expect(screen.getByText("%7")).toBeTruthy();
    expect(screen.getByText("%0")).toBeTruthy();
    expect(screen.getByText("%1")).toBeTruthy();
    // Flags and sizes come from the snapshot, not from a second tmux query.
    expect(
      screen.getByText("1 tmux.windowListPanes · tmux.windowBell"),
    ).toBeTruthy();
    expect(
      screen.getByText("2 tmux.windowListPanes · tmux.windowZoomed"),
    ).toBeTruthy();
    expect(screen.getByText("80x12 · tmux.windowListActive")).toBeTruthy();
  });

  it("switches to the pane of the chosen row", () => {
    const onSelectPane = vi.fn();
    const onClose = vi.fn();
    render(
      <TmuxWindowList
        snapshot={snapshot}
        onSelectWindow={vi.fn()}
        onSelectPane={onSelectPane}
        onClose={onClose}
      />,
    );

    fireEvent.click(screen.getByText("%0"));

    expect(onSelectPane).toHaveBeenCalledWith("@1", "%0");
    expect(onClose).toHaveBeenCalled();
  });

  it("selects a whole window from its window row", () => {
    const onSelectWindow = vi.fn();
    render(
      <TmuxWindowList
        snapshot={snapshot}
        onSelectWindow={onSelectWindow}
        onSelectPane={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByText("0: editor"));

    expect(onSelectWindow).toHaveBeenCalledWith("@0");
  });

  it("walks the list with the keyboard and closes on Escape", () => {
    const calls: unknown[] = [];
    const onClose = vi.fn();
    render(
      <TmuxWindowList
        snapshot={snapshot}
        onSelectWindow={(windowId) => calls.push(["window", windowId])}
        onSelectPane={(windowId, paneId) =>
          calls.push(["pane", windowId, paneId])
        }
        onClose={onClose}
      />,
    );

    // Opens on the active row: the active pane of the active window.
    const current = screen.getByText("%1").closest("button");
    expect(current?.getAttribute("aria-current")).toBe("true");

    // Up from the active pane lands on the sibling pane above it. Keys arrive
    // from whatever has focus, so they are dispatched from the document body.
    fireEvent.keyDown(document.body, { key: "ArrowUp" });
    fireEvent.keyDown(document.body, { key: "Enter" });
    expect(calls).toEqual([["pane", "@1", "%0"]]);

    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });
});
