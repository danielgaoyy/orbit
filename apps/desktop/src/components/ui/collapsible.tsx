import * as React from "react";
import { Collapsible as BaseCollapsible } from "@base-ui/react/collapsible";
import { cn } from "@/lib/utils";

function Collapsible(props: React.ComponentProps<typeof BaseCollapsible.Root>) {
  return <BaseCollapsible.Root data-slot="collapsible" {...props} />;
}

function CollapsibleTrigger(props: React.ComponentProps<typeof BaseCollapsible.Trigger>) {
  return <BaseCollapsible.Trigger data-slot="collapsible-trigger" {...props} />;
}

function CollapsibleContent({
  className,
  ...props
}: React.ComponentProps<typeof BaseCollapsible.Panel>) {
  return (
    <BaseCollapsible.Panel
      data-slot="collapsible-content"
      className={cn(
        "h-[var(--collapsible-panel-height)] overflow-hidden transition-[height,opacity] duration-[180ms] ease-[var(--ease-panel)] data-[starting-style]:h-0 data-[starting-style]:opacity-0 data-[ending-style]:h-0 data-[ending-style]:opacity-0 motion-reduce:transition-none [&[hidden]:not([hidden='until-found'])]:hidden",
        className,
      )}
      {...props}
    />
  );
}

export { Collapsible, CollapsibleTrigger, CollapsibleContent };
