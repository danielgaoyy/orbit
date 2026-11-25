import { memo, useMemo } from "react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
  SettingSelect,
  ToggleSwitch,
  type SettingSelectOption,
} from "@/components/settings/settings-primitives";
import { useI18n } from "@/lib/i18n";
import { modSymbol } from "@/lib/keybindings";
import { isWebRuntime, type ExternalEditorKind } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

export const GitWorktreesSection = memo(function GitWorktreesSection() {
  const { locale, t } = useI18n();
  const draftUseWorktree = useOrbitSelector((s) => s.draftUseWorktree);
  const worktreeBranchPrefix = useOrbitSelector((s) => s.worktreeBranchPrefix);
  const externalEditors = useOrbitSelector((s) => s.externalEditors);
  const preferredEditor = useOrbitSelector((s) => s.preferredEditor);
  const mod = modSymbol();

  const prefixOptions = useMemo<SettingSelectOption<string>[]>(
    () => [
      { value: "auto", label: t("composer.modeAuto"), description: t("settings.branchPrefixAutoDesc") },
      { value: "orbit/", label: "orbit/", mono: true },
      { value: "feat/", label: "feat/", mono: true },
      { value: "fix/", label: "fix/", mono: true },
      { value: "chore/", label: "chore/", mono: true },
    ],
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [locale],
  );

  // Synced preferences may contain a custom prefix from another client.
  const branchOptions = useMemo(
    () =>
      prefixOptions.some((item) => item.value === worktreeBranchPrefix)
        ? prefixOptions
        : [...prefixOptions, { value: worktreeBranchPrefix, label: worktreeBranchPrefix, mono: true }],
    [prefixOptions, worktreeBranchPrefix],
  );

  const installedEditors = useMemo(
    () => externalEditors.filter((ed) => ed.installed),
    [externalEditors],
  );

  const effectivePreferred: ExternalEditorKind | null =
    (preferredEditor && installedEditors.some((ed) => ed.kind === preferredEditor)
      ? preferredEditor
      : installedEditors[0]?.kind) ?? null;

  const editorOptions = useMemo<SettingSelectOption<ExternalEditorKind>[]>(
    () =>
      installedEditors.map((ed) => ({
        value: ed.kind,
        label: ed.label,
      })),
    [installedEditors],
  );

  return (
    <div className="flex flex-col gap-6">
      <SettingSectionHeader
        title={t("settings.groupGitWorktrees")}
        description={t("settings.gitWorktreesDesc")}
      />

      <div className="flex flex-col gap-2">
        <SettingGroupLabel>{t("settings.groupWorktrees")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            settingId="settings.isolateWorktrees"
            title={t("settings.isolateWorktrees")}
            description={t("settings.isolateWorktreesDesc")}
          >
            <ToggleSwitch
              ariaLabel={t("settings.isolateWorktrees")}
              checked={draftUseWorktree}
              onChange={(v) => orbitActions.setDraftConfig({ useWorktree: v })}
            />
          </SettingRow>

          <SettingRow
            settingId="settings.branchPrefix"
            title={t("settings.branchPrefix")}
            description={t("settings.branchPrefixDesc")}
            disabled={!draftUseWorktree}
          >
            <SettingSelect
              ariaLabel={t("settings.branchPrefix")}
              value={worktreeBranchPrefix}
              options={branchOptions}
              disabled={!draftUseWorktree}
              onChange={(pfx) => orbitActions.setWorktreePrefs({ worktreeBranchPrefix: pfx })}
              menuClassName="w-[200px]"
            />
          </SettingRow>
        </SettingCard>
      </div>

      {!isWebRuntime() && (
        <div className="flex flex-col gap-2">
          <SettingGroupLabel>{t("settings.groupEditor")}</SettingGroupLabel>
          <SettingCard>
            <SettingRow
              settingId="settings.externalEditor"
              title={t("settings.externalEditor")}
              description={t("settings.externalEditorDesc", { mod })}
            >
              <SettingSelect
                ariaLabel={t("settings.externalEditor")}
                value={effectivePreferred}
                options={editorOptions}
                onChange={(kind) => orbitActions.setPreferredEditor(kind)}
                placeholder={t("settings.noEditorsFound")}
                disabled={editorOptions.length === 0}
              />
            </SettingRow>
          </SettingCard>
        </div>
      )}
    </div>
  );
});
