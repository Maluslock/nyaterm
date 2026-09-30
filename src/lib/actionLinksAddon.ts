import type {
  IBufferCell,
  IBufferLine,
  IBufferRange,
  IDecoration,
  IDisposable,
  ILink,
  ILinkProvider,
  IMarker,
  ITerminalAddon,
  Terminal,
} from "@xterm/xterm";

/* -------------------------------------------------------------------------- */
/* Types                                                                        */
/* -------------------------------------------------------------------------- */

export type EntityKind =
  | "url"
  | "ip"
  | "hostPort"
  | "archive"
  | "file"
  | "custom";

export type ExecutionTrigger =
  | "plainClick"
  | "ctrlOrMetaClick"
  | "altClick"
  | "menu"
  | "programmatic";

export type ModifierKey = "ctrl" | "meta" | "alt" | "shift";

export interface BufferPosition {
  x: number;
  y: number;
}

export interface ViewportRangeLike {
  start: BufferPosition;
  end: BufferPosition;
}

export interface WindowedLineResult {
  lines: string[];
  startLineIndex: number;
  text: string;
}

export interface MatchInput {
  text: string;
  startLineIndex: number;
  terminal: Terminal;
  viewportLine: number;
}

export interface MatchResult {
  text: string;
  startIndex: number;
  endIndex: number;
  kind?: EntityKind | string;
  value?: string;
  data?: Record<string, string>;
  priority?: number;
}

export interface ActionContext {
  matcherId: string;
  matcherLabel: string;
  kind: EntityKind | string;
  text: string;
  value: string;
  data: Record<string, string>;
  range: IBufferRange;
  terminal: Terminal;
}

export interface ResolvedAction {
  id: string;
  label: string;
  command: string;
  isDefault?: boolean;
  danger?: boolean;
  hidden?: boolean;
}

export interface ActionDefinition {
  id: string;
  label: string;
  isDefault?: boolean;
  danger?: boolean;
  buildCommand: (ctx: ActionContext) => string | null;
  when?: (ctx: ActionContext) => boolean;
}

export interface ActionLink {
  text: string;
  range: IBufferRange;
  ctx: ActionContext;
  actions: ResolvedAction[];
}

/* -------------------------------------------------------------------------- */
/* Matcher Interface                                                             */
/* -------------------------------------------------------------------------- */

export interface ActionMatcher {
  id: string;
  label: string;
  priority?: number;
  match: (input: MatchInput) => MatchResult[];
  getActions: (ctx: ActionContext) => ActionDefinition[];
  getTooltip?: (ctx: ActionContext) => string;
  prefilter?: (input: MatchInput) => boolean;
}

export interface RegexMatcherOptions {
  id: string;
  label: string;
  regex: RegExp;
  priority?: number;
  kind?: EntityKind | string;
  validate?: (text: string, match: RegExpExecArray) => boolean;
  normalize?: (text: string, match: RegExpExecArray) => string;
  mapData?: (text: string, match: RegExpExecArray) => Record<string, string>;
  getActions: (ctx: ActionContext) => ActionDefinition[];
  getTooltip?: (ctx: ActionContext) => string;
  prefilter?: (input: MatchInput) => boolean;
}

/* -------------------------------------------------------------------------- */
/* Plugin options                                                                */
/* -------------------------------------------------------------------------- */

export interface TooltipShowArgs {
  event: MouseEvent;
  text: string;
  range: IBufferRange;
  link: ActionLink;
}

export interface MenuShowArgs {
  event: MouseEvent;
  text: string;
  range: IBufferRange;
  link: ActionLink;
  actions: ResolvedAction[];
  execute: (actionId: string) => void;
}

export interface ExecutionPolicy {
  beforeExecute?: (
    action: ResolvedAction,
    ctx: ActionContext,
    trigger: ExecutionTrigger,
  ) => boolean | Promise<boolean>;
  transformCommand?: (
    command: string,
    ctx: ActionContext,
    trigger: ExecutionTrigger,
  ) => string;
  resolveAliasCommand?: (
    command: string,
    ctx: ActionContext,
    trigger: ExecutionTrigger,
  ) => string;
  onExecutionError?: (
    error: unknown,
    action: ResolvedAction,
    ctx: ActionContext,
    trigger: ExecutionTrigger,
  ) => void;
}

