import type { ReactNode } from "react";
import { Select, type SelectOption, type SelectProps } from "@/components/ui/select";
import { HostStatusIndicator } from "@/components/ui/status-indicator";
import { Switch } from "@/components/ui/switch";
import { t } from "@/lib/i18n";
import type { HostConnectionState } from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";

export { HostStatusIndicator };

export function ToggleSwitch({
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
}

export type SettingSelectOption<T extends string | number> = SelectOption<T>;

export function SettingSelect<T extends string | number>({
  align = "end",
  menuClassName,
  ...props
}: SelectProps<T>) {
  return (
    <Select<T>
      align={align}
      menuClassName={cn("min-w-[180px]", menuClassName)}
      {...props}
    />
  );
}

export function SettingSectionHeader({
  title,
  description,
  badge,
  action,
}: {
  title: string;
  description: string;
  badge?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="flex items-start justify-between gap-4 pb-1">
      <div className="min-w-0 space-y-1">
        <div className="flex items-center gap-2">
          <h2 className="text-ink-primary text-[17px] font-semibold tracking-[-0.02em]">
            {title}
          </h2>
          {badge}
        </div>
        <p className="text-ink-secondary text-[13px] leading-relaxed">{description}</p>
      </div>
      {action && <div className="shrink-0 pt-0.5">{action}</div>}
    </div>
  );
}

export function SettingGroupLabel({
  children,
  action,
}: {
  children: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-2 px-0.5">
      <span className="text-ink-muted text-[11px] font-semibold tracking-[0.04em] uppercase">
        {children}
      </span>
      {action}
    </div>
  );
}

export function SettingCard({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "border-border-subtle bg-surface-raised divide-border-subtle divide-y overflow-hidden rounded-[12px] border",
        className,
      )}
    >
      {children}
    </div>
  );
}

export function SettingRow({
  title,
  description,
  children,
}: {
  title: ReactNode;
  description?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-3 px-4 py-3.5 sm:flex-row sm:items-center sm:justify-between">
      <div className="min-w-0 space-y-0.5 pr-3">
        <div className="text-ink-primary text-[13px] font-medium">{title}</div>
        {description && (
          <div className="text-ink-secondary text-[12px] leading-[1.45]">{description}</div>
        )}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
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
