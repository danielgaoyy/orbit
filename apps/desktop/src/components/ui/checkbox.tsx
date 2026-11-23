import { Checkbox as BaseCheckbox } from "@base-ui/react/checkbox";
import { Check } from "lucide-react";
import { cn } from "@/lib/utils";

export interface CheckboxProps {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
  id?: string;
  name?: string;
  ariaLabel?: string;
  "aria-label"?: string;
  size?: "default" | "sm";
  className?: string;
}

/**
 * Restrained monochrome Checkbox primitive built on Base UI Checkbox.
 * Replaces native browser checkbox widgets with crisp Orbit surface styling
 * and full keyboard/label accessibility.
 */
export function Checkbox({
  checked,
  onCheckedChange,
  disabled = false,
  id,
  name,
  ariaLabel,
  "aria-label": ariaLabelAttr,
  size = "default",
  className,
}: CheckboxProps) {
  return (
    <BaseCheckbox.Root
      id={id}
      name={name}
      checked={checked}
      disabled={disabled}
      aria-label={ariaLabel ?? ariaLabelAttr}
      onCheckedChange={(next) => onCheckedChange(Boolean(next))}
      data-slot="checkbox"
      className={cn(
        "border-border-strong bg-surface-base hover:border-ink-tertiary focus-visible:ring-ring/40 data-[checked]:border-action-primary-bg data-[checked]:bg-action-primary-bg data-[checked]:text-action-primary-fg inline-flex shrink-0 cursor-pointer items-center justify-center rounded-[4.5px] border transition-colors duration-100 outline-none focus-visible:ring-2 disabled:pointer-events-none disabled:opacity-45",
        size === "sm" ? "size-[14px]" : "size-[15px]",
        className,
      )}
    >
      <BaseCheckbox.Indicator className="flex items-center justify-center text-current">
        <Check
          className={cn(
            "stroke-[2.75]",
            size === "sm" ? "size-[9.5px]" : "size-[10.5px]",
          )}
        />
      </BaseCheckbox.Indicator>
    </BaseCheckbox.Root>
  );
}