export interface ActionLinksAddonOptions {
  sendInput?: (data: string) => void;
  executeCommand?: (
    command: string,
    action: ResolvedAction,
    ctx: ActionContext,
    trigger: ExecutionTrigger,
  ) => void | Promise<void>;
  showTooltip?: (args: TooltipShowArgs) => void;
  hideTooltip?: () => void;
  showMenu?: (args: MenuShowArgs) => void;
  allowPlainClickExecute?: boolean;
  allowCtrlOrMetaClickExecute?: boolean;
  allowAltClickMenu?: boolean;
  fallbackAltClickToDefaultAction?: boolean;
  maxScanLength?: number;
  policy?: ExecutionPolicy;
}

/* -------------------------------------------------------------------------- */
/* Built-in matcher option types                                                 */
/* -------------------------------------------------------------------------- */

export interface CommonMatcherOptions {
  priority?: number;
  label?: string;
  tooltip?: (ctx: ActionContext) => string;
  defaultAction?: string;
}

export interface IPv4MatcherOptions extends CommonMatcherOptions {
  actions?: Array<"ping" | "traceroute" | "ssh" | "curl-http">;
}

export interface ArchiveMatcherOptions extends CommonMatcherOptions {
  actions?: Array<"extract" | "list">;
}

export interface HostPortMatcherOptions extends CommonMatcherOptions {
  actions?: Array<"curl-http" | "curl-https" | "nc" | "telnet">;
}

/* -------------------------------------------------------------------------- */
/* Helpers                                                                       */
/* -------------------------------------------------------------------------- */

function resolveActions(
  matcher: ActionMatcher,
  ctx: ActionContext,
): ResolvedAction[] {
  const defs = matcher.getActions(ctx);
  return defs
    .filter((d) => !d.when || d.when(ctx))
    .map((d) => {
      const command = d.buildCommand(ctx) ?? "";
      return {
        id: d.id,
        label: d.label,
        command,
        isDefault: d.isDefault,
        danger: d.danger,
        hidden: !command,
      };
    })
    .filter((a) => !a.hidden);
}

function buildStringToCellMap(
  line: IBufferLine,
  stringLength: number,
  maxCols: number,
  scratchCell: IBufferCell,
): number[] {
  const map: number[] = [];
  let col = 0;
  let cellEndCol = 0;

  while (col < maxCols && map.length < stringLength) {
    const cell = line.getCell(col, scratchCell);
    if (!cell) break;

    const chars = cell.getChars();
    const width = cell.getWidth();
    const stride = width || 1;

    if (chars.length === 0) {
      map.push(col);
    } else {
      for (let i = 0; i < chars.length; i++) {
        map.push(col);
      }
    }

    cellEndCol = col + stride;
    col += stride;
  }

  map.push(cellEndCol);
  return map;
}

interface ParsedLine {
  full: string;
  offset: number;
  lineText: string;
  cellMap: number[] | null;
  endY: number;
}

const MAX_LOGICAL_LINE_SCAN_CHARS = 16 * 1024;
const MAX_LOGICAL_LINE_ROWS = 256;

function readLogicalLineAtAbsoluteY(
  terminal: Terminal,
  absY: number,
): ParsedLine | null {
  const buffer = terminal.buffer.active;
  const line = buffer.getLine(absY);
  if (!line) return null;

  let startY = absY;
  while (startY > 0 && buffer.getLine(startY)?.isWrapped) {
    if (absY - startY + 2 > MAX_LOGICAL_LINE_ROWS) return null;
    startY -= 1;
  }

  let endY = absY;
  while (endY + 1 < buffer.length) {
    const next = buffer.getLine(endY + 1);
    if (!next?.isWrapped) break;
    if (endY - startY + 2 > MAX_LOGICAL_LINE_ROWS) return null;
    endY += 1;
  }

  // Rebuild the full logical line from startY up through wrapped continuations
  let full = "";
  let offsetChars = 0;
  let lineText = "";
  let cellMap: number[] | null = null;
  const scratchCell = buffer.getNullCell();

  for (let y = startY; y <= endY; y++) {
    const l = buffer.getLine(y);
    if (!l) break;

    const maxCols = Math.min(l.length, terminal.cols);
    const text = l.translateToString(y === endY, 0, maxCols);
    if (full.length + text.length > MAX_LOGICAL_LINE_SCAN_CHARS) return null;

    if (y < absY) {
      offsetChars += text.length;
    } else if (y === absY) {
      lineText = text;
      cellMap =
        /[^\u0000-\u00FF]/.test(text) && text.length > 0
          ? buildStringToCellMap(l, text.length, maxCols, scratchCell)
          : null;
    }

    full += text;
  }

  return { full, offset: offsetChars, lineText, cellMap, endY };
}

