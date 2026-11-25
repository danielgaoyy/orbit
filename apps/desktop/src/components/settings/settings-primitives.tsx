import { createContext, memo, useContext, useRef, type ReactNode } from "react";
import { Dialog } from "@base-ui/react/dialog";
import { ChevronRight, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Select, type SelectOption, type SelectProps } from "@/components/ui/select";
import { HostStatusIndicator } from "@/components/ui/status-indicator";
import { Switch } from "@/components/ui/switch";
import { t } from "@/lib/i18n";
import { isLocalHostCard } from "@/lib/projects";
import type { HostCardView, HostConnectionState } from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";
import { useOrbitSelector } from "@/state/orbit-store";

export { HostStatusIndicator };

/**
 * Subscribe strictly to a host's boolean online state rather than the volatile
 * `HostCardView` object, avoiding re-renders on unrelated agent/fleet ticks.
 */
export function useHostIsOnline(hostId: string | null | undefined): boolean {
  return useOrbitSelector((s) => {
    if (!hostId) return false;
    const state = s.fleet.hosts.find((h) => h.host_id === hostId)?.connection_state;
    return state === "online_direct" || state === "online_relay";
  });
}

export const ToggleSwitch = memo(function ToggleSwitch({
  checked,
  onChange,
  disabled = false,
  ariaLabel,
  "aria-label": ariaLabelAttr,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  disabled?: boolean;
  ariaLabel?: string;
  "aria-label"?: string;
}) {
  return (
    <Switch
      checked={checked}
      disabled={disabled}
      aria-label={ariaLabel ?? ariaLabelAttr}
      onCheckedChange={(next) => {
        if (!disabled) onChange(Boolean(next));
      }}
    />
  );
});

export type SettingSelectOption<T extends string | number> = SelectOption<T>;

export function SettingSelect<T extends string | number>({
  align = "end",
  triggerClassName,
  menuClassName,
  ...props
}: SelectProps<T>) {
  return (
    <Select<T>
      align={align}
      triggerClassName={cn(
        "border-border-default bg-surface-base text-ink-primary text-[13px]",
        triggerClassName,
      )}
      menuClassName={cn(
        "settings-select-menu bg-surface-overlay border-border-default min-w-[180px]",
        menuClassName,
      )}
      {...props}
    />
  );
}

export const SettingPageHeader = memo(function SettingPageHeader({
  title,
  description,
  badge,
  action,
  breadcrumb,
  subpage = false,
}: {
  title: ReactNode;
  description?: ReactNode;
  badge?: ReactNode;
  action?: ReactNode;
  breadcrumb?: ReactNode;
  subpage?: boolean;
}) {
  const Heading = subpage || breadcrumb ? "h2" : "h1";
  return (
    <header className="flex flex-wrap items-start justify-between gap-x-4 gap-y-2 pb-1">
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-baseline gap-2">
          {breadcrumb && (
            <>
              <span className="text-ink-muted text-[13px]">{breadcrumb}</span>
              <span aria-hidden className="text-ink-faint">
                /
              </span>
            </>
          )}
          <Heading
            className={cn(
              "text-ink-primary font-semibold leading-tight tracking-[-0.025em]",
              subpage || breadcrumb ? "text-[17px]" : "text-[20px]",
            )}
          >
            {title}
          </Heading>
          {badge}
        </div>
        {description && (
          <p className="text-ink-secondary mt-1 text-[13px] leading-relaxed">{description}</p>
        )}
      </div>
      {action && <div className="flex shrink-0 items-center gap-2">{action}</div>}
    </header>
  );
});

export const SettingSectionHeader = SettingPageHeader;

export const SettingGroupLabel = memo(function SettingGroupLabel({
  children,
  action,
}: {
  children: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-2">
      <h3 className="text-ink-muted text-[11px] font-semibold tracking-[0.05em] uppercase">{children}</h3>
      {action}
    </div>
  );
});

interface SettingAnchorProps {
  settingId?: string;
}

export const SettingGroup = memo(function SettingGroup({
  title,
  action,
  children,
  settingId,
}: SettingAnchorProps & {
  title: ReactNode;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section data-setting-id={settingId} className="space-y-3">
      <SettingGroupLabel action={action}>{title}</SettingGroupLabel>
      {children}
    </section>
  );
});

