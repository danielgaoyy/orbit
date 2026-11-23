import { FilePenLine, Shield, ShieldCheck, ShieldOff } from "lucide-react";
import { defaultEffortDescription, formatEffortLabel } from "@/lib/model-catalog";
import {
  PROVIDER_LIST,
  type ProviderKind,
  type ReasoningEffort,
  type SessionMode,
  type ThinkingOptionSpec,
} from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";

export const DEFAULT_CUSTOM_THINKING_OPTIONS: ThinkingOptionSpec[] = (
  ["low", "medium", "high", "xhigh", "max"] as ReasoningEffort[]
).map((eff) => ({
  effort: eff,
  label: formatEffortLabel(eff),
  description: defaultEffortDescription(eff),
}));

export function conciseThinkingDescription(opt: ThinkingOptionSpec): string {
  const raw = (opt.description ?? "").trim();
  if (raw && raw.length <= 34) {
    return raw;
  }
  return defaultEffortDescription(opt.effort);
}

export const PROVIDERS: readonly { id: ProviderKind; label: string; shortLabel: string }[] =
  PROVIDER_LIST;

export const PERMISSION_MODES: {
  id: Exclude<SessionMode, "plan" | { custom: string }>;
  label: string;
  desc: string;
}[] = [
  {
    id: "default",
    label: "Always Ask",
    desc: "Require approval before file edits and commands",
  },
  {
    id: "auto",
    label: "Auto",
    desc: "Guardian risk filter",
  },
  {
    id: "accept_edits",
    label: "Accept Edits",
    desc: "Auto-approve file edits, ask before commands",
  },
  {
    id: "full_access",
    label: "Full Access",
    desc: "Edit files and run commands without prompts",
  },
];

export function normalizePermissionMode(mode: SessionMode): SessionMode {
  return mode === "plan" ? "default" : mode;
}

export function modeLabel(mode: SessionMode): string {
  const normalized = normalizePermissionMode(mode);
  if (typeof normalized === "object") return normalized.custom;
  switch (normalized) {
    case "plan":
    case "default":
      return "Always Ask";
    case "auto":
      return "Auto";
    case "accept_edits":
      return "Accept Edits";
    case "full_access":
      return "Full Access";
  }
}

export function ModeIcon({ mode, className }: { mode: SessionMode; className?: string }) {
  const normalized = normalizePermissionMode(mode);
  if (typeof normalized === "object") {
    return <Shield className={cn("size-[15px] stroke-[1.75]", className)} />;
  }
  switch (normalized) {
    case "plan":
    case "default":
      return <Shield className={cn("size-[15px] stroke-[1.75]", className)} />;
    case "auto":
      return <ShieldCheck className={cn("size-[15px] stroke-[1.75]", className)} />;
    case "accept_edits":
      return <FilePenLine className={cn("size-[15px] stroke-[1.75]", className)} />;
    case "full_access":
      return <ShieldOff className={cn("size-[15px] stroke-[1.75]", className)} />;
  }
}

export function basenameOfCwd(cwd: string): string {
  const trimmed = cwd.replace(/\/+$/, "");
  if (!trimmed || trimmed === "~") return "~";
  const idx = trimmed.lastIndexOf("/");
  return idx >= 0 ? trimmed.slice(idx + 1) : trimmed;
}

export function collapseStrippedToken(
  before: string,
  after: string,
): { text: string; cursor: number } {
  const cleanBefore = before.replace(/[ \t]+$/, "");
  const cleanAfter = after.replace(/^[ \t]+/, "");
  if (!cleanBefore) {
    return { text: cleanAfter, cursor: 0 };
  }
  if (!cleanAfter) {
    return { text: cleanBefore, cursor: cleanBefore.length };
  }
  return { text: `${cleanBefore} ${cleanAfter}`, cursor: cleanBefore.length + 1 };
}
