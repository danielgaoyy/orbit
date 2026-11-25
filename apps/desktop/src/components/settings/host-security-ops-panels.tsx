import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
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
  SettingGroup,
  SettingGroupLabel,
  SettingRow,
  SettingSelect,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import {
  countOverwrites,
  migrationPreviewKey,
  unexpectedOverwrites,
} from "@/components/settings/ide-migration-model";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ContextBanner } from "@/components/ui/context-banner";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useI18n, type MessageKey } from "@/lib/i18n";

const MIGRATION_SOURCE_KEYS: Record<string, MessageKey> = {
  claude: "securityOps.migrationSourceClaude",
  claude_code: "securityOps.migrationSourceClaude",
  codex: "securityOps.migrationSourceCodex",
  antigravity: "securityOps.migrationSourceAntigravity",
  cursor: "securityOps.migrationSourceCursor",
  vscode: "securityOps.migrationSourceVscode",
};

const MIGRATION_KIND_KEYS: Record<string, MessageKey> = {
  rule: "securityOps.migrationKindRules",
  rules: "securityOps.migrationKindRules",
  skill: "securityOps.migrationKindSkills",
  skills: "securityOps.migrationKindSkills",
  command: "securityOps.migrationKindCommands",
  commands: "securityOps.migrationKindCommands",
  mcp: "securityOps.migrationKindMcp",
};

const MIGRATION_DECISION_KEYS: Record<string, MessageKey> = {
  create: "securityOps.migrationDecisionCreate",
  unchanged: "securityOps.migrationDecisionUnchanged",
  skip: "securityOps.migrationDecisionSkip",
  overwrite: "securityOps.migrationDecisionOverwrite",
  rename: "securityOps.migrationDecisionRename",
};
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

