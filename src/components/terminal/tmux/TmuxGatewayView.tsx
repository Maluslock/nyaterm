/**
 * Native view for a session that is running tmux in control mode (`tmux -CC`).
 *
 * tmux panes are rendered as real split panes, each backed by its own local
 * xterm. tmux owns the layout, so the splitter is a mirror: resizing reports the
 * new client size and tmux replies with a fresh `%layout-change`.
 *
 * tmux windows are switched from the status strip **below** the panes (matching
 * where tmux itself puts its status line), and from WindTerm-style Alt hotkeys.
 */
import {
  Bell,
  ChevronLeft,
  ChevronRight,
  CircleDot,
  Columns2,
  LogOut,
  Maximize2,
  Plus,
  Rows2,
  SquareTerminal,
  X,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import {
  Fragment,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import {
  collectLayoutPanes,
  requestTmuxPaneFind,
  runTmuxCommandWithReply,
  sendTmuxPaneInput,
  tmuxCommandResponseEvent,
  resizeTmuxClient,
  runTmuxCommand,
  type TmuxCommandResponse,
  type TmuxGatewaySnapshot,
  type TmuxLayoutNode,
  type TmuxWindow,
} from "@/lib/tmuxGateway";
import { logger } from "@/lib/logger";
import {
  type SplitAxis,
  nextPaneResize,
  resizeCommand,
} from "@/lib/tmuxPaneResize";
import { TmuxPaneTerminal, type TmuxPaneCellMetrics } from "./TmuxPaneTerminal";

interface TmuxGatewayViewProps {
  sessionId: string;
  snapshot: TmuxGatewaySnapshot;
}

interface LayoutNodeProps {
  node: TmuxLayoutNode;
  sessionId: string;
  activePaneId: string | null;
  onSelectPane: (paneId: string) => void;
  onCellMetrics: (metrics: TmuxPaneCellMetrics) => void;
  /** Translate divider drag pixels into a tmux command; returns the new offset. */
  onResizePane: (
    paneId: string | undefined,
    axis: SplitAxis,
    deltaPx: number,
    sentCells: number,
  ) => number;
  /** Resize by whole cells, for the keyboard path. */
  onStepPane: (
    paneId: string | undefined,
    axis: SplitAxis,
    cells: number,
  ) => void;
  /** Right-click on a pane. */
  onPaneMenu: (
    paneId: string | undefined,
    point: { x: number; y: number },
  ) => void;
}

/** Pane at the given edge of a subtree — the one a divider drag resizes. */
function edgePaneId(
  node: TmuxLayoutNode,
  edge: "right" | "bottom",
): string | undefined {
  const panes = collectLayoutPanes(node).filter((pane) => pane.id);
  if (panes.length === 0) return undefined;
  const offset = edge === "right" ? "x" : "y";
  const size = edge === "right" ? "width" : "height";
  const furthest = panes.reduce((best, pane) =>
    pane[offset] + pane[size] > best[offset] + best[size] ? pane : best,
  );
  return furthest.id;
}

function LayoutNodeView({
  node,
  sessionId,
  activePaneId,
  onSelectPane,
  onCellMetrics,
  onResizePane,
  onStepPane,
  onPaneMenu,
}: LayoutNodeProps) {
  const { t } = useTranslation();

  if (node.kind === "leaf") {
    return (
      <TmuxPaneTerminal
        sessionId={sessionId}
        pane={node.pane}
        isActive={activePaneId !== null && node.pane.id === activePaneId}
        onSelect={onSelectPane}
        onCellMetrics={onCellMetrics}
        onContextMenu={onPaneMenu}
      />
    );
  }

  const isColumns = node.direction === "columns";

  const startDividerDrag = (
    event: React.PointerEvent<HTMLElement> | React.MouseEvent<HTMLElement>,
    childIndex: number,
  ) => {
    if (dividerDragActive) return;
    const paneId = edgePaneId(
      node.children[childIndex - 1],
      isColumns ? "right" : "bottom",
    );
    if (!paneId) return;
    event.preventDefault();
    dividerDragActive = true;
    const axis: SplitAxis = isColumns ? "columns" : "rows";
    const start = isColumns ? event.clientX : event.clientY;
    let sentCells = 0;
    const onMove = (moveEvent: PointerEvent | MouseEvent) => {
      const current = isColumns ? moveEvent.clientX : moveEvent.clientY;
      sentCells = onResizePane(paneId, axis, current - start, sentCells);
    };
    const onUp = () => {
      dividerDragActive = false;
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("mouseup", onUp);
    };
    // Both families are bound: engines without Pointer Events (older WebKitGTK)
    // only send the mouse ones, and a duplicate move at the same coordinate
    // computes a zero step, so it costs nothing.
    window.addEventListener("pointermove", onMove);
    window.addEventListener("mousemove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("mouseup", onUp);
  };

  return (
    <div
      className={`flex h-full w-full min-h-0 min-w-0 ${
        isColumns ? "flex-row" : "flex-col"
      }`}
    >
      {node.children.map((child, index) => (
        // Layout children are positional; tmux reports geometry, not ids.
        // biome-ignore lint/suspicious/noArrayIndexKey: positional by design
        <Fragment key={index}>
          {index > 0 ? (
            <hr
              data-tmux-gutter
              tabIndex={0}
              aria-orientation={isColumns ? "vertical" : "horizontal"}
              aria-label={t("tmux.resizePane")}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={Math.round(
                (isColumns
                  ? child.width / node.width
                  : child.height / node.height) * 100,
              )}
              className={`m-0 shrink-0 border-0 bg-[var(--df-border)] hover:bg-[var(--df-primary)] focus:bg-[var(--df-primary)] focus:outline-none ${
                isColumns ? "w-1 cursor-col-resize" : "h-1 cursor-row-resize"
              }`}
              onKeyDown={(event) => {
                const paneId = edgePaneId(
                  node.children[index - 1],
                  isColumns ? "right" : "bottom",
                );
                const axis: SplitAxis = isColumns ? "columns" : "rows";
                const back = isColumns ? "ArrowLeft" : "ArrowUp";
                const forward = isColumns ? "ArrowRight" : "ArrowDown";
                if (event.key === back) {
                  event.preventDefault();
                  onStepPane(paneId, axis, -1);
                } else if (event.key === forward) {
                  event.preventDefault();
                  onStepPane(paneId, axis, 1);
                }
              }}
              onPointerDown={(event) => startDividerDrag(event, index)}
              onMouseDown={(event) => startDividerDrag(event, index)}
            />
          ) : null}
          <div
            className="min-h-0 min-w-0"
            style={{
              // tmux already decided the proportions; mirror them.
              flexGrow: isColumns ? child.width : child.height,
              flexBasis: 0,
            }}
          >
            <LayoutNodeView
              node={child}
              sessionId={sessionId}
              activePaneId={activePaneId}
              onSelectPane={onSelectPane}
              onCellMetrics={onCellMetrics}
              onResizePane={onResizePane}
              onStepPane={onStepPane}
              onPaneMenu={onPaneMenu}
            />
          </div>
        </Fragment>
      ))}
    </div>
  );
}

/**
 * Commands worth one click.
 *
 * The strip hides the two thirds of tmux that has no button (pane resizing,
 * layout repair, break/join, synchronize-panes, capture-pane), and a cheatsheet
 * is not something a user should have to memorise — so the command line offers
 * the recipes, labelled with the command itself.
 */
const COMMAND_PRESETS = [
  "capture-pane -p -S -200",
  "split-window -h",
  "split-window -v",
  "resize-pane -Z",
  "select-layout tiled",
  "synchronize-panes on",
  "synchronize-panes off",
  "rename-window ",
  "break-pane",
  "list-windows -F '#{window_index}: #{window_name}'",
];

/**
 * Whether a divider drag is already running.
 *
 * Chromium delivers `pointerdown` and `mousedown` for the same press, and older
 * WebKitGTK builds deliver only the mouse events — so both are bound and the
 * first one wins.
 */
let dividerDragActive = false;

/** Recently submitted command lines, newest last (module scope: survives remounts). */
const commandHistory: string[] = [];
const COMMAND_HISTORY_LIMIT = 50;

function rememberCommand(command: string) {
  if (commandHistory[commandHistory.length - 1] === command) return;
  commandHistory.push(command);
  if (commandHistory.length > COMMAND_HISTORY_LIMIT) commandHistory.shift();
}

export function TmuxGatewayView({ sessionId, snapshot }: TmuxGatewayViewProps) {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const cellMetricsRef = useRef<TmuxPaneCellMetrics | null>(null);

  const handleCellMetrics = useCallback((metrics: TmuxPaneCellMetrics) => {
    cellMetricsRef.current = metrics;
  }, []);

  const activeWindow: TmuxWindow | null =
    snapshot.windows.find((window) => window.id === snapshot.activeWindowId) ??
    snapshot.windows[0] ??
    null;

  const activePaneId = useMemo(
    () => activeWindow?.panes.find((pane) => pane.active)?.id ?? null,
    [activeWindow],
  );

  const handleSelectPane = useCallback(
    (paneId: string) => {
      void runTmuxCommand(sessionId, `select-pane -t ${paneId}`).catch(
        () => {},
      );
    },
    [sessionId],
  );

  const send = useCallback(
    (command: string) => {
      void runTmuxCommand(sessionId, command).catch(() => {});
    },
    [sessionId],
  );

  // Dragging a divider cannot resize locally — the layout mirrors tmux — so the
  // drag is translated into `resize-pane` commands in cell units.
  const handlePaneResize = useCallback(
    (
      paneId: string | undefined,
      axis: SplitAxis,
      deltaPx: number,
      sentCells: number,
    ) => {
      const metrics = cellMetricsRef.current;
      const step = nextPaneResize({
        paneId,
        axis,
        deltaPx,
        cellWidth: metrics?.cellWidth ?? 0,
        cellHeight: metrics?.cellHeight ?? 0,
        sentCells,
      });
      if (step.command) send(step.command);
      return step.sentCells;
    },
    [send],
  );

  const openPaneMenu = useCallback(
    (paneId: string | undefined, point: { x: number; y: number }) => {
      const container = containerRef.current;
      if (!paneId || !container) return;
      const rect = container.getBoundingClientRect();
      setPaneMenu({ paneId, x: point.x - rect.left, y: point.y - rect.top });
    },
    [],
  );

  const handlePaneStep = useCallback(
    (paneId: string | undefined, axis: SplitAxis, cells: number) => {
      if (!paneId) return;
      const command = resizeCommand(paneId, axis, cells);
      if (command) send(command);
    },
    [send],
  );

  // tmux command line: the only way to reach commands the strip has no button
  // for (rename-window, resize-pane, select-layout, ...).
  const [commandLine, setCommandLine] = useState<string | null>(null);
  const [commandResult, setCommandResult] = useState<{
    output: string;
    error?: string;
  } | null>(null);
  const pendingRequestId = useRef<string | null>(null);
  const commandInputRef = useRef<HTMLInputElement | null>(null);
  const commandLineOpenRef = useRef(false);
  commandLineOpenRef.current = commandLine !== null;
  // Pane context menu, positioned relative to the pane container.
  const [paneMenu, setPaneMenu] = useState<{
    paneId: string;
    x: number;
    y: number;
  } | null>(null);

  // Command history survives view remounts, like a shell's.
  const [historyIndex, setHistoryIndex] = useState<number | null>(null);

  // Inline window rename, opened by double-clicking a window tab.
  const [renamingWindow, setRenamingWindow] = useState<{
    id: string;
    name: string;
  } | null>(null);

  const submitRename = useCallback(() => {
    const target = renamingWindow;
    setRenamingWindow(null);
    const name = target?.name.trim();
    if (!target || !name) return;
    send(`rename-window -t ${target.id} ${name}`);
  }, [renamingWindow, send]);

  useEffect(() => {
    if (commandLine === null) return;
    commandInputRef.current?.focus();
  }, [commandLine]);

  useEffect(() => {
    if (!paneMenu) return;
    const close = () => setPaneMenu(null);
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    window.addEventListener("pointerdown", close);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("pointerdown", close);
      window.removeEventListener("keydown", onKey);
    };
  }, [paneMenu]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void listen<TmuxCommandResponse>(
      tmuxCommandResponseEvent(sessionId),
      (event) => {
        if (disposed || event.payload.requestId !== pendingRequestId.current)
          return;
        pendingRequestId.current = null;
        setCommandResult({
          output: event.payload.output,
          error: event.payload.error,
        });
      },
    )
      .then((dispose) => {
        if (disposed) dispose();
        else unlisten = dispose;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [sessionId]);

  const submitCommandLine = useCallback(() => {
    const command = (commandLine ?? "").trim();
    if (!command) {
      setCommandLine(null);
      return;
    }
    rememberCommand(command);
    setHistoryIndex(null);
    const requestId = crypto.randomUUID();
    pendingRequestId.current = requestId;
    setCommandResult(null);
    setCommandLine("");
    void runTmuxCommandWithReply(sessionId, command, requestId).catch(
      (error) => {
        logger.warn({
          domain: "session.lifecycle",
          event: "tmux.command_failed",
          message: "Failed to run a tmux command",
          data: { session_id: sessionId, command },
          error,
        });
      },
    );
  }, [commandLine, sessionId]);

  // WindTerm-style switching: tmux keeps its own prefix key free, and these
  // never reach the pane.
  useEffect(() => {
    const MODIFIER_KEYS = new Set([
      "Shift",
      "Control",
      "Alt",
      "Meta",
      "CapsLock",
      "NumLock",
      "ScrollLock",
    ]);
    let prefixArmed = false;
    const onKeyDown = (event: KeyboardEvent) => {
      // The command line owns the keyboard while it is open, IME included.
      if (commandLineOpenRef.current) return;
      // IME composition and dead keys report legacy key values. Treating those
      // as the prefixed key typed the literal placeholder into the shell
      // (`Ctrl-b :` with an IME produced `ProcessProcess：：`).
      if (
        event.isComposing ||
        event.key === "Process" ||
        event.key === "Unidentified"
      ) {
        return;
      }
      if (
        event.ctrlKey &&
        !event.altKey &&
        !event.metaKey &&
        event.key === "b"
      ) {
        // Pane keystrokes are forwarded with `send-keys`, so tmux's own prefix
        // never reaches the tmux client. Swallow it and treat the next key as a
        // prefixed binding, so the familiar Ctrl-b d still detaches.
        event.preventDefault();
        event.stopPropagation();
        prefixArmed = true;
        return;
      }
      if (prefixArmed) {
        // A prefixed binding that needs Shift sends Shift down first (Ctrl-b ",
        // Ctrl-b %, Ctrl-b :). Consuming that as the binding broke every one of
        // them, so keep waiting for the real key.
        if (MODIFIER_KEYS.has(event.key)) return;
        prefixArmed = false;
        event.preventDefault();
        event.stopPropagation();
        const prefixCommands: Record<string, string> = {
          d: "detach-client",
          c: "new-window",
          n: "next-window",
          p: "previous-window",
          "%": "split-window -h",
          '"': "split-window -v",
          z: "resize-pane -Z",
          x: "kill-pane",
          "[": "copy-mode",
        };
        // ":" is Shift+";" on most layouts, toolkit-synthesised events can
        // report the unshifted key, and CJK input methods produce the full-width
        // form, so accept all three.
        if (event.key === ":" || event.key === ";" || event.key === "：") {
          setCommandLine((current) => current ?? "");
          return;
        }
        const command = prefixCommands[event.key];
        if (command) {
          send(command);
        } else if (activePaneId && event.key.length === 1) {
          // Not emulated here: hand tmux's prefix through to the pane verbatim.
          // Only a real character qualifies — forwarding a named key such as
          // "ArrowLeft" typed its name into the shell.
          void sendTmuxPaneInput(
            sessionId,
            activePaneId,
            `\u0002${event.key}`,
          ).catch(() => {});
        }
        return;
      }
      if (!event.altKey || event.ctrlKey || event.metaKey) return;
      // Alt+arrows follow tmux's directional pane selection, so tmux decides
      // which pane is "left of" the active one rather than duplicating layout
      // maths here.
      const paneByDirection: Record<string, string> = {
        ArrowLeft: "select-pane -L",
        ArrowRight: "select-pane -R",
        ArrowUp: "select-pane -U",
        ArrowDown: "select-pane -D",
      };
      const paneCommand = paneByDirection[event.key];
      if (paneCommand) {
        event.preventDefault();
        event.stopPropagation();
        send(paneCommand);
        return;
      }
      // Alt+[ / Alt+] cycle tmux windows.
      if (event.key === "[" || event.code === "BracketLeft") {
        event.preventDefault();
        event.stopPropagation();
        send("previous-window");
      } else if (event.key === "]" || event.code === "BracketRight") {
        event.preventDefault();
        event.stopPropagation();
        send("next-window");
      }
    };

    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [send, sessionId, activePaneId]);

  // tmux owns the layout, so the container's capacity drives the client size.
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    let frame = 0;
    const report = () => {
      frame = 0;
      const metrics = cellMetricsRef.current;
      if (!metrics || metrics.cellWidth <= 0 || metrics.cellHeight <= 0) return;
      // Gutters are chrome, not terminal cells, so they must not be counted as
      // usable columns — otherwise tmux lays panes out wider than the view.
      const gutterWidth = Array.from(
        container.querySelectorAll<HTMLElement>("[data-tmux-gutter]"),
      ).reduce((total, gutter) => total + gutter.clientWidth, 0);
      const availableWidth = Math.max(1, container.clientWidth - gutterWidth);
      const width = Math.floor(availableWidth / metrics.cellWidth);
      const height = Math.floor(container.clientHeight / metrics.cellHeight);
      if (width < 1 || height < 1) return;
      void resizeTmuxClient(sessionId, width, height).catch(() => {});
    };

    const observer = new ResizeObserver(() => {
      if (frame) cancelAnimationFrame(frame);
      frame = requestAnimationFrame(report);
    });
    observer.observe(container);
    // A pane may have reported metrics before the observer was installed.
    report();

    return () => {
      if (frame) cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [sessionId]);

  if (snapshot.windows.length === 0) {
    return null;
  }

  const strip = (
    <div className="flex h-7 shrink-0 items-center gap-1 overflow-x-auto border-t border-[var(--df-border)] bg-[var(--df-bg-panel)] px-1">
      <button
        type="button"
        aria-label={t("tmux.previousWindow")}
        title={t("tmux.previousWindow")}
        className="flex h-5 w-5 shrink-0 items-center justify-center rounded text-[var(--df-text-muted)] hover:bg-[var(--df-bg-hover)]"
        onClick={() => send("previous-window")}
      >
        <ChevronLeft className="h-3 w-3" />
      </button>
      <button
        type="button"
        aria-label={t("tmux.nextWindow")}
        title={t("tmux.nextWindow")}
        className="flex h-5 w-5 shrink-0 items-center justify-center rounded text-[var(--df-text-muted)] hover:bg-[var(--df-bg-hover)]"
        onClick={() => send("next-window")}
      >
        <ChevronRight className="h-3 w-3" />
      </button>
      <div className="mx-1 h-4 w-px shrink-0 bg-[var(--df-border)]" />
      {snapshot.windows.map((window) => {
        const isActive = window.id === activeWindow?.id;
        return (
          <div
            key={window.id}
            className={`group flex h-5 shrink-0 items-center gap-1 rounded px-2 text-xs ${
              isActive
                ? "bg-[var(--df-bg-hover)] text-[var(--df-text)]"
                : "text-[var(--df-text-muted)] hover:bg-[var(--df-bg-hover)]"
            }`}
          >
            {renamingWindow?.id === window.id ? (
              <input
                // biome-ignore lint/a11y/noAutofocus: the rename field is the point of the interaction
                autoFocus
                className="h-4 w-24 min-w-0 rounded bg-[var(--df-bg-terminal)] px-1 text-xs text-[var(--df-text)] outline-none"
                value={renamingWindow.name}
                aria-label={t("tmux.renameWindow")}
                onChange={(event) =>
                  setRenamingWindow({ id: window.id, name: event.target.value })
                }
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    submitRename();
                  } else if (event.key === "Escape") {
                    event.preventDefault();
                    setRenamingWindow(null);
                  }
                  event.stopPropagation();
                }}
                onBlur={() => setRenamingWindow(null)}
              />
            ) : (
              <button
                type="button"
                className="flex max-w-40 items-center gap-1 truncate"
                title={t("tmux.renameWindow")}
                onClick={() => send(`select-window -t ${window.id}`)}
                onDoubleClick={() =>
                  setRenamingWindow({ id: window.id, name: window.name })
                }
              >
                <span className="truncate">
                  {window.index}: {window.name || window.id}
                </span>
                {window.bell ? (
                  <Bell
                    className="h-3 w-3 shrink-0 text-[var(--df-primary)]"
                    aria-label={t("tmux.windowBell")}
                  />
                ) : null}
                {window.activity && !window.bell ? (
                  <CircleDot
                    className="h-3 w-3 shrink-0 text-[var(--df-text-muted)]"
                    aria-label={t("tmux.windowActivity")}
                  />
                ) : null}
              </button>
            )}
            <button
              type="button"
              aria-label={t("tmux.killWindow")}
              title={t("tmux.killWindow")}
              className="opacity-0 transition-opacity group-hover:opacity-100"
              onClick={() => send(`kill-window -t ${window.id}`)}
            >
              <X className="h-3 w-3" />
            </button>
          </div>
        );
      })}
      <button
        type="button"
        aria-label={t("tmux.newWindow")}
        title={t("tmux.newWindow")}
        className="flex h-5 w-5 shrink-0 items-center justify-center rounded text-[var(--df-text-muted)] hover:bg-[var(--df-bg-hover)]"
        onClick={() => send("new-window")}
      >
        <Plus className="h-3 w-3" />
      </button>
      <button
        type="button"
        aria-label={t("tmux.zoomPane")}
        title={t("tmux.zoomPane")}
        aria-pressed={activeWindow?.zoomed === true}
        className={`flex h-5 w-5 shrink-0 items-center justify-center rounded hover:bg-[var(--df-bg-hover)] ${
          activeWindow?.zoomed
            ? "bg-[var(--df-bg-hover)] text-[var(--df-primary)]"
            : "text-[var(--df-text-muted)]"
        }`}
        onClick={() => send("resize-pane -Z")}
      >
        <Maximize2 className="h-3 w-3" />
      </button>
      <button
        type="button"
        aria-label={t("tmux.splitHorizontal")}
        title={t("tmux.splitHorizontal")}
        className="flex h-5 w-5 shrink-0 items-center justify-center rounded text-[var(--df-text-muted)] hover:bg-[var(--df-bg-hover)]"
        onClick={() => send("split-window -h")}
      >
        <Columns2 className="h-3 w-3" />
      </button>
      <button
        type="button"
        aria-label={t("tmux.splitVertical")}
        title={t("tmux.splitVertical")}
        className="flex h-5 w-5 shrink-0 items-center justify-center rounded text-[var(--df-text-muted)] hover:bg-[var(--df-bg-hover)]"
        onClick={() => send("split-window -v")}
      >
        <Rows2 className="h-3 w-3" />
      </button>
      <select
        aria-label={t("tmux.layout")}
        title={t("tmux.layout")}
        className="h-5 shrink-0 rounded bg-transparent text-xs text-[var(--df-text-muted)] outline-none hover:bg-[var(--df-bg-hover)]"
        value=""
        onChange={(event) => {
          if (event.target.value) send(`select-layout ${event.target.value}`);
        }}
      >
        <option value="">{t("tmux.layout")}</option>
        <option value="even-horizontal">
          {t("tmux.layoutEvenHorizontal")}
        </option>
        <option value="even-vertical">{t("tmux.layoutEvenVertical")}</option>
        <option value="main-horizontal">
          {t("tmux.layoutMainHorizontal")}
        </option>
        <option value="tiled">{t("tmux.layoutTiled")}</option>
      </select>
      <button
        type="button"
        aria-label={t("tmux.detach")}
        title={t("tmux.detach")}
        className="flex h-5 w-5 shrink-0 items-center justify-center rounded text-[var(--df-text-muted)] hover:bg-[var(--df-bg-hover)]"
        onClick={() => send("detach-client")}
      >
        <LogOut className="h-3 w-3" />
      </button>
      <button
        type="button"
        aria-label={t("tmux.commandLine")}
        title={t("tmux.commandLine")}
        className="flex h-5 w-5 shrink-0 items-center justify-center rounded text-[var(--df-text-muted)] hover:bg-[var(--df-bg-hover)]"
        onClick={() => setCommandLine((current) => current ?? "")}
      >
        <SquareTerminal className="h-3 w-3" />
      </button>
    </div>
  );

  return (
    <div className="flex h-full w-full min-h-0 flex-col bg-[var(--df-bg-terminal)]">
      {/* Panes fill the space; the window strip sits below them. */}
      <div ref={containerRef} className="min-h-0 flex-1">
        {activeWindow?.layout ? (
          <LayoutNodeView
            node={activeWindow.layout}
            sessionId={sessionId}
            activePaneId={activePaneId}
            onSelectPane={handleSelectPane}
            onCellMetrics={handleCellMetrics}
            onResizePane={handlePaneResize}
            onStepPane={handlePaneStep}
            onPaneMenu={openPaneMenu}
          />
        ) : (
          <div className="flex h-full items-center justify-center text-xs text-[var(--df-text-muted)]">
            {collectLayoutPanes(activeWindow?.layout).length === 0
              ? t("tmux.waitingForLayout")
              : null}
          </div>
        )}
        {paneMenu ? (
          <div
            role="menu"
            aria-label={t("tmux.paneMenu")}
            className="absolute z-20 min-w-40 rounded border border-[var(--df-border)] bg-[var(--df-bg-panel)] py-1 text-xs shadow-lg"
            style={{ left: paneMenu.x, top: paneMenu.y }}
            onPointerDown={(event) => event.stopPropagation()}
          >
            <button
              type="button"
              role="menuitem"
              className="block w-full truncate px-3 py-1 text-left hover:bg-[var(--df-bg-hover)]"
              onClick={() => {
                requestTmuxPaneFind(sessionId, paneMenu.paneId);
                setPaneMenu(null);
              }}
            >
              {t("terminalCtx.find")}
            </button>
            {(
              [
                [
                  "tmux.splitHorizontal",
                  `split-window -h -t ${paneMenu.paneId}`,
                ],
                ["tmux.splitVertical", `split-window -v -t ${paneMenu.paneId}`],
                ["tmux.zoomPane", `resize-pane -t ${paneMenu.paneId} -Z`],
                ["tmux.swapPaneUp", `swap-pane -t ${paneMenu.paneId} -U`],
                ["tmux.swapPaneDown", `swap-pane -t ${paneMenu.paneId} -D`],
                ["tmux.breakPane", `break-pane -s ${paneMenu.paneId}`],
                ["tmux.killPane", `kill-pane -t ${paneMenu.paneId}`],
              ] as const
            ).map(([labelKey, command]) => (
              <button
                key={command}
                type="button"
                role="menuitem"
                className="block w-full truncate px-3 py-1 text-left hover:bg-[var(--df-bg-hover)]"
                onClick={() => {
                  send(command);
                  setPaneMenu(null);
                }}
              >
                {t(labelKey)}
              </button>
            ))}
          </div>
        ) : null}
      </div>
      {commandLine !== null ? (
        <div className="flex flex-col gap-1 border-t border-[var(--df-border)] px-2 py-1">
          <div
            className="flex items-center gap-1 overflow-x-auto"
            aria-label={t("tmux.commandPresets")}
            role="toolbar"
          >
            {COMMAND_PRESETS.map((preset) => (
              <button
                key={preset}
                type="button"
                className="shrink-0 rounded bg-[var(--df-bg-hover)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--df-text-muted)] hover:text-[var(--df-text)]"
                title={preset}
                onClick={() => {
                  setCommandLine(preset);
                  commandInputRef.current?.focus();
                }}
              >
                {preset.trim()}
              </button>
            ))}
          </div>
          <input
            ref={commandInputRef}
            className="h-6 w-full min-w-0 rounded bg-[var(--df-bg-hover)] px-2 font-mono text-xs text-[var(--df-text)] outline-none"
            value={commandLine}
            placeholder={t("tmux.commandPlaceholder")}
            aria-label={t("tmux.commandLine")}
            spellCheck={false}
            onChange={(event) => setCommandLine(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                submitCommandLine();
              } else if (event.key === "Escape") {
                event.preventDefault();
                setCommandLine(null);
                setHistoryIndex(null);
              } else if (event.key === "ArrowUp" && commandHistory.length > 0) {
                event.preventDefault();
                const next =
                  historyIndex === null
                    ? commandHistory.length - 1
                    : Math.max(0, historyIndex - 1);
                setHistoryIndex(next);
                setCommandLine(commandHistory[next] ?? "");
              } else if (event.key === "ArrowDown" && historyIndex !== null) {
                event.preventDefault();
                const next = historyIndex + 1;
                if (next >= commandHistory.length) {
                  setHistoryIndex(null);
                  setCommandLine("");
                } else {
                  setHistoryIndex(next);
                  setCommandLine(commandHistory[next] ?? "");
                }
              }
              event.stopPropagation();
            }}
          />
          {commandResult ? (
            <pre className="max-h-24 overflow-auto whitespace-pre-wrap break-all font-mono text-xs text-[var(--df-text-muted)]">
              {commandResult.error ?? commandResult.output}
            </pre>
          ) : null}
        </div>
      ) : null}
      {strip}
    </div>
  );
}

export default TmuxGatewayView;
