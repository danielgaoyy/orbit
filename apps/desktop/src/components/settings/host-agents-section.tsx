import { useState } from "react";
import { Check, RefreshCw } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
} from "@/components/settings/settings-primitives";
import { type ProviderKind, orbitIpc } from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import { ProviderIcon } from "@/components/ui/provider-icons";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const PROVIDERS: { id: ProviderKind; label: string; bin: string }[] = [
  { id: "claude_code", label: "Claude Code", bin: "claude" },
  { id: "codex", label: "Codex", bin: "codex" },
  { id: "antigravity", label: "Antigravity", bin: "agy" },
];

export function HostAgentsSection({ hostId }: { hostId: string }) {
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostCard = fleet.hosts.find((h) => h.host_id === hostId) ?? null;
  const hostDetail = useOrbitSelector((s) => s.hostDetails[hostId] ?? null);
  const draftProvider = useOrbitSelector((s) => s.draftProvider);

  const [refreshingModels, setRefreshingModels] = useState(false);

  const showHostBadge =
    Boolean(hostCard) &&
    (fleet.hosts.length > 1 || (hostCard !== null && !isLocalHostCard(hostCard)));

  const available =
    hostDetail?.host.available_providers ?? hostCard?.available_providers ?? [];
  const catalogs = hostDetail?.provider_catalogs ?? [];

  const handleRefreshModels = async () => {
    setRefreshingModels(true);
    try {
      await orbitIpc.refreshProviderModels(hostId, true);
      orbitActions.showToast("Refreshed model catalog");
    } finally {
      window.setTimeout(() => setRefreshingModels(false), 450);
    }
  };

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title="Agents"
        description="Installed coding agents and available models on this host."
        badge={
          showHostBadge && hostCard ? (
            <span className="bg-muted text-muted-foreground rounded-md px-2 py-0.5 text-[11.5px] font-medium">
              {hostCard.display_name}
            </span>
          ) : null
        }
        action={
          <button
            type="button"
            disabled={refreshingModels}
            onClick={() => void handleRefreshModels()}
            className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:opacity-50"
          >
            <RefreshCw
              className={cn("h-[13px] w-[13px]", refreshingModels && "animate-spin")}
            />
            <span>Refresh Models</span>
          </button>
        }
      />

      {/* Installed CLIs on this Host */}
      <div className="space-y-2">
        <SettingGroupLabel>Installed CLIs</SettingGroupLabel>
        <SettingCard>
          {PROVIDERS.map((p) => {
            const detected = available.includes(p.id);
            const catalog = catalogs.find((c) => c.provider === p.id);
            const modelCount = catalog?.models.length ?? 0;
            const isDefault = detected && draftProvider === p.id;

            return (
              <SettingRow
                key={p.id}
                title={
                  <span className="flex items-center gap-2">
                    <ProviderIcon provider={p.id} className="size-4" />
                    <span>{p.label}</span>
                  </span>
                }
                description={
                  detected
                    ? `${p.bin}${modelCount > 0 ? ` · ${modelCount} models` : ""}`
                    : `${p.bin} not found on PATH`
                }
              >
                <div className="flex items-center gap-2">
                  {detected &&
                    (isDefault ? (
                      <span className="text-muted-foreground inline-flex h-[26px] items-center gap-1 rounded-md px-2 text-[11.5px] font-medium">
                        <Check className="h-[12.5px] w-[12.5px] stroke-[2]" />
                        <span>Default</span>
                      </span>
                    ) : (
                      <button
                        type="button"
                        onClick={() => orbitActions.setDraftConfig({ provider: p.id })}
                        className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[26px] cursor-pointer items-center rounded-md px-2 text-[11.5px] font-medium transition-colors"
                      >
                        Set default
                      </button>
                    ))}

                  <span
                    className={cn(
                      "inline-flex h-[24px] items-center gap-1.5 rounded-md border px-2 text-[11.5px] font-medium",
                      detected
                        ? "border-emerald-500/25 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
                        : "border-border text-muted-foreground",
                    )}
                  >
                    <span
                      className={cn(
                        "h-[5.5px] w-[5.5px] rounded-full",
                        detected ? "bg-emerald-500" : "bg-zinc-400",
                      )}
                    />
                    {detected ? "Ready" : "Not installed"}
                  </span>
                </div>
              </SettingRow>
            );
          })}
        </SettingCard>
      </div>
    </div>
  );
}
