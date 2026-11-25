import { useCallback, useRef, type KeyboardEvent, type ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface SegmentedOption<T extends string> {
  value: T;
  label: ReactNode;
  icon?: ReactNode;
  disabled?: boolean;
  title?: string;
}

export interface SegmentedProps<T extends string> {
  value: T;
  onValueChange: (value: T) => void;
  options: readonly SegmentedOption<T>[];
  size?: "sm" | "md";
  fullWidth?: boolean;
  disabled?: boolean;
  ariaLabel?: string;
  "aria-label"?: string;
  "aria-labelledby"?: string;
  className?: string;
}

export function Segmented<T extends string>({
  value,
  onValueChange,
  options,
  size = "md",
  fullWidth = false,
  disabled = false,
  ariaLabel,
  "aria-label": ariaLabelAttr,
  "aria-labelledby": ariaLabelledBy,
  className,
}: SegmentedProps<T>) {
  const buttonRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const activeIndex = options.findIndex((opt) => opt.value === value);
  const firstEnabledIndex = options.findIndex((opt) => !disabled && !opt.disabled);
  const focusableIndex =
    activeIndex >= 0 && !disabled && !options[activeIndex]?.disabled
      ? activeIndex
      : firstEnabledIndex;

  const moveFocusAndSelect = useCallback(
    (startIndex: number, step: 1 | -1) => {
      const count = options.length;
      if (count === 0) return;
      for (let offset = 1; offset <= count; offset += 1) {
        const nextIndex = (startIndex + step * offset + count) % count;
        const opt = options[nextIndex];
        if (!opt || disabled || opt.disabled) continue;
        buttonRefs.current[nextIndex]?.focus();
        if (opt.value !== value) onValueChange(opt.value);
        return;
      }
    },
    [disabled, onValueChange, options, value],
  );

  const handleKeyDown = useCallback(
    (index: number, event: KeyboardEvent<HTMLButtonElement>) => {
      if (event.key === "ArrowRight" || event.key === "ArrowDown") {
        event.preventDefault();
        moveFocusAndSelect(index, 1);
      } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
        event.preventDefault();
        moveFocusAndSelect(index, -1);
      } else if (event.key === "Home") {
        event.preventDefault();
        moveFocusAndSelect(-1, 1);
      } else if (event.key === "End") {
        event.preventDefault();
        moveFocusAndSelect(options.length, -1);
      }
    },
    [moveFocusAndSelect, options.length],
  );

  return (
    <div
      role="radiogroup"
      data-slot="segmented"
      data-size={size}
      aria-label={ariaLabel ?? ariaLabelAttr}
      aria-labelledby={ariaLabelledBy}
      aria-disabled={disabled || undefined}
      style={
        fullWidth
          ? { gridTemplateColumns: `repeat(${Math.max(1, options.length)}, minmax(0, 1fr))` }
          : undefined
      }
      className={cn(
        "border-border-subtle bg-surface-recessed items-center gap-[2px] rounded-lg border p-[2px]",
        fullWidth ? "grid w-full" : "inline-flex",
        className,
      )}
    >
      {options.map((option, index) => {
        const selected = option.value === value;
        const itemDisabled = disabled || Boolean(option.disabled);
        return (
          <button
            key={option.value}
            ref={(node) => {
              buttonRefs.current[index] = node;
            }}
            type="button"
            role="radio"
            data-slot="segmented-item"
            data-checked={selected ? "" : undefined}
            aria-checked={selected}
            tabIndex={index === focusableIndex ? 0 : -1}
            disabled={itemDisabled}
            title={option.title}
            onClick={() => {
              if (!itemDisabled && option.value !== value) {
                onValueChange(option.value);
              }
            }}
            onKeyDown={(event) => handleKeyDown(index, event)}
            className={cn(
              "inline-flex shrink-0 cursor-pointer items-center justify-center gap-1.5 whitespace-nowrap rounded-md font-medium outline-none",
              "transition-[background-color,color,box-shadow] duration-[var(--dur-ui)] ease-[var(--ease-out-quart)] motion-reduce:transition-none",
              "focus-visible:ring-1 focus-visible:ring-ring/45",
              "disabled:pointer-events-none disabled:opacity-45",
              size === "sm" ? "h-[24px] px-2 text-[11px]" : "h-[28px] px-2.5 text-[13px]",
              fullWidth && "min-w-0",
              selected
                ? "bg-surface-overlay text-ink-primary shadow-2xs"
                : "text-ink-tertiary hover:text-ink-primary",
            )}
          >
            {option.icon && <span className="inline-flex shrink-0 items-center">{option.icon}</span>}
            <span className="truncate">{option.label}</span>
          </button>
        );
      })}
    </div>
  );
}
