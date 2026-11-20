import type {
  AgentCardView,
  GitReviewView,
  HostDetailView,
  TimelineItemView,
} from "@/lib/orbit-ipc";
import {
  isFileCreateToolCall,
  stripWorkspacePrefix,
} from "@/lib/timeline-coalesce";

export interface SessionTouchedFile {
  path: string;
  displayPath: string;
  action: "created" | "edited" | "read";
  additions: number;
  deletions: number;
  lastSeq: number;
}

export interface SessionBackgroundTask {
  id: string;
  kind: "shell" | "wakeup" | "queued";
  title: string;
  subtitle: string;
  status: "running" | "queued";
  startedAtMs: number | null;
  command?: string;
  outputPreview?: string | null;
}

export interface SessionSubagentEntry {
  id: string;
  role: string;
  promptPreview: string;
  status: "running" | "completed" | "failed" | "denied";
  childAgentId: string | null;
  lastSeq: number;
}

export interface SessionInsights {
  touchedFiles: SessionTouchedFile[];
  backgroundTasks: SessionBackgroundTask[];
  subagents: SessionSubagentEntry[];
  detectedLocalUrls: string[];
}

const LOCAL_URL_REGEX =
  /\bhttps?:\/\/(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\])(?::\d{2,5})(?:\/[^\s"'`<>)\]]*)?/gi;

function normalizeDetectedUrl(raw: string): string {
  return raw
    .replace(/0\.0\.0\.0/g, "localhost")
    .replace(/[.,;:!?]+$/, "");
}

function collectLocalUrls(text: string | null | undefined, out: Set<string>): void {
  if (!text) return;
  LOCAL_URL_REGEX.lastIndex = 0;
  let m: RegExpExecArray | null = null;
  while ((m = LOCAL_URL_REGEX.exec(text)) !== null) {
    const cleaned = normalizeDetectedUrl(m[0]);
    if (cleaned) out.add(cleaned);
    if (out.size >= 8) break;
  }
}

/**
 * Derive compact, deduplicated session inventory (touched files, active background tasks,
 * subagents, and detected localhost preview URLs) directly from the active timeline & host state.
 */
export function deriveSessionInsights(params: {
  items: readonly TimelineItemView[];
  cwd: string;
  currentAgent: AgentCardView | null;
  hostDetail: HostDetailView | null;
  queuedPrompts: readonly { id: string; displayText: string }[];
}): SessionInsights {
  const { items, cwd, currentAgent, hostDetail, queuedPrompts } = params;

  const fileMap = new Map<string, SessionTouchedFile>();
  const runningShells: SessionBackgroundTask[] = [];
  const subagentMap = new Map<string, SessionSubagentEntry>();
  const urlSet = new Set<string>();

  for (const item of items) {
    if (item.item_type === "assistant_message") {
      collectLocalUrls(item.text, urlSet);
      continue;
    }
    if (item.item_type !== "tool_call") continue;

    switch (item.detail.kind) {
      case "file_read": {
        const rawPath = item.detail.path.trim();
        if (!rawPath) break;
        const displayPath = stripWorkspacePrefix(rawPath, cwd);
        const prev = fileMap.get(displayPath);
        if (!prev) {
          fileMap.set(displayPath, {
            path: rawPath,
            displayPath,
            action: "read",
            additions: 0,
            deletions: 0,
            lastSeq: item.seq,
          });
        } else {
          prev.lastSeq = Math.max(prev.lastSeq, item.seq);
        }
        break;
      }
      case "file_edit": {
        const rawPath = item.detail.path.trim();
        if (!rawPath) break;
        const displayPath = stripWorkspacePrefix(rawPath, cwd);
        const isCreate = isFileCreateToolCall(item);
        const add = item.diff_files.reduce((acc, f) => acc + f.additions, 0);
        const del = item.diff_files.reduce((acc, f) => acc + f.deletions, 0);
        const prev = fileMap.get(displayPath);
        if (!prev) {
          fileMap.set(displayPath, {
            path: rawPath,
            displayPath,
            action: isCreate ? "created" : "edited",
            additions: add,
            deletions: del,
            lastSeq: item.seq,
          });
        } else {
          prev.action =
            prev.action === "created" || isCreate ? "created" : "edited";
          prev.additions += add;
          prev.deletions += del;
          prev.lastSeq = Math.max(prev.lastSeq, item.seq);
        }
        break;
      }
      case "shell": {
        collectLocalUrls(item.detail.output_preview, urlSet);
        if (item.status === "running") {
          const firstLine =
            item.detail.command
              .split(/\r?\n/)
              .map((l) => l.trim())
              .find((l) => l.length > 0) ?? "Running command";
          runningShells.push({
            id: `shell:${item.call_id}`,
            kind: "shell",
            title: firstLine,
            subtitle: item.detail.cwd
              ? stripWorkspacePrefix(item.detail.cwd, cwd)
              : "Shell command",
            status: "running",
            startedAtMs: item.timestamp_ms ?? null,
            command: item.detail.command,
            outputPreview: item.detail.output_preview,
          });
        }
        break;
      }
      case "subagent": {
        const key = item.detail.child_agent_id || item.call_id;
        subagentMap.set(key, {
          id: item.call_id,
          role: item.detail.role || "Subagent",
          promptPreview: item.detail.prompt_preview || "",
          status: item.status,
          childAgentId: item.detail.child_agent_id,
          lastSeq: item.seq,
        });
        break;
      }
      default:
        break;
    }
  }

  // Merge child agents discovered via `parent_agent_id` on the host
  if (currentAgent && hostDetail) {
    for (const ag of hostDetail.agents) {
      if (ag.parent_agent_id && ag.parent_agent_id === currentAgent.agent_id) {
        const existing = subagentMap.get(ag.agent_id);
        const status: SessionSubagentEntry["status"] = ag.is_running
          ? "running"
          : ag.status_label.toLowerCase().includes("error")
            ? "failed"
            : "completed";
        if (existing) {
          existing.status = status;
          existing.childAgentId = ag.agent_id;
        } else {
          subagentMap.set(ag.agent_id, {
            id: ag.agent_id,
            role: ag.title || "Subagent",
            promptPreview: ag.status_label,
            status,
            childAgentId: ag.agent_id,
            lastSeq: ag.latest_seq,
          });
        }
      }
    }
  }

  const backgroundTasks: SessionBackgroundTask[] = [...runningShells];

  if (
    currentAgent &&
    currentAgent.is_running &&
    /^Waking\s*\(/i.test(currentAgent.status_label)
  ) {
    backgroundTasks.unshift({
      id: `wakeup:${currentAgent.agent_id}`,
      kind: "wakeup",
      title: currentAgent.status_label,
      subtitle: "Autonomous background wakeup",
      status: "running",
      startedAtMs: null,
    });
  }

  for (const q of queuedPrompts) {
    backgroundTasks.push({
      id: `queue:${q.id}`,
      kind: "queued",
      title: q.displayText.replace(/\s+/g, " ").trim().slice(0, 72) || "Queued prompt",
      subtitle: "Queued · runs when turn finishes",
      status: "queued",
      startedAtMs: null,
    });
  }

  const touchedFiles = Array.from(fileMap.values()).sort((a, b) => {
    const aEdit = a.action === "read" ? 0 : 1;
    const bEdit = b.action === "read" ? 0 : 1;
    if (aEdit !== bEdit) return bEdit - aEdit;
    return b.lastSeq - a.lastSeq;
  });

  const subagents = Array.from(subagentMap.values()).sort((a, b) => {
    const aRun = a.status === "running" ? 1 : 0;
    const bRun = b.status === "running" ? 1 : 0;
    if (aRun !== bRun) return bRun - aRun;
    return b.lastSeq - a.lastSeq;
  });

  return {
    touchedFiles,
    backgroundTasks,
    subagents,
    detectedLocalUrls: Array.from(urlSet),
  };
}

/**
 * Build a concise, bounded read-only context summary of the active session for `/btw` side questions.
 */
export function buildSideQuestionContextSummary(params: {
  cwd: string;
  branchName: string | null;
  items: readonly TimelineItemView[];
  gitReview: GitReviewView | null;
}): string {
  const { cwd, branchName, items, gitReview } = params;
  const sections: string[] = [];

  sections.push(
    `Workspace: ${cwd || "~"}${branchName ? ` (branch: ${branchName})` : ""}`,
  );

  const changedFiles = gitReview?.status?.files ?? [];
  if (changedFiles.length > 0) {
    const list = changedFiles
      .slice(0, 20)
      .map((f) => `${f.status} ${f.path}`)
      .join(", ");
    sections.push(`Modified files (${changedFiles.length}): ${list}`);
  }

  const recentExchanges: string[] = [];
  const relevant = items.filter(
    (it) =>
      it.item_type === "user_prompt" ||
      it.item_type === "assistant_message" ||
      (it.item_type === "tool_call" &&
        (it.detail.kind === "file_edit" || it.detail.kind === "shell")),
  );

  for (const it of relevant.slice(-18)) {
    if (it.item_type === "user_prompt") {
      recentExchanges.push(`User: ${it.text.trim().slice(0, 700)}`);
    } else if (it.item_type === "assistant_message") {
      recentExchanges.push(`Assistant: ${it.text.trim().slice(0, 1200)}`);
    } else if (it.item_type === "tool_call") {
      if (it.detail.kind === "file_edit") {
        recentExchanges.push(
          `Tool [edit ${stripWorkspacePrefix(it.detail.path, cwd)}] (${it.status})`,
        );
      } else if (it.detail.kind === "shell") {
        recentExchanges.push(
          `Tool [shell: ${it.detail.command.slice(0, 140)}] (${it.status}${it.detail.exit_code !== null ? ` exit=${it.detail.exit_code}` : ""})`,
        );
      }
    }
  }

  if (recentExchanges.length > 0) {
    sections.push("Recent conversation trace:\n" + recentExchanges.join("\n\n"));
  }

  return sections.join("\n\n").slice(-10_500);
}

export function formatFileSizeCompact(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}