export const SettingCard = memo(function SettingCard({
  children,
  className,
  settingId,
}: SettingAnchorProps & {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      data-setting-id={settingId}
      className={cn(
        "relative border-border-subtle bg-surface-raised divide-border-subtle divide-y overflow-hidden rounded-[12px] border",
        className,
      )}
    >
      {children}
    </div>
  );
});

export const SettingRow = memo(function SettingRow({
  title,
  description,
  children,
  disabled = false,
  settingId,
}: SettingAnchorProps & {
  title: ReactNode;
  description?: ReactNode;
  children: ReactNode;
  disabled?: boolean;
}) {
  return (
    <div
      data-setting-id={settingId}
      data-disabled={disabled ? "" : undefined}
      className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 px-4 py-3"
    >
      <div className={cn("min-w-[140px] flex-1 transition-opacity", disabled && "opacity-55")}>
        <div className="text-ink-primary text-[13px] font-medium">{title}</div>
        {description && (
          <div className="text-ink-secondary mt-1 text-[13px] leading-[1.45]">{description}</div>
        )}
      </div>
      <div className="max-w-full shrink-0">{children}</div>
    </div>
  );
});

export const SettingDisclosureCard = memo(function SettingDisclosureCard({
  title,
  description,
  meta,
  open,
  defaultOpen = false,
  onOpenChange,
  settingId,
  contentClassName,
  children,
}: SettingAnchorProps & {
  title: ReactNode;
  description?: ReactNode;
  meta?: ReactNode;
  open?: boolean;
  defaultOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
  contentClassName?: string;
  children: ReactNode;
}) {
  return (
    <Collapsible
      data-setting-id={settingId}
      open={open}
      defaultOpen={defaultOpen}
      onOpenChange={onOpenChange}
      className="border-border-subtle bg-surface-raised overflow-hidden rounded-[12px] border"
    >
      <CollapsibleTrigger className="group/disclosure hover:bg-surface-recessed/55 focus-visible:bg-surface-recessed/55 focus-visible:ring-ring/40 flex w-full cursor-pointer items-center justify-between gap-3 px-4 py-3 text-left transition-colors outline-none focus-visible:ring-1 focus-visible:ring-inset">
        <span className="flex min-w-0 flex-1 items-center gap-2">
          <ChevronRight
            aria-hidden
            className="text-ink-muted size-[14px] shrink-0 transition-transform duration-150 group-data-[panel-open]/disclosure:rotate-90 motion-reduce:transition-none"
          />
          <span className="min-w-0 flex-1">
            <span className="text-ink-primary block truncate text-[13px] font-medium">{title}</span>
            {description && (
              <span className="text-ink-secondary mt-1 block truncate text-[11px]">{description}</span>
            )}
          </span>
        </span>
        {meta && <span className="flex shrink-0 items-center gap-2">{meta}</span>}
      </CollapsibleTrigger>
      <CollapsibleContent>
        <div className={cn("border-border-subtle border-t", contentClassName)}>{children}</div>
      </CollapsibleContent>
    </Collapsible>
  );
});

interface ResourceRowProps {
  title: ReactNode;
  description?: ReactNode;
  icon?: ReactNode;
  status?: ReactNode;
  action?: ReactNode;
  onClick?: () => void;
  settingId?: string;
}