/* -------------------------------------------------------------------------- */
/* Addon                                                                         */
/* -------------------------------------------------------------------------- */

export class ActionLinksAddon implements ITerminalAddon, ILinkProvider {
  private _terminal: Terminal | null = null;
  private _matchers: ActionMatcher[] = [];
  private _options: ActionLinksAddonOptions;
  private _disposables: IDisposable[] = [];
  private _providerDisposable: IDisposable | null = null;
  private _suspended = false;

  // Link computation cache: logical-line text → ActionLink[]
  private _cache = new Map<string, ActionLink[]>();

  // Decoration layer: key → {deco, marker}, line → keys, scanned line set
  private _decoCache = new Map<
    string,
    { deco: IDecoration; marker: IMarker }
  >();
  private _lineToDecoKeys = new Map<number, string[]>();
  private _scannedAbsLines = new Set<number>();
  private _writeDecoTimer: ReturnType<typeof setTimeout> | null = null;
  private _scrollThrottleTimer: ReturnType<typeof setTimeout> | null = null;
  private _scrollThrottlePending = false;
  private _lastViewportY = -1;
  private _sentinelMarker: IMarker | null = null;
  private _sentinelDisposable: IDisposable | null = null;
  private _bufferTrimmed = false;

  private static readonly OVERSCAN_LINES = 120;
  private static readonly DECORATION_DEBOUNCE_MS = 50;
  private static readonly DECORATION_THROTTLE_MS = 80;
  private static readonly MAX_CACHE_ENTRIES = 2000;

  constructor(
    matchers: ActionMatcher[] = [],
    options: ActionLinksAddonOptions = {},
  ) {
    this._matchers = [...matchers].sort(
      (a, b) => (b.priority ?? 0) - (a.priority ?? 0),
    );
    this._options = {
      allowCtrlOrMetaClickExecute: true,
      allowAltClickMenu: true,
      fallbackAltClickToDefaultAction: true,
      ...options,
    };
  }

  activate(terminal: Terminal): void {
    this._terminal = terminal;
    this._providerDisposable = terminal.registerLinkProvider(this);
    this._disposables.push(
      terminal.onWriteParsed(() => {
        this._cache.clear();
        this._scheduleWriteDecoRefresh();
      }),
      terminal.onResize(() => {
        this._clearAllDecorations();
        this._scheduleWriteDecoRefresh();
      }),
      terminal.onRender(() => {
        const currentViewportY = terminal.buffer.active?.viewportY ?? 0;
        if (currentViewportY !== this._lastViewportY) {
          this._lastViewportY = currentViewportY;
          this._scheduleScrollDecoRefresh();
        }
      }),
    );
  }

  dispose(): void {
    this._clearAllDecorations();
    this._providerDisposable?.dispose();
    for (const d of this._disposables) d.dispose();
    this._disposables = [];
    this._cache.clear();
    this._terminal = null;
  }

  registerMatcher(matcher: ActionMatcher): IDisposable {
    this._matchers.push(matcher);
    this._matchers.sort((a, b) => (b.priority ?? 0) - (a.priority ?? 0));
    this._cache.clear();
    this._clearAllDecorations();
    this._scheduleWriteDecoRefresh();
    return { dispose: () => this.unregisterMatcher(matcher.id) };
  }

  unregisterMatcher(id: string): void {
    this._matchers = this._matchers.filter((m) => m.id !== id);
    this._cache.clear();
    this._clearAllDecorations();
    this._scheduleWriteDecoRefresh();
  }

  setMatchers(matchers: ActionMatcher[]): void {
    this._matchers = [...matchers].sort(
      (a, b) => (b.priority ?? 0) - (a.priority ?? 0),
    );
    this._cache.clear();
    this._clearAllDecorations();
    this._scheduleWriteDecoRefresh();
  }

  getMatchers(): readonly ActionMatcher[] {
    return this._matchers;
  }

  setSuspended(suspended: boolean): void {
    if (this._suspended === suspended) return;
    this._suspended = suspended;

    if (suspended) {
      this._options.hideTooltip?.();
      this._clearAllDecorations();
      return;
    }

    this._cache.clear();
    this._scheduleWriteDecoRefresh();
  }