export const HostSandboxSection = memo(function HostSandboxSection({
  hostId,
  isOnline,
}: {
  hostId: string;
  isOnline: boolean;
}) {
  const { t } = useI18n();
  const [policy, setPolicy] = useState<SandboxPolicy | null>(null);
  const [sandboxLoaded, setSandboxLoaded] = useState(false);
  const latestPolicyRef = useRef<SandboxPolicy | null>(null);
  const sandboxSeqRef = useRef(0);
  const sandboxChainRef = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => {
    if (!isOnline) return;
    const loadSeq = ++sandboxSeqRef.current;
    void orbitIpc
      .getSandboxConfig(hostId)
      .then((res) => {
        if (sandboxSeqRef.current === loadSeq) {
          latestPolicyRef.current = res;
          setPolicy(res);
          setSandboxLoaded(true);
        }
      })
      .catch(() => undefined);
  }, [hostId, isOnline]);

  const effectivePolicy = policy ?? DEFAULT_SANDBOX_POLICY;

  // Apply sandbox toggles optimistically with serialized RPC writes so changing
  // one control never flash-disables sibling controls during the local round-trip.
  const commitSandbox = useCallback(
    (patch: Partial<SandboxPolicy>) => {
      const prevPolicy = latestPolicyRef.current ?? DEFAULT_SANDBOX_POLICY;
      const nextPolicy: SandboxPolicy = { ...prevPolicy, ...patch };
      latestPolicyRef.current = nextPolicy;
      const seq = ++sandboxSeqRef.current;
      setPolicy(nextPolicy);

      sandboxChainRef.current = sandboxChainRef.current.then(async () => {
        try {
          const updated = await orbitIpc.updateSandboxConfig(hostId, nextPolicy);
          if (sandboxSeqRef.current === seq) {
            const resolved = updated ?? nextPolicy;
            latestPolicyRef.current = resolved;
            setPolicy(resolved);
          }
          orbitActions.showToast(t("securityOps.sandboxSavedToast"));
        } catch (err) {
          if (sandboxSeqRef.current === seq) {
            latestPolicyRef.current = prevPolicy;
            setPolicy(prevPolicy);
          }
          orbitActions.showToast(err instanceof Error ? err.message : String(err));
        }
      });
    },
    [hostId, t],
  );

  const sandboxModeOptions = useMemo(
    () => [
      { value: "off" as const, label: t("securityOps.sandboxOff") },
      { value: "workspace_write" as const, label: t("securityOps.sandboxWorkspaceWrite") },
      { value: "strict_read_only" as const, label: t("securityOps.sandboxStrictReadOnly") },
    ],
    [t],
  );

  const activeModeLabel =
    effectivePolicy.mode === "off"
      ? t("securityOps.sandboxOff")
      : effectivePolicy.mode === "strict_read_only"
        ? t("securityOps.sandboxStrictReadOnly")
        : t("securityOps.sandboxWorkspaceWrite");

  return (
    <SettingGroup
      title={t("securityOps.sandboxGroup")}
      action={
        sandboxLoaded ? (
          <Badge variant={effectivePolicy.mode === "off" ? "secondary" : "success"}>
            {activeModeLabel}
          </Badge>
        ) : undefined
      }
    >
      <SettingCard>
        <SettingRow
          settingId="securityOps.sandboxModeTitle"
          title={t("securityOps.sandboxModeTitle")}
          description={t("securityOps.sandboxModeDesc")}
        >
          <SettingSelect<SandboxMode>
            value={effectivePolicy.mode}
            disabled={!isOnline || !sandboxLoaded}
            onChange={(mode) => commitSandbox({ mode })}
            options={sandboxModeOptions}
            triggerClassName="w-[180px]"
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
              disabled={!isOnline || !sandboxLoaded}
              ariaLabel={t("securityOps.sandboxNetworkTitle")}
              onChange={(allow_network) => commitSandbox({ allow_network })}
            />
          </SettingRow>
        )}

        {sandboxLoaded && effectivePolicy.mode !== "off" && (
          <div className="bg-surface-recessed/45 text-ink-muted flex items-center gap-2 px-4 py-2 text-[11px]">
            <Lock className="size-[13px] shrink-0 opacity-80" />
            <span>{t("securityOps.sandboxCredentialsProtected")}</span>
          </div>
        )}
      </SettingCard>
    </SettingGroup>
  );
});

