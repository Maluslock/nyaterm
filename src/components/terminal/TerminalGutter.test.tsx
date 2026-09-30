import { act, render } from "@testing-library/react";
import type { Terminal } from "@xterm/xterm";
import { afterEach, expect, it, vi } from "vitest";
import TerminalGutter from "./TerminalGutter";

afterEach(() => {
  vi.unstubAllGlobals();
});

it("keeps the line number gutter wide after scrolling back across a digit boundary", () => {
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  });
  vi.stubGlobal("cancelAnimationFrame", vi.fn());

  const element = document.createElement("div");
  element.innerHTML = '<div class="xterm-viewport"></div><div class="xterm-screen"></div>';
  const buffer = {
    baseY: 100,
    cursorY: 0,
    viewportY: 98,
    type: "normal",
    getLine: () => ({ isWrapped: false }),
  };
  let onScroll: (viewportY: number) => void = () => {};
  const disposable = { dispose: vi.fn() };
  const terminal = {
    element,
    buffer: { active: buffer },
    rows: 1,
    options: { fontSize: 12, fontFamily: "monospace" },
    _core: { _renderService: { dimensions: { css: { cell: { height: 18, width: 10 } } } } },
    onRender: () => disposable,
    onWriteParsed: () => disposable,
    onResize: () => disposable,
    onScroll: (listener: (viewportY: number) => void) => {
      onScroll = listener;
      return disposable;
    },
  } as unknown as Terminal;

  const { container } = render(
    <TerminalGutter
      terminalRef={{ current: terminal }}
      showLineNumbers
      showTimestamps={false}
      timestampFormat="[HH:mm:ss]"
      lineTimestamps={new Map()}
      getLineOffset={() => 0}
      sessionId="session-1"
    />,
  );
  const gutter = container.firstElementChild as HTMLElement;
  expect(gutter.style.width).toBe("32px");

  act(() => {
    buffer.viewportY = 99;
    onScroll(99);
  });
  expect(gutter.style.width).toBe("40px");

  act(() => {
    buffer.viewportY = 98;
    onScroll(98);
  });
  expect(gutter.style.width).toBe("40px");
});
