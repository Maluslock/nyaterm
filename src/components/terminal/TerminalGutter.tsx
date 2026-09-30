import type { Terminal } from "@xterm/xterm";
import type { RefObject } from "react";
import { useCallback, useEffect, useRef, useState } from "react";

interface TerminalGutterProps {
  terminalRef: RefObject<Terminal | null>;
  showLineNumbers: boolean;
  showTimestamps: boolean;
  timestampFormat: string;
  lineTimestamps: Map<number, number>;
  getLineOffset: () => number;
  sessionId?: string;
  suspended?: boolean;
}

interface GutterLine {
  key: number;
  lineNumber: string;
  timestamp: string;
}

interface GutterLayout {
  lines: GutterLine[];
  maxLineNumberDigits: number;
  sessionId?: string;
  rowHeight: number;
  topPadding: number;
  fontFamily: string;
  fontSize: number;
  cellWidth: number;
}

const DEFAULT_TIMESTAMP_FORMAT = "[HH:mm:ss]";
const MAX_TIMESTAMP_FORMAT_LENGTH = 64;
const TIMESTAMP_WIDTH_SAMPLE_MS = new Date(
  2099,
  11,
  28,
  23,
  59,
  59,
  999,
).getTime();
const GUTTER_COLUMN_GAP = 12;
const GUTTER_RIGHT_PADDING = 8;
const MAX_TIMESTAMP_LOOKBACK_ROWS = 512;

function normalizeTimestampFormat(format: string | undefined): string {
  if (!format || format.trim().length === 0) {
    return DEFAULT_TIMESTAMP_FORMAT;
  }

  return Array.from(format).slice(0, MAX_TIMESTAMP_FORMAT_LENGTH).join("");
}

function formatTimestamp(ms: number, format: string): string {
  try {
    const d = new Date(ms);
    const year = d.getFullYear();
    const month = d.getMonth() + 1;
    const day = d.getDate();
    const hours = d.getHours();
    const minutes = d.getMinutes();
    const seconds = d.getSeconds();
    const milliseconds = String(d.getMilliseconds()).padStart(3, "0");

    return normalizeTimestampFormat(format).replace(
      /YYYY|YY|MM|M|DD|D|HH|H|mm|m|ss|s|SSS|SS|S/g,
      (token) => {
        switch (token) {
          case "YYYY":
            return String(year);
          case "YY":
            return String(year).slice(-2);
          case "MM":
            return String(month).padStart(2, "0");
          case "M":
            return String(month);
          case "DD":
            return String(day).padStart(2, "0");
          case "D":
            return String(day);
          case "HH":
            return String(hours).padStart(2, "0");
          case "H":
            return String(hours);
          case "mm":
            return String(minutes).padStart(2, "0");
          case "m":
            return String(minutes);
          case "ss":
            return String(seconds).padStart(2, "0");
          case "s":
            return String(seconds);
          case "SSS":
            return milliseconds;
          case "SS":
            return milliseconds.slice(0, 2);
          case "S":
            return milliseconds.slice(0, 1);
          default:
            return token;
        }
      },
    );
  } catch {
    return formatTimestamp(ms, DEFAULT_TIMESTAMP_FORMAT);
  }
}

interface XTermCoreWithRenderDimensions {
  _core?: {
    _renderService?: {
      dimensions?: {
        css?: {
          cell?: {
            height?: number;
            width?: number;
          };
        };
      };
    };
  };
}