export const HostAuditChainSection = memo(function HostAuditChainSection({
  hostId,
  isOnline,
}: {
  hostId: string;
  isOnline: boolean;
}) {
  const { t } = useI18n();
  const [auditStatus, setAuditStatus] = useState<AuditChainVerification | null>(null);
  const [verifyingAudit, setVerifyingAudit] = useState(() => isOnline);
  const [exportingAudit, setExportingAudit] = useState(false);

  useEffect(() => {
    if (!isOnline) {
      setVerifyingAudit(false);
      return;
    }
    let active = true;
    setVerifyingAudit(true);
    void orbitIpc
      .getAuditChainStatus(hostId)
      .then((res) => {
        if (active) setAuditStatus(res);
      })
      .catch(() => undefined)
      .finally(() => {
        if (active) setVerifyingAudit(false);
      });
    return () => {
      active = false;
    };
  }, [hostId, isOnline]);

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
    <SettingGroup title={t("securityOps.auditChainGroup")}>
      <SettingCard>
        <div
          data-setting-id="securityOps.auditChainTitle"
          className="flex flex-col gap-3 px-4 py-3 sm:flex-row sm:items-center sm:justify-between"
        >
          <div className="min-w-0 space-y-1">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-ink-primary text-[13px] font-medium">
                {t("securityOps.auditChainTitle")}
              </span>
              {auditStatus ? (
                isAuditHealthy ? (
                  <Badge variant="success">
                    <CheckCircle2 className="size-[12px]" />
                    <span>{t("securityOps.auditChainVerified")}</span>
                  </Badge>
                ) : (
                  <Badge variant="destructive">
                    <AlertTriangle className="size-[12px]" />
                    <span>{t("securityOps.auditChainBroken", { seq: auditStatus.broken_seq ?? 0 })}</span>
                  </Badge>
                )
              ) : (
                <Badge variant="secondary">{t("securityOps.auditChainPending")}</Badge>
              )}
            </div>

            {auditStatus && (
              <div className="text-ink-secondary flex flex-wrap items-center gap-2 font-mono text-[11px] tabular-nums">
                <span>
                  {t("securityOps.auditEntriesCount", { count: auditStatus.verified_entries })}
                </span>
                <span aria-hidden className="text-ink-faint">
                  ·
                </span>
                <span className="inline-flex items-center gap-1">
                  <ShieldCheck className="text-signal-success size-[12px]" />
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
              className="text-[13px]"
            >
              {verifyingAudit ? <OrbitSpinner size={13} /> : <RefreshCw className="size-[13px]" />}
              <span>{t("securityOps.auditVerifyButton")}</span>
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={!isOnline || exportingAudit}
              onClick={() => void handleExportAudit()}
              className="text-[13px]"
            >
              {exportingAudit ? <OrbitSpinner size={13} /> : <FileDown className="size-[13px]" />}
              <span>{t("securityOps.auditExportButton")}</span>
            </Button>
          </div>
        </div>
      </SettingCard>
    </SettingGroup>
  );
});

export const HostSandboxAndAuditPanel = memo(function HostSandboxAndAuditPanel({
  hostId,
  isOnline,
}: {
  hostId: string;
  isOnline: boolean;
}) {
  return (
    <div className="flex flex-col gap-6">
      <HostSandboxSection hostId={hostId} isOnline={isOnline} />
      <HostAuditChainSection hostId={hostId} isOnline={isOnline} />
    </div>
  );
});

export const HostDiagnosticsSection = memo(function HostDiagnosticsSection({
  hostId,
  isOnline,
}: {
  hostId: string;
  isOnline: boolean;
}) {
  const { t } = useI18n();
  const [exportingDiag, setExportingDiag] = useState(false);
  const [lastDiagJson, setLastDiagJson] = useState<string | null>(null);
  const [copiedDiag, setCopiedDiag] = useState(false);

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

  return (
    <div className="flex flex-col gap-3">
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
  );
});

export const HostMigrationSection = memo(function HostMigrationSection({
  hostId,
  isOnline,
}: {
  hostId: string;
  isOnline: boolean;
}) {
  const { t } = useI18n();
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);

  const [source, setSource] = useState<"all" | ExternalIdeSource>("all");
  const [conflict, setConflict] = useState<MigrationConflict>("skip");
  const [includeWorkspace, setIncludeWorkspace] = useState(true);
  const [inspecting, setInspecting] = useState(false);
  const [executing, setExecuting] = useState(false);
  const [preview, setPreview] = useState<{
    key: string;
    items: ExternalIdeMigrationItem[];
    ran: boolean;
    unexpected: string[];
  } | null>(null);

  const activeCwd = useMemo(() => {
    if (!includeWorkspace) return null;
    if (selectedHostId === hostId && selectedCwd && selectedCwd !== "~") {
      return selectedCwd;
    }
    return null;
  }, [hostId, includeWorkspace, selectedCwd, selectedHostId]);

  const previewKey = migrationPreviewKey(source, activeCwd, conflict);
  // A preview only describes the run it was computed for: the daemon re-scans
  // and re-decides on execute, so stale inputs must force a fresh dry run.
  const fresh = preview !== null && preview.key === previewKey ? preview : null;
  const previewItems = fresh?.items ?? null;
  const migrationRan = fresh?.ran ?? false;
  const unexpected = fresh?.unexpected ?? [];
  const overwriteCount = previewItems === null ? 0 : countOverwrites(previewItems);

  const handleInspectMigration = useCallback(async () => {
    setInspecting(true);
    try {
      const items = await orbitIpc.inspectExternalIdeMigration(
        hostId,
        source === "all" ? null : source,
        activeCwd,
        conflict,
      );
      setPreview({
        key: migrationPreviewKey(source, activeCwd, conflict),
        items,
        ran: false,
        unexpected: [],
      });
    } catch (err) {
      orbitActions.showToast(err instanceof Error ? err.message : String(err));
    } finally {
      setInspecting(false);
    }
  }, [activeCwd, conflict, hostId, source]);

  const handleExecuteMigration = useCallback(async () => {
    const key = migrationPreviewKey(source, activeCwd, conflict);
    const confirmed = preview !== null && preview.key === key ? preview.items : null;
    if (confirmed === null) return;
    setExecuting(true);
    try {
      const items = await orbitIpc.executeExternalIdeMigration(
        hostId,
        source === "all" ? null : source,
        activeCwd,
        conflict,
      );
      setPreview({
        key,
        items,
        ran: true,
        // The daemon re-decided against the live filesystem; surface any file it
        // clobbered that the dry run had not flagged as an overwrite.
        unexpected: unexpectedOverwrites(confirmed, items),
      });
      orbitActions.showToast(
        t("securityOps.migrationExecutedToast", { count: items.length }),
      );
    } catch (err) {
      orbitActions.showToast(err instanceof Error ? err.message : String(err));
    } finally {
      setExecuting(false);
    }
  }, [activeCwd, conflict, hostId, preview, source, t]);

  return (
    <div data-setting-id="securityOps.migrationGroup" className="flex flex-col gap-3">
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
            onChange={setSource}
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
            onChange={setConflict}
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
              onChange={setIncludeWorkspace}
            />
          </SettingRow>
        )}

        <div className="flex flex-col gap-3 px-4 py-3">
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
            {!migrationRan && overwriteCount > 0 && (
              <span className="text-signal-warning text-[11px] tabular-nums">
                {t("securityOps.migrationOverwriteCount", { count: overwriteCount })}
              </span>
            )}
          </div>

          {unexpected.length > 0 && (
            <ContextBanner
              variant="inline"
              tone="warning"
              icon={<AlertTriangle className="size-3.5" />}
            >
              <div className="flex min-w-0 flex-col gap-1">
                <span className="text-[13px]">
                  {t("securityOps.migrationOverwriteMismatch", { count: unexpected.length })}
                </span>
                <span className="text-ink-secondary truncate font-mono text-[11px]">
                  {unexpected.join(" · ")}
                </span>
              </div>
            </ContextBanner>
          )}

          {previewItems !== null && (
            previewItems.length === 0 ? (
              <div className="text-ink-muted text-[13px]">
                {t("securityOps.migrationEmpty")}
              </div>
            ) : (
              <div className="border-border-subtle overflow-hidden rounded-[8px] border">
                <div className="bg-surface-recessed/70 border-border-subtle text-ink-muted flex items-center justify-between border-b px-3 py-2 text-[11px] font-medium">
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
                          {MIGRATION_SOURCE_KEYS[item.source]
                            ? t(MIGRATION_SOURCE_KEYS[item.source])
                            : item.source}
                        </Badge>
                        <Badge variant="secondary" className="shrink-0 text-[11px]">
                          {MIGRATION_KIND_KEYS[item.kind]
                            ? t(MIGRATION_KIND_KEYS[item.kind])
                            : item.kind}
                        </Badge>
                        <span className="text-ink-primary truncate font-mono text-[11px]">
                          {item.name}
                        </span>
                      </div>
                      <Badge
                        variant={item.decision === "overwrite" ? "warning" : "outline"}
                        className="shrink-0 font-mono text-[11px]"
                      >
                        {MIGRATION_DECISION_KEYS[item.decision]
                          ? t(MIGRATION_DECISION_KEYS[item.decision])
                          : item.decision}
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
  );
});
