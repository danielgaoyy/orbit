import * as React from "react";
import { cn } from "@/lib/utils";

/**
 * Official shadcn/ui `<Kbd>` primitive adapted to Orbit's pixel scale.
 */
export function Kbd({ className, ...props }: React.ComponentProps<"kbd">) {
  return (
    <kbd
      data-slot="kbd"
      className={cn(
        "bg-muted text-muted-foreground pointer-events-none inline-flex h-[20px] w-fit min-w-[20px] items-center justify-center gap-1 rounded-[5px] px-1.5 font-sans text-[11.5px] leading-none font-medium select-none dark:bg-white/[0.09]",
        "[&_svg:not([class*='size-'])]:size-3",
        "[[data-slot=tooltip-content]_&]:bg-background/20 [[data-slot=tooltip-content]_&]:text-background dark:[[data-slot=tooltip-content]_&]:bg-background/15",
        className,
      )}
      {...props}
    />
  );
}

/**
 * Official shadcn/ui `<KbdGroup>` container for multi-key chords (e.g. `⇧` + `Tab`, `⌘` + `K`).
 */
export function KbdGroup({ className, ...props }: React.ComponentProps<"kbd">) {
  return (
    <kbd
      data-slot="kbd-group"
      className={cn("inline-flex items-center gap-1", className)}
      {...props}
    />
  );
}

const MODIFIER_GLYPHS = new Set(["⌘", "⇧", "⌥", "⌃"]);

export function tokenizeChord(chord: string): string[] {
  const trimmed = chord.trim();
  if (!trimmed) return [];

  // e.g. `Ctrl+Shift+A` or `Ctrl+\``
  if (trimmed.includes("+") && trimmed.length > 1) {
    return trimmed
      .split("+")
      .map((part) => part.trim())
      .filter(Boolean);
  }

  // e.g. `Esc Esc`
  if (trimmed.includes(" ")) {
    return trimmed
      .split(/\s+/)
      .flatMap((part) => tokenizeChord(part))
      .filter(Boolean);
  }

  // Peel leading macOS modifier glyphs (`⌘⇧A` -> `["⌘", "⇧", "A"]`, `⇧Tab` -> `["⇧", "Tab"]`)
  const tokens: string[] = [];
  let i = 0;
  while (i < trimmed.length && MODIFIER_GLYPHS.has(trimmed[i])) {
    tokens.push(trimmed[i]);
    i += 1;
  }
  const rest = trimmed.slice(i);
  if (rest) {
    tokens.push(rest);
  }
  return tokens;
}

/**
 * Renders any shortcut string (single key, multi-key chord, ` / ` alternatives, or ` .. ` range)
 * using official shadcn `<KbdGroup>` + `<Kbd>` keycaps.
 */
export function ShortcutKbd({
  keys,
  className,
  kbdClassName,
}: {
  keys: string;
  className?: string;
  kbdClassName?: string;
}) {
  if (keys.includes(" / ")) {
    const alts = keys.split(" / ");
    return (
      <span className={cn("inline-flex items-center gap-1.5", className)}>
        {alts.map((alt, idx) => (
          <React.Fragment key={idx}>
            {idx > 0 && <span className="text-muted-foreground/55 text-[11.5px]">/</span>}
            <ShortcutKbd keys={alt} kbdClassName={kbdClassName} />
          </React.Fragment>
        ))}
      </span>
    );
  }

  if (keys.includes(" .. ")) {
    const parts = keys.split(" .. ");
    return (
      <span className={cn("inline-flex items-center gap-1", className)}>
        {parts.map((part, idx) => (
          <React.Fragment key={idx}>
            {idx > 0 && <span className="text-muted-foreground/55 text-[11.5px]">..</span>}
            <ShortcutKbd keys={part} kbdClassName={kbdClassName} />
          </React.Fragment>
        ))}
      </span>
    );
  }

  const tokens = tokenizeChord(keys);
  if (tokens.length === 0) return null;
  if (tokens.length === 1) {
    return <Kbd className={cn(className, kbdClassName)}>{tokens[0]}</Kbd>;
  }

  return (
    <KbdGroup className={className}>
      {tokens.map((tok, idx) => (
        <Kbd key={`${tok}-${idx}`} className={kbdClassName}>
          {tok}
        </Kbd>
      ))}
    </KbdGroup>
  );
}
