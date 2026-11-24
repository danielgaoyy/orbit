import type {
  PermissionRuleDecision,
  PermissionRulePatternKind,
  PermissionRuleRecord,
  PermissionRuleScopeKind,
  PermissionRuleSource,
  PermissionRuleToolKind,
  ProviderKind,
} from "@/lib/orbit-ipc";
import type { OrbitUiState } from "../../state/store-types.ts";

export type PermissionRuleFilterTab = "all" | "always_allow" | "manual" | "deny";

export const EMPTY_PERMISSION_RULES: readonly PermissionRuleRecord[] = Object.freeze([]);
export const EMPTY_WORKSPACE_PATHS: readonly string[] = Object.freeze([]);

export interface PermissionRulesHostSnapshot {
  readonly status: "idle" | "loading" | "ready" | "error";
  readonly rules: readonly PermissionRuleRecord[];
  readonly saving: boolean;
  readonly error: string | null;
}

const IDLE_RULES_SNAPSHOT: PermissionRulesHostSnapshot = Object.freeze({
  status: "idle",
  rules: EMPTY_PERMISSION_RULES,
  saving: false,
  error: null,
});

/**
 * Stable React 19 `useSyncExternalStore` selector factory for host permission rules.
 * Guarantees referential identity across unrelated store updates so `useOrbitSelector`
 * never triggers `#185` re-render loops.
 */
export function createHostPermissionRulesSelector(hostId: string) {
  let prevEntry: OrbitUiState["permissionRulesByHost"][string] | undefined;
  let prevSnapshot: PermissionRulesHostSnapshot = IDLE_RULES_SNAPSHOT;

  return (state: Pick<OrbitUiState, "permissionRulesByHost">): PermissionRulesHostSnapshot => {
    const entry = hostId ? state.permissionRulesByHost[hostId] : undefined;
    if (entry === prevEntry) return prevSnapshot;
    prevEntry = entry;
    if (!entry) {
      prevSnapshot = IDLE_RULES_SNAPSHOT;
      return prevSnapshot;
    }
    prevSnapshot = {
      status: entry.status,
      rules: entry.rules,
      saving: Boolean(entry.saving),
      error: entry.error ?? null,
    };
    return prevSnapshot;
  };
}

/**
 * Pure filter & search predicate for the Settings Permission Rules ledger.
 */
export function filterPermissionRules(
  rules: readonly PermissionRuleRecord[],
  query: string,
  filter: PermissionRuleFilterTab,
): PermissionRuleRecord[] {
  const needle = query.trim().toLowerCase();
  return rules.filter((rule) => {
    if (filter === "always_allow" && rule.source !== "always_allow_grant") {
      return false;
    }
    if (filter === "manual" && rule.source !== "manual") {
      return false;
    }
    if (filter === "deny" && rule.decision !== "deny") {
      return false;
    }
    if (!needle) return true;
    return (
      rule.pattern.toLowerCase().includes(needle) ||
      (rule.project_cwd ?? "").toLowerCase().includes(needle) ||
      (rule.provider ?? "all").toLowerCase().includes(needle) ||
      rule.tool_kind.toLowerCase().includes(needle) ||
      rule.pattern_kind.toLowerCase().includes(needle) ||
      (rule.note ?? "").toLowerCase().includes(needle)
    );
  });
}

export function basenameFromPath(rawPath: string | null | undefined): string {
  const cleaned = (rawPath ?? "").trim().replace(/\\/g, "/").replace(/\/+$/, "");
  if (!cleaned) return "";
  const parts = cleaned.split("/");
  return parts[parts.length - 1] || cleaned;
}

export function defaultPatternKindForTool(
  toolKind: PermissionRuleToolKind,
): PermissionRulePatternKind {
  switch (toolKind) {
    case "shell":
      return "command_prefix";
    case "file_write":
    case "file_read":
      return "path_glob";
    case "mcp_tool":
    case "network":
    case "any":
      return "tool_name";
  }
}

export function formatProviderShortLabel(provider: ProviderKind | null | undefined): string | null {
  if (!provider) return null;
  switch (provider) {
    case "claude_code":
      return "Claude Code";
    case "codex":
      return "Codex";
    case "antigravity":
      return "Antigravity";
    case "opencode":
      return "OpenCode";
    case "gemini_cli":
      return "Gemini CLI";
    case "cursor_agent":
      return "Cursor";
    case "copilot":
      return "Copilot";
    case "goose":
      return "Goose";
    case "pi":
      return "Pi";
    case "kimi":
      return "Kimi";
    case "cline":
      return "Cline";
    case "mistral_vibe":
      return "Mistral Vibe";
    case "augment":
      return "Augment";
    case "acp":
      return "ACP";
    default:
      return String(provider);
  }
}

export interface PermissionRuleDraft {
  decision: PermissionRuleDecision;
  scopeKind: PermissionRuleScopeKind;
  projectCwd: string;
  toolKind: PermissionRuleToolKind;
  patternKind: PermissionRulePatternKind;
  provider: ProviderKind | "all";
  pattern: string;
  note: string;
}

export function createInitialPermissionRuleDraft(defaultProjectCwd = ""): PermissionRuleDraft {
  return {
    decision: "allow",
    scopeKind: defaultProjectCwd ? "project" : "host",
    projectCwd: defaultProjectCwd,
    toolKind: "shell",
    patternKind: "command_prefix",
    provider: "all",
    pattern: "",
    note: "",
  };
}

export function countAlwaysAllowRules(rules: readonly PermissionRuleRecord[]): number {
  let count = 0;
  for (const r of rules) {
    if (r.source === ("always_allow_grant" satisfies PermissionRuleSource)) {
      count += 1;
    }
  }
  return count;
}
