import { useRef, useState } from "react";
import { useSettingsWorkspaces } from "./settings-workspaces";
import { Folder } from "lucide-react";
import { HostMemorySection } from "./host-memory-section";
import { HostRulesSection } from "./host-rules-section";
import { InstructionsSection, type InstructionsGuard } from "./instructions-section";
import { SettingPageHeader, SettingSelect } from "./settings-primitives";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/dialog-tabs";
import { useI18n } from "@/lib/i18n";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

type ContextTab = "instructions" | "rules" | "memory";

export function InstructionsMemorySection({
  hostId, tab, onTabChange,
}: {
  hostId: string | null;
  tab: ContextTab;
  onTabChange: (tab: ContextTab) => void;
}) {
  const { t } = useI18n();
  const target = useOrbitSelector((s) => s.instructionsTarget);
  const instructionGuard = useRef<InstructionsGuard>(null);
  const [headerActions, setHeaderActions] = useState<HTMLDivElement | null>(null);
  const [globalRulesHost, setGlobalRulesHost] = useState<string | null>(null);

  const workspaces = useSettingsWorkspaces(hostId, target?.hostId === hostId ? target.cwd : undefined);
  const workspace = workspaces[0]?.value ?? null;
  const isGlobal = tab === "rules" && (!workspace || globalRulesHost === hostId);
  const cwd = isGlobal ? "~" : workspace;
  const workspaceOptions = tab === "rules" && hostId
    ? [...workspaces, { value: "~", label: t("context.globalRules"), description: "~/.orbit/rules/" }]
    : workspaces;
  const requestChange = (change: () => void) => {
    if (instructionGuard.current) instructionGuard.current.beforeChange(change);
    else change();
  };

  return (
    <Tabs
      value={tab}
      onValueChange={(next) => {
        if (next === "instructions" || next === "rules" || next === "memory") {
          requestChange(() => onTabChange(next));
        }
      }}
      className="@container flex min-h-0 min-w-0 flex-1 flex-col gap-4"
    >
      <SettingPageHeader
        title={t("settings.nav4Instructions")}
        description={t("context.description")}
        action={<div ref={setHeaderActions} />}
      />
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-3">
        <TabsList aria-label={t("settings.nav4Instructions")}>
          <TabsTrigger className="text-[13px]" value="instructions">{t("settings.subTabInstructions")}</TabsTrigger>
          <TabsTrigger className="text-[13px]" value="rules" disabled={!hostId}>{t("context.pathRules")}</TabsTrigger>
          <TabsTrigger className="text-[13px]" value="memory" disabled={!hostId}>{t("memory.title")}</TabsTrigger>
        </TabsList>
        <div data-setting-id="context.workspace" className="text-ink-muted flex min-w-0 items-center gap-2">
          <Folder aria-hidden className="size-[15px]" />
          <SettingSelect
            value={cwd}
            ariaLabel={t("context.workspace")}
            placeholder={t("context.workspace")}
            options={workspaceOptions}
            disabled={!workspaceOptions.length}
            onChange={(next) => requestChange(() => {
              if (!hostId) return;
              setGlobalRulesHost(next === "~" ? hostId : null);
              if (next !== "~") orbitActions.setInstructionsTarget({ hostId, cwd: next });
            })}
            triggerClassName="max-w-[260px] text-[13px]"
          />
        </div>
      </div>
      <TabsContent value="instructions" className="flex min-h-0 min-w-0 flex-1 flex-col">
        {tab === "instructions" && (
          <InstructionsSection key={JSON.stringify([hostId, cwd])} hostId={hostId} cwd={cwd}
            headerActions={headerActions} ref={instructionGuard} />
        )}
      </TabsContent>
      <TabsContent value="rules">
        {tab === "rules" && hostId && <HostRulesSection key={JSON.stringify([hostId, cwd])} hostId={hostId} cwd={cwd ?? "~"} />}
      </TabsContent>
      <TabsContent value="memory">
        {tab === "memory" && hostId && <HostMemorySection key={JSON.stringify([hostId, cwd])} hostId={hostId} cwd={cwd ?? ""} />}
      </TabsContent>
    </Tabs>
  );
}
