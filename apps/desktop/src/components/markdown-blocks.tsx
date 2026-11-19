import { memo, useState } from "react";
import { Check, Copy } from "lucide-react";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { InlineSpanView, RichBlockView, TableAlignmentView } from "@/lib/orbit-ipc";
import { usePacedRichBlocks } from "@/lib/text-reveal";
import { useOrbitSelector } from "@/state/orbit-store";

const lightTokenColorCache = new Map<string, string>();

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

const InlineSpans = memo(
  function InlineSpans({ spans }: { spans: InlineSpanView[] }) {
    return (
      <>
        {spans.map((span, idx) => {
          if (span.inline_code) {
            return (
              <code
                key={idx}
                className="text-foreground/90 rounded-[5px] bg-black/[0.055] px-1.5 py-[1.5px] font-mono text-[13px] dark:bg-white/[0.09]"
              >
                {span.text}
              </code>
            );
          }
          let className = "";
          if (span.bold) className += " font-semibold text-foreground";
          if (span.italic) className += " italic";
          if (span.strikethrough) className += " line-through opacity-65";

          if (span.link_url) {
            return (
              <a
                key={idx}
                href={span.link_url}
                target="_blank"
                rel="noreferrer"
                className={`text-foreground decoration-foreground/30 hover:decoration-foreground font-medium underline-offset-3 hover:underline ${className}`}
              >
                {span.text}
              </a>
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

const HighlightedCodeBlock = memo(
  function HighlightedCodeBlock({
    language,
    raw_code,
    lines,
    isDark,
  }: Extract<RichBlockView, { kind: "code_block" }> & { isDark: boolean }) {
    const [copied, setCopied] = useState(false);

    const handleCopy = async () => {
      await navigator.clipboard.writeText(raw_code);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    };

    return (
      <div className="border-border my-3 overflow-hidden rounded-xl border bg-zinc-50 text-zinc-800 dark:bg-[#141519] dark:text-zinc-200">
        <div className="border-border text-muted-foreground flex items-center justify-between border-b bg-black/[0.02] px-3.5 py-1.5 text-xs select-none dark:bg-white/[0.02]">
          <span className="font-mono text-[11.5px]">{language || "text"}</span>
          <button
            type="button"
            onClick={() => void handleCopy()}
            className="text-muted-foreground hover:text-foreground orbit-press inline-flex cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[11.5px] hover:bg-black/[0.05] dark:hover:bg-white/[0.06]"
          >
            <span className="relative inline-flex size-3.5 items-center justify-center">
              <Copy
                className={`size-3.5 transition-all duration-150 ease-[var(--ease-out-expo)] ${
                  copied ? "scale-50 -rotate-12 opacity-0" : "scale-100 rotate-0 opacity-100"
                }`}
              />
              <Check
                className={`absolute size-3.5 text-emerald-600 transition-all duration-150 ease-[var(--ease-out-expo)] dark:text-emerald-400 ${
                  copied ? "scale-100 rotate-0 opacity-100" : "scale-50 rotate-12 opacity-0"
                }`}
              />
            </span>
            <span>{copied ? "Copied" : "Copy"}</span>
          </button>
        </div>
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
      </div>
    );
  },
  (prev, next) =>
    prev.isDark === next.isDark &&
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

const MarkdownTableBlock = memo(function MarkdownTableBlock({
  alignments,
  headers,
  rows,
}: Extract<RichBlockView, { kind: "table" }>) {
  const colCount = Math.max(
    headers.length,
    rows.reduce((max, row) => Math.max(max, row.cells.length), 0),
    1,
  );
  const colIndices = Array.from({ length: colCount }, (_, i) => i);

  return (
    <div className="border-border my-3 overflow-hidden rounded-xl border bg-black/[0.015] dark:bg-white/[0.02]">
      <ScrollArea orientation="horizontal" viewportClassName="w-full">
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
      </ScrollArea>
    </div>
  );
});

export const MarkdownBlocksView = memo(function MarkdownBlocksView({
  blocks,
  streaming = false,
  compact = false,
}: {
  blocks: RichBlockView[];
  streaming?: boolean;
  compact?: boolean;
}) {
  const isDark = useOrbitSelector((s) => s.resolvedTheme === "dark");
  const visibleBlocks = usePacedRichBlocks(blocks, streaming);

  return (
    <div
      className={
        compact
          ? "selectable-text text-foreground/78 space-y-1 text-[13px] leading-[1.5] break-words"
          : "selectable-text text-foreground/95 space-y-3 text-[14.5px] leading-[1.72] break-words"
      }
    >
      {visibleBlocks.map((block, idx) => {
        switch (block.kind) {
          case "heading": {
            const sizeClass =
              block.level === 1
                ? "text-[18px] font-semibold tracking-[-0.01em] text-foreground mt-4"
                : block.level === 2
                  ? "text-[16px] font-semibold tracking-[-0.01em] text-foreground mt-3.5"
                  : "text-[14.5px] font-semibold text-foreground mt-3";
            return (
              <div key={idx} className={sizeClass}>
                <InlineSpans spans={block.spans} />
              </div>
            );
          }
          case "paragraph":
            return (
              <p key={idx} className="whitespace-pre-wrap">
                <InlineSpans spans={block.spans} />
              </p>
            );
          case "code_block":
            return <HighlightedCodeBlock key={idx} {...block} isDark={isDark} />;
          case "list_item":
            return (
              <div key={idx} className="flex items-start gap-2.5 pl-1">
                <span className="text-foreground/75 tabular-nums shrink-0 select-none">
                  {block.checked !== null
                    ? block.checked
                      ? "✓"
                      : "○"
                    : block.ordered
                      ? `${block.index}.`
                      : "•"}
                </span>
                <div className="flex-1 whitespace-pre-wrap">
                  <InlineSpans spans={block.spans} />
                </div>
              </div>
            );
          case "block_quote":
            return (
              <blockquote
                key={idx}
                className="border-border text-muted-foreground border-l-2 pl-3.5 text-[14px]"
              >
                <InlineSpans spans={block.spans} />
              </blockquote>
            );
          case "table":
            return (
              <MarkdownTableBlock
                key={idx}
                kind="table"
                alignments={block.alignments}
                headers={block.headers}
                rows={block.rows}
              />
            );
          case "horizontal_rule":
            return <hr key={idx} className="border-border my-3.5 select-none" />;
        }
      })}
    </div>
  );
});
