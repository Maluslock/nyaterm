import { describe, expect, it } from "vitest";
import { TerminalOutputPressureTracker } from "./terminalOutputPressure";

describe("TerminalOutputPressureTracker", () => {
  it("detects a burst before backlog accumulates and recovers after quiet", () => {
    const tracker = new TerminalOutputPressureTracker();
    tracker.noteOutput("a".repeat(20_000), 20_000, 0);
    tracker.noteOutput("b".repeat(24_000), 24_000, 20);
    tracker.noteOutput("c".repeat(30_000), 30_000, 40);

    expect(tracker.getReasons(0, 40)).toContain("burst");
    expect(tracker.isStrained(0, 200)).toBe(true);
    expect(tracker.isStrained(0, 440)).toBe(false);
  });

  it("tracks unbroken lines across chunks and resets on CR or LF", () => {
    const tracker = new TerminalOutputPressureTracker();
    tracker.noteOutput("x".repeat(40_000), 40_000, 0);
    tracker.noteOutput("y".repeat(30_000), 30_000, 200);
    expect(tracker.getReasons(0, 200)).toContain("long-line");

    tracker.noteOutput("\rshort\nnext", 11, 700);
    expect(tracker.getReasons(0, 700)).toContain("long-line");
    expect(tracker.isStrained(0, 700)).toBe(true);
    expect(tracker.getReasons(0, 1100)).not.toContain("long-line");
    expect(tracker.isStrained(0, 1100)).toBe(false);
  });

  it("detects a long line that ends within one chunk", () => {
    const tracker = new TerminalOutputPressureTracker();
    tracker.noteOutput(`${"x".repeat(70_000)}\n`, 70_001, 0);
    expect(tracker.isStrained(0, 0)).toBe(true);
    expect(tracker.isStrained(0, 400)).toBe(false);
  });

  it("keeps backlog pressure until the queue drains", () => {
    const tracker = new TerminalOutputPressureTracker();
    expect(tracker.isStrained(128 * 1024, 0)).toBe(true);
    expect(tracker.isStrained(128 * 1024, 1000)).toBe(true);
    expect(tracker.isStrained(0, 1200)).toBe(true);
    expect(tracker.isStrained(0, 1400)).toBe(false);
  });
});