export default function TerminalGutter({
  terminalRef,
  showLineNumbers,
  showTimestamps,
  timestampFormat,
  lineTimestamps,
  getLineOffset,
  sessionId,
  suspended = false,
}: TerminalGutterProps) {
  const rafRef = useRef(0);
  const viewportYRef = useRef(0);

  const [layout, setLayout] = useState<GutterLayout>({
    lines: [],
    maxLineNumberDigits: 1,
    sessionId,
    rowHeight: 18,
    topPadding: 0,
    fontFamily: "inherit",
    fontSize: 12,
    cellWidth: 8,
  });

  const computeLines = useCallback(() => {
    if (suspended) return;
    const terminal = terminalRef.current;
    if (!terminal || !terminal.element) return;

    const el = terminal.element;
    const buf = terminal.buffer.active;
    const viewport = el.querySelector(".xterm-viewport") as HTMLElement | null;

    const screen = el.querySelector(".xterm-screen") as HTMLElement | null;
    const screenHeight =
      viewport?.clientHeight ?? screen?.clientHeight ?? el.clientHeight;
    const topPadding = screen?.offsetTop ?? 0;

    const core = (terminal as Terminal & XTermCoreWithRenderDimensions)._core;
    const measuredCell = core?._renderService?.dimensions?.css?.cell;
    const rowHeight =
      measuredCell?.height && measuredCell.height > 0
        ? measuredCell.height
        : terminal.rows > 0
          ? screenHeight / terminal.rows
          : 18;
    const fontSize = Number(terminal.options.fontSize ?? 12);
    const cellWidth =
      measuredCell?.width && measuredCell.width > 0
        ? measuredCell.width
        : fontSize * 0.62;
    const viewportY = Math.max(
      0,
      Math.min(buf.baseY, viewportYRef.current || buf.viewportY),
    );
    const rows = terminal.rows;
    const cursorAbsoluteY = buf.baseY + buf.cursorY;
    const lineOffset = buf.type === "alternate" ? 0 : getLineOffset();

    const resolveTimestamp = (bufferLine: number): number | undefined => {
      let y = bufferLine;

      while (y >= 0 && bufferLine - y < MAX_TIMESTAMP_LOOKBACK_ROWS) {
        const ts =
          buf.type === "alternate"
            ? undefined
            : lineTimestamps.get(lineOffset + y);
        if (ts) return ts;

        const line = buf.getLine(y);
        if (!line?.isWrapped) break;
        y -= 1;
      }

      return undefined;
    };

    const nextLines: GutterLine[] = [];

    for (let i = 0; i < rows; i += 1) {
      const bufferLine = viewportY + i;
      const line = buf.getLine(bufferLine);
      const isWrapped = line?.isWrapped ?? false;
      const hasRenderedRow = bufferLine <= cursorAbsoluteY;
      const ts = showTimestamps ? resolveTimestamp(bufferLine) : undefined;
      const logicalLine = lineOffset + bufferLine;

      nextLines.push({
        key: logicalLine,
        timestamp:
          showTimestamps && hasRenderedRow && !isWrapped && ts
            ? formatTimestamp(ts, timestampFormat)
            : "",
        lineNumber:
          showLineNumbers && hasRenderedRow && !isWrapped
            ? String(logicalLine + 1)
            : "",
      });
    }

    const visibleLineNumberDigits = nextLines.reduce(
      (max, line) => Math.max(max, line.lineNumber.length),
      1,
    );

    setLayout((previous) => ({
      lines: nextLines,
      maxLineNumberDigits:
        previous.sessionId === sessionId
          ? Math.max(previous.maxLineNumberDigits, visibleLineNumberDigits)
          : visibleLineNumberDigits,
      sessionId,
      rowHeight,
      topPadding,
      fontFamily: String(terminal.options.fontFamily ?? "inherit"),
      fontSize,
      cellWidth,
    }));
  }, [
    suspended,
    terminalRef,
    lineTimestamps,
    getLineOffset,
    showLineNumbers,
    showTimestamps,
    timestampFormat,
    sessionId,
  ]);

  const scheduleUpdate = useCallback(() => {
    cancelAnimationFrame(rafRef.current);
    rafRef.current = requestAnimationFrame(() => {
      computeLines();
    });
  }, [computeLines]);

  useEffect(() => {
    if (suspended) {
      cancelAnimationFrame(rafRef.current);
      return;
    }

    let disposed = false;
    let handleExternalRefresh: ((event: Event) => void) | null = null;
    let disposables: Array<{ dispose: () => void }> = [];

    const attach = () => {
      if (disposed) return;

      const terminal = terminalRef.current;
      if (!terminal) {
        rafRef.current = requestAnimationFrame(attach);
        return;
      }

      viewportYRef.current = terminal.buffer.active.viewportY;
      scheduleUpdate();

      disposables = [
        terminal.onRender(() => {
          viewportYRef.current = terminal.buffer.active.viewportY;
          scheduleUpdate();
        }),
        terminal.onWriteParsed(() => {
          viewportYRef.current = terminal.buffer.active.viewportY;
          scheduleUpdate();
        }),
        terminal.onScroll((viewportY) => {
          viewportYRef.current = viewportY;
          scheduleUpdate();
        }),
        terminal.onResize(() => {
          viewportYRef.current = terminal.buffer.active.viewportY;
          scheduleUpdate();
        }),
      ];

      handleExternalRefresh = (event: Event) => {
        const customEvent = event as CustomEvent<{ sessionId?: string }>;
        if (
          !sessionId ||
          !customEvent.detail?.sessionId ||
          customEvent.detail.sessionId === sessionId
        ) {
          scheduleUpdate();
        }
      };

      window.addEventListener("nyaterm:refresh-gutter", handleExternalRefresh);
    };

    attach();

    return () => {
      disposed = true;
      cancelAnimationFrame(rafRef.current);
      disposables.forEach((d) => {
        d.dispose();
      });
      if (handleExternalRefresh) {
        window.removeEventListener(
          "nyaterm:refresh-gutter",
          handleExternalRefresh,
        );
      }
    };
  }, [suspended, terminalRef, scheduleUpdate, sessionId]);

  useEffect(() => {
    if (suspended) return;
    scheduleUpdate();
  }, [scheduleUpdate, suspended]);

  if (suspended || (!showLineNumbers && !showTimestamps)) {
    return null;
  }

  const lineNumWidth = showLineNumbers
    ? Math.max(Math.ceil(layout.cellWidth * layout.maxLineNumberDigits) + 2, 24)
    : 0;
  const timestampTemplate = formatTimestamp(
    TIMESTAMP_WIDTH_SAMPLE_MS,
    timestampFormat,
  );
  const tsWidth = showTimestamps
    ? Math.ceil(layout.cellWidth * timestampTemplate.length) + 2
    : 0;
  const columnGap = showLineNumbers && showTimestamps ? GUTTER_COLUMN_GAP : 0;
  const gutterWidth = tsWidth + lineNumWidth + columnGap + GUTTER_RIGHT_PADDING;

  return (
    <div
      className="nyaterm-wallpaper-transparent-surface nyaterm-terminal-surface shrink-0 select-none overflow-hidden border-r"
      style={{
        boxSizing: "content-box",
        width: gutterWidth,
        paddingTop: layout.topPadding,
        borderColor:
          "color-mix(in srgb, var(--df-terminal-fg, var(--df-text)) 18%, var(--df-terminal-surface-bg))",
        backgroundColor: "var(--df-terminal-surface-bg)",
        fontFamily: layout.fontFamily,
        fontSize: layout.fontSize,
      }}
    >
      {layout.lines.map((line) => (
        <div
          key={line.key}
          className="flex items-center justify-end whitespace-nowrap text-right tabular-nums"
          style={{
            height: layout.rowHeight,
            lineHeight: `${layout.rowHeight}px`,
            columnGap,
            paddingRight: GUTTER_RIGHT_PADDING,
          }}
        >
          {showTimestamps && (
            <span
              className="inline-block text-right"
              style={{
                width: tsWidth,
                color: "var(--df-terminal-fg, var(--df-text))",
                opacity: 0.85,
              }}
            >
              {line.timestamp}
            </span>
          )}

          {showLineNumbers && (
            <span
              className="inline-block text-right"
              style={{
                width: lineNumWidth,
                color: "var(--df-terminal-fg, var(--df-text))",
                opacity: 0.7,
              }}
            >
              {line.lineNumber}
            </span>
          )}
        </div>
      ))}
    </div>
  );
}
