import {
  createContext,
  memo,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { ArrowUpRight, Check, ChevronDown, ChevronRight, Copy, Play } from "lucide-react";
import { DiagramBlock } from "@/components/diagram-block";
import { InlineMathSpan, MathBlockView } from "@/components/math-block";
import { SimpleTooltip } from "@/components/ui/overlays";
import { resolveTokenColor } from "@/lib/code-highlight";
import { useI18n } from "@/lib/i18n";
import {
  CODE_BLOCK_COLLAPSE_THRESHOLD,
  type GroupedRenderNode,
  type ListItemBlock,
  type NestedListGroupNode,
  buildNestedListForest,
  classifyRunnableShellSnippet,
  groupMarkdownBlocks,
} from "@/lib/markdown-helpers";
import { ScrollArea } from "@/components/ui/scroll-area";
import { orbitActions } from "@/state/orbit-store";
import {
  type InlineSpanView,
  type RichBlockView,
  type TableAlignmentView,
  type TableCellView,
  type TableRowView,
  detectDiagramKind,
} from "@/lib/orbit-ipc";
import { openOrbitLink, parseOrbitLinkTarget, prefetchOrbitLink } from "@/lib/file-links";
import {
  cancelHoverPrefetch,
  scheduleHoverPrefetch,
  shouldPrefetchOnPointerDown,
} from "@/lib/file-prefetch";
import {
  type ResolvedSubagentItem,
  type SubagentNameMatcher,
  buildSubagentNameMatcher,
} from "@/lib/subagents";
import { usePacedRichBlocks } from "@/lib/text-reveal";
import { cn } from "@/lib/utils";

const MarkdownThemeContext = createContext<boolean>(false);

export interface MarkdownLinkContextValue {
  hostId?: string;
  cwd?: string;
  baseFilePath?: string;
  onAnchor?: (fragment: string) => void;
  subagents?: readonly ResolvedSubagentItem[];
  subagentMatcher?: SubagentNameMatcher | null;
  onOpenSubagent?: (subagent: ResolvedSubagentItem) => void;
}

const MarkdownLinkContext = createContext<MarkdownLinkContextValue>({});

export function MarkdownThemeProvider({
  isDark,
  children,
}: {
  isDark: boolean;
  children: ReactNode;
}) {
  return <MarkdownThemeContext.Provider value={isDark}>{children}</MarkdownThemeContext.Provider>;
}

export function MarkdownLinkProvider({
  hostId,
  cwd,
  baseFilePath,
  onAnchor,
  subagents,
  subagentMatcher: precompiledMatcher,
  onOpenSubagent,
  children,
}: MarkdownLinkContextValue & { children: ReactNode }) {
  const resolvedMatcher = useMemo(
    () => {
      if (!onOpenSubagent) return null;
      if (precompiledMatcher !== undefined) return precompiledMatcher;
      return buildSubagentNameMatcher(subagents);
    },
    [precompiledMatcher, subagents, onOpenSubagent],
  );
  const value = useMemo<MarkdownLinkContextValue>(
    () => ({
      hostId,
      cwd,
      baseFilePath,
      onAnchor,
      subagents,
      subagentMatcher: resolvedMatcher,
      onOpenSubagent,
    }),
    [hostId, cwd, baseFilePath, onAnchor, subagents, resolvedMatcher, onOpenSubagent],
  );
  return <MarkdownLinkContext.Provider value={value}>{children}</MarkdownLinkContext.Provider>;
}

const EMPTY_PACING_BLOCKS: RichBlockView[] = [];

function spansAreEqual(a: InlineSpanView[], b: InlineSpanView[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    const sa = a[i];
    const sb = b[i];
    if (
      sa.text !== sb.text ||
      sa.bold !== sb.bold ||
      sa.italic !== sb.italic ||
      sa.inline_code !== sb.inline_code ||
      sa.strikethrough !== sb.strikethrough ||
      sa.link_url !== sb.link_url ||
      (sa.math_ml ?? null) !== (sb.math_ml ?? null)
    ) {
      return false;
    }
  }
  return true;
}

function tableCellsAreEqual(a: TableCellView[], b: TableCellView[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    if (!spansAreEqual(a[i].spans, b[i].spans)) return false;
  }
  return true;
}

function tableRowsAreEqual(a: TableRowView[], b: TableRowView[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    if (!tableCellsAreEqual(a[i].cells, b[i].cells)) return false;
  }
  return true;
}

function tableAlignmentsAreEqual(a: TableAlignmentView[], b: TableAlignmentView[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    if (a[i] !== b[i]) return false;
  }
  return true;
}

function richBlockIsEqual(a: RichBlockView, b: RichBlockView): boolean {
  if (a === b) return true;
  if (a.kind !== b.kind) return false;
  switch (a.kind) {
    case "heading":
      return (
        b.kind === "heading" && a.level === b.level && spansAreEqual(a.spans, b.spans)
      );
    case "paragraph":
      return b.kind === "paragraph" && spansAreEqual(a.spans, b.spans);
    case "block_quote":
      return b.kind === "block_quote" && spansAreEqual(a.spans, b.spans);
    case "list_item":
      return (
        b.kind === "list_item" &&
        a.ordered === b.ordered &&
        a.index === b.index &&
        a.checked === b.checked &&
        (a.depth ?? 0) === (b.depth ?? 0) &&
        spansAreEqual(a.spans, b.spans)
      );
    case "code_block":
      return (
        b.kind === "code_block" &&
        a.language === b.language &&
        a.raw_code === b.raw_code &&
        a.lines.length === b.lines.length
      );
    case "table":
      return (
        b.kind === "table" &&
        tableAlignmentsAreEqual(a.alignments, b.alignments) &&
        tableCellsAreEqual(a.headers, b.headers) &&
        tableRowsAreEqual(a.rows, b.rows)
      );
    case "math_block":
      return b.kind === "math_block" && a.tex === b.tex && a.math_ml === b.math_ml;
    case "horizontal_rule":
      return b.kind === "horizontal_rule";
  }
}

const InlineSpans = memo(
  function InlineSpans({ spans }: { spans: InlineSpanView[] }) {
    const linkCtx = useContext(MarkdownLinkContext);
    return (
      <>
        {spans.map((span, idx) => {
          let className = "";
          if (span.bold) className += " font-semibold text-foreground";
          if (span.italic) className += " italic";
          if (span.strikethrough) className += " line-through opacity-65";

          if (span.link_url) {
            const href = span.link_url;
            const parsed = parseOrbitLinkTarget(href);
            const linkTitle =
              parsed?.kind === "file"
                ? `Open ${parsed.rawPath} in Side Panel`
                : parsed?.kind === "browser"
                  ? `Open ${parsed.url} in Browser Tab`
                  : href;
            return (
              <a
                key={idx}
                href={href}
                title={linkTitle}
                onMouseEnter={() => {
                  if (parsed?.kind === "file") {
                    scheduleHoverPrefetch(() =>
                      prefetchOrbitLink(href, { ...linkCtx, intent: "hover" }),
                    );
                  }
                }}
                onMouseLeave={() => {
                  if (parsed?.kind === "file") {
                    cancelHoverPrefetch();
                  }
                }}
                onPointerDown={(e) => {
                  if (parsed?.kind === "file" && shouldPrefetchOnPointerDown(e)) {
                    cancelHoverPrefetch();
                    prefetchOrbitLink(href, { ...linkCtx, intent: "press" });
                  }
                }}
                onClick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  cancelHoverPrefetch();
                  openOrbitLink(href, linkCtx);
                }}
                className={cn(
                  span.inline_code
                    ? "group/link cursor-pointer no-underline"
                    : "text-ink-link hover:text-ink-link-hover decoration-ink-link/30 hover:decoration-ink-link-hover/80 cursor-pointer underline underline-offset-[3px] transition-colors",
                  className,
                )}
              >
                {span.math_ml ? (
                  <InlineMathSpan tex={span.text} mathMl={span.math_ml} />
                ) : span.inline_code ? (
                  <code className="text-ink-link group-hover/link:text-ink-link-hover group-hover/link:decoration-ink-link-hover/55 rounded-[5px] bg-black/[0.055] px-1.5 py-[0.5px] align-baseline font-mono text-[0.88em] leading-[1.35] underline-offset-2 transition-colors group-hover/link:bg-black/[0.085] group-hover/link:underline [overflow-wrap:anywhere] dark:bg-white/[0.09] dark:group-hover/link:bg-white/[0.14]">
                    {span.text}
                  </code>
                ) : (
                  span.text
                )}
              </a>
            );
          }

          if (span.math_ml) {
            return (
              <InlineMathSpan
                key={idx}
                tex={span.text}
                mathMl={span.math_ml}
                className={className || undefined}
              />
            );
          }

          if (span.inline_code) {
            const matchedSubagent =
              linkCtx.subagentMatcher && linkCtx.onOpenSubagent
                ? linkCtx.subagentMatcher.matchExact(span.text)
                : null;
            if (matchedSubagent && linkCtx.onOpenSubagent) {
              const onOpen = linkCtx.onOpenSubagent;
              return (
                <button
                  key={idx}
                  type="button"
                  title="Open subagent details"
                  onClick={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    onOpen(matchedSubagent);
                  }}
                  className={cn(
                    "group/salink text-foreground/92 hover:text-foreground inline cursor-pointer rounded-[5px] bg-black/[0.055] px-1.5 py-[0.5px] align-baseline font-mono text-[0.88em] leading-[1.35] underline decoration-foreground/25 decoration-dotted underline-offset-2 transition-colors hover:bg-black/[0.085] hover:decoration-foreground/75 [overflow-wrap:anywhere] dark:bg-white/[0.09] dark:hover:bg-white/[0.14]",
                    className,
                  )}
                >
                  <span>{span.text}</span>
                  <ArrowUpRight className="ml-[2px] inline-block size-[11px] -translate-y-[0.5px] opacity-45 transition-opacity group-hover/salink:opacity-90" />
                </button>
              );
            }
            return (
              <code
                key={idx}
                className="text-foreground/90 rounded-[5px] bg-black/[0.055] px-1.5 py-[0.5px] align-baseline font-mono text-[0.88em] leading-[1.35] [overflow-wrap:anywhere] dark:bg-white/[0.09]"
              >
                {span.text}
              </code>
            );
          }

          if (linkCtx.subagentMatcher && linkCtx.onOpenSubagent) {
            const segments = linkCtx.subagentMatcher.splitText(span.text);
            if (segments.length > 1 || segments[0]?.kind === "subagent") {
              const onOpen = linkCtx.onOpenSubagent;
              return (
                <span key={idx} className={className || undefined}>
                  {segments.map((seg, segIdx) =>
                    seg.kind === "subagent" ? (
                      <button
                        key={segIdx}
                        type="button"
                        title="Open subagent details"
                        onClick={(e) => {
                          e.preventDefault();
                          e.stopPropagation();
                          onOpen(seg.subagent);
                        }}
                        className="group/salink text-foreground/95 decoration-foreground/30 hover:decoration-foreground/80 hover:text-foreground inline cursor-pointer items-baseline rounded-[3px] underline decoration-dotted underline-offset-2 transition-colors"
                      >
                        <span>{seg.text}</span>
                        <ArrowUpRight className="ml-[2px] inline-block size-[11px] -translate-y-[0.5px] opacity-40 transition-opacity group-hover/salink:opacity-90" />
                      </button>
                    ) : (
                      <span key={segIdx}>{seg.text}</span>
                    ),
                  )}
                </span>
              );
            }
          }

          return (
            <span key={idx} className={className || undefined}>
              {span.text}
            </span>
          );
        })}
      </>
    );
  },
  (prev, next) => spansAreEqual(prev.spans, next.spans),
);

