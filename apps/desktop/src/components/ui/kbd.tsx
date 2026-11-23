import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const kbdVariants = cva(
  "pointer-events-none inline-flex w-fit items-center justify-center gap-1 font-mono leading-none font-medium select-none [&_svg:not([class*='size-'])]:size-3 [[data-slot=tooltip-content]_&]:bg-surface-recessed [[data-slot=tooltip-content]_&]:border-border-subtle [[data-slot=tooltip-content]_&]:text-ink-secondary",
  {
    variants: {
      variant: {
        default:
          "bg-surface-recessed border-border-subtle text-ink-tertiary border shadow-[0_1px_0_0_var(--border-subtle)]",
        subtle: "bg-surface-recessed/75 border-border-subtle/80 text-ink-muted border",
        inverted:
          "bg-action-primary-fg/15 border-action-primary-fg/20 text-action-primary-fg/90 border",
      },
      size: {
        xs: "h-[17px] min-w-[17px] rounded-[4px] px-1 text-[10px]",
        sm: "h-[18px] min-w-[18px] rounded-[4.5px] px-1 text-[10.5px]",
        default: "h-[20px] min-w-[20px] rounded-[5px] px-1.5 text-[11px]",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

export interface KbdProps
  extends React.ComponentProps<"kbd">,
    VariantProps<typeof kbdVariants> {}

/**
 * Official shadcn/ui `<Kbd>` primitive styled with Orbit's monochrome precision tokens.
 */
export function Kbd({ className, variant, size, ...props }: KbdProps) {
  return (
    <kbd
      data-slot="kbd"
      className={cn(kbdVariants({ variant, size }), className)}
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
      className={cn("inline-flex items-center gap-0.5", className)}
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
  size,
  variant,
  className,
  kbdClassName,
}: {
  keys: string;
  size?: KbdProps["size"];
  variant?: KbdProps["variant"];
  className?: string;
  kbdClassName?: string;
}) {
  if (keys.includes(" / ")) {
    const alts = keys.split(" / ");
    return (
      <span className={cn("inline-flex items-center gap-1.5", className)}>
        {alts.map((alt, idx) => (
          <React.Fragment key={idx}>
            {idx > 0 && <span className="text-ink-faint text-[11px]">/</span>}
            <ShortcutKbd keys={alt} size={size} variant={variant} kbdClassName={kbdClassName} />
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
            {idx > 0 && <span className="text-ink-faint text-[11px]">..</span>}
            <ShortcutKbd keys={part} size={size} variant={variant} kbdClassName={kbdClassName} />
          </React.Fragment>
        ))}
      </span>
    );
  }

  const tokens = tokenizeChord(keys);
  if (tokens.length === 0) return null;
  if (tokens.length === 1) {
    return (
      <Kbd size={size} variant={variant} className={cn(className, kbdClassName)}>
        {tokens[0]}
      </Kbd>
    );
  }

  return (
    <KbdGroup className={className}>
      {tokens.map((tok, idx) => (
        <Kbd key={`${tok}-${idx}`} size={size} variant={variant} className={kbdClassName}>
          {tok}
        </Kbd>
      ))}
    </KbdGroup>
  );
}

export { kbdVariants };
