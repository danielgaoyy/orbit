import * as React from "react";
import { useRender } from "@base-ui/react/use-render";
import { cn } from "@/lib/utils";

import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { ChevronRightIcon } from "lucide-react";

function Trace({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="trace"
      className={cn(
        "flex w-full min-w-0 flex-col gap-2 *:has-[>[data-slot=trace-line]]:flex *:has-[>[data-slot=trace-line]]:w-fit *:has-[>[data-slot=trace-line]]:max-w-full *:has-[>[data-slot=trace-line]]:min-w-0 *:has-[>[data-slot=trace-line]]:rounded-md *:has-[>[data-slot=trace-line]]:text-left *:has-[>[data-slot=trace-line]]:outline-none *:has-[>[data-slot=trace-line]]:focus-visible:ring-1 *:has-[>[data-slot=trace-line]]:focus-visible:ring-ring/45",
        className,
      )}
      {...props}
    />
  );
}

function TraceLine({
  render,
  className,
  status,
  ...props
}: useRender.ComponentProps<"span"> & {
  status?: "running" | "error" | "pending";
}) {
  return useRender({
    defaultTagName: "span",
    render,
    state: { slot: "trace-line", status },
    props: {
      ...props,
      className: cn(
        "flex min-h-[24px] w-fit max-w-full min-w-0 items-center gap-2 text-[13px] leading-[18px] text-muted-foreground transition-colors in-[a,button]:cursor-pointer in-[a:hover,button:hover]:text-foreground data-[status=error]:text-destructive data-[status=error]:in-[a:hover,button:hover]:text-destructive data-[status=pending]:text-ink-muted data-[status=running]:*:data-[slot=trace-title]:shimmer",
        className,
      ),
    },
  });
}

function TraceIcon({ className, ...props }: React.ComponentProps<"span">) {
  return (
    <span
      data-slot="trace-icon"
      aria-hidden="true"
      className={cn(
        "flex size-4 shrink-0 items-center justify-center [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 [[data-slot=trace-title]~&]:-ml-1",
        className,
      )}
      {...props}
    />
  );
}

function TraceTitle({ render, className, ...props }: useRender.ComponentProps<"span">) {
  return useRender({
    defaultTagName: "span",
    render,
    state: { slot: "trace-title" },
    props: { ...props, className: cn("min-w-0 truncate", className) },
  });
}

function TraceDescription({ className, ...props }: React.ComponentProps<"span">) {
  return (
    <span
      data-slot="trace-description"
      className={cn("shrink-0 font-mono text-[13px] leading-[18px] text-ink-muted tabular-nums", className)}
      {...props}
    />
  );
}

function TraceCollapsible({
  className,
  ...props
}: Omit<React.ComponentProps<typeof Collapsible>, "open"> &
  (
    | { open?: undefined }
    | {
        open: boolean;
        onOpenChange: NonNullable<React.ComponentProps<typeof Collapsible>["onOpenChange"]>;
      }
  )) {
  return (
    <Collapsible
      data-slot="trace-collapsible"
      className={cn("flex w-full min-w-0 flex-col", className)}
      {...props}
    />
  );
}

function TraceCollapsibleTrigger({
  className,
  children,
  ...props
}: React.ComponentProps<typeof CollapsibleTrigger>) {
  return (
    <CollapsibleTrigger
      data-slot="trace-collapsible-trigger"
      className={cn(
        "group/trace-collapsible-trigger flex w-fit max-w-full min-w-0 cursor-pointer items-center gap-1 rounded-md text-left text-[13px] leading-[18px] text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring/45 has-[[data-status=error]]:text-destructive",
        className,
      )}
      {...props}
    >
      {children}
      <TraceIcon className="motion-safe:transition-transform motion-safe:duration-[180ms] motion-safe:ease-[var(--ease-panel)] group-aria-expanded/trace-collapsible-trigger:rotate-90">
        <ChevronRightIcon />
      </TraceIcon>
    </CollapsibleTrigger>
  );
}

function TraceCollapsibleContent({
  className,
  children,
  ...props
}: React.ComponentProps<typeof CollapsibleContent>) {
  return (
    <CollapsibleContent
      data-slot="trace-collapsible-content"
      className="group/trace-panel min-w-0 duration-[200ms] ease-[var(--ease-panel)] data-[ending-style]:duration-[150ms] data-[ending-style]:ease-[var(--ease-exit)]"
      {...props}
    >
      <div className="min-w-0 pt-2 pb-1">
        <div
          data-slot="trace-collapsible-content-inner"
          className={cn(
            "ml-[calc(--spacing(2)-1px)] min-w-0 border-l pr-1 pl-4 text-[13px] leading-[1.5] wrap-break-word text-muted-foreground transition-transform duration-[200ms] ease-[var(--ease-panel)] group-data-[starting-style]/trace-panel:-translate-y-[4px] group-data-[ending-style]/trace-panel:-translate-y-[3px] group-data-[ending-style]/trace-panel:duration-[150ms] group-data-[ending-style]/trace-panel:ease-[var(--ease-exit)] motion-reduce:transition-none",
            className,
          )}
        >
          {children}
        </div>
      </div>
    </CollapsibleContent>
  );
}

export {
  Trace,
  TraceLine,
  TraceIcon,
  TraceTitle,
  TraceDescription,
  TraceCollapsible,
  TraceCollapsibleTrigger,
  TraceCollapsibleContent,
};
