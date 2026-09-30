import { Terminal } from "@xterm/xterm";
import { afterEach, describe, expect, it } from "vitest";
import { stampTerminalWrittenLines } from "./terminalLineTimestamps";
import type { XTermInternalTrimSource } from "./xterminalInternalTypes";

const terminals: Terminal[] = [];

afterEach(() => {
  for (const terminal of terminals) terminal.dispose();
  terminals.length = 0;
});

function createHarness(cols = 80, rows = 24, scrollback = 1000) {
  const terminal = new Terminal({ cols, rows, scrollback });
  terminals.push(terminal);
  const timestamps = new Map<number, number>();
  let lineOffset = 0;
  const onTrim = (terminal as Terminal & XTermInternalTrimSource)._core?._bufferService?.buffers
    ?.normal?.lines?.onTrim;
  if (!onTrim) throw new Error("xterm trim event is unavailable");
  onTrim((amount) => {
    lineOffset += amount;
  });

  return {
    terminal,
    timestamps,
    async write(data: string, ts: number) {
      const currentLine = () =>
        lineOffset + terminal.buffer.active.baseY + terminal.buffer.active.cursorY;
      const before = currentLine();
      await new Promise<void>((resolve) => terminal.write(data, resolve));
      stampTerminalWrittenLines(timestamps, terminal, lineOffset, before, currentLine(), ts);
    },
    currentLine: () => lineOffset + terminal.buffer.active.baseY + terminal.buffer.active.cursorY,
  };
}

describe("terminal line timestamps", () => {
  it("assigns the next batch's time to the row after a trailing newline", async () => {
    const harness = createHarness();
    await harness.write("A\r\n", 1000);
    expect(harness.timestamps.get(0)).toBe(1000);
    expect(harness.timestamps.has(1)).toBe(false);

    await harness.write("B\r\n", 5000);
    expect(harness.timestamps.get(1)).toBe(5000);
    expect(harness.timestamps.has(2)).toBe(false);
  });

  it("timestamps populated rows in a multiline write", async () => {
    const harness = createHarness();
    await harness.write("A\r\nB\r\n", 1000);
    expect([...harness.timestamps]).toEqual([
      [0, 1000],
      [1, 1000],
    ]);
  });

  it("keeps the first time for later writes and CR overwrites on the same row", async () => {
    const harness = createHarness();
    await harness.write("A", 1000);
    await harness.write("B\r\n", 5000);
    await harness.write("progress", 6000);
    await harness.write("\rprogress done", 7000);
    expect(harness.timestamps.get(0)).toBe(1000);
    expect(harness.timestamps.get(1)).toBe(6000);
    expect(harness.timestamps.has(2)).toBe(false);
  });

  it("does not pre-stamp a blank row after an automatic wrap", async () => {
    const harness = createHarness(4);
    await harness.write("ABCD", 1000);
    await harness.write("E", 5000);
    expect(harness.timestamps.get(0)).toBe(1000);
    expect(harness.timestamps.get(1)).toBe(5000);
  });

  it("preserves absolute line ownership after scrollback trimming", async () => {
    const harness = createHarness(80, 2, 2);
    for (let i = 0; i < 6; i += 1) {
      await harness.write(`${i}\r\n`, 1000 + i);
    }
    expect(harness.currentLine()).toBe(6);
    expect(harness.timestamps.get(5)).toBe(1005);
    expect(harness.timestamps.has(6)).toBe(false);
    await harness.write("last", 9000);
    expect(harness.timestamps.get(6)).toBe(9000);
  });
});
