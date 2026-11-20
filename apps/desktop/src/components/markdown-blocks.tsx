import {
  createContext,
  memo,
  useContext,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { Check, Copy } from "lucide-react";
import { DiagramBlock } from "@/components/diagram-block";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  type InlineSpanView,
  type RichBlockView,
  type TableAlignmentView,
  type TableCellView,
  type TableRowView,
  detectDiagramKind,
} from "@/lib/orbit-ipc";
import { openOrbitLink, parseOrbitLinkTarget } from "@/lib/file-links";
import { usePacedRichBlocks } from "@/lib/text-reveal";
import { cn } from "@/lib/utils";

const MarkdownThemeContext = createContext<boolean>(false);

export interface MarkdownLinkContextValue {
  hostId?: string;
  cwd?: string;
  baseFilePath?: string;
  onAnchor?: (fragment: string) => void;
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
  children,
}: MarkdownLinkContextValue & { children: ReactNode }) {
  const value = useMemo<MarkdownLinkContextValue>(
    () => ({ hostId, cwd, baseFilePath, onAnchor }),
    [hostId, cwd, baseFilePath, onAnchor],
  );
  return <MarkdownLinkContext.Provider value={value}>{children}</MarkdownLinkContext.Provider>;
}

const lightTokenColorCache = new Map<string, string>();
const EMPTY_PACING_BLOCKS: RichBlockView[] = [];

/**
 * `orbit-client-core` highlights tokens against `base16-ocean.dark` pastel foregrounds.
 * When the desktop UI follows macOS Light Mode, shift bright pastel tokens into deep ink tones
 * while preserving hue differentiation so code blocks sit naturally on a light paper surface.
 */