  setOptions(options: Partial<ActionLinksAddonOptions>): void {
    this._options = { ...this._options, ...options };
  }

  getOptions(): Readonly<ActionLinksAddonOptions> {
    return this._options;
  }

  /* ILinkProvider */
  provideLinks(
    bufferLineNumber: number,
    callback: (links: ILink[] | undefined) => void,
  ): void {
    const terminal = this._terminal;
    if (
      !terminal ||
      this._suspended ||
      this._matchers.length === 0 ||
      terminal.buffer.active.type === "alternate"
    ) {
      callback(undefined);
      return;
    }

    const parsed = this._getLogicalLineByBufferLine(terminal, bufferLineNumber);
    if (!parsed) {
      callback(undefined);
      return;
    }
    const actionLinks = this._getCachedActionLinks(
      terminal,
      parsed.full,
      bufferLineNumber,
    );

    const ilinks: ILink[] = [];
    for (const al of actionLinks) {
      const ilink = this._actionLinkToILink(al, parsed, bufferLineNumber);
      if (ilink) ilinks.push(ilink);
    }

    callback(ilinks.length > 0 ? ilinks : undefined);
  }

  /* ── Decoration layer ────────────────────────────────────────────────────── */

  private _clearAllDecoTimers(): void {
    if (this._writeDecoTimer) {
      clearTimeout(this._writeDecoTimer);
      this._writeDecoTimer = null;
    }
    if (this._scrollThrottleTimer) {
      clearTimeout(this._scrollThrottleTimer);
      this._scrollThrottleTimer = null;
    }
    this._scrollThrottlePending = false;
  }

  private _disposeSentinel(): void {
    this._sentinelDisposable?.dispose();
    this._sentinelMarker?.dispose();
    this._sentinelDisposable = null;
    this._sentinelMarker = null;
  }

  private _installSentinel(terminal: Terminal): void {
    this._disposeSentinel();

    const buffer = terminal.buffer.active;
    if (!buffer || buffer.length === 0) return;

    const cursorAbsY = buffer.baseY + buffer.cursorY;
    const marker = terminal.registerMarker(-cursorAbsY);
    if (!marker || marker.line < 0) return;

    this._sentinelMarker = marker;
    this._sentinelDisposable = marker.onDispose(() => {
      this._bufferTrimmed = true;
      this._sentinelMarker = null;
      this._sentinelDisposable = null;
    });
  }

  private _canRefreshDeco(): boolean {
    return !this._suspended && this._matchers.length > 0;
  }

  /** Debounced refresh for write/resize/matcher-change events. */
  private _scheduleWriteDecoRefresh(): void {
    if (!this._canRefreshDeco()) return;
    if (this._writeDecoTimer) clearTimeout(this._writeDecoTimer);
    this._writeDecoTimer = setTimeout(() => {
      this._writeDecoTimer = null;
      const term = this._terminal;
      if (term) this._refreshDecorations(term);
    }, ActionLinksAddon.DECORATION_DEBOUNCE_MS);
  }

  /** Leading+trailing throttle for scroll events. */
  private _scheduleScrollDecoRefresh(): void {
    if (!this._canRefreshDeco()) return;
    if (this._scrollThrottleTimer !== null) {
      this._scrollThrottlePending = true;
      return;
    }
    const term = this._terminal;
    if (term) this._refreshDecorations(term);
    this._scrollThrottleTimer = setTimeout(() => {
      this._scrollThrottleTimer = null;
      if (this._scrollThrottlePending) {
        this._scrollThrottlePending = false;
        this._scheduleScrollDecoRefresh();
      }
    }, ActionLinksAddon.DECORATION_THROTTLE_MS);
  }

