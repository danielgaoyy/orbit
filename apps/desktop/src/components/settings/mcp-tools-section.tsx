import { useState } from "react";
import { HostHooksSection } from "./host-hooks-section";
import { HostLspSection } from "./host-lsp-section";
import { HostMcpSection } from "./host-mcp-section";
import { SettingPageHeader, SettingSelect } from "./settings-primitives";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/lib/i18n";
import { useSettingsWorkspaces } from "./settings-workspaces";

export function McpToolsSection({ hostId }: { hostId: string }) {
  const { t } = useI18n();
  const projects = useSettingsWorkspaces(hostId);
  const [scope, setScope] = useState<"global" | "project">("global");
  const [picked, setPicked] = useState("");
  const project =
    projects.find((option) => option.value === picked) ?? projects[0];
  const cwd = scope === "project" && project ? project.value : null;

  const workspacePicker = project && (
    <SettingSelect
      size="sm"
      ariaLabel={t("devTools.workspace")}
      value={project.value}
      options={projects}
      onChange={setPicked}
      triggerClassName="max-w-[320px] text-[13px]"
    />
  );

  return (
    <div className="flex flex-col gap-[24px]">
      <SettingPageHeader
        title={t("settings.nav4Mcp")}
        description={t("devTools.description")}
      />
      <div
        data-setting-id="devTools.scope"
        className="flex flex-wrap items-center gap-[8px]"
      >
        <div
          role="group"
          aria-label={t("devTools.scope")}
          className="border-border-subtle bg-surface-recessed inline-flex gap-px rounded-lg border p-[3px]"
        >
          {(["global", "project"] as const).map((value) => (
            <Button
              key={value}
              type="button"
              size="xs"
              variant="ghost"
              aria-pressed={value === (cwd ? "project" : "global")}
              disabled={value === "project" && !project}
              onClick={() => setScope(value)}
              className="text-[13px] aria-pressed:bg-surface-raised aria-pressed:text-ink-primary"
            >
              {value === "global"
                ? t("devTools.global")
                : t("devTools.project")}
            </Button>
          ))}
        </div>
        {cwd && workspacePicker}
      </div>
      <div
        key={JSON.stringify([hostId, cwd])}
        className="flex flex-col gap-[32px]"
      >
        <HostMcpSection
          hostId={hostId}
          workspaceCwd={cwd}
          importProjects={projects}
          onSelectDestination={(next) => {
            setPicked(next);
            setScope("project");
          }}
        />
        <HostHooksSection hostId={hostId} workspaceCwd={cwd} />
      </div>
      <HostLspSection
        key={project?.value ?? hostId}
        hostId={hostId}
        workspaceCwd={project?.value ?? null}
        workspacePicker={!cwd && workspacePicker}
      />
    </div>
  );
}
