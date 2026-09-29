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
import { ChevronLeft, ChevronRight, LogOut, Plus, SquareTerminal, X } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  collectLayoutPanes,
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
}

function LayoutNodeView({
  node,
  sessionId,
  activePaneId,
  onSelectPane,
  onCellMetrics,
}: LayoutNodeProps) {
  if (node.kind === "leaf") {
    return (
      <TmuxPaneTerminal
        sessionId={sessionId}
        pane={node.pane}
        isActive={activePaneId !== null && node.pane.id === activePaneId}
        onSelect={onSelectPane}
        onCellMetrics={onCellMetrics}
      />
    );
  }

  const isColumns = node.direction === "columns";
  return (
    <div
      className={`flex h-full w-full min-h-0 min-w-0 ${
        isColumns ? "flex-row" : "flex-col"
      }`}
    >
      {node.children.map((child, index) => (
        <div
          // Layout children are positional; tmux reports geometry, not ids.
          // biome-ignore lint/suspicious/noArrayIndexKey: positional by design
          key={index}
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
          />
        </div>
      ))}
    </div>
  );
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
      void runTmuxCommand(sessionId, `select-pane -t ${paneId}`).catch(() => {});
    },
    [sessionId],
  );

  const send = useCallback(
    (command: string) => {
      void runTmuxCommand(sessionId, command).catch(() => {});
    },
    [sessionId],
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

  useEffect(() => {
    if (commandLine === null) return;
    commandInputRef.current?.focus();
  }, [commandLine]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void listen<TmuxCommandResponse>(tmuxCommandResponseEvent(sessionId), (event) => {
      if (disposed || event.payload.requestId !== pendingRequestId.current) return;
      pendingRequestId.current = null;
      setCommandResult({ output: event.payload.output, error: event.payload.error });
    })
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
    const requestId = crypto.randomUUID();
    pendingRequestId.current = requestId;
    setCommandResult(null);
    setCommandLine("");
    void runTmuxCommandWithReply(sessionId, command, requestId).catch((error) => {
      logger.warn({
        domain: "session.lifecycle",
        event: "tmux.command_failed",
        message: "Failed to run a tmux command",
        data: { session_id: sessionId, command },
        error,
      });
    });
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
      if (event.ctrlKey && !event.altKey && !event.metaKey && event.key === "b") {
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
          void sendTmuxPaneInput(sessionId, activePaneId, `\u0002${event.key}`).catch(
            () => {},
          );
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
            <button
              type="button"
              className="max-w-40 truncate"
              title={window.name || window.id}
              onClick={() => send(`select-window -t ${window.id}`)}
            >
              {window.index}: {window.name || window.id}
            </button>
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
          />
        ) : (
          <div className="flex h-full items-center justify-center text-xs text-[var(--df-text-muted)]">
            {collectLayoutPanes(activeWindow?.layout).length === 0
              ? t("tmux.waitingForLayout")
              : null}
          </div>
        )}
      </div>
      {commandLine !== null ? (
        <div className="flex flex-col gap-1 border-t border-[var(--df-border)] px-2 py-1">
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