  private _refreshDecorations(terminal: Terminal): void {
    if (
      this._suspended ||
      !terminal.buffer?.active ||
      this._matchers.length === 0
    )
      return;
    if (terminal.buffer.active.type === "alternate") {
      this._clearAllDecorations();
      return;
    }

    if (this._bufferTrimmed) {
      this._clearAllDecorations();
    }
    if (!this._sentinelMarker) {
      this._installSentinel(terminal);
    }

    const buffer = terminal.buffer.active;
    const cursorAbsY = buffer.baseY + buffer.cursorY;
    const rows = terminal.rows;
    const viewportY = buffer.viewportY;
    const totalLines = buffer.length;
    const scanStart = Math.max(0, viewportY - ActionLinksAddon.OVERSCAN_LINES);
    const scanEnd = Math.min(
      totalLines - 1,
      viewportY + rows - 1 + ActionLinksAddon.OVERSCAN_LINES,
    );
    const requiredKeys = new Set<string>();

    for (let absLineY = scanStart; absLineY <= scanEnd; absLineY++) {
      const line = buffer.getLine(absLineY);
      if (!line) continue;

      // Reuse memoized result for immutable scrollback lines
      if (this._scannedAbsLines.has(absLineY)) {
        for (const k of this._lineToDecoKeys.get(absLineY) ?? [])
          requiredKeys.add(k);
        continue;
      }

      const parsed = this._getLogicalLineByAbsoluteLine(terminal, absLineY);
      if (!parsed) {
        if (absLineY < buffer.baseY) {
          this._scannedAbsLines.add(absLineY);
        }
        continue;
      }
      const actionLinks = this._getCachedActionLinks(
        terminal,
        parsed.full,
        absLineY + 1,
      );

      const lineKeys: string[] = [];
      for (const al of actionLinks) {
        const span = this._getActionLinkCellSpan(al, parsed);
        if (!span) continue;

        const key = this._ensureDecoration(
          absLineY,
          span.cellStart,
          span.cellWidth,
          cursorAbsY,
        );
        if (key) {
          lineKeys.push(key);
          requiredKeys.add(key);
        }
      }

      // Only memoize fully-scrollback logical lines. A wrapped logical line
      // touching the live screen can still change through shell redraws.
      if (parsed.endY < buffer.baseY) {
        this._scannedAbsLines.add(absLineY);
        if (lineKeys.length > 0) {
          this._lineToDecoKeys.set(absLineY, lineKeys);
        } else {
          this._lineToDecoKeys.delete(absLineY);
        }
      }
    }

    // Evict decorations that scrolled out of the current viewport
    const staleKeys: string[] = [];
    for (const key of this._decoCache.keys()) {
      if (!requiredKeys.has(key)) staleKeys.push(key);
    }
    for (const key of staleKeys) {
      const entry = this._decoCache.get(key);
      if (entry) {
        this._decoCache.delete(key);
        entry.deco.dispose();
        entry.marker.dispose();
      }
    }

    for (const absLineY of this._scannedAbsLines) {
      if (absLineY < scanStart || absLineY > scanEnd) {
        this._scannedAbsLines.delete(absLineY);
        this._lineToDecoKeys.delete(absLineY);
      }
    }
  }

  private _ensureDecoration(
    absLineY: number,
    colStart: number,
    width: number,
    cursorAbsY: number,
  ): string | null {
    if (width <= 0 || !this._terminal) return null;
    const key = `${absLineY}:${colStart}:${width}`;
    if (this._decoCache.has(key)) return key;

    const marker = this._terminal.registerMarker(absLineY - cursorAbsY);
    if (!marker) return null;

    const deco = this._terminal.registerDecoration({
      marker,
      x: colStart,
      width,
      layer: "top",
    });

    if (!deco) {
      marker.dispose();
      return null;
    }

    deco.onRender((el) => {
      el.style.borderBottom = "1px dashed rgba(100, 160, 255, 0.4)";
      el.style.boxSizing = "border-box";
      el.style.pointerEvents = "none";
    });

    deco.onDispose(() => {
      this._decoCache.delete(key);
      const keys = this._lineToDecoKeys.get(absLineY);
      if (keys) {
        const filtered = keys.filter((k) => k !== key);
        if (filtered.length === 0) {
          this._lineToDecoKeys.delete(absLineY);
          this._scannedAbsLines.delete(absLineY);
        } else {
          this._lineToDecoKeys.set(absLineY, filtered);
        }
      }
    });

    this._decoCache.set(key, { deco, marker });
    return key;
  }

  private _clearAllDecorations(): void {
    this._clearAllDecoTimers();
    const entries = [...this._decoCache.values()];
    this._cache.clear();
    this._decoCache.clear();
    this._lineToDecoKeys.clear();
    this._scannedAbsLines.clear();
    this._lastViewportY = -1;
    this._disposeSentinel();
    this._bufferTrimmed = false;
    for (const { deco, marker } of entries) {
      deco.dispose();
      marker.dispose();
    }
  }

