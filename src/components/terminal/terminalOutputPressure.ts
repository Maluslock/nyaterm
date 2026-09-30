import { XTERM_PERFORMANCE_CONFIG } from "@/lib/xtermPerformance";

export type TerminalOutputPressureReason = "backlog" | "burst" | "long-line";

const config = XTERM_PERFORMANCE_CONFIG.output;

export class TerminalOutputPressureTracker {
  private bursts: Array<{ at: number; bytes: number }> = [];
  private burstHead = 0;
  private burstBytes = 0;
  private unbrokenChars = 0;
  private lastPressureAt = -Infinity;
  private lastLongLineAt = -Infinity;

  noteOutput(data: string, bytes: number, now = Date.now()): void {
    this.expireBursts(now);
    this.bursts.push({ at: now, bytes });
    this.burstBytes += bytes;

    let runStart = 0;
    const breaks = /[\n\r]/g;
    let match = breaks.exec(data);
    let longLineSeen = false;
    while (match) {
      if (
        this.unbrokenChars + match.index - runStart >=
        config.pressure.longLineChars
      ) {
        longLineSeen = true;
      }
      this.unbrokenChars = 0;
      runStart = match.index + 1;
      match = breaks.exec(data);
    }
    this.unbrokenChars += data.length - runStart;

    if (this.unbrokenChars >= config.pressure.longLineChars || longLineSeen) {
      this.lastLongLineAt = now;
      this.lastPressureAt = now;
    }
    if (this.burstBytes >= config.pressure.burstBytes) {
      this.lastPressureAt = now;
    }
  }

  getReasons(
    backlogBytes: number,
    now = Date.now(),
  ): TerminalOutputPressureReason[] {
    this.expireBursts(now);
    const reasons: TerminalOutputPressureReason[] = [];
    if (backlogBytes >= config.strainedBacklogBytes) {
      reasons.push("backlog");
      this.lastPressureAt = now;
    }
    if (this.burstBytes >= config.pressure.burstBytes) reasons.push("burst");
    if (now - this.lastLongLineAt < config.pressure.quietMs) {
      reasons.push("long-line");
    }
    return reasons;
  }

  isStrained(backlogBytes: number, now = Date.now()): boolean {
    if (this.getReasons(backlogBytes, now).length > 0) {
      return true;
    }
    return now - this.lastPressureAt < config.pressure.quietMs;
  }

  getRecoveryDelayMs(now = Date.now()): number {
    return Math.max(0, this.lastPressureAt + config.pressure.quietMs - now);
  }

  private expireBursts(now: number): void {
    while (
      this.burstHead < this.bursts.length &&
      now - this.bursts[this.burstHead].at >= config.pressure.burstWindowMs
    ) {
      this.burstBytes -= this.bursts[this.burstHead].bytes;
      this.burstHead += 1;
    }
    if (this.burstHead > 1024 && this.burstHead * 2 >= this.bursts.length) {
      this.bursts = this.bursts.slice(this.burstHead);
      this.burstHead = 0;
    }
  }
}
