import { useState, type ReactNode } from "react";
import { Sparkles } from "lucide-react";
import {
  SettingCard,
  SettingDrawer,
  SettingGroup,
  SettingResourceRow,
} from "./settings-primitives";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/lib/i18n";
import { orbitIpc, type OrchestrationSkillInfo } from "@/lib/orbit-ipc";
import { orbitActions } from "@/state/orbit-store";

export function OrchestrationSkills({
  hostId,
  skills,
  online,
  actionsForSkill,
  onSynced,
}: {
  hostId: string;
  skills: readonly OrchestrationSkillInfo[];
  online: boolean;
  actionsForSkill: (id: string) => ReactNode;
  onSynced: () => void;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const selected = skills.find((skill) => skill.id === selectedId);
  async function sync(ids: string[]) {
    if (busy) return;
    setBusy(true);
    try {
      await orbitIpc.syncOrchestrationSkills(hostId, ids);
      setOpen(false);
      onSynced();
    } catch (error) {
      orbitActions.showToast(String(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <SettingGroup
      title={t("settings.agentBundledSkills")}
      settingId="settings.agentBundledSkills"
      action={
        skills.length > 0 && (
          <Button
            size="sm"
            variant="outline"
            disabled={busy || !online}
            onClick={() => void sync(skills.map((skill) => skill.id))}
          >
            {t("settings.skillSyncAll")}
          </Button>
        )
      }
    >
      <SettingCard>
        {!skills.length && (
          <p className="px-4 py-3 text-[13px] text-ink-secondary">{t("settings.skillNoBundled")}</p>
        )}
        {skills.map((skill) => (
          <SettingResourceRow
            key={skill.id}
            icon={<Sparkles />}
            title={skill.name}
            description={skill.slash_command}
            status={t(
              skill.outdated
                ? "settings.skillOutdated"
                : skill.installed
                  ? "settings.agentInstalled"
                  : "settings.agentNotInstalled",
            )}
            action={actionsForSkill(skill.id)}
            onClick={() => {
              setSelectedId(skill.id);
              setOpen(true);
            }}
          />
        ))}
      </SettingCard>
      {selected && (
        <SettingDrawer
          open={open}
          onOpenChange={(next) => {
            if (!busy) setOpen(next);
          }}
          onOpenChangeComplete={(visible) => {
            if (!visible) setSelectedId(null);
          }}
          title={selected.name}
          description={selected.slash_command}
        >
          <div className="space-y-4 text-[13px] text-ink-secondary">
            <p>{selected.description}</p>
            {selected.installed_targets.length > 0 && (
              <ul className="break-all font-mono text-[11px]">
                {selected.installed_targets.map((path) => (
                  <li key={path}>{path}</li>
                ))}
              </ul>
            )}
            <Button
              size="sm"
              disabled={busy || !online}
              onClick={() => {
                const ids = new Set(
                  skills.filter((skill) => skill.installed).map((skill) => skill.id),
                );
                if (selected.installed && !selected.outdated) ids.delete(selected.id);
                else ids.add(selected.id);
                void sync([...ids]);
              }}
            >
              {t(
                selected.outdated
                  ? "settings.skillUpdate"
                  : selected.installed
                    ? "settings.skillUninstall"
                    : "settings.skillInstall",
              )}
            </Button>
          </div>
        </SettingDrawer>
      )}
    </SettingGroup>
  );
}