  /* ── Link provider internals ─────────────────────────────────────────────── */

  private _getLogicalLineByAbsoluteLine(
    terminal: Terminal,
    absoluteLine: number,
  ): ParsedLine | null {
    return readLogicalLineAtAbsoluteY(terminal, absoluteLine);
  }

  private _getLogicalLineByBufferLine(
    terminal: Terminal,
    bufferLineNumber: number,
  ): ParsedLine | null {
    if (bufferLineNumber <= 0) return null;
    return this._getLogicalLineByAbsoluteLine(terminal, bufferLineNumber - 1);
  }

  private _getLogicalLineByViewportLine(
    terminal: Terminal,
    viewportLine: number,
  ): ParsedLine | null {
    const absoluteLine = terminal.buffer.active.viewportY + viewportLine;
    return this._getLogicalLineByAbsoluteLine(terminal, absoluteLine);
  }

  private _getCachedActionLinks(
    terminal: Terminal,
    logicalText: string,
    bufferLineNumber: number,
  ): ActionLink[] {
    const cached = this._cache.get(logicalText);
    if (cached) return cached;

    const actionLinks = this._computeActionLinks(
      terminal,
      logicalText,
      bufferLineNumber,
    );
    this._cache.set(logicalText, actionLinks);

    if (this._cache.size > ActionLinksAddon.MAX_CACHE_ENTRIES) {
      const oldestKey = this._cache.keys().next().value;
      if (oldestKey !== undefined) {
        this._cache.delete(oldestKey);
      }
    }

    return actionLinks;
  }

  private _computeActionLinks(
    terminal: Terminal,
    logicalText: string,
    bufferLineNumber: number,
  ): ActionLink[] {
    const viewportLine =
      bufferLineNumber - terminal.buffer.active.viewportY - 1;
    const input: MatchInput = {
      text: logicalText,
      startLineIndex: 0,
      terminal,
      viewportLine,
    };

    const results: ActionLink[] = [];
    const covered: Array<[number, number]> = [];

    const sorted = [...this._matchers].sort(
      (a, b) => (b.priority ?? 0) - (a.priority ?? 0),
    );

    for (const matcher of sorted) {
      if (matcher.prefilter && !matcher.prefilter(input)) continue;

      const matches = matcher.match(input);
      for (const m of matches) {
        const overlaps = covered.some(
          ([s, e]) => m.startIndex < e && m.endIndex > s,
        );
        if (overlaps) continue;
        covered.push([m.startIndex, m.endIndex]);

        const placeholderRange: IBufferRange = {
          start: { x: 1, y: Math.max(1, bufferLineNumber) },
          end: { x: 1, y: Math.max(1, bufferLineNumber) },
        };

        const ctx: ActionContext = {
          matcherId: matcher.id,
          matcherLabel: matcher.label,
          kind: m.kind ?? "custom",
          text: m.text,
          value: m.value ?? m.text,
          data: m.data ?? {},
          range: placeholderRange,
          terminal,
        };

        const actions = resolveActions(matcher, ctx);
        if (actions.length === 0) continue;

        results.push({
          text: m.text,
          range: placeholderRange,
          ctx: {
            ...ctx,
            data: {
              ...ctx.data,
              _startIndex: String(m.startIndex),
              _endIndex: String(m.endIndex),
            },
          },
          actions,
        });
      }
    }

    return results;
  }

  private _getActionLinkCellSpan(
    al: ActionLink,
    parsed: ParsedLine,
  ): { cellStart: number; cellWidth: number } | null {
    const startIndex = Number(al.ctx.data._startIndex ?? "0");
    const endIndex = Number(al.ctx.data._endIndex ?? "0");
    const lineStart = parsed.offset;
    const lineEnd = parsed.offset + parsed.lineText.length;

    if (endIndex <= lineStart || startIndex >= lineEnd) return null;

    const segmentStart = Math.max(startIndex - parsed.offset, 0);
    const segmentEnd = Math.min(
      endIndex - parsed.offset,
      parsed.lineText.length,
    );
    if (segmentEnd <= segmentStart) return null;

    const cellStart = parsed.cellMap
      ? (parsed.cellMap[segmentStart] ?? segmentStart)
      : segmentStart;
    const cellEnd = parsed.cellMap
      ? (parsed.cellMap[segmentEnd] ?? segmentEnd)
      : segmentEnd;
    const cellWidth = cellEnd - cellStart;
    if (cellWidth <= 0) return null;

    return { cellStart, cellWidth };
  }

