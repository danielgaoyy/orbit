import {
  type CSSProperties,
  type ReactNode,
  createElement,
  memo,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Check, Copy } from "lucide-react";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";

const MATHML_NAMESPACE = "http://www.w3.org/1998/Math/MathML";

const ALLOWED_MATHML_TAGS = new Set([
  "math",
  "mrow",
  "mi",
  "mn",
  "mo",
  "mtext",
  "mspace",
  "mfrac",
  "msqrt",
  "mroot",
  "msub",
  "msup",
  "msubsup",
  "munder",
  "mover",
  "munderover",
  "mtable",
  "mtr",
  "mtd",
  "mstyle",
  "mphantom",
]);

const ALLOWED_MATHML_ATTRS = new Set([
  "display",
  "mathvariant",
  "stretchy",
  "fence",
  "form",
  "lspace",
  "rspace",
  "movablelimits",
  "largeop",
  "linethickness",
  "accent",
  "accentunder",
  "width",
  "displaystyle",
  "scriptlevel",
  "columnalign",
  "rowalign",
]);

const MAX_MATHML_WALK_DEPTH = 80;

let sharedDomParser: DOMParser | null = null;

function getSharedDomParser(): DOMParser | null {
  if (typeof DOMParser === "undefined") {
    return null;
  }
  if (!sharedDomParser) {
    sharedDomParser = new DOMParser();
  }
  return sharedDomParser;
}

function parseSafeMathStyle(raw: string): CSSProperties | undefined {
  let fontWeight: number | undefined;
  let fontStyle: "italic" | undefined;
  let textAlign: "left" | "right" | "center" | undefined;
  for (const decl of raw.split(";")) {
    const [prop, val] = decl.split(":").map((s) => s.trim().toLowerCase());
    if (prop === "font-weight" && (val === "700" || val === "bold")) {
      fontWeight = 700;
    } else if (prop === "font-style" && val === "italic") {
      fontStyle = "italic";
    } else if (prop === "text-align" && (val === "left" || val === "right" || val === "center")) {
      textAlign = val;
    }
  }
  if (fontWeight === undefined && fontStyle === undefined && textAlign === undefined) {
    return undefined;
  }
  return { fontWeight, fontStyle, textAlign };
}

function buildSafeMathNode(node: Node, key: string, depth: number): ReactNode {
  if (depth > MAX_MATHML_WALK_DEPTH) {
    return null;
  }
  if (node.nodeType === 3) {
    return node.nodeValue ?? "";
  }
  if (node.nodeType !== 1) {
    return null;
  }
  const el = node as Element;
  if (el.namespaceURI !== MATHML_NAMESPACE) {
    return null;
  }
  const tag = el.localName.toLowerCase();
  if (!ALLOWED_MATHML_TAGS.has(tag)) {
    return null;
  }

  const props: Record<string, unknown> = { key };
  for (let i = 0; i < el.attributes.length; i += 1) {
    const attr = el.attributes[i];
    const name = attr.name.toLowerCase();
    if (name === "style") {
      const safeStyle = parseSafeMathStyle(attr.value);
      if (safeStyle) {
        props.style = safeStyle;
      }
      continue;
    }
    if (ALLOWED_MATHML_ATTRS.has(name)) {
      props[name] = attr.value;
    }
  }

  const children: ReactNode[] = [];
  for (let i = 0; i < el.childNodes.length; i += 1) {
    const child = buildSafeMathNode(el.childNodes[i], `${key}.${i}`, depth + 1);
    if (child !== null && child !== "") {
      children.push(child);
    }
  }

  return createElement(tag, props, children.length > 0 ? children : undefined);
}

