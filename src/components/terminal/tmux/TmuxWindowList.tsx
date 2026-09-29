/**
 * tmux's `choose-tree`: every window in the session with its panes nested under
 * it, so a pane can be reached by name instead of by clicking the right cell.
 *
 * Control mode keeps the window strip honest, but the strip has room for a title
 * and nothing else — this is where the ids and sizes live, and where a window
 * that is off the edge of the strip can still be found.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TmuxGatewaySnapshot } from "@/lib/tmuxGateway";
import { collectLayoutPanes } from "@/lib/tmuxGateway";

export interface TmuxWindowListProps {
  snapshot: TmuxGatewaySnapshot;
  /** Switch to a window (`select-window`). */
  onSelectWindow: (windowId: string) => void;
  /** Switch to a window and its pane (`select-window` + `select-pane`). */
  onSelectPane: (windowId: string, paneId: string) => void;
  onClose: () => void;
}

/** One row of the list: a window, or one of its panes. */
interface ListRow {
  key: string;
  windowId: string;
  paneId?: string;
  depth: 0 | 1;
  label: string;
  detail: string;
  active: boolean;
}

export function TmuxWindowList({
  snapshot,
  onSelectWindow,
  onSelectPane,
  onClose,
}: TmuxWindowListProps) {
  const { t } = useTranslation();
  const [selected, setSelected] = useState(0);
  const listRef = useRef<HTMLDivElement | null>(null);

  const rows = useMemo<ListRow[]>(() => {
    const built: ListRow[] = [];
    for (const window of snapshot.windows) {
      const isActiveWindow = window.id === snapshot.activeWindowId;
      const panes = collectLayoutPanes(window.layout);
      const flags = [
        window.zoomed ? t("tmux.windowZoomed") : null,
        window.bell ? t("tmux.windowBell") : null,
        window.activity ? t("tmux.windowActivity") : null,
      ].filter(Boolean);
      built.push({
        key: window.id,
        windowId: window.id,
        depth: 0,
        label: `${window.index}: ${window.name}`,
        detail: [`${panes.length} ${t("tmux.windowListPanes")}`, ...flags].join(
          " · ",
        ),
        active: isActiveWindow,
      });
      for (const pane of panes) {
        built.push({
          key: `${window.id}:${pane.id ?? pane.index}`,
          windowId: window.id,
          paneId: pane.id,
          depth: 1,
          label: pane.id ?? `#${pane.index}`,
          detail: `${pane.width}x${pane.height}${
            window.active && pane.active
              ? ` · ${t("tmux.windowListActive")}`
              : ""
          }`,
          active: isActiveWindow && pane.active,
        });
      }
    }
    return built;
  }, [snapshot, t]);

  // The active row is where the cursor starts: `choose-tree` opens on the
  // current pane, not at the top of the list. Only once — after that the cursor
  // belongs to whoever is moving it, even if the snapshot changes underneath.
  const placedCursorRef = useRef(false);
  useEffect(() => {
    if (placedCursorRef.current) return;
    placedCursorRef.current = true;
    // Prefer the current pane; a window row is only the fallback for a layout
    // that has not arrived yet.
    const activePane = rows.findIndex((row) => row.depth === 1 && row.active);
    const activeIndex =
      activePane >= 0 ? activePane : rows.findIndex((row) => row.active);
    setSelected(activeIndex >= 0 ? activeIndex : 0);
  }, [rows]);

  const choose = (row: ListRow | undefined) => {
    if (!row) return;
    if (row.paneId) onSelectPane(row.windowId, row.paneId);
    else onSelectWindow(row.windowId);
    onClose();
  };

  return (
    <div
      role="dialog"
      aria-label={t("tmux.windowList")}
      className="absolute inset-0 z-30 flex items-start justify-center bg-black/40 pt-4"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={listRef}
        className="max-h-[70%] w-[420px] max-w-[90%] overflow-y-auto rounded border border-[var(--df-border)] bg-[var(--df-bg-panel)] py-1 text-xs shadow-lg"
      >
        <div className="px-3 py-1 text-[11px] text-[var(--df-text-muted)]">
          {t("tmux.windowListHint")}
        </div>
        {rows.map((row, index) => (
          <button
            key={row.key}
            type="button"
            role="menuitem"
            aria-current={row.active ? "true" : undefined}
            className={`flex w-full items-center gap-2 px-3 py-1 text-left ${
              index === selected ? "bg-[var(--df-bg-hover)]" : ""
            } ${row.active ? "text-[var(--df-text)]" : "text-[var(--df-text-muted)]"}`}
            style={{ paddingLeft: row.depth === 0 ? 12 : 28 }}
            onMouseEnter={() => setSelected(index)}
            onClick={() => choose(row)}
          >
            <span className="truncate">{row.label}</span>
            <span className="ml-auto shrink-0 text-[10px] opacity-70">
              {row.detail}
            </span>
          </button>
        ))}
      </div>
      {/* Keyboard handling lives on the panel, so the terminal keeps its keys
          until the list is actually open. */}
      <WindowListKeys
        rows={rows}
        selected={selected}
        onMove={setSelected}
        onChoose={() => choose(rows[selected])}
        onClose={onClose}
      />
    </div>
  );
}

function WindowListKeys({
  rows,
  selected,
  onMove,
  onChoose,
  onClose,
}: {
  rows: ListRow[];
  selected: number;
  onMove: (index: number) => void;
  onChoose: () => void;
  onClose: () => void;
}) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // Capture phase: the gateway view listens there too, and while the list is
      // open the list owns the keyboard.
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        onClose();
      } else if (event.key === "ArrowDown" || event.key === "j") {
        event.preventDefault();
        event.stopPropagation();
        onMove(Math.min(selected + 1, rows.length - 1));
      } else if (event.key === "ArrowUp" || event.key === "k") {
        event.preventDefault();
        event.stopPropagation();
        onMove(Math.max(selected - 1, 0));
      } else if (event.key === "Enter") {
        event.preventDefault();
        event.stopPropagation();
        onChoose();
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [onChoose, onClose, onMove, rows.length, selected]);

  return null;
}

export default TmuxWindowList;