export const SettingResourceRow = memo(function SettingResourceRow({
  title,
  description,
  icon,
  status,
  action,
  onClick,
  settingId,
}: ResourceRowProps) {
  const body = (
    <>
      {icon && (
        <span className="text-ink-muted flex size-[18px] shrink-0 items-center justify-center [&_svg]:size-[15px] [&_svg]:stroke-[1.75]">
          {icon}
        </span>
      )}
      <span className="min-w-0 flex-1">
        <span className="text-ink-primary block break-words text-[13px] font-medium">{title}</span>
        {description && (
          <span className="text-ink-muted mt-1 block break-words text-[11px]">{description}</span>
        )}
      </span>
      {status && (
        <span className="text-ink-secondary shrink-0 font-mono text-[11px] tabular-nums">{status}</span>
      )}
      {onClick && <ChevronRight className="text-ink-faint size-[14px] shrink-0" />}
    </>
  );
  const layout = "flex min-h-[44px] min-w-0 flex-1 items-center gap-3 px-4 py-2 text-left";
  return (
    <div data-setting-id={settingId} className="flex min-w-0 items-center">
      {onClick ? (
        <button
          type="button"
          onClick={onClick}
          className={cn(
            layout,
            "cursor-pointer outline-none hover:bg-surface-recessed focus-visible:bg-surface-recessed focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring/40",
          )}
        >
          {body}
        </button>
      ) : (
        <div className={layout}>{body}</div>
      )}
      {action && <div className="flex shrink-0 items-center gap-2 pr-4">{action}</div>}
    </div>
  );
});

export function SettingNavRow(props: ResourceRowProps & { onClick: () => void }) {
  return <SettingResourceRow {...props} />;
}

export const SettingsDrawerPortalContext = createContext<HTMLElement | null>(null);

export function SettingDrawer({
  open,
  onOpenChange,
  onOpenChangeComplete,
  title,
  description,
  children,
}: {
  open: boolean;
  onOpenChange: (value: boolean) => void;
  onOpenChangeComplete?: (value: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  children: ReactNode;
}) {
  const portalContainer = useContext(SettingsDrawerPortalContext);
  const contained = Boolean(portalContainer);
  const popupRef = useRef<HTMLDivElement>(null);
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange} onOpenChangeComplete={onOpenChangeComplete}>
      <Dialog.Portal container={portalContainer ?? undefined}>
        <Dialog.Backdrop
          className={cn(
            "settings-drawer-scrim bg-ink-primary/15 inset-0 z-50",
            contained ? "absolute" : "fixed",
          )}
        />
        <Dialog.Popup
          ref={popupRef}
          data-slot="setting-drawer"
          initialFocus={() => {
            const popup = popupRef.current;
            if (!popup) return false;
            const active = document.activeElement;
            if (active instanceof HTMLElement && active !== popup && popup.contains(active)) {
              return false;
            }
            return popup;
          }}
          className={cn(
            "settings-drawer border-border-default bg-surface-overlay text-ink-primary inset-y-0 right-0 z-50 flex w-[480px] max-w-full flex-col border-l text-[13px] outline-none",
            contained ? "absolute shadow-xl" : "fixed",
          )}
        >
          <header className="border-border-subtle flex items-start gap-4 border-b p-5">
            <div className="min-w-0 flex-1">
              <Dialog.Title className="text-[17px] font-semibold">{title}</Dialog.Title>
              {description && (
                <Dialog.Description className="text-ink-secondary mt-1 text-[13px]">
                  {description}
                </Dialog.Description>
              )}
            </div>
            <Dialog.Close aria-label={t("common.close")} render={<Button size="icon" variant="ghost" />}>
              <X className="size-[16px]" />
            </Dialog.Close>
          </header>
          <div className="orbit-native-scroll min-h-0 flex-1 overflow-y-auto overscroll-contain p-5">
            {children}
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

export function settingsHostTransport(host: HostCardView): string {
  if (isLocalHostCard(host)) return t("common.local");
  if (host.connection_state.endsWith("_relay")) return t("common.relay");
  if (host.via_ssh_tunnel) return host.ssh_alias ? `SSH · ${host.ssh_alias}` : "SSH";
  if (host.connection_state.endsWith("_direct")) return t("common.direct");
  if (host.route_mode === "relay_only") return t("common.relay");
  if (host.route_mode === "ssh_tunnel") return "SSH";
  return host.route_mode === "direct_only" ? t("common.direct") : t("common.offline");
}

export function hostConnectionLabel(state: HostConnectionState): string {
  switch (state) {
    case "online_direct":
      return t("settings.onlineDirect");
    case "online_relay":
      return t("settings.onlineRelay");
    case "connecting_direct":
    case "connecting_relay":
      return t("settings.statusConnecting");
    default:
      return t("settings.statusOffline");
  }
}
