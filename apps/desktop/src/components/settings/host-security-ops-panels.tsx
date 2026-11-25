import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  Check,
  CheckCircle2,
  Copy,
  Download,
  FileDown,
  FolderInput,
  Lock,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSelect,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useI18n } from "@/lib/i18n";
import {
  type AuditChainVerification,
  type ExternalIdeMigrationItem,
  type ExternalIdeSource,
  type MigrationConflict,
  type SandboxMode,
  type SandboxPolicy,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

const DEFAULT_SANDBOX_POLICY: SandboxPolicy = {
  mode: "workspace_write",
  writable_roots: [],
  denied_read_paths: [
    "~/.ssh",
    "~/.aws",
    "~/.gnupg",
    "~/.kube",
    "~/.orbit/pairing-secret",
    "~/.orbit/daemon.json",
    "~/.orbit/config.json",
    "~/.orbit/host-keypair.json",
  ],
  allow_network: false,
  allowed_domains: [],
  denied_domains: [],
};

function triggerTextDownload(filename: string, content: string, mimeType: string): void {
  if (typeof document === "undefined") return;
  const blob = new Blob([content], { type: mimeType });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export function HostSandboxAndAuditPanel({
  hostId,
  isOnline,
}: {
  hostId: string;
  isOnline: boolean;
}) {
  const { t } = useI18n();
  const [policy, setPolicy] = useState<SandboxPolicy | null>(null);
  const [sandboxLoaded, setSandboxLoaded] = useState(false);
  const [savingSandbox, setSavingSandbox] = useState(false);

  const [auditStatus, setAuditStatus] = useState<AuditChainVerification | null>(null);
  const [verifyingAudit, setVerifyingAudit] = useState(false);
  const [exportingAudit, setExportingAudit] = useState(false);

  const loadSandboxAndAudit = useCallback(async () => {
    if (!isOnline) return;
    setVerifyingAudit(true);
    try {
      const [sandboxRes, auditRes] = await Promise.allSettled([
        orbitIpc.getSandboxConfig(hostId),
        orbitIpc.getAuditChainStatus(hostId),
      ]);
      if (sandboxRes.status === "fulfilled") {
        setPolicy(sandboxRes.value);
        setSandboxLoaded(true);
      }
      if (auditRes.status === "fulfilled") {
        setAuditStatus(auditRes.value);
      }
    } finally {
      setVerifyingAudit(false);
    }
  }, [hostId, isOnline]);

  useEffect(() => {
    void loadSandboxAndAudit();
  }, [loadSandboxAndAudit]);

  const effectivePolicy = policy ?? DEFAULT_SANDBOX_POLICY;

  const commitSandbox = useCallback(
    async (nextPolicy: SandboxPolicy) => {
      setSavingSandbox(true);
      try {
        const updated = await orbitIpc.updateSandboxConfig(hostId, nextPolicy);
        setPolicy(updated ?? nextPolicy);
        orbitActions.showToast(t("securityOps.sandboxSavedToast"));
      } catch (err) {
        orbitActions.showToast(err instanceof Error ? err.message : String(err));
      } finally {
        setSavingSandbox(false);
      }
    },
    [hostId, t],
  );

  const handleVerifyAudit = useCallback(async () => {
    setVerifyingAudit(true);
    try {
      const res = await orbitIpc.getAuditChainStatus(hostId);
      setAuditStatus(res);
      orbitActions.showToast(
        res.broken_seq === null
          ? t("securityOps.auditVerifiedToast", { count: res.verified_entries })
          : t("securityOps.auditBrokenToast", { seq: res.broken_seq }),
      );
    } catch (err) {
      orbitActions.showToast(err instanceof Error ? err.message : String(err));
    } finally {
      setVerifyingAudit(false);
    }
  }, [hostId, t]);

  const handleExportAudit = useCallback(async () => {
    setExportingAudit(true);
    try {
      const lines: string[] = [];
      let afterSeq = 0;
      while (true) {
        const chunk = await orbitIpc.exportAuditChain(hostId, afterSeq);
        if (chunk.jsonl.trim()) lines.push(chunk.jsonl.trimEnd());
        if (chunk.next_after_seq === null || chunk.next_after_seq <= afterSeq) break;
        afterSeq = chunk.next_after_seq;
      }
      const content = lines.join("\n") + (lines.length > 0 ? "\n" : "");
      triggerTextDownload(`orbit-audit-${hostId}.jsonl`, content, "application/x-ndjson");
      orbitActions.showToast(t("securityOps.auditExportedToast"));
    } catch (err) {
      orbitActions.showToast(err instanceof Error ? err.message : String(err));
    } finally {
      setExportingAudit(false);
    }
  }, [hostId, t]);

  const isAuditHealthy = auditStatus !== null && auditStatus.broken_seq === null;

  return (
    <>
      {/* OS Sandbox Policy */}
      <div className="flex flex-col gap-2">
        <SettingGroupLabel>{t("securityOps.sandboxGroup")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            settingId="securityOps.sandboxModeTitle"
            title={t("securityOps.sandboxModeTitle")}
            description={t("securityOps.sandboxModeDesc")}
          >
            <SettingSelect<SandboxMode>
              value={effectivePolicy.mode}
              disabled={!isOnline || !sandboxLoaded || savingSandbox}
              onChange={(mode) => void commitSandbox({ ...effectivePolicy, mode })}
              options={[
                { value: "off", label: t("securityOps.sandboxOff") },
                { value: "workspace_write", label: t("securityOps.sandboxWorkspaceWrite") },
                { value: "strict_read_only", label: t("securityOps.sandboxStrictReadOnly") },
              ]}
            />
          </SettingRow>

          {effectivePolicy.mode !== "off" && (
            <SettingRow
              settingId="securityOps.sandboxNetworkTitle"
              title={t("securityOps.sandboxNetworkTitle")}
              description={t("securityOps.sandboxNetworkDesc")}
            >
              <ToggleSwitch
                checked={effectivePolicy.allow_network}
                disabled={!isOnline || !sandboxLoaded || savingSandbox}
                ariaLabel={t("securityOps.sandboxNetworkTitle")}
                onChange={(allow_network) =>
                  void commitSandbox({ ...effectivePolicy, allow_network })
                }
              />
            </SettingRow>
          )}

          {sandboxLoaded && effectivePolicy.mode !== "off" && (
            <div className="text-ink-muted flex items-center gap-2 px-4 py-2.5 text-[11px]">
              <Lock className="size-3.5 shrink-0 opacity-75" />
              <span>{t("securityOps.sandboxCredentialsProtected")}</span>
            </div>
          )}
        </SettingCard>
      </div>

      {/* Tamper-Evident Audit Chain */}
      <div className="flex flex-col gap-2">
        <SettingGroupLabel>{t("securityOps.auditChainGroup")}</SettingGroupLabel>
        <SettingCard>
          <div data-setting-id="securityOps.auditChainTitle" className="flex flex-col gap-3 px-4 py-3.5 sm:flex-row sm:items-center sm:justify-between">
            <div className="min-w-0 space-y-1.5">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-ink-primary text-[13px] font-medium">
                  {t("securityOps.auditChainTitle")}
                </span>
                {auditStatus ? (
                  isAuditHealthy ? (
                    <Badge
                      variant="outline"
                      className="border-signal-success/35 bg-signal-success/10 text-signal-success gap-1 text-[11px]"
                    >
                      <CheckCircle2 className="size-3" />
                      {t("securityOps.auditChainVerified")}
                    </Badge>
                  ) : (
                    <Badge
                      variant="outline"
                      className="border-signal-danger/40 bg-signal-danger/10 text-signal-danger gap-1 text-[11px]"
                    >
                      <AlertTriangle className="size-3" />
                      {t("securityOps.auditChainBroken", { seq: auditStatus.broken_seq ?? 0 })}
                    </Badge>
                  )
                ) : (
                  <Badge variant="outline" className="text-ink-muted text-[11px]">
                    {t("securityOps.auditChainPending")}
                  </Badge>
                )}
              </div>

              {auditStatus && (
                <div className="text-ink-secondary flex flex-wrap items-center gap-3 font-mono text-[11px] tabular-nums">
                  <span>
                    {t("securityOps.auditEntriesCount", { count: auditStatus.verified_entries })}
                  </span>
                  <span className="text-ink-faint">·</span>
                  <span className="inline-flex items-center gap-1">
                    <ShieldCheck className="text-signal-success size-3" />
                    {t("securityOps.auditCheckpointsCount", {
                      count: auditStatus.signed_checkpoints,
                    })}
                  </span>
                </div>
              )}
            </div>

            <div className="flex shrink-0 items-center gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={!isOnline || verifyingAudit}
                onClick={() => void handleVerifyAudit()}
              >
                {verifyingAudit ? (
                  <OrbitSpinner size={12} />
                ) : (
                  <RefreshCw data-icon="inline-start" />
                )}
                {t("securityOps.auditVerifyButton")}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={!isOnline || exportingAudit}
                onClick={() => void handleExportAudit()}
              >
                {exportingAudit ? (
                  <OrbitSpinner size={12} />
                ) : (
                  <FileDown data-icon="inline-start" />
                )}
                {t("securityOps.auditExportButton")}
              </Button>
            </div>
          </div>
        </SettingCard>
      </div>
    </>
  );
}

export function HostDiagnosticsAndMigrationPanel({
  hostId,
  isOnline,
}: {
  hostId: string;
  isOnline: boolean;
}) {
  const { t } = useI18n();
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);

  const [exportingDiag, setExportingDiag] = useState(false);
  const [lastDiagJson, setLastDiagJson] = useState<string | null>(null);
  const [copiedDiag, setCopiedDiag] = useState(false);

  const [source, setSource] = useState<"all" | ExternalIdeSource>("all");
  const [conflict, setConflict] = useState<MigrationConflict>("skip");
  const [includeWorkspace, setIncludeWorkspace] = useState(true);
  const [inspecting, setInspecting] = useState(false);
  const [executing, setExecuting] = useState(false);
  const [previewItems, setPreviewItems] = useState<ExternalIdeMigrationItem[] | null>(null);
  const [migrationRan, setMigrationRan] = useState(false);

  const activeCwd = useMemo(() => {
    if (!includeWorkspace) return null;
    if (selectedHostId === hostId && selectedCwd && selectedCwd !== "~") {
      return selectedCwd;
    }
    return null;
  }, [hostId, includeWorkspace, selectedCwd, selectedHostId]);

  const handleExportDiagnostics = useCallback(async () => {
    setExportingDiag(true);
    try {
      const bundle = await orbitIpc.exportDiagnosticBundle(hostId);
      const formatted = JSON.stringify(bundle, null, 2);
      setLastDiagJson(formatted);
      triggerTextDownload(`orbit-diagnostics-${hostId}.json`, formatted, "application/json");
      orbitActions.showToast(t("securityOps.diagExportedToast"));
    } catch (err) {
      orbitActions.showToast(err instanceof Error ? err.message : String(err));
    } finally {
      setExportingDiag(false);
    }
  }, [hostId, t]);

  const handleCopyDiagnostics = useCallback(async () => {
    if (!lastDiagJson) return;
    try {
      await navigator.clipboard.writeText(lastDiagJson);
      setCopiedDiag(true);
      orbitActions.showToast(t("securityOps.diagCopiedToast"));
      window.setTimeout(() => setCopiedDiag(false), 1800);
    } catch {
      orbitActions.showToast(t("common.copyFailed"));
    }
  }, [lastDiagJson, t]);

  const handleInspectMigration = useCallback(async () => {
    setInspecting(true);
    setMigrationRan(false);
    try {
      const items = await orbitIpc.inspectExternalIdeMigration(
        hostId,
        source === "all" ? null : source,
        activeCwd,
        conflict,
      );
      setPreviewItems(items);
    } catch (err) {
      orbitActions.showToast(err instanceof Error ? err.message : String(err));
    } finally {
      setInspecting(false);
    }
  }, [activeCwd, conflict, hostId, source]);

  const handleExecuteMigration = useCallback(async () => {
    setExecuting(true);
    try {
      const items = await orbitIpc.executeExternalIdeMigration(
        hostId,
        source === "all" ? null : source,
        activeCwd,
        conflict,
      );
      setPreviewItems(items);
      setMigrationRan(true);
      orbitActions.showToast(
        t("securityOps.migrationExecutedToast", { count: items.length }),
      );
    } catch (err) {
      orbitActions.showToast(err instanceof Error ? err.message : String(err));
    } finally {
      setExecuting(false);
    }
  }, [activeCwd, conflict, hostId, source, t]);

  return (
    <>
      {/* Diagnostic Bundle Export */}
      <div className="flex flex-col gap-2">
        <SettingGroupLabel>{t("securityOps.diagGroup")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            settingId="securityOps.diagTitle"
            title={t("securityOps.diagTitle")}
            description={t("securityOps.diagDesc")}
          >
            <div className="flex items-center gap-2">
              {lastDiagJson && (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => void handleCopyDiagnostics()}
                >
                  {copiedDiag ? (
                    <Check data-icon="inline-start" className="text-signal-success" />
                  ) : (
                    <Copy data-icon="inline-start" />
                  )}
                  {t("common.copy")}
                </Button>
              )}
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={!isOnline || exportingDiag}
                onClick={() => void handleExportDiagnostics()}
              >
                {exportingDiag ? (
                  <OrbitSpinner size={12} />
                ) : (
                  <Download data-icon="inline-start" />
                )}
                {t("securityOps.diagExportButton")}
              </Button>
            </div>
          </SettingRow>
        </SettingCard>
      </div>

      {/* Import from IDE / CLI */}
      <div data-setting-id="securityOps.migrationGroup" className="flex flex-col gap-2">
        <SettingGroupLabel>{t("securityOps.migrationGroup")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            settingId="securityOps.migrationSourceTitle"
            title={t("securityOps.migrationSourceTitle")}
            description={t("securityOps.migrationSourceDesc")}
          >
            <SettingSelect<"all" | ExternalIdeSource>
              value={source}
              disabled={!isOnline || inspecting || executing}
              onChange={(val) => {
                setSource(val);
                setPreviewItems(null);
                setMigrationRan(false);
              }}
              options={[
                { value: "all", label: t("securityOps.migrationAllSources") },
                { value: "claude", label: "Claude Code" },
                { value: "codex", label: "Codex" },
                { value: "antigravity", label: "Antigravity" },
                { value: "cursor", label: "Cursor" },
                { value: "vscode", label: "VS Code" },
              ]}
            />
          </SettingRow>

          <SettingRow
            settingId="securityOps.migrationConflictTitle"
            title={t("securityOps.migrationConflictTitle")}
            description={t("securityOps.migrationConflictDesc")}
          >
            <SettingSelect<MigrationConflict>
              value={conflict}
              disabled={!isOnline || inspecting || executing}
              onChange={(val) => {
                setConflict(val);
                setPreviewItems(null);
                setMigrationRan(false);
              }}
              options={[
                { value: "skip", label: t("securityOps.migrationConflictSkip") },
                { value: "rename", label: t("securityOps.migrationConflictRename") },
                { value: "overwrite", label: t("securityOps.migrationConflictOverwrite") },
              ]}
            />
          </SettingRow>

          {selectedHostId === hostId && selectedCwd && selectedCwd !== "~" && (
            <SettingRow
              settingId="securityOps.migrationScopeTitle"
              title={t("securityOps.migrationScopeTitle")}
              description={selectedCwd}
            >
              <ToggleSwitch
                checked={includeWorkspace}
                disabled={!isOnline || inspecting || executing}
                ariaLabel={t("securityOps.migrationScopeTitle")}
                onChange={(include) => {
                  setIncludeWorkspace(include);
                  setPreviewItems(null);
                  setMigrationRan(false);
                }}
              />
            </SettingRow>
          )}

          <div className="flex flex-col gap-3 px-4 py-3.5">
            <div className="flex flex-wrap items-center gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={!isOnline || inspecting || executing}
                onClick={() => void handleInspectMigration()}
              >
                {inspecting ? (
                  <OrbitSpinner size={12} />
                ) : (
                  <FolderInput data-icon="inline-start" />
                )}
                {t("securityOps.migrationPreviewButton")}
              </Button>
              <Button
                type="button"
                size="sm"
                disabled={
                  !isOnline ||
                  inspecting ||
                  executing ||
                  migrationRan ||
                  previewItems === null ||
                  previewItems.length === 0
                }
                onClick={() => void handleExecuteMigration()}
              >
                {executing && <OrbitSpinner size={12} />}
                {t("securityOps.migrationExecuteButton")}
              </Button>
            </div>

            {previewItems !== null && (
              previewItems.length === 0 ? (
                <div className="text-ink-muted text-[13px]">
                  {t("securityOps.migrationEmpty")}
                </div>
              ) : (
                <div className="border-border-subtle overflow-hidden rounded-[8px] border">
                  <div className="bg-surface-recessed/70 border-border-subtle text-ink-muted flex items-center justify-between border-b px-3 py-1.5 text-[11px] font-medium">
                    <span>
                      {migrationRan
                        ? t("securityOps.migrationResultLabel", { count: previewItems.length })
                        : t("securityOps.migrationDryRunLabel", { count: previewItems.length })}
                    </span>
                  </div>
                  <div className="orbit-native-scroll divide-border-subtle/70 max-h-[200px] divide-y overflow-y-auto">
                    {previewItems.map((item, idx) => (
                      <div
                        key={`${item.source}:${item.kind}:${item.name}:${idx}`}
                        className="flex items-center justify-between gap-2 px-3 py-2 text-[13px]"
                      >
                        <div className="flex min-w-0 items-center gap-2">
                          <Badge variant="outline" className="shrink-0 font-mono text-[11px]">
                            {item.source}
                          </Badge>
                          <Badge variant="secondary" className="shrink-0 text-[11px]">
                            {item.kind}
                          </Badge>
                          <span className="text-ink-primary truncate font-mono text-[11px]">
                            {item.name}
                          </span>
                        </div>
                        <Badge variant="outline" className="shrink-0 font-mono text-[11px]">
                          {item.decision}
                        </Badge>
                      </div>
                    ))}
                  </div>
                </div>
              )
            )}
          </div>
        </SettingCard>
      </div>
    </>
  );
}
