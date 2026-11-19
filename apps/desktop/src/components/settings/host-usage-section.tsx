import { useEffect, useRef, useState } from "react";
import { Loader2, RefreshCw } from "lucide-react";
import {
  SettingCard,
  SettingSectionHeader,
} from "@/components/settings/settings-primitives";
import { orbitIpc } from "@/lib/orbit-ipc";
import { ProviderIcon } from "@/components/ui/provider-icons";
import { useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

function formatProviderName(provider: string): string {
  switch (provider) {
    case "claude_code":
      return "Claude Code";
    case "codex":
      return "Codex";
    case "antigravity":
      return "Antigravity";
    default:
      return provider.replace("_", " ");
  }
}

function formatResetTime(resetsAt: string | null): string | null {
  if (!resetsAt) return null;
  const ts = Date.parse(resetsAt);
  if (Number.isNaN(ts)) return resetsAt;
  const diffMin = Math.round((ts - Date.now()) / 60_000);
  if (diffMin <= 0) return "Resets soon";
  if (diffMin < 60) return `Resets in ${diffMin}m`;
  const hours = Math.floor(diffMin / 60);
  const mins = diffMin % 60;
  if (hours < 24) {
    return mins > 0 ? `Resets in ${hours}h ${mins}m` : `Resets in ${hours}h`;
  }
  const days = Math.floor(hours / 24);
  return `Resets in ${days}d`;
}

export function HostUsageSection({ hostId }: { hostId: string }) {
  const hostCard = useOrbitSelector(
    (s) => s.fleet.hosts.find((h) => h.host_id === hostId) ?? null,
  );
  const hostDetail = useOrbitSelector((s) => s.hostDetails[hostId] ?? null);
  const [refreshing, setRefreshing] = useState(false);
  const autoFetchedHostRef = useRef<string | null>(null);

  const quotas = hostDetail?.quotas ?? [];

  useEffect(() => {
    if (quotas.length > 0 || autoFetchedHostRef.current === hostId) return;
    autoFetchedHostRef.current = hostId;
    setRefreshing(true);
    void orbitIpc.refreshQuotas(hostId).finally(() => {
      window.setTimeout(() => setRefreshing(false), 450);
    });
  }, [hostId, quotas.length]);

  const handleRefresh = async () => {
    setRefreshing(true);
    try {
      await orbitIpc.refreshQuotas(hostId);
    } finally {
      window.setTimeout(() => setRefreshing(false), 450);
    }
  };

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title="Usage"
        description="Live subscription limits and remaining credits."
        badge={
          hostCard ? (
            <span className="bg-muted text-muted-foreground rounded-md px-2 py-0.5 text-[11.5px] font-medium">
              {hostCard.display_name}
            </span>
          ) : null
        }
        action={
          <button
            type="button"
            disabled={refreshing}
            onClick={() => void handleRefresh()}
            className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:opacity-50"
          >
            <RefreshCw className={cn("h-[13px] w-[13px]", refreshing && "animate-spin")} />
            <span>Refresh</span>
          </button>
        }
      />

      <SettingCard>
        {quotas.length === 0 ? (
          <div className="text-muted-foreground flex flex-col items-center gap-2.5 py-10 text-center text-[13px]">
            {refreshing ? (
              <span className="inline-flex items-center gap-2">
                <Loader2 className="h-[14px] w-[14px] animate-spin" />
                <span>Checking usage…</span>
              </span>
            ) : (
              <>
                <span>No usage snapshots available.</span>
                <button
                  type="button"
                  onClick={() => void handleRefresh()}
                  className="border-border text-foreground hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-3 text-[12px] font-medium transition-colors"
                >
                  <RefreshCw className="h-[13px] w-[13px]" />
                  <span>Check Usage</span>
                </button>
              </>
            )}
          </div>
        ) : (
          quotas.map((q) => (
            <div key={q.provider} className="space-y-3 px-4 py-4">
              <div className="flex items-center justify-between gap-2">
                <div className="flex min-w-0 items-center gap-2">
                  <ProviderIcon provider={q.provider} className="h-[15px] w-[15px]" />
                  <span className="text-foreground text-[13.5px] font-medium">
                    {formatProviderName(q.provider)}
                  </span>
                  {q.account_label && (
                    <span className="text-muted-foreground truncate text-[12px]">
                      {q.account_label}
                    </span>
                  )}
                </div>
                {q.credits_remaining !== null && (
                  <span className="tabular-nums shrink-0 font-mono text-[12px] text-emerald-600 dark:text-emerald-400">
                    {q.credits_remaining.toFixed(2)} credits
                  </span>
                )}
              </div>

              {q.error ? (
                <div className="selectable-text text-muted-foreground text-[12.5px]">
                  {q.error}
                </div>
              ) : (
                <div className="space-y-2.5">
                  {q.windows.map((w) => {
                    const resetLabel = formatResetTime(w.resets_at);
                    return (
                      <div key={w.label} className="space-y-1">
                        <div className="text-muted-foreground tabular-nums flex items-center justify-between gap-2 text-[12px]">
                          <span className="truncate">{w.label}</span>
                          <span className="flex shrink-0 items-center gap-2">
                            {resetLabel && (
                              <span className="text-muted-foreground/70 text-[11.5px]">
                                {resetLabel}
                              </span>
                            )}
                            <span className="text-foreground/85 font-medium">
                              {Math.round(w.used_percent)}%
                            </span>
                          </span>
                        </div>
                        <div className="bg-muted h-1.5 w-full overflow-hidden rounded-full">
                          <div
                            className={cn(
                              "h-full transition-all duration-300",
                              w.used_percent >= 85
                                ? "bg-rose-500"
                                : w.used_percent >= 70
                                  ? "bg-amber-500"
                                  : "bg-foreground/75",
                            )}
                            style={{ width: `${Math.min(100, Math.max(0, w.used_percent))}%` }}
                          />
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          ))
        )}
      </SettingCard>
    </div>
  );
}