/** Copy feedback belongs to the latest click, even if an older write resolves later. */
export function useClipboardCopy(text: string) {
  const [copied, setCopied] = useState(false);
  const copyTimerRef = useRef<number | undefined>(undefined);
  const requestRef = useRef(0);

  useEffect(() => () => {
    requestRef.current++;
    window.clearTimeout(copyTimerRef.current);
  }, []);

  const copy = useCallback(() => {
    const request = ++requestRef.current;
    window.clearTimeout(copyTimerRef.current);
    setCopied(false);
    void navigator.clipboard?.writeText(text).then(() => {
      if (request !== requestRef.current) return;
      setCopied(true);
      copyTimerRef.current = window.setTimeout(() => setCopied(false), 1500);
    }).catch(() => {});
  }, [text]);

  return { copied, copy };
}

type CodeBlockLine = Extract<RichBlockView, { kind: "code_block" }>["lines"][number];

function codeLineIsEqual(a: CodeBlockLine, b: CodeBlockLine): boolean {
  if (a === b) return true;
  if (a.line_number !== b.line_number || a.tokens.length !== b.tokens.length) return false;
  for (let i = 0; i < a.tokens.length; i += 1) {
    const ta = a.tokens[i];
    const tb = b.tokens[i];
    if (
      ta.text !== tb.text ||
      ta.fg_hex !== tb.fg_hex ||
      ta.bold !== tb.bold ||
      ta.italic !== tb.italic
    ) {
      return false;
    }
  }
  return true;
}

