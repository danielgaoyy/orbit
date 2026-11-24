import { Tooltip as BaseTooltip } from "@base-ui/react/tooltip";
import { cn } from "@/lib/utils";

export { SimpleTooltip, TooltipProvider } from "./overlays";

export const Tooltip = BaseTooltip.Root;
export const TooltipTrigger = BaseTooltip.Trigger;

export function TooltipContent({
  side = "top",
  sideOffset = 6,
  align = "center",
  alignOffset = 0,
  className,
  ...props
}: BaseTooltip.Popup.Props &
  Pick<BaseTooltip.Positioner.Props, "side" | "sideOffset" | "align" | "alignOffset">) {
  return (
    <BaseTooltip.Portal>
      <BaseTooltip.Positioner
        side={side}
        sideOffset={sideOffset}
        align={align}
        alignOffset={alignOffset}
        className="z-50"
      >
        <BaseTooltip.Popup
          data-slot="tooltip-content"
          className={(state) =>
            cn(
              "text-ink-primary floating-surface orbit-tooltip-content flex max-w-xs items-center gap-1.5 rounded-lg border px-2.5 py-1 text-[12px] select-none",
              typeof className === "function" ? className(state) : className,
            )
          }
          {...props}
        />
      </BaseTooltip.Positioner>
    </BaseTooltip.Portal>
  );
}
