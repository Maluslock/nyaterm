import type { Terminal } from "@xterm/xterm";
import { describe, expect, it, vi } from "vitest";
import { ActionLinksAddon, type ActionMatcher } from "./actionLinksAddon";

function createTerminal(rowCount: number, rowText: string) {
  const translate = vi.fn(() => rowText);
  const terminal = {
    cols: rowText.length,
    buffer: {
      active: {
        type: "normal",
        viewportY: 0,
        length: rowCount,
        getLine: (y: number) =>
          y >= 0 && y < rowCount
            ? {
                isWrapped: y > 0,
                length: rowText.length,
                translateToString: translate,
              }
            : undefined,
        getNullCell: () => ({}),
      },
    },
    registerLinkProvider: () => ({ dispose: () => {} }),
    onWriteParsed: () => ({ dispose: () => {} }),
    onResize: () => ({ dispose: () => {} }),
    onRender: () => ({ dispose: () => {} }),
  } as unknown as Terminal;
  return { terminal, translate };
}

describe("ActionLinksAddon logical line budget", () => {
  const matcher: ActionMatcher = {
    id: "test",
    label: "Test",
    match: vi.fn(() => []),
    getActions: () => [],
  };

  it("stops before translating an excessively wrapped line", () => {
    const { terminal, translate } = createTerminal(10_000, "x".repeat(80));
    const addon = new ActionLinksAddon([matcher]);
    addon.activate(terminal);
    expect(addon.computeLinksForLine(5_000)).toEqual([]);
    expect(translate).not.toHaveBeenCalled();
    addon.dispose();
  });

  it("stops assembling after the character limit", () => {
    const { terminal, translate } = createTerminal(200, "x".repeat(1_000));
    const addon = new ActionLinksAddon([matcher]);
    addon.activate(terminal);
    expect(addon.computeLinksForLine(0)).toEqual([]);
    expect(translate.mock.calls.length).toBeLessThan(20);
    addon.dispose();
  });

  it("passes the complete text of a normal wrapped line to matchers", () => {
    const { terminal } = createTerminal(3, "abc");
    const match = vi.fn(() => []);
    const addon = new ActionLinksAddon([{ ...matcher, match }]);
    addon.activate(terminal);
    expect(addon.computeLinksForLine(1)).toEqual([]);
    expect(match).toHaveBeenCalledWith(
      expect.objectContaining({ text: "abcabcabc" }),
    );
    addon.dispose();
  });
});
