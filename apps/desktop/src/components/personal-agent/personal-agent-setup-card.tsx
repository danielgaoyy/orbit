import { memo, useCallback, useEffect, useMemo, useState } from "react";
import { Bot, Server, Settings2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useI18n } from "@/lib/i18n";
import { type RouterHostCapabilities, orbitIpc } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { selectPersonalAgentRef } from "@/state/selectors/personal-agent-selectors";
import {
  buildDefaultRouterConfig,
  selectRecommendedRouterHost,
} from "./personal-agent-briefing-model";
import { refreshPersonalAgentBriefing } from "./use-personal-agent-briefing";

/**
 * Unconfigured Personal Agent home screen:
 * Centered recommendation card picking the best fleet host from `discoverFleetRouterHosts()`,
 * with 1-click activation (`configurePersonalRouter` + `setPrimaryRouterSelection`) and
 * a secondary link to `openSettings("personal_router")`.
 */
export const PersonalAgentSetupCard = memo(function PersonalAgentSetupCard() {
  const { t } = useI18n();
  const fleetHosts = useOrbitSelector((s) => s.fleet.hosts);
  const existingPa = useOrbitSelector(selectPersonalAgentRef);

  const [candidates, setCandidates] = useState<RouterHostCapabilities[]>([]);
  const [discovering, setDiscovering] = useState(true);
  const [activating, setActivating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setDiscovering(true);
    void orbitIpc
      .discoverFleetRouterHosts()
      .then((hosts) => {
        if (active) setCandidates(Array.isArray(hosts) ? hosts : []);
      })
      .catch((err) => {
        if (active) setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (active) setDiscovering(false);
      });
    return () => {
      active = false;
    };
  }, [fleetHosts.length]);

  // If a Personal Agent becomes available while on the setup view, seamlessly open it.
  useEffect(() => {
    if (existingPa) {
      orbitActions.selectAgent(existingPa.hostId, existingPa.agentId);
    }
  }, [existingPa]);

  const recommended = useMemo(() => selectRecommendedRouterHost(candidates), [candidates]);

  const capabilitySummary = useMemo(() => {
    if (!recommended) return "";
    const parts: string[] = [];
    if (recommended.recommendation_reason?.trim()) {
      parts.push(recommended.recommendation_reason.trim());
    } else {
      // The daemon's `recommendation_reason` already lists Hermes detection; only add
      // client-side tags when the daemon did not supply a reason.
      parts.push(
        recommended.is_local
          ? t("briefing.setupTagLocal")
          : t("briefing.setupTagAlwaysOn"),
      );
      if (recommended.hermes_installed) {
        parts.push(t("briefing.setupTagHermes"));
      }
    }
    return parts.join(" · ");
  }, [recommended, t]);

  const handleOneClickEnable = useCallback(async () => {
    if (!recommended || !recommended.online || activating) return;
    setActivating(true);
    setError(null);
    try {
      const existingSnap = await orbitIpc
        .getPersonalRouterStatus(recommended.host_id)
        .catch(() => null);
      const nextCfg = buildDefaultRouterConfig(recommended, existingSnap?.config ?? null);
      const snap = await orbitIpc.configurePersonalRouter(recommended.host_id, nextCfg, true);
      if (typeof orbitIpc.setPrimaryRouterSelection === "function") {
        await orbitIpc.setPrimaryRouterSelection({
          host_id: recommended.host_id,
          agent_id: snap.router_agent?.agent_id ?? recommended.active_router_agent_id ?? null,
          updated_at_ms: Date.now(),
        });
      }
      orbitActions.setPrimaryRouterHostId(recommended.host_id);
      await orbitActions.reloadHostDetail(recommended.host_id, true);
      refreshPersonalAgentBriefing({ includeActivity: true });
      const targetAgentId =
        snap.router_agent?.agent_id ?? recommended.active_router_agent_id ?? null;
      if (targetAgentId) {
        orbitActions.selectAgent(recommended.host_id, targetAgentId);
      } else {
        orbitActions.openGlobalOverview();
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setActivating(false);
    }
  }, [recommended, activating]);

  return (
    <div className="bg-surface-base orbit-stage-enter flex min-h-0 flex-1 items-center justify-center px-6 py-12">
      <Card className="border-border-default bg-surface-raised w-full max-w-[440px] gap-5 rounded-xl p-6 shadow-xs">
        <CardHeader className="gap-2.5">
          <div className="bg-surface-recessed border-border-subtle text-ink-secondary inline-flex size-9 items-center justify-center rounded-xl border">
            <Bot className="size-[17px] stroke-[1.75]" />
          </div>
          <div className="flex flex-col gap-1">
            <CardTitle className="text-[15px] font-medium tracking-[-0.01em]">
              {t("briefing.setupTitle")}
            </CardTitle>
            <CardDescription className="text-ink-secondary text-[13px] leading-[1.5]">
              {t("briefing.setupDescription")}
            </CardDescription>
          </div>
        </CardHeader>

        <CardContent className="flex flex-col gap-3">
          {discovering ? (
            <div className="bg-surface-recessed border-border-subtle text-ink-tertiary flex items-center gap-2 rounded-xl border px-3.5 py-3 text-[13px]">
              <OrbitSpinner size={13} />
              <span>{t("briefing.setupDiscovering")}</span>
            </div>
          ) : recommended ? (
            <div className="bg-surface-recessed border-border-subtle flex items-center gap-3 rounded-xl border px-3.5 py-3">
              <div className="bg-surface-raised border-border-subtle text-ink-secondary inline-flex size-8 shrink-0 items-center justify-center rounded-[8px] border">
                <Server className="size-[15px] stroke-[1.75]" />
              </div>
              <div className="min-w-0 flex-1">
                <div className="text-ink-primary truncate text-[13px] font-medium">
                  {t("briefing.setupRecommendedHost", { host: recommended.host_name })}
                </div>
                {capabilitySummary && (
                  <div className="text-ink-tertiary mt-0.5 truncate text-[11px]">
                    {capabilitySummary}
                  </div>
                )}
              </div>
              <span
                aria-hidden="true"
                className={
                  recommended.online
                    ? "bg-signal-success size-[7px] shrink-0 rounded-full"
                    : "bg-ink-muted/50 size-[7px] shrink-0 rounded-full"
                }
              />
            </div>
          ) : (
            <div className="bg-surface-recessed border-border-subtle text-ink-tertiary rounded-xl border px-3.5 py-3 text-[13px]">
              {t("briefing.setupNoHosts")}
            </div>
          )}

          {error && (
            <div
              role="alert"
              className="border-signal-danger-border bg-signal-danger-bg text-signal-danger rounded-[8px] border px-3 py-2 text-[13px]"
            >
              {error}
            </div>
          )}
        </CardContent>

        <CardFooter className="flex-col items-stretch gap-2">
          <Button
            type="button"
            disabled={discovering || activating || !recommended || !recommended.online}
            onClick={() => void handleOneClickEnable()}
            className="w-full"
          >
            {activating && <OrbitSpinner size={13} />}
            <span>
              {activating
                ? t("briefing.setupEnabling")
                : recommended
                  ? t("briefing.setupEnableOnHost", { host: recommended.host_name })
                  : t("briefing.setupEnableDefault")}
            </span>
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => orbitActions.openSettings("personal_router")}
            className="text-ink-secondary hover:text-ink-primary w-full"
          >
            <Settings2 className="size-[13.5px] stroke-[1.75]" />
            <span>{t("briefing.setupCustomSettings")}</span>
          </Button>
        </CardFooter>
      </Card>
    </div>
  );
});
