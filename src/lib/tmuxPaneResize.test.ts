import { describe, expect, it } from "vitest";
import { nextPaneResize } from "./tmuxPaneResize";

describe("nextPaneResize", () => {
  it("does nothing before the drag crosses a cell", () => {
    const step = nextPaneResize({
      paneId: "%1",
      axis: "columns",
      deltaPx: 4,
      cellWidth: 8,
      cellHeight: 16,
      sentCells: 0,
    });

    expect(step.command).toBeNull();
    expect(step.sentCells).toBe(0);
  });

  it("grows the pane when dragging a column divider right", () => {
    const step = nextPaneResize({
      paneId: "%1",
      axis: "columns",
      deltaPx: 24,
      cellWidth: 8,
      cellHeight: 16,
      sentCells: 0,
    });

    expect(step.command).toBe("resize-pane -t %1 -R 3");
    expect(step.sentCells).toBe(3);
  });

  it("shrinks the pane when dragging back past the start", () => {
    const step = nextPaneResize({
      paneId: "%1",
      axis: "columns",
      deltaPx: -17,
      cellWidth: 8,
      cellHeight: 16,
      sentCells: 0,
    });

    expect(step.command).toBe("resize-pane -t %1 -L 2");
    expect(step.sentCells).toBe(-2);
  });

  it("sends only the increment since the last command", () => {
    // 5 cells of travel after 3 were already sent: only 2 more cells.
    const step = nextPaneResize({
      paneId: "%2",
      axis: "rows",
      deltaPx: 5 * 16,
      cellWidth: 8,
      cellHeight: 16,
      sentCells: 3,
    });

    expect(step.command).toBe("resize-pane -t %2 -D 2");
    expect(step.sentCells).toBe(5);
  });

  it("ignores drags without a resolved pane or a usable cell size", () => {
    const noPane = nextPaneResize({
      paneId: undefined,
      axis: "rows",
      deltaPx: 100,
      cellWidth: 8,
      cellHeight: 16,
      sentCells: 0,
    });
    const noMetrics = nextPaneResize({
      paneId: "%1",
      axis: "rows",
      deltaPx: 100,
      cellWidth: 0,
      cellHeight: 0,
      sentCells: 0,
    });

    expect(noPane.command).toBeNull();
    expect(noMetrics.command).toBeNull();
  });
});