  private _actionLinkToILink(
    al: ActionLink,
    parsed: ParsedLine,
    bufferLineNumber: number,
  ): ILink | null {
    const span = this._getActionLinkCellSpan(al, parsed);
    if (!span) return null;

    const range: IBufferRange = {
      start: { x: span.cellStart + 1, y: bufferLineNumber },
      end: { x: span.cellStart + span.cellWidth, y: bufferLineNumber },
    };

    const ctxWithRange: ActionContext = { ...al.ctx, range };
    const alWithRange: ActionLink = { ...al, range, ctx: ctxWithRange };

    return {
      range,
      text: al.text,
      decorations: { pointerCursor: true, underline: false },
      activate: (event: MouseEvent) => {
        this._handleActivate(event, alWithRange);
      },
      hover: (event: MouseEvent) => {
        this._handleHover(event, alWithRange);
      },
      leave: () => {
        this._options.hideTooltip?.();
      },
    };
  }

  private _handleHover(event: MouseEvent, al: ActionLink): void {
    const { showTooltip } = this._options;
    if (!showTooltip) return;
    showTooltip({ event, text: al.text, range: al.range, link: al });
  }

  private async _handleActivate(
    event: MouseEvent,
    al: ActionLink,
  ): Promise<void> {
    const {
      allowCtrlOrMetaClickExecute,
      allowAltClickMenu,
      allowPlainClickExecute,
      fallbackAltClickToDefaultAction,
      showMenu,
    } = this._options;

    const isCtrlMeta = event.ctrlKey || event.metaKey;
    const isAlt = event.altKey;

    if (isAlt && allowAltClickMenu) {
      if (showMenu) {
        showMenu({
          event,
          text: al.text,
          range: al.range,
          link: al,
          actions: al.actions,
          execute: (actionId) => this._executeById(al, actionId, "menu"),
        });
        return;
      }
      if (fallbackAltClickToDefaultAction) {
        await this._executeDefault(al, "altClick");
      }
      return;
    }

    if (isCtrlMeta && allowCtrlOrMetaClickExecute) {
      await this._executeDefault(al, "ctrlOrMetaClick");
      return;
    }

    if (!isCtrlMeta && !isAlt && allowPlainClickExecute) {
      await this._executeDefault(al, "plainClick");
    }
  }

  private async _executeDefault(
    al: ActionLink,
    trigger: ExecutionTrigger,
  ): Promise<void> {
    const defaultAction = al.actions.find((a) => a.isDefault) ?? al.actions[0];
    if (defaultAction) {
      await this._executeById(al, defaultAction.id, trigger);
    }
  }

  private async _executeById(
    al: ActionLink,
    actionId: string,
    trigger: ExecutionTrigger,
  ): Promise<void> {
    const action = al.actions.find((a) => a.id === actionId);
    if (!action || !action.command) return;

    const { policy, sendInput, executeCommand } = this._options;

    try {
      if (policy?.beforeExecute) {
        const ok = await policy.beforeExecute(action, al.ctx, trigger);
        if (!ok) return;
      }

      let cmd = action.command;
      if (policy?.resolveAliasCommand)
        cmd = policy.resolveAliasCommand(cmd, al.ctx, trigger);
      if (policy?.transformCommand)
        cmd = policy.transformCommand(cmd, al.ctx, trigger);

      if (executeCommand) {
        await executeCommand(cmd, action, al.ctx, trigger);
      } else if (sendInput) {
        sendInput(`${cmd}\r`);
      }
    } catch (err) {
      policy?.onExecutionError?.(err, action, al.ctx, trigger);
    }
  }

  async executeAction(
    link: ActionLink,
    actionId?: string,
    trigger: ExecutionTrigger = "programmatic",
  ): Promise<void> {
    if (actionId) {
      await this._executeById(link, actionId, trigger);
    } else {
      await this._executeDefault(link, trigger);
    }
  }

  computeLinksForLine(viewportLine: number): ActionLink[] {
    const terminal = this._terminal;
    if (!terminal) return [];
    const parsed = this._getLogicalLineByViewportLine(terminal, viewportLine);
    if (!parsed) return [];
    return this._getCachedActionLinks(
      terminal,
      parsed.full,
      terminal.buffer.active.viewportY + viewportLine + 1,
    );
  }
}