function adaptTokenColor(hex: string, isDark: boolean): string {
  if (isDark || !hex.startsWith("#") || hex.length !== 7) return hex;
  const cached = lightTokenColorCache.get(hex);
  if (cached !== undefined) return cached;

  const r = Number.parseInt(hex.slice(1, 3), 16);
  const g = Number.parseInt(hex.slice(3, 5), 16);
  const b = Number.parseInt(hex.slice(5, 7), 16);
  if (Number.isNaN(r) || Number.isNaN(g) || Number.isNaN(b)) return hex;

  // Neutral near-white default tokens (`#C0C5CE` / `#E2E8F0`) -> crisp charcoal ink
  const spread = Math.max(r, g, b) - Math.min(r, g, b);
  let computed: string;
  if (spread < 24 && r > 150) {
    computed = "#1F2937";
  } else {
    // Scale down luminance while keeping chromatic hue saturation
    const scale = 0.52;
    const dr = Math.max(0, Math.min(255, Math.round(r * scale)));
    const dg = Math.max(0, Math.min(255, Math.round(g * scale)));
    const db = Math.max(0, Math.min(255, Math.round(b * scale)));
    computed = `rgb(${dr}, ${dg}, ${db})`;
  }
  lightTokenColorCache.set(hex, computed);
  return computed;
}

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
      sa.link_url !== sb.link_url
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
                onClick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  openOrbitLink(href, linkCtx);
                }}
                className={`text-foreground decoration-foreground/30 hover:decoration-foreground cursor-pointer font-medium underline-offset-3 hover:underline ${className}`}
              >
                {span.inline_code ? (
                  <code className="text-foreground/90 rounded-[5px] bg-black/[0.055] px-1.5 py-[1.5px] font-mono text-[13px] [overflow-wrap:anywhere] dark:bg-white/[0.09]">
                    {span.text}
                  </code>
                ) : (
                  span.text
                )}
              </a>
            );
          }

          if (span.inline_code) {
            return (
              <code
                key={idx}
                className="text-foreground/90 rounded-[5px] bg-black/[0.055] px-1.5 py-[1.5px] font-mono text-[13px] [overflow-wrap:anywhere] dark:bg-white/[0.09]"
              >
                {span.text}
              </code>
            );
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
    const [copied, setCopied] = useState(false);

    const handleCopy = async () => {
      await navigator.clipboard.writeText(raw_code);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    };

    const codeBody = (
      <ScrollArea
        orientation="horizontal"
        viewportClassName="px-3.5 py-3 font-mono text-[13px] leading-[1.6]"
      >
        {lines.map((line) => (
          <div key={line.line_number} className="flex min-w-fit">
            <span className="tabular-nums mr-3.5 w-6 shrink-0 text-right text-zinc-400 select-none dark:text-zinc-600">
              {line.line_number}
            </span>
            <span className="whitespace-pre">
              {line.tokens.map((tok, idx) => (
                <span
                  key={idx}
                  style={{
                    color: adaptTokenColor(tok.fg_hex, isDark),
                    fontWeight: tok.bold ? 600 : undefined,
                    fontStyle: tok.italic ? "italic" : undefined,
                  }}
                >
                  {tok.text}
                </span>
              ))}
            </span>
          </div>
        ))}
      </ScrollArea>
    );

    if (embedded) {
      return codeBody;
    }

    const intrinsicHeight = Math.max(84, lines.length * 21 + 52);

    return (
      <div
        style={
          settled ? { containIntrinsicBlockSize: `auto ${intrinsicHeight}px` } : undefined
        }
        className={cn(
          "border-border my-3 overflow-hidden rounded-xl border bg-zinc-50 text-zinc-800 dark:bg-[#141519] dark:text-zinc-200",
          settled && "orbit-heavy-block-settled",
        )}
      >
        <div className="border-border text-muted-foreground flex items-center justify-between border-b bg-black/[0.02] px-3.5 py-1.5 text-xs select-none dark:bg-white/[0.02]">
          <span className="font-mono text-[11.5px]">{language || "text"}</span>
          <button
            type="button"
            onClick={() => void handleCopy()}
            className="text-muted-foreground hover:text-foreground orbit-press inline-flex cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[11.5px] hover:bg-black/[0.05] dark:hover:bg-white/[0.06]"
          >
            <span className="relative inline-flex size-3.5 items-center justify-center">
              <Copy
                className={`size-3.5 transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)] ${
                  copied ? "scale-50 -rotate-12 opacity-0" : "scale-100 rotate-0 opacity-100"
                }`}
              />
              <Check
                className={`absolute size-3.5 text-emerald-600 transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)] dark:text-emerald-400 ${
                  copied ? "scale-100 rotate-0 opacity-100" : "scale-50 rotate-12 opacity-0"
                }`}
              />
            </span>
            <span>{copied ? "Copied" : "Copy"}</span>
          </button>
        </div>
        {codeBody}
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
          "border-border my-3 overflow-hidden rounded-xl border bg-black/[0.015] dark:bg-white/[0.02]",
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
          <table className="w-full border-collapse text-[13.5px] leading-[1.55]">
            {headers.length > 0 && (
              <thead className="border-border border-b bg-black/[0.035] dark:bg-white/[0.045]">
                <tr>
                  {colIndices.map((colIdx) => (
                    <th
                      key={colIdx}
                      className={`border-border/70 text-foreground border-r px-3.5 py-2 font-semibold whitespace-pre-wrap last:border-r-0 ${tableAlignClass(
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
                  className="border-border/65 border-b last:border-b-0 even:bg-black/[0.015] dark:even:bg-white/[0.018]"
                >
                  {colIndices.map((colIdx) => (
                    <td
                      key={colIdx}
                      className={`border-border/60 text-foreground/90 border-r px-3.5 py-2 align-top whitespace-pre-wrap last:border-r-0 ${tableAlignClass(
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

type ListItemBlock = Extract<RichBlockView, { kind: "list_item" }>;
type ListGroupKind = "ordered" | "unordered" | "checklist";

type GroupedRenderNode =
  | { kind: "block"; block: RichBlockView; keyIndex: number }
  | { kind: "list_group"; listKind: ListGroupKind; items: ListItemBlock[]; keyIndex: number };

function classifyListItem(item: ListItemBlock): ListGroupKind {
  if (item.checked !== null) return "checklist";
  return item.ordered ? "ordered" : "unordered";
}

function groupMarkdownBlocks(blocks: RichBlockView[]): GroupedRenderNode[] {
  const nodes: GroupedRenderNode[] = [];
  for (let i = 0; i < blocks.length; i += 1) {
    const block = blocks[i];
    if (block.kind === "list_item") {
      const listKind = classifyListItem(block);
      const prev = nodes[nodes.length - 1];
      if (prev && prev.kind === "list_group" && prev.listKind === listKind) {
        prev.items.push(block);
      } else {
        nodes.push({ kind: "list_group", listKind, items: [block], keyIndex: i });
      }
    } else {
      nodes.push({ kind: "block", block, keyIndex: i });
    }
  }
  return nodes;
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
    if (node.listKind === "ordered") {
      return (
        <ol
          start={node.items[0]?.index ?? 1}
          className={cn(
            "marker:text-foreground/70 marker:tabular-nums list-decimal pl-6",
            listSpacingClass,
          )}
        >
          {node.items.map((item, itemIdx) => (
            <li key={itemIdx} value={item.index} className="pl-1 whitespace-pre-wrap">
              <InlineSpans spans={item.spans} />
            </li>
          ))}
        </ol>
      );
    }
    if (node.listKind === "unordered") {
      return (
        <ul className={cn("marker:text-foreground/65 list-disc pl-[20px]", listSpacingClass)}>
          {node.items.map((item, itemIdx) => (
            <li key={itemIdx} className="pl-1 whitespace-pre-wrap">
              <InlineSpans spans={item.spans} />
            </li>
          ))}
        </ul>
      );
    }
    return (
      <ul className={cn("list-none pl-0.5", listSpacingClass)}>
        {node.items.map((item, itemIdx) => (
          <li key={itemIdx} className="relative pl-5 whitespace-pre-wrap">
            <span className="text-foreground/75 absolute top-0 left-0 select-none">
              {item.checked ? "✓" : "○"}
            </span>
            <InlineSpans spans={item.spans} />
          </li>
        ))}
      </ul>
    );
  }

  const block = node.block;
  const settled = !isStreamingBlock;
  switch (block.kind) {
    case "heading": {
      const sizeClass =
        block.level === 1
          ? "text-[18px] font-semibold tracking-[-0.01em] text-foreground mt-4"
          : block.level === 2
            ? "text-[16px] font-semibold tracking-[-0.01em] text-foreground mt-3.5"
            : "text-[14.5px] font-semibold text-foreground mt-3";
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
        <blockquote className="border-border text-muted-foreground border-l-2 pl-3.5 text-[14px]">
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
    case "horizontal_rule":
      return <hr className="border-border my-3.5 select-none" />;
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
    ? "selectable-text text-foreground/78 min-w-0 space-y-1 text-[13px] leading-[1.5] break-words"
    : bubble
      ? "selectable-text text-foreground/95 min-w-0 space-y-2 text-[14.5px] leading-[1.64] break-words"
      : "selectable-text text-foreground/95 min-w-0 space-y-3 text-[14.5px] leading-[1.72] break-words";
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
