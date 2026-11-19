import type { ReactNode } from "react";
import { Check, ChevronsUpDown, Loader2, ShieldCheck } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/overlays";
import type { HostCardView, HostConnectionState } from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";

export function encodeHostPairingUri(host: HostCardView): string {
  const offer = {
    host_id: host.host_id,
    hostname: host.display_name,
    daemon_pubkey_b64: host.daemon_pubkey_b64,
    direct_endpoints: host.direct_endpoints,
    relay_url: host.relay_url,
    pairing_secret: "orbit-desktop-export",
  };
  const bytes = new TextEncoder().encode(JSON.stringify(offer));
  let bin = "";
  bytes.forEach((b) => {
    bin += String.fromCharCode(b);
  });
  const b64Url = btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  return `orbit://pair/${b64Url}`;
}

export function ToggleSwitch({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative inline-flex h-[22px] w-[38px] shrink-0 cursor-pointer items-center rounded-full border border-transparent transition-colors duration-150",
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

export interface SettingSelectOption<T extends string | number> {
  value: T;
  label: string;
  description?: string;
  badge?: string;
  disabled?: boolean;
  mono?: boolean;
}

export function SettingSelect<T extends string | number>({
  value,
  options,
  onChange,
  placeholder = "Select…",
  disabled = false,
  triggerClassName,
  menuClassName,
}: {
  value: T | null;
  options: SettingSelectOption<T>[];
  onChange: (value: T) => void;
  placeholder?: string;
  disabled?: boolean;
  triggerClassName?: string;
  menuClassName?: string;
}) {
  const active = value !== null ? (options.find((opt) => opt.value === value) ?? null) : null;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild disabled={disabled}>
        <button
          type="button"
          disabled={disabled}
          className={cn(
            "border-border bg-background text-foreground hover:bg-accent/70 focus-visible:ring-foreground/20 inline-flex h-[30px] min-w-[144px] cursor-pointer items-center justify-between gap-2 rounded-lg border px-2.5 text-[12.5px] font-medium transition-colors outline-none focus-visible:ring-1 disabled:pointer-events-none disabled:opacity-45",
            triggerClassName,
          )}
        >
          <span
            className={cn(
              "truncate",
              active?.mono && "font-mono text-[12px]",
              !active && "text-muted-foreground font-normal",
            )}
          >
            {active ? active.label : placeholder}
          </span>
          <ChevronsUpDown className="text-muted-foreground/75 h-[13px] w-[13px] shrink-0" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="end"
        sideOffset={5}
        className={cn("min-w-[180px]", menuClassName)}
      >
        {options.map((opt) => {
          const selected = opt.value === value;
          return (
            <DropdownMenuItem
              key={String(opt.value)}
              disabled={opt.disabled}
              onSelect={() => {
                if (!opt.disabled && !selected) {
                  onChange(opt.value);
                }
              }}
              className={cn(opt.description && "items-start py-2")}
            >
              <div className="min-w-0 flex-1">
                <div
                  className={cn(
                    "truncate text-[12.5px]",
                    selected ? "text-foreground font-medium" : "text-foreground/90 font-normal",
                    opt.mono && "font-mono text-[12px]",
                  )}
                >
                  {opt.label}
                </div>
                {opt.description && (
                  <div className="text-muted-foreground mt-0.5 text-[11.5px] leading-snug">
                    {opt.description}
                  </div>
                )}
              </div>
              {selected ? (
                <Check
                  className={cn(
                    "text-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[2]",
                    opt.description && "mt-0.5",
                  )}
                />
              ) : opt.badge ? (
                <span className="text-muted-foreground/65 shrink-0 text-[10.5px]">
                  {opt.badge}
                </span>
              ) : null}
            </DropdownMenuItem>
          );
        })}
      </DropdownMenuContent>
    </DropdownMenu>
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
  switch (state) {
    case "online_direct":
      return (
        <span
          className="h-[6px] w-[6px] shrink-0 rounded-full bg-emerald-500"
          title="Online"
        />
      );
    case "online_relay":
      return (
        <ShieldCheck
          className="h-[12px] w-[12px] shrink-0 text-indigo-500 dark:text-indigo-400"
          aria-label="Relay"
        />
      );
    case "connecting_direct":
    case "connecting_relay":
      return <Loader2 className="h-[11.5px] w-[11.5px] shrink-0 animate-spin text-amber-500" />;
    default:
      return (
        <span
          className="h-[6px] w-[6px] shrink-0 rounded-full bg-zinc-400/70"
          title="Offline"
        />
      );
  }
}

export function hostConnectionLabel(state: HostConnectionState): string {
  switch (state) {
    case "online_direct":
      return "Online · Direct";
    case "online_relay":
      return "Online · E2EE Relay";
    case "connecting_direct":
    case "connecting_relay":
      return "Connecting";
    default:
      return "Offline";
  }
}
