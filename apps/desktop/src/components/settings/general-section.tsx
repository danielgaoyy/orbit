import { useEffect, useMemo, useState } from "react";
import { Check, Download, Loader2 } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
  SettingSelect,
  type SettingSelectOption,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import { modSymbol } from "@/lib/keybindings";
import {
  type ExternalEditorKind,
  type ReleaseUpdateInfo,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type PresetPermissionMode = "default" | "accept_edits" | "full_access";

const PREFIX_OPTIONS: SettingSelectOption<string>[] = [
  { value: "auto", label: "Auto", description: "Infer from thread topic" },
  { value: "orbit/", label: "orbit/", mono: true },
  { value: "feat/", label: "feat/", mono: true },
  { value: "fix/", label: "fix/", mono: true },
  { value: "chore/", label: "chore/", mono: true },
];

const PERMISSION_OPTIONS: SettingSelectOption<PresetPermissionMode>[] = [
  {
    value: "default",
    label: "Always Ask",
    description: "Confirm file edits and shell commands",
  },
  {
    value: "accept_edits",
    label: "Accept Edits",
    description: "Auto-approve edits, ask for commands",
  },
  {
    value: "full_access",
    label: "Full Access",
    description: "Run edits and commands without prompting",
  },
];

export function GeneralSection() {
  const enterToSend = useOrbitSelector((s) => s.enterToSend);
  const draftMode = useOrbitSelector((s) => s.draftMode);
  const draftUseWorktree = useOrbitSelector((s) => s.draftUseWorktree);
  const worktreeBranchPrefix = useOrbitSelector((s) => s.worktreeBranchPrefix);
  const coalesceExploration = useOrbitSelector((s) => s.coalesceExploration);
  const externalEditors = useOrbitSelector((s) => s.externalEditors);
  const preferredEditor = useOrbitSelector((s) => s.preferredEditor);
  const mod = modSymbol();

  const [appVersion, setAppVersion] = useState("0.3.1");
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<ReleaseUpdateInfo | null>(null);
  const [updateStatus, setUpdateStatus] = useState<string | null>(null);
  const [installingUpdate, setInstallingUpdate] = useState(false);

  useEffect(() => {
    void orbitIpc.getAppVersion().then(setAppVersion).catch(() => {});
  }, []);

  const activePermissionMode: PresetPermissionMode =
    draftMode === "accept_edits" || draftMode === "full_access" ? draftMode : "default";

  const activePrefix = PREFIX_OPTIONS.some((o) => o.value === worktreeBranchPrefix)
    ? worktreeBranchPrefix
    : "auto";

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

  const handleCheckUpdate = async () => {
    setCheckingUpdate(true);
    setUpdateStatus(null);
    try {
      const info = await orbitIpc.checkAppUpdate();
      setUpdateInfo(info);
      if (!info.has_newer_version) {
        setUpdateStatus("You are on the latest version.");
      }
    } catch {
      setUpdateStatus("Unable to check for updates. Try again later.");
    } finally {
      setCheckingUpdate(false);
    }
  };

  const handleInstallUpdate = async () => {
    if (!updateInfo) return;
    setInstallingUpdate(true);
    setUpdateStatus("Downloading update…");
    try {
      await orbitIpc.performAppSelfUpdate(updateInfo, (ev) => {
        if (ev.status) {
          const pctVal = ev.progress <= 1 ? Math.round(ev.progress * 100) : Math.round(ev.progress);
          const pct = pctVal > 0 && pctVal < 100 ? ` (${pctVal}%)` : "";
          setUpdateStatus(`${ev.status}${pct}`);
        }
      });
    } catch {
      setUpdateStatus("Update failed. Try downloading manually.");
      setInstallingUpdate(false);
    }
  };

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title="General"
        description="Composer behavior, permissions, Git worktrees, and editor handoff."
      />

      {/* Workspace */}
      <div className="space-y-2">
        <SettingGroupLabel>Workspace</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title="Send with Enter"
            description={
              enterToSend
                ? "Press Enter to send, Shift+Enter for a new line."
                : `Press ${mod}Enter to send, Enter for a new line.`
            }
          >
            <ToggleSwitch
              checked={enterToSend}
              onChange={(v) => orbitActions.setEnterToSend(v)}
            />
          </SettingRow>

          <SettingRow
            title="Default access permissions"
            description="Initial approval level for new conversations."
          >
            <SettingSelect
              value={activePermissionMode}
              options={PERMISSION_OPTIONS}
              onChange={(mode) => orbitActions.setDraftConfig({ mode })}
              menuClassName="w-[260px]"
            />
          </SettingRow>

          <SettingRow
            title="Auto-collapse completed steps"
            description="Fold intermediate tool calls and thoughts once a reply arrives."
          >
            <ToggleSwitch
              checked={coalesceExploration}
              onChange={(v) => orbitActions.setWorktreePrefs({ coalesceExploration: v })}
            />
          </SettingRow>

          <SettingRow
            title="External editor"
            description={`Default editor opened with ${mod}O.`}
          >
            <SettingSelect
              value={effectivePreferred}
              options={editorOptions}
              onChange={(kind) => orbitActions.setPreferredEditor(kind)}
              placeholder="No editors found"
              disabled={editorOptions.length === 0}
            />
          </SettingRow>
        </SettingCard>
      </div>

      {/* Git & Worktrees */}
      <div className="space-y-2">
        <SettingGroupLabel>Git & Worktrees</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title="Isolate threads in worktrees"
            description="Start new conversations in an isolated Git worktree by default."
          >
            <ToggleSwitch
              checked={draftUseWorktree}
              onChange={(v) => orbitActions.setDraftConfig({ useWorktree: v })}
            />
          </SettingRow>

          <div className={cn(!draftUseWorktree && "opacity-55 transition-opacity")}>
            <SettingRow
              title="Branch prefix"
              description="Prefix for auto-generated worktree branches."
            >
              <SettingSelect
                value={activePrefix}
                options={PREFIX_OPTIONS}
                disabled={!draftUseWorktree}
                onChange={(pfx) => orbitActions.setWorktreePrefs({ worktreeBranchPrefix: pfx })}
                menuClassName="w-[200px]"
              />
            </SettingRow>
          </div>
        </SettingCard>
      </div>

      {/* Software Updates */}
      <div className="space-y-2">
        <SettingGroupLabel>Updates</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title={`Orbit ${appVersion}`}
            description={
              updateInfo?.has_newer_version
                ? `Version ${updateInfo.latest_version} is available.`
                : (updateStatus ?? "Check for the latest desktop release.")
            }
          >
            {updateInfo?.has_newer_version ? (
              <button
                type="button"
                disabled={installingUpdate}
                onClick={() => void handleInstallUpdate()}
                className="bg-foreground text-background inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-3 text-[12.5px] font-medium transition-opacity hover:opacity-90 disabled:opacity-50"
              >
                {installingUpdate ? (
                  <Loader2 className="h-[13.5px] w-[13.5px] animate-spin" />
                ) : (
                  <Download className="h-[13.5px] w-[13.5px]" />
                )}
                <span>{installingUpdate ? "Installing…" : "Update Now"}</span>
              </button>
            ) : (
              <button
                type="button"
                disabled={checkingUpdate}
                onClick={() => void handleCheckUpdate()}
                className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-3 text-[12.5px] font-medium transition-colors disabled:opacity-50"
              >
                {checkingUpdate ? (
                  <Loader2 className="h-[13.5px] w-[13.5px] animate-spin" />
                ) : updateInfo && !updateInfo.has_newer_version ? (
                  <Check className="h-[13.5px] w-[13.5px] text-emerald-500" />
                ) : null}
                <span>
                  {checkingUpdate
                    ? "Checking…"
                    : updateInfo && !updateInfo.has_newer_version
                      ? "Up to date"
                      : "Check for Updates"}
                </span>
              </button>
            )}
          </SettingRow>
        </SettingCard>
      </div>
    </div>
  );
}
