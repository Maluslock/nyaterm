/**
 * Drag maths for the tmux split dividers.
 *
 * tmux owns the layout, so dragging a divider cannot resize locally: it has to
 * become a `resize-pane` command. The command takes a cell count, not pixels, and
 * it is relative to the pane's current edge — so a drag is translated into the
 * *incremental* difference since the last command that was sent (`sentCells`),
 * otherwise repeated absolute deltas would compound while tmux re-lays out.
 */

export type SplitAxis = "columns" | "rows";

/** Direction letters tmux accepts for `resize-pane`. */
export type ResizeDirection = "L" | "R" | "U" | "D";

export interface PaneResizeStep {
  /** Command to send, or `null` when the drag has not crossed a cell yet. */
  command: string | null;
  /** Cell offset already accounted for, to pass to the next call. */
  sentCells: number;
}

export interface PaneResizeInput {
  paneId: string | undefined;
  axis: SplitAxis;
  /** Pointer movement since the drag started, in pixels. */
  deltaPx: number;
  cellWidth: number;
  cellHeight: number;
  /** Cells already sent as commands during this drag. */
  sentCells: number;
}

export function nextPaneResize({
  paneId,
  axis,
  deltaPx,
  cellWidth,
  cellHeight,
  sentCells,
}: PaneResizeInput): PaneResizeStep {
  const cellSize = axis === "columns" ? cellWidth : cellHeight;
  if (!paneId || !Number.isFinite(cellSize) || cellSize <= 0) {
    return { command: null, sentCells };
  }

  const cells = Math.trunc(deltaPx / cellSize);
  const step = cells - sentCells;
  if (step === 0) {
    return { command: null, sentCells };
  }

  return {
    command: resizeCommand(paneId, axis, step),
    sentCells: cells,
  };
}

/**
 * tmux command that moves one edge of a pane by `cells` (negative resizes the
 * other way). Used by both the pointer drag and the keyboard nudge.
 */
export function resizeCommand(
  paneId: string,
  axis: SplitAxis,
  cells: number,
): string | null {
  if (!paneId || cells === 0) return null;
  return `resize-pane -t ${paneId} -${directionFor(axis, cells)} ${Math.abs(cells)}`;
}

function directionFor(axis: SplitAxis, step: number): ResizeDirection {
  if (axis === "columns") {
    return step > 0 ? "R" : "L";
  }
  return step > 0 ? "D" : "U";
}
