import type { IMarker } from "@xterm/xterm";
import type { SessionType } from "@/types/global";

export function startsObviousInteractiveSession(command: string): boolean {
  return /^(?:[^\s/\\]+[/\\])?(?:python(?:3(?:\.\d+)?)?|node|irb|psql|sqlite3|mysql|cmd|powershell|pwsh|bash|zsh|fish|sh|wsl)(?:\.exe)?(?:\s+[-/]i|\s+[/]k)?\s*$/iu.test(
    command.trim(),
  );
}

export function nextFallbackInteractiveState(
  interactive: boolean,
  data: string,
  command: string,
): boolean {
  if (data === "\x04") return false;
  if (data !== "\r" || !command) return interactive;
  if (startsObviousInteractiveSession(command)) return true;
  if (/^(?:(?:exit|quit)(?:\(\))?|\.exit|\.quit|\\q)$/iu.test(command.trim())) {
    return false;
  }
  return interactive;
}

export function shouldRecordFallbackCommand(options: {
  command: string;
  sessionType: SessionType;
  shellIntegrationEnabled: boolean;
  bufferType: string;
  disconnected: boolean;
  aiCapturing: boolean;
  credentialPrompt: boolean;
  interactive: boolean;
}): boolean {
  return (
    Boolean(options.command) &&
    (options.sessionType === "Local" || options.sessionType === "SSH") &&
    !options.shellIntegrationEnabled &&
    options.bufferType === "normal" &&
    !options.disconnected &&
    !options.aiCapturing &&
    !options.credentialPrompt &&
    !options.interactive
  );
}

export class CommandNavigation {
  private readonly markers = new Set<IMarker>();
  private lastAdded: IMarker | null = null;
  private pendingPrompt: IMarker | null = null;
  private position: IMarker | null = null;
  private selectionStart: IMarker | null = null;
  private selectionEnd: IMarker | null = null;

  add(marker: IMarker): void {
    if (marker.isDisposed) return;
    if (
      this.lastAdded &&
      !this.lastAdded.isDisposed &&
      this.lastAdded.line === marker.line
    ) {
      marker.dispose();
      return;
    }
    this.markers.add(marker);
    this.lastAdded = marker;
    marker.onDispose(() => {
      this.markers.delete(marker);
      if (this.lastAdded === marker) this.lastAdded = null;
      if (this.position === marker) this.position = null;
      if (this.selectionStart === marker || this.selectionEnd === marker) {
        this.resetSelection();
      }
    });
  }

  promptStart(marker: IMarker | null): void {
    if (this.pendingPrompt && !this.pendingPrompt.isDisposed)
      this.pendingPrompt.dispose();
    this.pendingPrompt = marker;
  }

  commandStart(): void {
    if (this.pendingPrompt && !this.pendingPrompt.isDisposed)
      this.add(this.pendingPrompt);
    this.pendingPrompt = null;
  }

  clear(): void {
    if (this.pendingPrompt && !this.pendingPrompt.isDisposed)
      this.pendingPrompt.dispose();
    this.pendingPrompt = null;
    const markers = [...this.markers];
    this.markers.clear();
    this.lastAdded = null;
    this.reset();
    for (const marker of markers) {
      if (!marker.isDisposed) marker.dispose();
    }
  }

  reset(): void {
    this.position = null;
    this.resetSelection();
  }

  resetPosition(): void {
    this.position = null;
  }

  resetSelection(): void {
    this.selectionStart = null;
    this.selectionEnd = null;
  }

  private ordered(): IMarker[] {
    return [...this.markers]
      .filter((marker) => !marker.isDisposed)
      .sort((a, b) => a.line - b.line);
  }

  navigate(direction: -1 | 1, cursorLine: number): number | null {
    const current = this.position?.line ?? cursorLine;
    const markers = this.ordered();
    if (direction < 0) {
      const target = markers.filter((marker) => marker.line < current).pop();
      if (!target) return null;
      this.position = target;
      this.resetSelection();
      return target.line;
    }
    const target = markers.find((marker) => marker.line > current);
    if (target && target.line < cursorLine) {
      this.position = target;
      this.resetSelection();
      return target.line;
    }
    if (current < cursorLine) {
      this.position = null;
      this.resetSelection();
      return cursorLine;
    }
    return null;
  }

  select(cursorLine: number): { start: number; end: number } | null {
    const markers = this.ordered();
    const start =
      this.selectionStart ??
      markers
        .filter((marker) => marker.line <= (this.position?.line ?? cursorLine))
        .pop();
    if (!start) return null;
    const included = this.selectionEnd ?? start;
    const next = markers.find((marker) => marker.line > included.line);
    if (this.selectionEnd && !next) return null;
    const last = this.selectionEnd ? next! : start;
    const following = markers.find((marker) => marker.line > last.line);
    this.selectionStart = start;
    this.selectionEnd = last;
    return {
      start: start.line,
      end: Math.max(start.line, following ? following.line - 1 : cursorLine),
    };
  }

  get size(): number {
    return this.markers.size;
  }
}