const HighlightedCodeLine = memo(
  function HighlightedCodeLine({
    line,
    showLineNumbers,
    isDark,
  }: {
    line: CodeBlockLine;
    showLineNumbers: boolean;
    isDark: boolean;
  }) {
    return (
      <div className="flex min-w-fit">
        {showLineNumbers && (
          <span className="text-ink-faint tabular-nums mr-3.5 w-6 shrink-0 text-right select-none">
            {line.line_number}
          </span>
        )}
        <span className="whitespace-pre">
          {line.tokens.map((tok, idx) => (
            <span
              key={idx}
              style={{
                color: resolveTokenColor(tok, isDark),
                fontWeight: tok.bold ? 600 : undefined,
                fontStyle: tok.italic ? "italic" : undefined,
              }}
            >
              {tok.text}
            </span>
          ))}
        </span>
      </div>
    );
  },
  (prev, next) =>
    prev.isDark === next.isDark &&
    prev.showLineNumbers === next.showLineNumbers &&
    codeLineIsEqual(prev.line, next.line),
);

export const HighlightedCodeBlock = memo(
  function HighlightedCodeBlock({
    language,
    raw_code,
    lines,
    isDark,
    embedded = false,
    settled = false,
  }: Extract<RichBlockView, { kind: "code_block" }> & {
    isDark: boolean;
    embedded?: boolean;
    settled?: boolean;
  }) {
    const { t } = useI18n();
    const linkCtx = useContext(MarkdownLinkContext);
    const { copied, copy } = useClipboardCopy(raw_code);
    const [collapsed, setCollapsed] = useState(false);
    const [justRan, setJustRan] = useState(false);
    const runTimerRef = useRef<number | undefined>(undefined);

    useEffect(() => () => {
      window.clearTimeout(runTimerRef.current);
    }, []);

    const canCollapse = !embedded && lines.length > CODE_BLOCK_COLLAPSE_THRESHOLD;
    // Skip regex shell classification while actively streaming at 60Hz; classify once settled.
    const shellMeta = useMemo(
      () => (embedded || !settled ? null : classifyRunnableShellSnippet(language, raw_code)),
      [embedded, settled, language, raw_code],
    );

    const handleRunInTerminal = useCallback(() => {
      if (!shellMeta?.runnable) return;
      orbitActions.runCommandInTerminal(shellMeta.cleanedCommand, {
        execute: shellMeta.safeToAutoExecute,
        hostId: linkCtx.hostId,
        cwd: linkCtx.cwd,
      });
      setJustRan(true);
      window.clearTimeout(runTimerRef.current);
      runTimerRef.current = window.setTimeout(() => setJustRan(false), 1500);
    }, [shellMeta, linkCtx.hostId, linkCtx.cwd]);

    const showLineNumbers = lines.length > 2;
    const codeBody = (
      <ScrollArea
        orientation="horizontal"
        viewportClassName="px-3.5 py-3 font-mono text-[13px] leading-[1.6]"
      >
        {lines.map((line) => (
          <HighlightedCodeLine
            key={line.line_number}
            line={line}
            showLineNumbers={showLineNumbers}
            isDark={isDark}
          />
        ))}
      </ScrollArea>
    );

    if (embedded) {
      return codeBody;
    }

    const intrinsicHeight = collapsed ? 40 : Math.max(84, lines.length * 21 + 52);

    return (
      <div
        style={
          settled ? { containIntrinsicBlockSize: `auto ${intrinsicHeight}px` } : undefined
        }
        className={cn(
          "border-border-subtle bg-surface-recessed text-ink-primary my-3 overflow-hidden rounded-xl border first:mt-0 last:mb-0",
          settled && "orbit-heavy-block-settled",
        )}
      >
        <div
          className={cn(
            "border-border-subtle text-ink-muted flex items-center justify-between gap-2 px-3.5 py-1.5 text-[11px] select-none",
            !collapsed && "border-b",
          )}
        >
          <div className="flex min-w-0 items-center gap-2">
            {canCollapse ? (
              <button
                type="button"
                aria-expanded={!collapsed}
                onClick={() => setCollapsed((prev) => !prev)}
                className="text-ink-muted hover:text-ink-primary hover:bg-accent -ml-1 inline-flex cursor-pointer items-center gap-1 rounded-md px-1.5 py-0.5 font-mono text-[11px] transition-colors"
              >
                {collapsed ? (
                  <ChevronRight className="size-3 shrink-0" />
                ) : (
                  <ChevronDown className="size-3 shrink-0" />
                )}
                <span>{language || "text"}</span>
                <span className="text-ink-faint tabular-nums">
                  · {t("markdown.lineCount", { count: lines.length })}
                </span>
              </button>
            ) : (
              <span className="font-mono text-[11px]">{language || "text"}</span>
            )}
          </div>

          <div className="flex shrink-0 items-center gap-1">
            {shellMeta?.runnable && (
              <SimpleTooltip
                label={
                  shellMeta.safeToAutoExecute
                    ? t("markdown.runInTerminal")
                    : t("markdown.pasteInTerminal")
                }
              >
                <button
                  type="button"
                  onClick={handleRunInTerminal}
                  aria-label={
                    shellMeta.safeToAutoExecute
                      ? t("markdown.runInTerminal")
                      : t("markdown.pasteInTerminal")
                  }
                  className="text-ink-muted hover:text-ink-primary hover:bg-accent orbit-press inline-flex cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[11px] transition-colors"
                >
                  <span className="relative inline-flex size-3.5 items-center justify-center">
                    <Play
                      className={`size-3 transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)] ${
                        justRan ? "scale-50 opacity-0" : "scale-100 opacity-100"
                      }`}
                    />
                    <Check
                      className={`text-signal-success absolute size-3.5 transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)] ${
                        justRan ? "scale-100 opacity-100" : "scale-50 opacity-0"
                      }`}
                    />
                  </span>
                  <span>
                    {justRan
                      ? t("markdown.sentToTerminal")
                      : shellMeta.safeToAutoExecute
                        ? t("markdown.run")
                        : t("markdown.pasteToTerminalShort")}
                  </span>
                </button>
              </SimpleTooltip>
            )}

            <button
              type="button"
              onClick={copy}
              className="text-ink-muted hover:text-ink-primary hover:bg-accent orbit-press inline-flex cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[11px]"
            >
              <span className="relative inline-flex size-3.5 items-center justify-center">
                <Copy
                  className={`size-3.5 transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)] ${
                    copied ? "scale-50 -rotate-12 opacity-0" : "scale-100 rotate-0 opacity-100"
                  }`}
                />
                <Check
                  className={`text-signal-success absolute size-3.5 transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)] ${
                    copied ? "scale-100 rotate-0 opacity-100" : "scale-50 rotate-12 opacity-0"
                  }`}
                />
              </span>
              <span>{t(copied ? "common.copied" : "common.copy")}</span>
            </button>
          </div>
        </div>
        {!collapsed && codeBody}
      </div>
    );
  },
  (prev, next) =>
    prev.isDark === next.isDark &&
    prev.embedded === next.embedded &&
    prev.settled === next.settled &&
    prev.language === next.language &&
    prev.raw_code === next.raw_code &&
    prev.lines.length === next.lines.length,
);

