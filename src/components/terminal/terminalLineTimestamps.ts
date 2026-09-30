import type { Terminal } from "@xterm/xterm";

export function stampTerminalWrittenLines(
  timestamps: Map<number, number>,
  terminal: Terminal,
  lineOffset: number,
  from: number,
  to: number,
  ts: number,
) {
  const start = Math.min(from, to);
  let end = Math.max(from, to);
  const buffer = terminal.buffer.active;
  const cursorLine = buffer.getLine(to - lineOffset);

  // A newline can move the cursor onto an untouched row that belongs to the next write.
  if (to > from && buffer.cursorX === 0 && cursorLine?.translateToString(true) === "") {
    end -= 1;
  }

  for (let y = start; y <= end; y += 1) {
    if (!timestamps.has(y)) {
      timestamps.set(y, ts);
    }
  }

  const keepFrom = Math.max(0, start - 3000);
  for (const key of Array.from(timestamps.keys())) {
    if (key < keepFrom) {
      timestamps.delete(key);
    }
  }
}
