/**
 * One tmux pane rendered as a local xterm instance.
 *
 * Output arrives pre-demultiplexed on `tmux-pane-output-<session>`, filtered by
 * pane id. Input goes back through `send-keys -H` on the backend, so every byte
 * (control characters, ESC sequences, UTF-8) reaches the pane verbatim.
 *
 * The pane reuses the same terminal features as ordinary sessions — keyword
 * highlighting (including the built-in semantic rule categories) and the
 * timestamp / line-number gutter — rather than reimplementing them.
 */
import { listen } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTerminalAppSettings } from "@/context/AppContext";
import { useTheme } from "@/context/ThemeContext";
import { useCommandHistory } from "@/hooks/useCommandHistory";
import { useKeywordHighlighter } from "@/hooks/useKeywordHighlighter";
import { buildTerminalThemeColors } from "@/lib/backgroundImage";
import { invoke } from "@/lib/invoke";
import { hexLuminance } from "@/lib/keywordHighlightPresets";
import { buildTerminalCommandInput } from "@/lib/sessionInput";
import {
  applyTerminalInputData,
  createTerminalInputState,
  canSuggestFromTracker,
} from "@/lib/terminalInputTracker";
import { applyTmuxPaneInput } from "@/lib/tmuxPaneInput";
import {
  sendTmuxPaneInput,
  type TmuxPane,
  type TmuxPaneOutput,
  tmuxPaneOutputEvent,
} from "@/lib/tmuxGateway";
import { commandStartsSuggestionSuppressingProgram } from "@/lib/commandSuggestionSuppression";
import CommandSuggestions from "../CommandSuggestions";
import TerminalGutter from "../TerminalGutter";
import "@xterm/xterm/css/xterm.css";

export interface TmuxPaneCellMetrics {
  cellWidth: number;
  cellHeight: number;
}

interface TmuxPaneTerminalProps {
  sessionId: string;
  pane: TmuxPane;
  isActive: boolean;
  onSelect: (paneId: string) => void;
  onCellMetrics?: (metrics: TmuxPaneCellMetrics) => void;
}

function resolveFontSize(value: unknown): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 14;
}

