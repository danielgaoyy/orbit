import type { ReactNode } from "react";
import { Loader2, ShieldCheck } from "lucide-react";
import { Select, type SelectOption, type SelectProps } from "@/components/ui/select";
import { t, useI18n } from "@/lib/i18n";
import type { HostConnectionState } from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";

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
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel ?? ariaLabelAttr}
      disabled={disabled}
      onClick={() => {
        if (!disabled) onChange(!checked);
      }}
      className={cn(
        "relative inline-flex h-[22px] w-[38px] shrink-0 cursor-pointer items-center rounded-full border border-transparent transition-colors duration-150 disabled:cursor-not-allowed disabled:opacity-50",
        checked ? "bg-foreground" : "bg-muted-foreground/25",
      )}
    >
      <span
        className={cn(
          "pointer-events-none inline-block h-[18px] w-[18px] rounded-full shadow-xs transition-transform duration-150",
          checked ? "bg-background translate-x-[17px]" : "translate-x-[1px] bg-white",
        )}
      />
    </button>
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
          <h2 className="text-foreground text-[17.5px] font-semibold tracking-[-0.015em]">
            {title}
          </h2>
          {badge}
        </div>
        <p className="text-muted-foreground text-[13px] leading-relaxed">{description}</p>
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
      <span className="text-muted-foreground text-[12px] font-medium">{children}</span>
      {action}
    </div>
  );
}

export function SettingCard({ children }: { children: ReactNode }) {
  return (
    <div className="border-border bg-background/60 divide-border divide-y overflow-hidden rounded-[13px] border">
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
        <div className="text-foreground text-[13.5px] font-medium">{title}</div>
        {description && (
          <div className="text-muted-foreground text-[12.5px] leading-[1.45]">
            {description}
          </div>
        )}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

export function HostStatusIndicator({ state }: { state: HostConnectionState }) {
  const { t: translate } = useI18n();
  switch (state) {
    case "online_direct":
      return (
        <span
          className="h-[6px] w-[6px] shrink-0 rounded-full bg-emerald-500"
          title={translate("common.online")}
        />
      );
    case "online_relay":
      return (
        <ShieldCheck
          className="h-[12px] w-[12px] shrink-0 text-indigo-500 dark:text-indigo-400"
          aria-label={translate("common.relay")}
        />
      );
    case "connecting_direct":
    case "connecting_relay":
      return <Loader2 className="h-[11.5px] w-[11.5px] shrink-0 animate-spin text-amber-500" />;
    default:
      return (
        <span
          className="h-[6px] w-[6px] shrink-0 rounded-full bg-zinc-400/70"
          title={translate("common.offline")}
        />
      );
  }
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