function tableAlignClass(alignment: TableAlignmentView | undefined): string {
  switch (alignment) {
    case "center":
      return "text-center";
    case "right":
      return "text-right";
    default:
      return "text-left";
  }
}

const MarkdownTableBlock = memo(
  function MarkdownTableBlock({
    alignments,
    headers,
    rows,
    settled = false,
  }: Extract<RichBlockView, { kind: "table" }> & { settled?: boolean }) {
    const colCount = Math.max(
      headers.length,
      rows.reduce((max, row) => Math.max(max, row.cells.length), 0),
      1,
    );
    const colIndices = useMemo(() => Array.from({ length: colCount }, (_, i) => i), [colCount]);
    const scrollViewportRef = useRef<HTMLDivElement | null>(null);
    const scrollLeftRef = useRef<number>(0);

    // Preserve horizontal scrollLeft even if the table itself grows row-by-row during streaming
    useLayoutEffect(() => {
      const el = scrollViewportRef.current;
      const saved = scrollLeftRef.current;
      if (!el || saved <= 0) return;
      const maxLeft = Math.max(0, el.scrollWidth - el.clientWidth);
      const clamped = Math.min(saved, maxLeft);
      if (Math.abs(el.scrollLeft - clamped) > 0.5) {
        el.scrollLeft = clamped;
      }
    }, [alignments, headers, rows]);

    const intrinsicHeight = Math.max(96, (rows.length + 1) * 38 + 20);

    return (
      <div
        style={
          settled ? { containIntrinsicBlockSize: `auto ${intrinsicHeight}px` } : undefined
        }
        className={cn(
          "border-border-subtle my-3 overflow-hidden rounded-xl border first:mt-0 last:mb-0",
          settled && "orbit-heavy-block-settled",
        )}
      >
        <div
          ref={scrollViewportRef}
          onScroll={(e) => {
            scrollLeftRef.current = e.currentTarget.scrollLeft;
          }}
          className="orbit-native-scroll w-full overflow-x-auto overflow-y-hidden overscroll-x-contain"
        >
          <table className="w-full border-collapse text-[13px] leading-[1.55]">
            {headers.length > 0 && (
              <thead className="border-border-subtle bg-surface-recessed/65 border-b">
                <tr>
                  {colIndices.map((colIdx) => (
                    <th
                      key={colIdx}
                      className={`text-ink-secondary px-3.5 py-2 text-[12px] leading-[1.45] font-medium whitespace-pre-wrap ${tableAlignClass(
                        alignments[colIdx],
                      )}`}
                    >
                      <InlineSpans spans={headers[colIdx]?.spans ?? []} />
                    </th>
                  ))}
                </tr>
              </thead>
            )}
            <tbody>
              {rows.map((row, rowIdx) => (
                <tr
                  key={rowIdx}
                  className="border-border-subtle/80 hover:bg-accent/45 border-b transition-colors last:border-b-0"
                >
                  {colIndices.map((colIdx) => (
                    <td
                      key={colIdx}
                      className={`text-foreground/88 px-3.5 py-2 align-top whitespace-pre-wrap ${tableAlignClass(
                        alignments[colIdx],
                      )}`}
                    >
                      <InlineSpans spans={row.cells[colIdx]?.spans ?? []} />
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    );
  },
  (prev, next) =>
    prev.settled === next.settled &&
    tableAlignmentsAreEqual(prev.alignments, next.alignments) &&
    tableCellsAreEqual(prev.headers, next.headers) &&
    tableRowsAreEqual(prev.rows, next.rows),
);

function unorderedMarkerClass(depth: number): string {
  const mod = depth % 3;
  if (mod === 1) return "list-[circle]";
  if (mod === 2) return "list-[square]";
  return "list-disc";
}

function nestedListGroupIsEqual(a: NestedListGroupNode, b: NestedListGroupNode): boolean {
  if (a === b) return true;
  if (
    a.listKind !== b.listKind ||
    a.depth !== b.depth ||
    a.startIndex !== b.startIndex ||
    a.items.length !== b.items.length
  ) {
    return false;
  }
  for (let i = 0; i < a.items.length; i += 1) {
    if (!nestedListItemIsEqual(a.items[i], b.items[i])) return false;
  }
  return true;
}

function nestedListItemIsEqual(
  a: NestedListGroupNode["items"][number],
  b: NestedListGroupNode["items"][number],
): boolean {
  if (a === b) return true;
  if (!richBlockIsEqual(a.item, b.item)) return false;
  if (a.children.length !== b.children.length) return false;
  for (let i = 0; i < a.children.length; i += 1) {
    if (!nestedListGroupIsEqual(a.children[i], b.children[i])) return false;
  }
  return true;
}

const MemoNestedListItem = memo(
  function MemoNestedListItem({
    entry,
    listKind,
    listSpacingClass,
    itemKey,
  }: {
    entry: NestedListGroupNode["items"][number];
    listKind: NestedListGroupNode["listKind"];
    listSpacingClass: string;
    itemKey: string;
  }) {
    if (listKind === "ordered") {
      return (
        <li value={entry.item.index} className="pl-1 whitespace-pre-wrap">
          <InlineSpans spans={entry.item.spans} />
          {entry.children.map((child, childIdx) => (
            <MemoNestedListGroup
              key={`${itemKey}-${childIdx}`}
              group={child}
              listSpacingClass={listSpacingClass}
              groupKey={`${itemKey}-${childIdx}`}
            />
          ))}
        </li>
      );
    }

    if (listKind === "unordered") {
      return (
        <li className="pl-1 whitespace-pre-wrap">
          <InlineSpans spans={entry.item.spans} />
          {entry.children.map((child, childIdx) => (
            <MemoNestedListGroup
              key={`${itemKey}-${childIdx}`}
              group={child}
              listSpacingClass={listSpacingClass}
              groupKey={`${itemKey}-${childIdx}`}
            />
          ))}
        </li>
      );
    }

    return (
      <li className="relative pl-5.5 whitespace-pre-wrap">
        <span
          aria-hidden="true"
          className={cn(
            "absolute top-[4px] left-0 inline-flex size-[14px] items-center justify-center rounded-[4px] border select-none",
            entry.item.checked
              ? "border-border-strong bg-ink-primary/12 text-ink-primary"
              : "border-border-strong bg-transparent",
          )}
        >
          {entry.item.checked && <Check className="size-2.5 stroke-[2.5]" />}
        </span>
        <InlineSpans spans={entry.item.spans} />
        {entry.children.map((child, childIdx) => (
          <MemoNestedListGroup
            key={`${itemKey}-${childIdx}`}
            group={child}
            listSpacingClass={listSpacingClass}
            groupKey={`${itemKey}-${childIdx}`}
          />
        ))}
      </li>
    );
  },
  (prev, next) =>
    prev.listKind === next.listKind &&
    prev.listSpacingClass === next.listSpacingClass &&
    prev.itemKey === next.itemKey &&
    nestedListItemIsEqual(prev.entry, next.entry),
);

const MemoNestedListGroup = memo(
  function MemoNestedListGroup({
    group,
    listSpacingClass,
    groupKey,
  }: {
    group: NestedListGroupNode;
    listSpacingClass: string;
    groupKey: string;
  }) {
    const isRoot = group.depth === 0;
    const spacing = isRoot ? listSpacingClass : "mt-1 [&>li+li]:mt-1";

    if (group.listKind === "ordered") {
      return (
        <ol
          data-list-depth={group.depth}
          start={group.startIndex}
          className={cn(
            "marker:text-foreground/70 marker:tabular-nums list-decimal",
            isRoot ? "pl-6" : "pl-[20px]",
            spacing,
          )}
        >
          {group.items.map((entry, idx) => (
            <MemoNestedListItem
              key={idx}
              entry={entry}
              listKind="ordered"
              listSpacingClass={listSpacingClass}
              itemKey={`${groupKey}-${idx}`}
            />
          ))}
        </ol>
      );
    }

    if (group.listKind === "unordered") {
      return (
        <ul
          data-list-depth={group.depth}
          className={cn(
            "marker:text-foreground/65 pl-[20px]",
            unorderedMarkerClass(group.depth),
            spacing,
          )}
        >
          {group.items.map((entry, idx) => (
            <MemoNestedListItem
              key={idx}
              entry={entry}
              listKind="unordered"
              listSpacingClass={listSpacingClass}
              itemKey={`${groupKey}-${idx}`}
            />
          ))}
        </ul>
      );
    }

    return (
      <ul
        data-list-depth={group.depth}
        className={cn("list-none", isRoot ? "pl-0.5" : "pl-[20px]", spacing)}
      >
        {group.items.map((entry, idx) => (
          <MemoNestedListItem
            key={idx}
            entry={entry}
            listKind="checklist"
            listSpacingClass={listSpacingClass}
            itemKey={`${groupKey}-${idx}`}
          />
        ))}
      </ul>
    );
  },
  (prev, next) =>
    prev.listSpacingClass === next.listSpacingClass &&
    prev.groupKey === next.groupKey &&
    nestedListGroupIsEqual(prev.group, next.group),
);

function renderNestedListGroup(
  group: NestedListGroupNode,
  listSpacingClass: string,
  key: string,
): ReactNode {
  return (
    <MemoNestedListGroup
      key={key}
      group={group}
      listSpacingClass={listSpacingClass}
      groupKey={key}
    />
  );
}

function groupedNodeIsEqual(a: GroupedRenderNode, b: GroupedRenderNode): boolean {
  if (a === b) return true;
  if (a.kind !== b.kind || a.keyIndex !== b.keyIndex) return false;
  if (a.kind === "block" && b.kind === "block") {
    return richBlockIsEqual(a.block, b.block);
  }
  if (a.kind === "list_group" && b.kind === "list_group") {
    if (a.listKind !== b.listKind || a.items.length !== b.items.length) return false;
    for (let i = 0; i < a.items.length; i += 1) {
      if (!richBlockIsEqual(a.items[i], b.items[i])) return false;
    }
    return true;
  }
  return false;
}

function renderGroupedNodeContent(
  node: GroupedRenderNode,
  isDark: boolean,
  listSpacingClass: string,
  isStreamingBlock: boolean,
) {
  if (node.kind === "list_group") {
    const forest = buildNestedListForest(node.items);
    if (forest.length === 1) {
      return renderNestedListGroup(forest[0], listSpacingClass, `lg-${node.keyIndex}-0`);
    }
    return (
      <div className="flex flex-col gap-1.5">
        {forest.map((group, idx) =>
          renderNestedListGroup(group, listSpacingClass, `lg-${node.keyIndex}-${idx}`),
        )}
      </div>
    );
  }

  const block = node.block;
  const settled = !isStreamingBlock;
  switch (block.kind) {
    case "heading": {
      const sizeClass =
        block.level === 1
          ? "text-[20px] leading-[1.3] font-semibold tracking-[-0.02em] text-foreground mt-4 first:mt-0"
          : block.level === 2
            ? "text-[17px] leading-[1.35] font-semibold tracking-[-0.015em] text-foreground mt-3.5 first:mt-0"
            : "text-[15px] leading-[1.45] font-semibold text-foreground mt-3 first:mt-0";
      return (
        <div className={sizeClass}>
          <InlineSpans spans={block.spans} />
        </div>
      );
    }
    case "paragraph":
      return (
        <p className="whitespace-pre-wrap">
          <InlineSpans spans={block.spans} />
        </p>
      );
    case "code_block": {
      const diagramKind = detectDiagramKind(block.language, block.raw_code);
      if (diagramKind) {
        return (
          <DiagramBlock
            kind={diagramKind}
            block={block}
            isDark={isDark}
            isStreamingBlock={isStreamingBlock}
          />
        );
      }
      return <HighlightedCodeBlock {...block} isDark={isDark} settled={settled} />;
    }
    case "block_quote":
      return (
        <blockquote className="border-border-strong text-ink-secondary border-l-[2.5px] py-0.5 pl-3.5">
          <InlineSpans spans={block.spans} />
        </blockquote>
      );
    case "table":
      return (
        <MarkdownTableBlock
          kind="table"
          alignments={block.alignments}
          headers={block.headers}
          rows={block.rows}
          settled={settled}
        />
      );
    case "math_block":
      return <MathBlockView tex={block.tex} mathMl={block.math_ml} settled={settled} />;
    case "horizontal_rule":
      return <hr className="border-border-subtle my-3.5 first:mt-0 last:mb-0 select-none" />;
    case "list_item":
      return null;
  }
}

/**
 * Isolated block/list-group node:
 * - Settled blocks bypass pacing completely and freeze via structural equality so streaming new text
 *   below never re-renders preceding paragraphs, code blocks, diagrams, or tables.
 * - Only the active trailing block/list-item runs 60Hz character reveal pacing while keeping the
 *   exact same component instance across `isStreamingTail: true -> false` completion.
 */
const MarkdownGroupedNode = memo(
  function MarkdownGroupedNode({
    node,
    isDark,
    listSpacingClass,
    isStreamingTail,
  }: {
    node: GroupedRenderNode;
    isDark: boolean;
    listSpacingClass: string;
    isStreamingTail: boolean;
  }) {
    const pacingInput = useMemo(() => {
      if (!isStreamingTail) return EMPTY_PACING_BLOCKS;
      if (node.kind === "block") return [node.block];
      const lastItem = node.items[node.items.length - 1];
      return lastItem ? [lastItem] : EMPTY_PACING_BLOCKS;
    }, [isStreamingTail, node]);

    const pacingResetKey =
      node.kind === "block"
        ? `b:${node.keyIndex}`
        : `l:${node.keyIndex}:${node.items.length - 1}`;

    const pacedTail = usePacedRichBlocks(pacingInput, isStreamingTail, pacingResetKey);

    const resolvedNode = useMemo<GroupedRenderNode | null>(() => {
      if (!isStreamingTail) return node;
      if (node.kind === "block") {
        const revealed = pacedTail[0];
        if (!revealed) return null;
        return revealed === node.block ? node : { kind: "block", block: revealed, keyIndex: node.keyIndex };
      }
      const prefix = node.items.slice(0, -1);
      const revealedLast = pacedTail[0] as ListItemBlock | undefined;
      if (!revealedLast) {
        return prefix.length > 0 ? { ...node, items: prefix } : null;
      }
      if (node.items.length > 0 && revealedLast === node.items[node.items.length - 1]) {
        return node;
      }
      return { ...node, items: [...prefix, revealedLast] };
    }, [isStreamingTail, node, pacedTail]);

    if (!resolvedNode) return null;
    return renderGroupedNodeContent(resolvedNode, isDark, listSpacingClass, isStreamingTail);
  },
  (prev, next) =>
    prev.isDark === next.isDark &&
    prev.listSpacingClass === next.listSpacingClass &&
    prev.isStreamingTail === next.isStreamingTail &&
    groupedNodeIsEqual(prev.node, next.node),
);

export const MarkdownBlocksView = memo(function MarkdownBlocksView({
  blocks,
  streaming = false,
  compact = false,
  bubble = false,
}: {
  blocks: RichBlockView[];
  streaming?: boolean;
  compact?: boolean;
  bubble?: boolean;
}) {
  const isDark = useContext(MarkdownThemeContext);
  const groupedNodes = useMemo(() => groupMarkdownBlocks(blocks), [blocks]);

  const containerClass = compact
    ? "selectable-text text-foreground/82 min-w-0 space-y-1 text-[13px] leading-[1.55] break-words"
    : bubble
      ? "selectable-text text-foreground/92 min-w-0 space-y-2 text-[15px] leading-[1.65] break-words"
      : "selectable-text text-foreground/88 min-w-0 space-y-3 text-[15px] leading-[1.65] break-words";
  const listSpacingClass = compact || bubble ? "space-y-0.5" : "space-y-1";

  return (
    <div className={containerClass}>
      {groupedNodes.map((node, idx) => (
        <MarkdownGroupedNode
          key={node.keyIndex}
          node={node}
          isDark={isDark}
          listSpacingClass={listSpacingClass}
          isStreamingTail={streaming && idx === groupedNodes.length - 1}
        />
      ))}
    </div>
  );
});
