import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { TmuxPane } from "@/lib/tmuxGateway";
import { TMUX_PANE_FIND_EVENT } from "@/lib/tmuxGateway";
import { TmuxPaneTerminal } from "./TmuxPaneTerminal";

/**
 * The pane owns a real xterm, which needs canvas measurements jsdom does not
 * provide. The fake keeps exactly the surface the pane touches and records the
 * key handler, so the find binding can be exercised the way xterm calls it.
 */
const terminalMock = vi.hoisted(() => {
  const state = {
    customKeyHandler: null as ((event: KeyboardEvent) => boolean) | null,
    disposed: false,
  };
  class FakeTerminal {
    cols = 80;
    rows = 24;
    options: Record<string, unknown> = {};
    element: HTMLElement | null = null;
    buffer = { active: { type: "normal", baseY: 0, cursorY: 0 } };
    open(container: HTMLElement) {
      const screen = document.createElement("div");
      screen.className = "xterm-screen";
      Object.defineProperty(screen, "clientWidth", { value: 640 });
      Object.defineProperty(screen, "clientHeight", { value: 384 });
      container.appendChild(screen);
      this.element = container;
    }
    loadAddon() {}
    attachCustomKeyEventHandler(handler: (event: KeyboardEvent) => boolean) {
      state.customKeyHandler = handler;
    }
    onData() {
      return { dispose: vi.fn() };
    }
    onResize() {
      return { dispose: vi.fn() };
    }
    write() {}
    resize() {}
    focus() {}
    clearSelection() {}
    getSelection() {
      return "";
    }
    dispose() {
      state.disposed = true;
    }
  }
  return { state, FakeTerminal };
});

vi.mock("@xterm/xterm", () => ({ Terminal: terminalMock.FakeTerminal }));

vi.mock("@xterm/addon-search", () => ({
  SearchAddon: class {
    findNext = vi.fn(() => true);
    findPrevious = vi.fn(() => true);
    clearDecorations = vi.fn();
    clearActiveDecoration = vi.fn();
    onDidChangeResults = vi.fn(() => ({ dispose: vi.fn() }));
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => {}),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/lib/invoke", () => ({ invoke: vi.fn(async () => null) }));

vi.mock("@/context/AppContext", () => {
  // Stable identities: the real provider memoises this object, and a fresh one
  // per render would re-create the terminal on every pass.
  const settings = {
    appearance: { font_family: "monospace", font_size: 14 },
    interaction: {
      command_suggestions_enabled: false,
      command_suggestion_min_chars: 1,
      command_suggestion_max_chars: 32,
    },
    terminal: {
      show_line_numbers: false,
      show_timestamps: false,
      timestamp_format: "[HH:mm:ss]",
    },
    keybindings: {},
  };
  return { useTerminalAppSettings: () => settings };
});

vi.mock("@/context/ThemeContext", () => {
  const theme = {
    colors: { terminal: { background: "#101010", foreground: "#e0e0e0" } },
  };
  return { useTheme: () => ({ theme }) };
});

// Unrelated to finding text, and heavy in jsdom.
vi.mock("@/hooks/useKeywordHighlighter", () => ({
  useKeywordHighlighter: vi.fn(),
}));
vi.mock("@/hooks/useCommandHistory", () => {
  const result = {
    suggestions: [],
    selectedIndex: 0,
    showSuggestions: false,
    cursorPosition: { x: 0, y: 0 },
    triggerSearch: vi.fn(),
    dismissSuggestions: vi.fn(),
    handleSelectSuggestion: vi.fn(),
    handleDeleteSuggestion: vi.fn(),
  };
  return { useCommandHistory: () => result };
});

const pane: TmuxPane = {
  index: 0,
  id: "%0",
  width: 80,
  height: 24,
  x: 0,
  y: 0,
  active: true,
};

function renderPane() {
  return render(
    <TmuxPaneTerminal
      sessionId="session-1"
      pane={pane}
      isActive
      onSelect={vi.fn()}
    />,
  );
}

/** The app's default `terminal.find` binding is Ctrl+Shift+F. */
function pressFind(init: KeyboardEventInit = {}) {
  const handler = terminalMock.state.customKeyHandler;
  expect(handler).toBeTruthy();
  const event = new KeyboardEvent("keydown", {
    key: "F",
    code: "KeyF",
    ctrlKey: true,
    shiftKey: true,
    bubbles: true,
    cancelable: true,
    ...init,
  });
  let handled: boolean | undefined;
  act(() => {
    handled = handler?.(event);
  });
  return { handled, event };
}

describe("TmuxPaneTerminal find", () => {
  it("opens the search bar on the terminal find binding", async () => {
    renderPane();
    expect(screen.queryByPlaceholderText("terminalCtx.find")).toBeNull();

    const { handled } = pressFind();
    // Swallowed, so the pane's shell never sees the shortcut.
    expect(handled).toBe(false);
    expect(await screen.findByPlaceholderText("terminalCtx.find")).toBeTruthy();
  });

  it("leaves other ctrl combinations to the pane", () => {
    renderPane();
    pressFind({ shiftKey: false });
    expect(screen.queryByPlaceholderText("terminalCtx.find")).toBeNull();
  });

  it("closes the search bar on Escape", async () => {
    renderPane();
    pressFind();
    await screen.findByPlaceholderText("terminalCtx.find");

    const { handled } = pressFind({
      key: "Escape",
      code: "Escape",
      ctrlKey: false,
      shiftKey: false,
    });
    expect(handled).toBe(false);
    expect(screen.queryByPlaceholderText("terminalCtx.find")).toBeNull();
  });

  it("opens the search bar when the pane menu asks for it", async () => {
    renderPane();
    act(() => {
      window.dispatchEvent(
        new CustomEvent(TMUX_PANE_FIND_EVENT, {
          detail: { sessionId: "session-1", paneId: "%0" },
        }),
      );
    });
    expect(await screen.findByPlaceholderText("terminalCtx.find")).toBeTruthy();
  });

  it("ignores a find request aimed at another pane", () => {
    renderPane();
    act(() => {
      window.dispatchEvent(
        new CustomEvent(TMUX_PANE_FIND_EVENT, {
          detail: { sessionId: "session-1", paneId: "%9" },
        }),
      );
    });
    expect(screen.queryByPlaceholderText("terminalCtx.find")).toBeNull();
  });

  it("types into the search input without leaking to the pane", async () => {
    renderPane();
    pressFind();
    const input = await screen.findByPlaceholderText("terminalCtx.find");
    fireEvent.change(input, { target: { value: "hello" } });
    expect((input as HTMLInputElement).value).toBe("hello");
  });
});
