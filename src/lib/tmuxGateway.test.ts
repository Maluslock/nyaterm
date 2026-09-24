import { describe, expect, it } from "vitest";
import {
  collectLayoutPanes,
  tmuxPaneOutputEvent,
  tmuxStateEvent,
  type TmuxLayoutNode,
} from "./tmuxGateway";

const leaf = (index: number): TmuxLayoutNode => ({
  kind: "leaf",
  width: 40,
  height: 12,
  x: 0,
  y: 0,
  pane: { index, id: `%${index}`, width: 40, height: 12, x: 0, y: 0, active: false },
});

describe("tmuxGateway event names", () => {
  it("scopes gateway events to the session", () => {
    expect(tmuxStateEvent("abc")).toBe("tmux-state-abc");
    expect(tmuxPaneOutputEvent("abc")).toBe("tmux-pane-output-abc");
  });
});

describe("collectLayoutPanes", () => {
  it("flattens nested layouts in tmux order", () => {
    const layout: TmuxLayoutNode = {
      kind: "split",
      direction: "columns",
      width: 80,
      height: 24,
      x: 0,
      y: 0,
      children: [
        leaf(0),
        {
          kind: "split",
          direction: "rows",
          width: 39,
          height: 24,
          x: 41,
          y: 0,
          children: [leaf(1), leaf(2)],
        },
      ],
    };

    expect(collectLayoutPanes(layout).map((pane) => pane.index)).toEqual([0, 1, 2]);
  });

  it("returns a single pane for a leaf and nothing for undefined", () => {
    expect(collectLayoutPanes(leaf(7)).map((pane) => pane.id)).toEqual(["%7"]);
    expect(collectLayoutPanes(undefined)).toEqual([]);
  });
});
