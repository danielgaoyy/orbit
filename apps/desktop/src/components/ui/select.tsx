import * as React from "react";
import { Select as BaseSelect } from "@base-ui/react/select";
import { Check, ChevronsUpDown } from "lucide-react";
import { cn } from "@/lib/utils";

export interface SelectOption<T extends string | number> {
  value: T;
  label: string;
  description?: string;
  badge?: string;
  disabled?: boolean;
  mono?: boolean;
  group?: string;
}

export interface SelectProps<T extends string | number> {
  value: T | null;
  options: readonly SelectOption<T>[];
  onChange: (value: T) => void;
  placeholder?: string;
  disabled?: boolean;
  size?: "default" | "sm" | "compact";
  align?: "start" | "center" | "end";
  ariaLabel?: string;
  "aria-label"?: string;
  triggerClassName?: string;
  menuClassName?: string;
}

function renderSelectOptionItem<T extends string | number>(opt: SelectOption<T>) {
  return (
    <BaseSelect.Item
      key={String(opt.value)}
      value={opt.value}
      disabled={opt.disabled}
      data-slot="select-item"
      className={cn(
        "data-[highlighted]:bg-accent data-[highlighted]:text-accent-foreground relative flex cursor-pointer items-center justify-between gap-3 rounded-[8px] px-2.5 py-1.5 text-[12.5px] transition-colors duration-100 outline-none select-none data-[disabled]:pointer-events-none data-[disabled]:opacity-45",
        opt.description && "items-start py-2",
      )}
    >
      <div className="min-w-0 flex-1">
        <BaseSelect.ItemText
          className={cn(
            "text-foreground/90 data-[selected]:text-foreground block truncate text-[12.5px] data-[selected]:font-medium",
            opt.mono && "font-mono text-[12px]",
          )}
        >
          {opt.label}
        </BaseSelect.ItemText>
        {opt.description && (
          <div className="text-muted-foreground mt-0.5 text-[11.5px] leading-snug">
            {opt.description}
          </div>
        )}
      </div>

      <span className="flex shrink-0 items-center gap-1.5">
        {opt.badge && (
          <span className="text-muted-foreground/65 text-[10.5px]">{opt.badge}</span>
        )}
        <BaseSelect.ItemIndicator
          className={cn(
            "text-foreground flex items-center justify-center",
            opt.description && "mt-0.5",
          )}
        >
          <Check className="h-[13.5px] w-[13.5px] shrink-0 stroke-[2]" />
        </BaseSelect.ItemIndicator>
      </span>
    </BaseSelect.Item>
  );
}

/**
 * Restrained monochrome Select primitive built on Base UI Select.
 * Supports `default`, `sm`, and `compact` toolbar sizes, optional option groups,
 * keyboard navigation, typeahead, and smooth viewport-clamped scrolling.
 */
export function Select<T extends string | number>({
  value,
  options,
  onChange,
  placeholder = "Select…",
  disabled = false,
  size = "default",
  align = "start",
  ariaLabel,
  "aria-label": ariaLabelAttr,
  triggerClassName,
  menuClassName,
}: SelectProps<T>) {
  const active = value !== null ? (options.find((opt) => opt.value === value) ?? null) : null;

  const grouped = React.useMemo(() => {
    const hasGroups = options.some((opt) => Boolean(opt.group));
    if (!hasGroups) return null;
    const order: string[] = [];
    const buckets = new Map<string, SelectOption<T>[]>();
    for (const opt of options) {
      const key = opt.group ?? "";
      let list = buckets.get(key);
      if (!list) {
        list = [];
        buckets.set(key, list);
        order.push(key);
      }
      list.push(opt);
    }
    return order.map((label) => ({ label, items: buckets.get(label) ?? [] }));
  }, [options]);

  const sizeClass =
    size === "compact"
      ? "h-[24px] min-w-0 rounded-md px-2 text-[11.5px] gap-1.5"
      : size === "sm"
        ? "h-[28px] min-w-[78px] rounded-lg px-2 text-[12px] gap-1.5"
        : "h-[30px] min-w-[144px] rounded-lg px-2.5 text-[12.5px] gap-2";

  return (
    <BaseSelect.Root<T>
      value={value}
      disabled={disabled}
      modal={false}
      onValueChange={(next) => {
        if (next !== null && next !== value) {
          onChange(next);
        }
      }}
    >
      <BaseSelect.Trigger
        aria-label={ariaLabel ?? ariaLabelAttr}
        data-slot="select-trigger"
        className={cn(
          "border-border bg-background text-foreground hover:bg-accent/70 focus-visible:ring-foreground/20 flex cursor-pointer items-center justify-between border font-medium transition-colors outline-none focus-visible:ring-1 disabled:pointer-events-none disabled:opacity-45",
          sizeClass,
          triggerClassName,
        )}
      >
        <span
          className={cn(
            "truncate",
            active?.mono && (size === "compact" ? "font-mono text-[11px]" : "font-mono text-[12px]"),
            !active && "text-muted-foreground font-normal",
          )}
        >
          {active ? active.label : placeholder}
        </span>
        <BaseSelect.Icon className="flex shrink-0 items-center">
          <ChevronsUpDown
            className={cn(
              "text-muted-foreground/75 shrink-0",
              size === "compact" ? "h-[11.5px] w-[11.5px]" : "h-[13px] w-[13px]",
            )}
          />
        </BaseSelect.Icon>
      </BaseSelect.Trigger>

      <BaseSelect.Portal>
        <BaseSelect.Positioner
          side="bottom"
          align={align}
          sideOffset={5}
          collisionPadding={8}
          alignItemWithTrigger={false}
          className="z-[60] outline-none"
        >
          <BaseSelect.Popup
            data-slot="select-content"
            className={cn(
              "bg-popover text-popover-foreground floating-surface orbit-floating-content min-w-[148px] overflow-hidden rounded-[13px] border border-black/[0.09] p-1 outline-none dark:border-white/[0.10]",
              menuClassName,
            )}
          >
            <BaseSelect.List className="max-h-[min(16rem,var(--available-height))] overflow-y-auto outline-none">
              {grouped
                ? grouped.map((section, idx) => (
                    <BaseSelect.Group key={section.label || `group-${idx}`}>
                      {idx > 0 && (
                        <BaseSelect.Separator className="bg-border -mx-1 my-1 h-px" />
                      )}
                      {section.label && (
                        <BaseSelect.GroupLabel className="text-muted-foreground/80 px-2.5 py-1 text-[11px] font-medium select-none">
                          {section.label}
                        </BaseSelect.GroupLabel>
                      )}
                      {section.items.map((opt) => renderSelectOptionItem(opt))}
                    </BaseSelect.Group>
                  ))
                : options.map((opt) => renderSelectOptionItem(opt))}
            </BaseSelect.List>
          </BaseSelect.Popup>
        </BaseSelect.Positioner>
      </BaseSelect.Portal>
    </BaseSelect.Root>
  );
}
