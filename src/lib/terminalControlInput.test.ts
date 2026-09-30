import type { Terminal } from "@xterm/xterm";
import { describe, expect, it, vi } from "vitest";
import {
  clearTerminalAll,
  registerTerminalUserInputMarker,
  sendTerminalClearInput,
} from "./terminalControlInput";

describe("terminal control input origin", () => {
  it("marks programmatic user input before emitting terminal data", () => {
    const order: string[] = [];
    const terminal = {
      clearSelection: vi.fn(),
      input: vi.fn(() => order.push("input")),
      focus: vi.fn(),
    } as unknown as Terminal;
    const unregister = registerTerminalUserInputMarker(terminal, () => {
      order.push("mark");
    });

    sendTerminalClearInput(terminal, { focus: true });

    expect(order).toEqual(["mark", "input"]);
    expect(terminal.input).toHaveBeenCalledWith("\x0c", true);
    expect(terminal.focus).toHaveBeenCalledOnce();
    unregister();
  });

  it("clears scrollback without sending Ctrl+L or changing the CMD cursor row", async () => {
    const canvas = vi
      .spyOn(HTMLCanvasElement.prototype, "getContext")
      .mockReturnValue({
        measureText: () => ({ width: 10 }),
      } as unknown as CanvasRenderingContext2D);
    const { Terminal: XTerminal } = await import("@xterm/xterm");
    const terminal = new XTerminal({ cols: 20, rows: 3 });
    await new Promise<void>((resolve) =>
      terminal.write(
        "first line\r\nold line\r\nsecond line\r\nprompt> abcdef\x1b[?1h",
        resolve,
      ),
    );
    const input = vi.fn();
    const disposable = terminal.onData(input);

    expect(terminal.buffer.active.baseY).toBeGreaterThan(0);
    clearTerminalAll(terminal);
    await new Promise<void>((resolve) => terminal.write("", resolve));

    expect(terminal.buffer.active.length).toBe(3);
    expect(terminal.buffer.active.baseY).toBe(0);
    expect(terminal.buffer.active.cursorY).toBe(2);
    expect(terminal.buffer.active.cursorX).toBe(14);
    expect(terminal.buffer.active.getLine(0)?.translateToString(true)).toBe("");
    expect(terminal.buffer.active.getLine(1)?.translateToString(true)).toBe("");
    expect(terminal.buffer.active.getLine(2)?.translateToString(true)).toBe("prompt> abcdef");
    expect(input).not.toHaveBeenCalled();
    expect(terminal.modes.applicationCursorKeysMode).toBe(true);

    disposable.dispose();
    terminal.dispose();
    canvas.mockRestore();
  });

  it("requests a redraw for shells that handle Ctrl+L", async () => {
    const { Terminal: XTerminal } = await import("@xterm/xterm");
    const terminal = new XTerminal({ cols: 20, rows: 3 });
    await new Promise<void>((resolve) => terminal.write("prompt> ", resolve));
    const input = vi.fn();
    const disposable = terminal.onData(input);

    clearTerminalAll(terminal, { shellRedraw: true });

    expect(input).toHaveBeenCalledWith("\x0c");
    disposable.dispose();
    terminal.dispose();
  });
});