function parseMathMlToReactTree(mathMl: string): ReactNode | null {
  const parser = getSharedDomParser();
  if (!parser) {
    return null;
  }
  try {
    const doc = parser.parseFromString(mathMl, "application/xml");
    if (doc.getElementsByTagName("parsererror").length > 0) {
      return null;
    }
    const root = doc.documentElement;
    if (!root || root.namespaceURI !== MATHML_NAMESPACE || root.localName.toLowerCase() !== "math") {
      return null;
    }
    return buildSafeMathNode(root, "m", 0);
  } catch {
    return null;
  }
}

export const InlineMathSpan = memo(function InlineMathSpan({
  tex,
  mathMl,
  className,
}: {
  tex: string;
  mathMl: string;
  className?: string;
}) {
  const tree = useMemo(() => parseMathMlToReactTree(mathMl), [mathMl]);

  if (!tree) {
    return (
      <code className="text-foreground/90 rounded-[5px] bg-black/[0.055] px-1.5 py-[1.5px] font-mono text-[13px] dark:bg-white/[0.09]">
        {tex}
      </code>
    );
  }

  return (
    <span
      className={cn("orbit-math-inline inline-block align-baseline", className)}
      title={tex}
    >
      {tree}
    </span>
  );
});

export const MathBlockView = memo(
  function MathBlockView({
    tex,
    mathMl,
    settled = false,
  }: {
    tex: string;
    mathMl: string;
    settled?: boolean;
  }) {
    const [copied, setCopied] = useState(false);
    const copyTimerRef = useRef<number | null>(null);
    const tree = useMemo(() => parseMathMlToReactTree(mathMl), [mathMl]);

    useEffect(() => {
      return () => {
        if (copyTimerRef.current !== null) {
          window.clearTimeout(copyTimerRef.current);
          copyTimerRef.current = null;
        }
      };
    }, []);

    const handleCopyLatex = async () => {
      try {
        await navigator.clipboard.writeText(tex);
        setCopied(true);
        if (copyTimerRef.current !== null) {
          window.clearTimeout(copyTimerRef.current);
        }
        copyTimerRef.current = window.setTimeout(() => {
          copyTimerRef.current = null;
          setCopied(false);
        }, 1500);
      } catch {
        // Clipboard permission denied or unavailable; ignore gracefully
      }
    };

    const intrinsicHeight = Math.max(64, tex.split("\n").length * 28 + 36);

    return (
      <div
        style={settled ? { containIntrinsicBlockSize: `auto ${intrinsicHeight}px` } : undefined}
        className={cn(
          "group relative my-3 overflow-hidden rounded-xl border border-black/[0.065] bg-black/[0.015] dark:border-white/[0.075] dark:bg-white/[0.02]",
          settled && "orbit-heavy-block-settled",
        )}
      >
        <div className="pointer-events-none absolute top-2 right-2 z-10 opacity-0 transition-opacity duration-150 group-focus-within:pointer-events-auto group-focus-within:opacity-100 group-hover:pointer-events-auto group-hover:opacity-100">
          <button
            type="button"
            onClick={() => void handleCopyLatex()}
            title="Copy LaTeX"
            className="border-border/80 bg-background/90 text-muted-foreground hover:text-foreground orbit-press inline-flex h-6 cursor-pointer items-center gap-1 rounded-md border px-2 text-[11px] shadow-2xs backdrop-blur-md"
          >
            {copied ? (
              <Check className="size-3 text-emerald-600 dark:text-emerald-400" />
            ) : (
              <Copy className="size-3" />
            )}
            <span>{copied ? "Copied" : "Copy LaTeX"}</span>
          </button>
        </div>

        <ScrollArea orientation="horizontal" viewportClassName="px-5 py-4">
          <div className="orbit-math-block flex min-w-full items-center justify-center text-center">
            {tree ?? (
              <code className="text-foreground/90 font-mono text-[13px] whitespace-pre">
                {tex}
              </code>
            )}
          </div>
        </ScrollArea>
      </div>
    );
  },
  (prev, next) =>
    prev.tex === next.tex && prev.mathMl === next.mathMl && prev.settled === next.settled,
);
