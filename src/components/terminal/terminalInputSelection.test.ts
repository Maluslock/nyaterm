import type { Terminal } from "@xterm/xterm";
import { describe, expect, it, vi } from "vitest";
import { createTerminalInputState } from "@/lib/terminalInputTracker";
import { getInputIndexAtBufferPosition } from "./terminalInputSelection";

describe("Smart Cursor logical line budget", () => {
  it("falls back before translating a line wrapped across thousands of rows", () => {
    const translate = vi.fn(() => "x".repeat(80));
    const terminal = {
      cols: 80,
      buffer: {
        active: {
          type: "normal",
          length: 10_000,
          baseY: 9_999,
          cursorY: 0,
          cursorX: 1,
          getLine: (y: number) =>
            y >= 0 && y < 10_000
              ? { isWrapped: y > 0, length: 80, translateToString: translate }
              : undefined,
        },
      },
    } as unknown as Terminal;
    const state = { ...createTerminalInputState(), value: "x", cursor: 1 };

    expect(
      getInputIndexAtBufferPosition(terminal, state, { x: 1, y: 9_999 }),
    ).toBeNull();
    expect(translate).not.toHaveBeenCalled();
  });
});