export function TmuxPaneTerminal({
  sessionId,
  pane,
  isActive,
  onSelect,
  onCellMetrics,
}: TmuxPaneTerminalProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const paneIdRef = useRef<string | null>(pane.id ?? null);
  paneIdRef.current = pane.id ?? null;

  const { theme } = useTheme();
  const {
    appearance,
    interaction,
    terminal: terminalSettings,
  } = useTerminalAppSettings();
  const terminalThemeColors = useMemo(
    () => buildTerminalThemeColors(theme.colors.terminal, appearance),
    [appearance, theme],
  );

  const [terminalInstance, setTerminalInstance] = useState<Terminal | null>(null);

  const commandSuggestionsEnabled = interaction.command_suggestions_enabled;
  const commandSuggestionMinChars = interaction.command_suggestion_min_chars;
  const commandSuggestionMaxChars = interaction.command_suggestion_max_chars;

  // Per-pane input tracking. tmux panes get no shell-integration markers (the
  // host session's capture runs before the gateway sees anything), so the pane
  // tracks its own line and registers submissions itself.
  const inputStateRef = useRef(createTerminalInputState());
  const suggestionSuppressedRef = useRef(false);

  // The gutter is addressed by a synthetic key so it never collides with the
  // host session's own gutter.
  const paneKey = `tmux:${pane.id ?? pane.index}`;
  const lineTimestampsRef = useRef(new Map<number, number>());
  const gutterLineOffsetRef = useRef(0);
  const getLineOffset = useCallback(() => gutterLineOffsetRef.current, []);

  const showLineNumbers = terminalSettings.show_line_numbers ?? false;
  const showTimestamps = terminalSettings.show_timestamps ?? false;
  const timestampFormat = terminalSettings.timestamp_format ?? "[HH:mm:ss]";
  const showGutter = showLineNumbers || showTimestamps;

  const isDark = useMemo(
    () => hexLuminance(theme.colors.terminal.background) < 0.5,
    [theme],
  );

  // Read inside stable callbacks without making them depend on settings.
  const stampConfigRef = useRef({ enabled: showTimestamps, paneKey });
  stampConfigRef.current = { enabled: showTimestamps, paneKey };
  const refreshFrameRef = useRef<number | null>(null);

  /**
   * Record the first-seen time for the line the pane just wrote to, then ask the
   * gutter to repaint. Coalesced to one repaint per frame.
   */
  const stampWrittenLines = useCallback(() => {
    const terminal = terminalRef.current;
    const { enabled, paneKey: key } = stampConfigRef.current;
    if (!terminal || !enabled) return;

    const buffer = terminal.buffer.active;
    if (buffer.type === "alternate") return;

    const cursorLine = buffer.baseY + buffer.cursorY;
    const map = lineTimestampsRef.current;
    if (!map.has(cursorLine)) {
      map.set(cursorLine, Date.now());
    }
    const keepFrom = Math.max(0, cursorLine - 3000);
    for (const line of Array.from(map.keys())) {
      if (line < keepFrom) map.delete(line);
    }

    if (refreshFrameRef.current === null) {
      refreshFrameRef.current = requestAnimationFrame(() => {
        refreshFrameRef.current = null;
        window.dispatchEvent(
          new CustomEvent("nyaterm:refresh-gutter", {
            detail: { sessionId: key },
          }),
        );
      });
    }
  }, []);

  // Creation-time-only values are read through a ref: the terminal must be
  // built once and then *updated* in place, otherwise a theme or font change
  // would tear down the pane and lose its scrollback.
  const initialOptionsRef = useRef({
    fontFamily: appearance.font_family || "JetBrains Mono, monospace",
    fontSize: resolveFontSize(appearance.font_size),
    theme: { ...terminalThemeColors },
  });

  const reportCellMetrics = useCallback(() => {
    const terminal = terminalRef.current;
    if (!terminal || !onCellMetrics) return;
    // `.xterm-screen` is exactly cols x rows cells, so this is an exact
    // measurement rather than a guess from font metrics.
    const screen = terminal.element?.querySelector<HTMLElement>(".xterm-screen");
    if (!screen || !terminal.cols || !terminal.rows) return;
    const width = screen.clientWidth;
    const height = screen.clientHeight;
    if (width <= 0 || height <= 0) return;
    onCellMetrics({
      cellWidth: width / terminal.cols,
      cellHeight: height / terminal.rows,
    });
  }, [onCellMetrics]);

  /** Gate the suggestion popup for this pane. */
  const canShowCommandSuggestions = useCallback(
    (options?: { allowEmpty?: boolean }) => {
      const terminal = terminalRef.current;
      // Full-screen programs own the alternate screen; never suggest there.
      if (!terminal || terminal.buffer.active.type === "alternate") return false;
      if (suggestionSuppressedRef.current) return false;
      const state = inputStateRef.current;
      if (options?.allowEmpty) {
        return !state.desynced && !state.multiline;
      }
      return canSuggestFromTracker(state);
    },
    [],
  );

  /** Write a chosen suggestion into the pane, replacing the current line. */
  const applySuggestion = useCallback(
    (command: string, execute: boolean) => {
      const paneId = paneIdRef.current;
      if (!paneId) return;

      const state = inputStateRef.current;
      const replaceCurrentLine = state.lineRewriteRequired;
      const input = replaceCurrentLine
        ? `\u0005\u0015${command}`
        : `${"\x7f".repeat(state.value.length)}${command}`;
      const data = buildTerminalCommandInput(input, execute);

      void sendTmuxPaneInput(sessionId, paneId, data).catch(() => {});

      if (execute) {
        void invoke("register_command_submission", {
          sessionId,
          command,
        }).catch(() => {});
        if (commandStartsSuggestionSuppressingProgram(command)) {
          suggestionSuppressedRef.current = true;
        }
      }

      // The pane's shell echoes the rewrite; mirror it locally so the tracker
      // and the popup stay in step with what is on the line. Submitting clears
      // the line, otherwise the tracker holds the new text.
      inputStateRef.current = execute
        ? createTerminalInputState()
        : applyTerminalInputData(createTerminalInputState(), command);
    },
    [sessionId],
  );

  const {
    suggestions,
    selectedIndex,
    showSuggestions,
    cursorPosition,
    triggerSearch,
    dismissSuggestions,
    handleSelectSuggestion,
    handleDeleteSuggestion,
  } = useCommandHistory(
    terminalRef,
    inputStateRef,
    applySuggestion,
    canShowCommandSuggestions,
    commandSuggestionsEnabled,
    commandSuggestionMinChars,
    commandSuggestionMaxChars,
  );

  const syncSuggestions = useCallback(() => {
    if (canShowCommandSuggestions()) triggerSearch();
    else dismissSuggestions();
  }, [canShowCommandSuggestions, dismissSuggestions, triggerSearch]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const terminal = new Terminal({
      ...initialOptionsRef.current,
      cursorBlink: true,
      allowProposedApi: true,
      scrollback: 5000,
    });
    terminal.open(container);
    terminalRef.current = terminal;
    setTerminalInstance(terminal);
    reportCellMetrics();

    const dataSubscription = terminal.onData((data) => {
      const paneId = paneIdRef.current;
      if (!paneId) return;

      const result = applyTmuxPaneInput(inputStateRef.current, data);
      inputStateRef.current = result.nextState;

      if (result.submission) {
        void invoke("register_command_submission", {
          sessionId,
          command: result.submission,
        }).catch(() => {});
      }
      // A fresh prompt line means any suppressed full-screen program has exited.
      if (result.submitted) {
        suggestionSuppressedRef.current = false;
      }

      void sendTmuxPaneInput(sessionId, paneId, data).catch(() => {});
      syncSuggestions();
    });

    const resizeSubscription = terminal.onResize(() => reportCellMetrics());

    let disposed = false;
    let unlisten: (() => void) | null = null;
    void listen<TmuxPaneOutput>(tmuxPaneOutputEvent(sessionId), (event) => {
      const paneId = paneIdRef.current;
      if (!paneId || event.payload.paneId !== paneId) return;
      terminal.write(event.payload.data, () => stampWrittenLines());
    })
      .then((dispose) => {
        if (disposed) dispose();
        else unlisten = dispose;
      })
      .catch(() => {});

    return () => {
      disposed = true;
      unlisten?.();
      dataSubscription.dispose();
      resizeSubscription.dispose();
      if (refreshFrameRef.current !== null) {
        cancelAnimationFrame(refreshFrameRef.current);
        refreshFrameRef.current = null;
      }
      terminal.dispose();
      terminalRef.current = null;
      setTerminalInstance(null);
      lineTimestampsRef.current.clear();
    };
  }, [sessionId, reportCellMetrics, stampWrittenLines, syncSuggestions]);

  // Keyword highlighting is shared with ordinary sessions, including the
  // built-in semantic rule categories.
  useKeywordHighlighter(terminalInstance, terminalSettings, paneKey, isDark, {
    suspended: !isActive,
    releaseCachesAfterDelay: true,
  });

  // tmux owns pane geometry: resize the local terminal to match it exactly.
  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal || pane.width <= 0 || pane.height <= 0) return;
    if (terminal.cols === pane.width && terminal.rows === pane.height) return;
    terminal.resize(pane.width, pane.height);
  }, [pane.width, pane.height]);

  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal) return;
    terminal.options.theme = { ...terminalThemeColors };
    terminal.options.fontFamily =
      appearance.font_family || "JetBrains Mono, monospace";
    terminal.options.fontSize = resolveFontSize(appearance.font_size);
  }, [terminalThemeColors, appearance.font_family, appearance.font_size]);

  useEffect(() => {
    if (isActive) terminalRef.current?.focus();
  }, [isActive]);

  const handleMouseDown = useCallback(() => {
    if (!pane.id) return;
    onSelect(pane.id);
    terminalRef.current?.focus();
  }, [onSelect, pane.id]);

  return (
    <div
      data-tmux-pane={pane.id ?? pane.index}
      data-active={isActive ? "true" : "false"}
      onMouseDown={handleMouseDown}
      className={`flex h-full w-full min-h-0 min-w-0 overflow-hidden bg-[var(--df-bg-terminal)] ${
        isActive ? "ring-1 ring-inset ring-[var(--df-accent)]" : ""
      }`}
    >
      {showGutter && (
        // Tagged so the view can subtract gutter width when reporting the tmux
        // client size — gutters are chrome, not terminal cells.
        <div data-tmux-gutter className="h-full shrink-0">
          <TerminalGutter
            terminalRef={terminalRef}
            showLineNumbers={showLineNumbers}
            showTimestamps={showTimestamps}
            timestampFormat={timestampFormat}
            lineTimestamps={lineTimestampsRef.current}
            getLineOffset={getLineOffset}
            sessionId={paneKey}
            suspended={!isActive}
          />
        </div>
      )}
      <div ref={containerRef} className="h-full w-full min-h-0 min-w-0" />
      <CommandSuggestions
        suggestions={suggestions}
        visible={
          commandSuggestionsEnabled &&
          isActive &&
          showSuggestions &&
          canShowCommandSuggestions({ allowEmpty: suggestions.length > 0 })
        }
        selectedIndex={selectedIndex}
        cursorPosition={cursorPosition}
        onSelect={handleSelectSuggestion}
        onDismiss={dismissSuggestions}
        onDeleteHistory={handleDeleteSuggestion}
      />
    </div>
  );
}

export default TmuxPaneTerminal;
