import { useMemo, useState } from "react";
import { HostHooksSection } from "./host-hooks-section";
import { HostMcpSection } from "./host-mcp-section";
import { SettingPageHeader, SettingSelect } from "./settings-primitives";
import { Segmented, type SegmentedOption } from "@/components/ui/segmented";
import { useI18n } from "@/lib/i18n";
import { useSettingsWorkspaces } from "./settings-workspaces";

function useScopedWorkspace(hostId: string) {
  const { t } = useI18n();
  const projects = useSettingsWorkspaces(hostId);
  const [scope, setScope] = useState<"global" | "project">("global");
  const [picked, setPicked] = useState("");
  const project =
    projects.find((option) => option.value === picked) ?? projects[0];
  const cwd = scope === "project" && project ? project.value : null;

  const scopeOptions = useMemo<readonly SegmentedOption<"global" | "project">[]>(
    () => [
      { value: "global", label: t("devTools.global") },
      { value: "project", label: t("devTools.project"), disabled: !project },
    ],
    [project, t],
  );

  const scopeBar = (
    <div
      data-setting-id="devTools.scope"
      className="flex flex-wrap items-center gap-2"
    >
      <Segmented<"global" | "project">
        value={cwd ? "project" : "global"}
        onValueChange={setScope}
        options={scopeOptions}
        ariaLabel={t("devTools.scope")}
      />
      {cwd && project && (
        <SettingSelect
          size="sm"
          ariaLabel={t("devTools.workspace")}
          value={project.value}
          options={projects}
          onChange={setPicked}
          triggerClassName="max-w-[320px] text-[13px]"
        />
      )}
    </div>
  );

  return { projects, cwd, setPicked, setScope, scopeBar };
}

export function McpSection({ hostId }: { hostId: string }) {
  const { t } = useI18n();
  const { projects, cwd, setPicked, setScope, scopeBar } = useScopedWorkspace(hostId);

  return (
    <div className="flex flex-col gap-6">
      <SettingPageHeader
        title={t("settings.nav4Mcp")}
        description={t("devTools.description")}
      />
      {scopeBar}
      <div key={JSON.stringify([hostId, cwd])}>
        <HostMcpSection
          hostId={hostId}
          workspaceCwd={cwd}
          importProjects={projects}
          onSelectDestination={(next) => {
            setPicked(next);
            setScope("project");
          }}
        />
      </div>
    </div>
  );
}

export function HooksSection({ hostId }: { hostId: string }) {
  const { t } = useI18n();
  const { cwd, scopeBar } = useScopedWorkspace(hostId);

  return (
    <div className="flex flex-col gap-6">
      <SettingPageHeader
        title={t("hooks.title")}
        description={t("hooks.description")}
      />
      {scopeBar}
      <div key={JSON.stringify([hostId, cwd])}>
        <HostHooksSection hostId={hostId} workspaceCwd={cwd} />
      </div>
    </div>
  );
}
