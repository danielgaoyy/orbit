import { useState } from "react";
import { Check, Copy } from "lucide-react";
import type { InlineSpanView, RichBlockView } from "@/lib/orbit-ipc";
import { Button } from "@/components/ui/button";

function InlineSpans({ spans }: { spans: InlineSpanView[] }) {
  return (
    <>
      {spans.map((span, idx) => {
        if (span.inline_code) {
          return (
            <code
              key={idx}
              className="bg-muted text-primary rounded px-1.5 py-0.5 font-mono text-[12.5px]"
            >
              {span.text}
            </code>
          );
        }
        let className = "";
        if (span.bold) className += " font-semibold";
        if (span.italic) className += " italic";
        if (span.strikethrough) className += " line-through opacity-75";

        if (span.link_url) {
          return (
            <a
              key={idx}
              href={span.link_url}
              target="_blank"
              rel="noreferrer"
              className={`text-primary underline underline-offset-3 ${className}`}
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
}

function HighlightedCodeBlock({
  language,
  raw_code,
  lines,
}: Extract<RichBlockView, { kind: "code_block" }>) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    await navigator.clipboard.writeText(raw_code);
    setCopied(true);
    setTimeout(() => setCopied(false), 1600);
  };

  return (
    <div className="my-2 overflow-hidden rounded-lg border border-white/10 bg-[#0B0F19]">
      <div className="flex items-center justify-between border-b border-white/8 bg-white/[0.03] px-3 py-1.5 text-xs text-zinc-400">
        <span className="font-mono">{language || "text"}</span>
        <Button variant="ghost" size="xs" onClick={handleCopy} className="h-6 px-2 text-zinc-400">
          {copied ? <Check className="size-3.5 text-emerald-400" /> : <Copy className="size-3.5" />}
          {copied ? "Copied" : "Copy"}
        </Button>
      </div>
      <div className="overflow-x-auto p-3 font-mono text-[12.5px] leading-5">
        {lines.map((line) => (
          <div key={line.line_number} className="flex min-w-fit">
            <span className="mr-4 w-7 shrink-0 text-right text-zinc-600 select-none">
              {line.line_number}
            </span>
            <span className="whitespace-pre">
              {line.tokens.map((tok, idx) => (
                <span
                  key={idx}
                  style={{
                    color: tok.fg_hex,
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
      </div>
    </div>
  );
}

export function MarkdownBlocksView({ blocks }: { blocks: RichBlockView[] }) {
  return (
    <div className="space-y-2 text-sm leading-relaxed break-words">
      {blocks.map((block, idx) => {
        switch (block.kind) {
          case "heading": {
            const sizeClass =
              block.level === 1
                ? "text-lg font-bold mt-3"
                : block.level === 2
                  ? "text-base font-semibold mt-2.5"
                  : "text-sm font-semibold mt-2";
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
            return <HighlightedCodeBlock key={idx} {...block} />;
          case "list_item":
            return (
              <div key={idx} className="flex items-start gap-2 pl-1">
                <span className="text-muted-foreground mt-0.5 shrink-0 font-mono text-xs">
                  {block.checked !== null
                    ? block.checked
                      ? "[x]"
                      : "[ ]"
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
                className="border-primary/50 text-muted-foreground border-l-2 pl-3 italic"
              >
                <InlineSpans spans={block.spans} />
              </blockquote>
            );
          case "horizontal_rule":
            return <hr key={idx} className="border-border my-3" />;
        }
      })}
    </div>
  );
}
