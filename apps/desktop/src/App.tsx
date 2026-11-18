import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  ArrowUp,
  ArrowUpFromLine,
  BrainCircuit,
  Check,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Cpu,
  Folder,
  FolderGit2,
  FolderPlus,
  FolderTree,
  GitBranch,
  HelpCircle,
  Home,
  Loader2,
  MoreHorizontal,
  Plus,
  RefreshCw,
  Server,
  Settings2,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  Square,
  Terminal as TerminalIcon,
  Trash2,
  Wifi,
  WifiOff,
  Zap,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/dialog-tabs";
import { Input } from "@/components/ui/input";
import { DiffFilesViewer } from "@/components/diff-viewer";
import { MarkdownBlocksView } from "@/components/markdown-blocks";
import { XtermPane, type XtermStreamChunk } from "@/components/xterm-pane";
import {
  type AgentCardView,
  type AgentTimelineView,
  type ClientProjectView,
  type CoreNotification,
  type FleetOverviewView,
  type GitReviewView,
  type HostCardView,
  type HostDetailView,
  type HostRouteMode,
  type ModelOption,
  type ProviderKind,
  type QuestionItem,
  type SessionMode,
  type SshHostItem,
  type TimelineItemView,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";

// =============================================================================
// CONSTANTS & HELPERS
// =============================================================================

const PROVIDERS: Array<{
  id: ProviderKind;
  label: string;
  shortLabel: string;
  desc: string;
}> = [
  {
    id: "antigravity",
    label: "Antigravity",
    shortLabel: "Antigravity",
    desc: "深度推理与多步编排",
  },
  {
    id: "codex",
    label: "Codex",
    shortLabel: "Codex",
    desc: "极速代码修改与测试闭环",
  },
  {
    id: "claude_code",
    label: "Claude Code",
    shortLabel: "Claude",
    desc: "复杂架构重构与细致审查",
  },
];

const MODES: Array<{
  id: "accept_edits" | "plan" | "full_access";
  label: string;
  shortLabel: string;
  desc: string;
}> = [
  {
    id: "accept_edits",
    label: "自动改码",
    shortLabel: "自动",
    desc: "默认推荐：自动读写项目代码，仅在高危外部命令前请求确认",
  },
  {
    id: "plan",
    label: "先出方案",
    shortLabel: "方案",
    desc: "只读分析代码并给出方案选择题，拍板前绝不修改文件",
  },
  {
    id: "full_access",
    label: "全自动推进",
    shortLabel: "全自动",
    desc: "全程自主改码、跑测试并修复报错，适合独立跑完长任务",
  },
];

const ROUTE_MODES: Array<{
  id: HostRouteMode;
  label: string;
  desc: string;
}> = [
  {
    id: "auto",
    label: "智能自动 (推荐)",
    desc: "配置 SSH 别名时优先建立加密端口转发，局域网直连，外网自动回退中继",
  },
  {
    id: "ssh_tunnel",
    label: "强制 SSH 隧道",
    desc: "始终通过 ~/.ssh/config 建立独立保活 SSH 隧道（最适合云上 DevBox）",
  },
  {
    id: "direct_only",
    label: "仅局域网 / Tailscale 直连",
    desc: "仅通过 WebSocket 直连地址通信，不尝试 SSH 或中继",
  },
  {
    id: "relay_only",
    label: "仅 E2EE 加密中继",
    desc: "强制经由零知识 Noise_NK 加密中继节点转发",
  },
];

const FALLBACK_MODELS: Record<ProviderKind, ModelOption[]> = {
  antigravity: [
    {
      id: "gemini-4-carbon-high",
      label: "Gemini 4 Carbon (High)",
      description: "最新旗舰深度推理与多步编排模型",
      is_default: true,
    },
    {
      id: "gemini-4-carbon-medium",
      label: "Gemini 4 Carbon (Medium)",
      description: "速度与深度均衡主力",
      is_default: false,
    },
    {
      id: "gemini-4-argon-high",
      label: "Gemini 4 Argon (High)",
      description: "高强度复杂代码推理",
      is_default: false,
    },
    {
      id: "claude-opus-5-5-high",
      label: "Claude Opus 5.5 (High)",
      description: "Antigravity 内置 Opus 深度模式",
      is_default: false,
    },
    {
      id: "claude-sonnet-5-5-high",
      label: "Claude Sonnet 5.5 (High)",
      description: "高响应速度编码模式",
      is_default: false,
    },
  ],
  codex: [
    {
      id: "gpt-6.1-sol",
      label: "GPT-6.1-Sol",
      description: "最新日常编码与重构主力模型",
      is_default: true,
    },
    {
      id: "gpt-6-astra",
      label: "GPT-6-Astra",
      description: "高复杂度架构与攻坚模型",
      is_default: false,
    },
    {
      id: "gpt-6.1-terra",
      label: "GPT-6.1-Terra",
      description: "均衡稳定编码模型",
      is_default: false,
    },
    {
      id: "gpt-5.4",
      label: "GPT-5.4",
      description: "通用高兼容 Codex 模型",
      is_default: false,
    },
  ],
  claude_code: [
    {
      id: "claude-opus-5-5",
      label: "Opus 5.5",
      description: "最新旗舰深度架构与推理模型",
      is_default: true,
    },
    {
      id: "claude-sonnet-5-5",
      label: "Sonnet 5.5",
      description: "高速高质主力编码模型",
      is_default: false,
    },
    {
      id: "claude-opus-4-6",
      label: "Opus 4.6",
      description: "经典稳定复杂工程模型",
      is_default: false,
    },
    {
      id: "claude-sonnet-4-6",
      label: "Sonnet 4.6",
      description: "轻量快速日常开发",
      is_default: false,
    },
  ],
};

function folderBasename(p: string): string {
  const clean = p.replace(/\/+$/, "");
  if (!clean || clean === "~") return "主目录";
  const idx = clean.lastIndexOf("/");
  return idx >= 0 && idx + 1 < clean.length ? clean.slice(idx + 1) : clean;
}

function shortenPath(p: string): string {
  const parts = p.split("/");
  if (parts.length > 3 && (parts[1] === "home" || parts[1] === "Users")) {
    return `~/${parts.slice(3).join("/")}`;
  }
  return p;
}

function shortModelLabel(label: string): string {
  return label
    .replace(" (High)", "")
    .replace(" (Medium)", " Med")
    .replace("Claude ", "");
}

function isHostOnline(state: HostCardView["connection_state"]): boolean {
  return state === "online_direct" || state === "online_relay";
}

function matchesProjectCwd(project: ClientProjectView, cwd: string): boolean {
  const clean = cwd.replace(/\/+$/, "");
  if (project.directories.some((d) => d.replace(/\/+$/, "") === clean)) {
    return true;
  }
  const base = folderBasename(project.directories[0] ?? "");
  return Boolean(base && clean.includes(`/.orbit/worktrees/${base}-`));
}

interface SidebarProjectGroup {
  project: ClientProjectView;
  host: HostCardView;
  threads: Array<{
    host: HostCardView;
    agent: AgentCardView;
    isWorktree: boolean;
    gitBranch: string | null;
  }>;
}

// =============================================================================
// SUB-COMPONENTS: CONNECTION BADGE, QUESTION CARD, THINKING, TOOLS & LIVE BANNER
// =============================================================================

function ConnectionBadge({ host }: { host: HostCardView }) {
  switch (host.connection_state) {
    case "online_direct":
      return (
        <Badge variant="success" className="gap-1 text-[10.5px]">
          <Wifi className="size-3" />
          {host.via_ssh_tunnel ? "SSH 加密隧道" : "直连在线"}
          {host.last_ping_ms !== null && ` · ${host.last_ping_ms}ms`}
        </Badge>
      );
    case "online_relay":
      return (
        <Badge variant="relay" className="gap-1 text-[10.5px]">
          <ShieldCheck className="size-3" /> E2EE 中继
          {host.last_ping_ms !== null && ` · ${host.last_ping_ms}ms`}
        </Badge>
      );
    case "connecting_direct":
    case "connecting_relay":
      return (
        <Badge variant="warning" className="gap-1 text-[10.5px]">
          <Loader2 className="size-3 animate-spin" /> 连接中…
        </Badge>
      );
    default:
      return (
        <Badge variant="outline" className="gap-1 text-[10.5px] text-amber-300/80">
          <WifiOff className="size-3" /> 自动重连中
        </Badge>
      );
  }
}

function LiveTurnActivityBanner({
  agent,
  items,
}: {
  agent: AgentCardView;
  items: TimelineItemView[];
}) {
  const [elapsedSec, setElapsedSec] = useState(0);

  useEffect(() => {
    setElapsedSec(0);
    const id = window.setInterval(() => {
      setElapsedSec((s) => s + 1);
    }, 1000);
    return () => window.clearInterval(id);
  }, [agent.agent_id, items.length]);

  const phaseText = useMemo(() => {
    if (agent.agent_id.startsWith("opt_") || agent.status_label === "启动中") {
      return "正在启动工作区会话与连接引擎…";
    }
    let lastUserIdx = -1;
    for (let i = items.length - 1; i >= 0; i--) {
      if (items[i].item_type === "user_prompt") {
        lastUserIdx = i;
        break;
      }
    }
    const turnItems = lastUserIdx >= 0 ? items.slice(lastUserIdx + 1) : items;
    for (let i = turnItems.length - 1; i >= 0; i--) {
      const it = turnItems[i];
      if (it.item_type === "tool_call" && it.status === "running") {
        return `正在执行工具 · ${it.summary_title}`;
      }
    }
    const tail = turnItems[turnItems.length - 1];
    if (tail?.item_type === "assistant_thinking") {
      return "Thinking · 正在深度思考推理…";
    }
    if (tail?.item_type === "assistant_message" && tail.streaming) {
      return "正在流式输出回复…";
    }
    return "Thinking · 正在思考与分析上下文…";
  }, [agent, items]);

  return (
    <div className="flex items-center justify-between gap-3 rounded-xl border border-sky-500/25 bg-sky-500/[0.07] px-4 py-2.5 text-xs text-sky-200">
      <div className="flex min-w-0 items-center gap-2.5">
        <Loader2 className="size-4 shrink-0 animate-spin text-sky-400" />
        <span className="truncate font-medium">{phaseText}</span>
      </div>
      {elapsedSec > 0 && (
        <span className="shrink-0 font-mono text-[11px] text-zinc-400">{elapsedSec}s</span>
      )}
    </div>
  );
}

function ThinkingBlock({
  text,
  isActivelyThinking,
}: {
  text: string;
  isActivelyThinking: boolean;
}) {
  const [open, setOpen] = useState(false);
  const trimmed = text.trim();
  if (!trimmed && !isActivelyThinking) return null;

  const lines = trimmed ? trimmed.split("\n") : [];
  const previewTail = lines.slice(-3).join("\n");

  return (
    <div
      className={cn(
        "rounded-xl border px-3.5 py-2.5 text-xs transition-colors",
        isActivelyThinking
          ? "border-violet-500/35 bg-violet-500/[0.06]"
          : "border-white/8 bg-white/[0.02]",
      )}
    >
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full cursor-pointer items-center justify-between gap-2 text-left"
      >
        <span className="flex items-center gap-2 font-medium">
          {isActivelyThinking ? (
            <Loader2 className="size-3.5 animate-spin text-violet-400" />
          ) : (
            <BrainCircuit className="size-3.5 text-violet-400/80" />
          )}
          <span className={isActivelyThinking ? "text-violet-200" : "text-zinc-400"}>
            {isActivelyThinking
              ? "Thinking · 思考推理中…"
              : `思考过程 (${lines.length} 行)`}
          </span>
        </span>
        <ChevronDown
          className={cn(
            "size-3.5 text-zinc-500 transition-transform",
            open && "rotate-180",
          )}
        />
      </button>
      {open && trimmed ? (
        <pre className="mt-2 max-h-72 overflow-y-auto font-mono text-[11.5px] leading-relaxed whitespace-pre-wrap text-zinc-300">
          {trimmed}
        </pre>
      ) : (
        !open &&
        isActivelyThinking &&
        previewTail && (
          <pre className="mt-1.5 line-clamp-3 font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-zinc-400">
            {previewTail}
          </pre>
        )
      )}
    </div>
  );
}

function ToolCallCard({ item }: { item: Extract<TimelineItemView, { item_type: "tool_call" }> }) {
  const isRunning = item.status === "running";
  const isFailed = item.status === "failed" || item.status === "denied";
  const hasOutput =
    (item.detail.kind === "shell" && Boolean(item.detail.output_preview?.trim())) ||
    item.diff_files.length > 0 ||
    (item.detail.kind === "subagent" && Boolean(item.detail.prompt_preview?.trim()));

  const [open, setOpen] = useState<boolean>(() => isFailed || item.diff_files.length > 0);

  return (
    <div
      className={cn(
        "overflow-hidden rounded-xl border bg-[#0B0F19] text-xs",
        isRunning
          ? "border-sky-500/35"
          : isFailed
            ? "border-rose-500/35"
            : "border-white/8",
      )}
    >
      <div
        onClick={() => hasOutput && setOpen((v) => !v)}
        className={cn(
          "flex items-center justify-between gap-2 px-3.5 py-2.5",
          hasOutput && "cursor-pointer hover:bg-white/[0.02]",
        )}
      >
        <div className="flex min-w-0 items-center gap-2.5">
          {isRunning ? (
            <Loader2 className="size-3.5 shrink-0 animate-spin text-sky-400" />
          ) : isFailed ? (
            <AlertTriangle className="size-3.5 shrink-0 text-rose-400" />
          ) : (
            <CheckCircle2 className="size-3.5 shrink-0 text-emerald-400" />
          )}
          <code className="truncate font-mono text-[12px] text-zinc-200">
            {item.summary_title}
          </code>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {item.detail.kind === "shell" && item.detail.exit_code !== null && (
            <Badge
              variant={item.detail.exit_code === 0 ? "success" : "destructive"}
              className="px-1.5 py-0 font-mono text-[10px]"
            >
              exit {item.detail.exit_code}
            </Badge>
          )}
          {isRunning && (
            <span className="text-[11px] font-medium text-sky-300">执行中…</span>
          )}
          {hasOutput && (
            <ChevronDown
              className={cn(
                "size-3.5 text-zinc-500 transition-transform",
                open && "rotate-180",
              )}
            />
          )}
        </div>
      </div>

      {open && hasOutput && (
        <div className="border-t border-white/6 bg-black/35 p-3">
          {item.detail.kind === "shell" && item.detail.output_preview && (
            <pre className="max-h-52 overflow-auto font-mono text-[11.5px] leading-relaxed whitespace-pre-wrap text-zinc-300">
              {item.detail.output_preview}
            </pre>
          )}
          {item.detail.kind === "subagent" && item.detail.prompt_preview && (
            <p className="text-xs text-zinc-400">{item.detail.prompt_preview}</p>
          )}
          {item.diff_files.length > 0 && <DiffFilesViewer files={item.diff_files} />}
        </div>
      )}
    </div>
  );
}

function QuestionCard({
  hostId,
  agentId,
  requestId,
  questions,
  resolvedAnswers,
}: {
  hostId: string;
  agentId: string;
  requestId: string;
  questions: QuestionItem[];
  resolvedAnswers: string[][] | null;
}) {
  const isInstantSingle = questions.length === 1 && !questions[0]?.is_multi_select;
  const [selections, setSelections] = useState<Record<number, string[]>>(() => {
    const init: Record<number, string[]> = {};
    questions.forEach((q, idx) => {
      if (q.options.length > 0) init[idx] = [q.options[0]];
    });
    return init;
  });

  const handleTapOption = (qIdx: number, opt: string, multi: boolean) => {
    if (resolvedAnswers) return;
    if (isInstantSingle) {
      void orbitIpc.answerQuestion(hostId, agentId, requestId, [[opt]]);
      return;
    }
    setSelections((prev) => {
      const cur = prev[qIdx] ?? [];
      if (!multi) return { ...prev, [qIdx]: [opt] };
      return {
        ...prev,
        [qIdx]: cur.includes(opt) ? cur.filter((x) => x !== opt) : [...cur, opt],
      };
    });
  };

  return (
    <Card className="border-sky-500/35 bg-sky-500/[0.06]">
      <CardHeader className="pb-1">
        <CardTitle className="flex items-center gap-2 text-sm text-sky-300">
          <HelpCircle className="size-4" /> 请点选方案方向
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        {questions.map((q, qIdx) => (
          <div key={q.id || qIdx} className="space-y-2">
            <div className="text-sm font-medium text-zinc-100">{q.question}</div>
            <div className="grid gap-1.5">
              {q.options.map((opt) => {
                const chosen = resolvedAnswers
                  ? resolvedAnswers[qIdx]?.includes(opt)
                  : !isInstantSingle && selections[qIdx]?.includes(opt);
                return (
                  <button
                    key={opt}
                    type="button"
                    disabled={Boolean(resolvedAnswers)}
                    onClick={() => handleTapOption(qIdx, opt, q.is_multi_select)}
                    className={cn(
                      "flex w-full cursor-pointer items-center justify-between rounded-xl border px-3.5 py-2.5 text-left text-xs transition-colors disabled:cursor-default",
                      chosen
                        ? "border-sky-400/60 bg-sky-500/20 text-white"
                        : "border-white/10 bg-white/[0.03] text-zinc-200 hover:bg-white/[0.07]",
                    )}
                  >
                    <span>{opt}</span>
                    {chosen && <Check className="size-3.5 text-sky-300" />}
                  </button>
                );
              })}
            </div>
          </div>
        ))}
        {resolvedAnswers ? (
          <div className="text-xs text-sky-300">
            已选择：{resolvedAnswers.flat().join(" · ")}
          </div>
        ) : !isInstantSingle ? (
          <Button
            size="sm"
            onClick={() => {
              const ordered = questions.map((_, idx) => selections[idx] ?? []);
              void orbitIpc.answerQuestion(hostId, agentId, requestId, ordered);
            }}
          >
            确认选择
          </Button>
        ) : (
          <div className="text-[11px] text-zinc-500">
            点选任意选项即刻生效；如有补充说明也可直接在下方对话框输入。
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function TimelineEntry({
  hostId,
  agentId,
  item,
  isLastItem,
  isAgentRunning,
}: {
  hostId: string;
  agentId: string;
  item: TimelineItemView;
  isLastItem: boolean;
  isAgentRunning: boolean;
}) {
  switch (item.item_type) {
    case "user_prompt":
      return (
        <div className="flex justify-end">
          <div
            className={cn(
              "max-w-[80%] rounded-2xl rounded-br-sm border px-4 py-2.5 text-sm",
              item.seq === 0
                ? "border-sky-400/35 bg-sky-500/15"
                : "border-white/10 bg-[#1B2130]",
            )}
          >
            <MarkdownBlocksView blocks={item.blocks} />
            {item.seq === 0 && (
              <div className="mt-1 text-right text-[10.5px] text-sky-300/75">
                已提交发送…
              </div>
            )}
          </div>
        </div>
      );

    case "assistant_thinking":
      return (
        <ThinkingBlock
          text={item.text}
          isActivelyThinking={isAgentRunning && isLastItem}
        />
      );

    case "assistant_message":
      return (
        <div className="py-1 text-sm leading-relaxed text-zinc-100">
          <MarkdownBlocksView blocks={item.blocks} />
          {isAgentRunning && item.streaming && (
            <span className="ml-1 inline-block animate-pulse font-bold text-sky-400">▍</span>
          )}
        </div>
      );

    case "tool_call":
      return <ToolCallCard item={item} />;

    case "permission_request":
      return (
        <Card className="border-amber-500/40 bg-amber-500/[0.07]">
          <CardHeader className="pb-1">
            <CardTitle className="flex items-center gap-2 text-sm text-amber-300">
              <ShieldAlert className="size-4" /> {item.title || "需要确认操作权限"}
            </CardTitle>
            <CardDescription className="rounded-lg bg-black/35 p-2 font-mono text-xs text-amber-100/90">
              {item.detail.kind === "shell"
                ? `$ ${item.detail.command}`
                : item.detail.kind === "file_edit" || item.detail.kind === "file_read"
                  ? item.detail.path
                  : JSON.stringify(item.detail)}
            </CardDescription>
          </CardHeader>
          <CardContent>
            {item.resolved_decision ? (
              <Badge variant="secondary">已确认：{item.resolved_decision}</Badge>
            ) : (
              <div className="flex flex-wrap gap-2">
                <Button
                  size="sm"
                  onClick={() =>
                    void orbitIpc.resolvePermission(
                      hostId,
                      agentId,
                      item.request_id,
                      "allow",
                    )
                  }
                >
                  允许执行
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    void orbitIpc.resolvePermission(
                      hostId,
                      agentId,
                      item.request_id,
                      "allow_always",
                    )
                  }
                >
                  本对话不再询问
                </Button>
                <Button
                  size="sm"
                  variant="destructive"
                  onClick={() =>
                    void orbitIpc.resolvePermission(
                      hostId,
                      agentId,
                      item.request_id,
                      "deny",
                    )
                  }
                >
                  拒绝
                </Button>
              </div>
            )}
          </CardContent>
        </Card>
      );

    case "question_request":
      return (
        <QuestionCard
          hostId={hostId}
          agentId={agentId}
          requestId={item.request_id}
          questions={item.questions}
          resolvedAnswers={item.resolved_answers}
        />
      );

    case "notice":
      return (
        <div
          className={cn(
            "flex items-center gap-2 rounded-lg px-3 py-1.5 text-xs",
            item.level === "error"
              ? "bg-rose-500/12 text-rose-300"
              : "bg-white/[0.03] text-zinc-400",
          )}
        >
          <AlertTriangle className="size-3.5 shrink-0" />
          <span>{item.message}</span>
        </div>
      );
  }
}

// =============================================================================
// UNIFIED ZERO-FORM INLINE PILL COMPOSER (Project + Engine + Model + Mode)
// =============================================================================

function OrbitInlineComposer({
  isNewChat,
  isRunning,
  projectLabel,
  hasExtraDirs,
  useWorktree,
  provider,
  modelLabel,
  mode,
  placeholder,
  onOpenProjectPicker,
  onOpenProviderPicker,
  onOpenModelPicker,
  onOpenModePicker,
  onSend,
  onStop,
}: {
  isNewChat: boolean;
  isRunning: boolean;
  projectLabel?: string;
  hasExtraDirs?: boolean;
  useWorktree?: boolean;
  provider: ProviderKind;
  modelLabel: string;
  mode: SessionMode;
  placeholder: string;
  onOpenProjectPicker?: () => void;
  onOpenProviderPicker: () => void;
  onOpenModelPicker: () => void;
  onOpenModePicker: () => void;
  onSend: (text: string) => void;
  onStop: () => void;
}) {
  const [text, setText] = useState("");
  const provMeta = PROVIDERS.find((p) => p.id === provider) ?? PROVIDERS[0];
  const modeMeta = MODES.find((m) => m.id === mode) ?? MODES[0];
  const hasText = text.trim().length > 0;

  const submit = () => {
    const trimmed = text.trim();
    if (!trimmed) {
      if (isRunning) onStop();
      return;
    }
    setText("");
    onSend(trimmed);
  };

  return (
    <div className="rounded-2xl border border-white/12 bg-[#141822] shadow-xl transition-colors focus-within:border-white/25">
      <textarea
        value={text}
        rows={isNewChat ? 3 : 2}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.nativeEvent.isComposing) return;
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            submit();
          }
        }}
        placeholder={placeholder}
        className="max-h-44 w-full resize-none bg-transparent px-4 pt-3.5 pb-2 text-sm leading-relaxed text-zinc-100 placeholder:text-zinc-500 focus:outline-none"
      />
      <div className="flex items-center justify-between gap-2 px-3 pb-2.5">
        <div className="flex flex-wrap items-center gap-1">
          {isNewChat && onOpenProjectPicker && (
            <button
              type="button"
              onClick={onOpenProjectPicker}
              className="flex max-w-60 cursor-pointer items-center gap-1.5 rounded-lg px-2.5 py-1 text-xs font-medium text-zinc-300 transition-colors hover:bg-white/8"
            >
              {useWorktree ? (
                <GitBranch className="size-3.5 shrink-0 text-emerald-400" />
              ) : hasExtraDirs ? (
                <FolderTree className="size-3.5 shrink-0 text-emerald-400" />
              ) : (
                <Folder className="size-3.5 shrink-0 text-sky-400" />
              )}
              <span className="truncate">{projectLabel ?? "选择项目目录"}</span>
              <ChevronDown className="size-3 shrink-0 opacity-50" />
            </button>
          )}

          <button
            type="button"
            onClick={onOpenProviderPicker}
            className="flex cursor-pointer items-center gap-1.5 rounded-lg px-2.5 py-1 text-xs font-medium text-zinc-300 transition-colors hover:bg-white/8"
          >
            <Sparkles className="size-3.5 text-sky-400" />
            <span>{provMeta.shortLabel}</span>
            <ChevronDown className="size-3 opacity-50" />
          </button>

          <button
            type="button"
            onClick={onOpenModelPicker}
            className="flex max-w-44 cursor-pointer items-center gap-1.5 rounded-lg px-2.5 py-1 text-xs font-medium text-zinc-300 transition-colors hover:bg-white/8"
          >
            <BrainCircuit className="size-3.5 shrink-0 text-violet-400" />
            <span className="truncate">{modelLabel}</span>
            <ChevronDown className="size-3 shrink-0 opacity-50" />
          </button>

          <button
            type="button"
            onClick={onOpenModePicker}
            className="flex cursor-pointer items-center gap-1.5 rounded-lg px-2.5 py-1 text-xs font-medium text-zinc-300 transition-colors hover:bg-white/8"
          >
            <Zap className="size-3.5 text-amber-400" />
            <span>{modeMeta.label}</span>
            <ChevronDown className="size-3 opacity-50" />
          </button>
        </div>

        <div className="flex items-center gap-1.5">
          {isRunning && hasText && (
            <button
              type="button"
              onClick={onStop}
              title="中断当前回合"
              className="flex size-8 cursor-pointer items-center justify-center rounded-full bg-white/10 text-zinc-200 hover:bg-white/20"
            >
              <Square className="size-3.5" />
            </button>
          )}
          <button
            type="button"
            disabled={!hasText && !isRunning}
            onClick={submit}
            className={cn(
              "flex size-8 cursor-pointer items-center justify-center rounded-full transition-all disabled:cursor-not-allowed disabled:opacity-30",
              isRunning && !hasText
                ? "bg-white/15 text-white hover:bg-white/25"
                : hasText
                  ? "bg-zinc-100 text-zinc-950 hover:bg-white"
                  : "bg-white/8 text-zinc-500",
            )}
          >
            {isRunning && !hasText ? (
              <Square className="size-3.5" />
            ) : (
              <ArrowUp className="size-4" />
            )}
          </button>
        </div>
      </div>
    </div>
  );
}

// =============================================================================
// MAIN TAURI DESKTOP APPLICATION (`Orbit v0.2.1`)
// =============================================================================

export default function App() {
  const [fleet, setFleet] = useState<FleetOverviewView>({
    revision: 0,
    total_pending_approvals: 0,
    hosts: [],
    projects: [],
  });
  const [hostDetailsById, setHostDetailsById] = useState<Record<string, HostDetailView>>({});
  const [selectedHostId, setSelectedHostId] = useState<string | null>(null);
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);
  const [timeline, setTimeline] = useState<AgentTimelineView | null>(null);
  const [gitReview, setGitReview] = useState<GitReviewView | null>(null);
  const [selectedDiffPath, setSelectedDiffPath] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<string>("chat");
  const [collapsedProjects, setCollapsedProjects] = useState<Record<string, boolean>>({});

  // Draft composer state for New Chat
  const [draftProjectId, setDraftProjectId] = useState<string | null>(null);
  const [draftProvider, setDraftProvider] = useState<ProviderKind>("antigravity");
  const [draftModelByProv, setDraftModelByProv] = useState<Partial<Record<ProviderKind, string>>>(
    {},
  );
  const [draftMode, setDraftMode] = useState<SessionMode>("accept_edits");
  const [draftWorktree, setDraftWorktree] = useState<boolean>(false);

  // Terminal raw stream buffer per host
  const termSeqRef = useRef<number>(0);
  const [termChunksByHost, setTermChunksByHost] = useState<Record<string, XtermStreamChunk[]>>({});

  // Choice / Config Modals
  const [projectPickerOpen, setProjectPickerOpen] = useState(false);
  const [providerPickerOpen, setProviderPickerOpen] = useState(false);
  const [modelPickerOpen, setModelPickerOpen] = useState(false);
  const [modePickerOpen, setModePickerOpen] = useState(false);

  // Directory Browser & Project Multi-Directory Configurator
  const [dirBrowserOpen, setDirBrowserOpen] = useState(false);
  const [dirBrowserHostId, setDirBrowserHostId] = useState<string | null>(null);
  const [appendTargetProject, setAppendTargetProject] = useState<ClientProjectView | null>(null);
  const [managingProject, setManagingProject] = useState<ClientProjectView | null>(null);
  const [editingProjectName, setEditingProjectName] = useState("");

  // Machines & Host Config Tuning Modal
  const [machinesModalOpen, setMachinesModalOpen] = useState(false);
  const [sshHosts, setSshHosts] = useState<SshHostItem[]>([]);
  const [sshConnectingAlias, setSshConnectingAlias] = useState<string | null>(null);
  const [pairingInput, setPairingInput] = useState("");
  const [machineFeedback, setMachineFeedback] = useState<string | null>(null);

  const [editingHost, setEditingHost] = useState<HostCardView | null>(null);
  const [cfgName, setCfgName] = useState("");
  const [cfgRouteMode, setCfgRouteMode] = useState<HostRouteMode>("auto");
  const [cfgSshAlias, setCfgSshAlias] = useState("");
  const [cfgRemotePort, setCfgRemotePort] = useState("6868");
  const [cfgDirectEp, setCfgDirectEp] = useState("");
  const [cfgRelayUrl, setCfgRelayUrl] = useState("");
  const [cfgStatusMsg, setCfgStatusMsg] = useState<string | null>(null);
  const [cfgBusy, setCfgBusy] = useState(false);

  const chatEndRef = useRef<HTMLDivElement | null>(null);
  const selectedHostIdRef = useRef<string | null>(null);
  const selectedAgentIdRef = useRef<string | null>(null);
  selectedHostIdRef.current = selectedHostId;
  selectedAgentIdRef.current = selectedAgentId;

  const reloadHost = useCallback(async (hostId: string) => {
    const detail = await orbitIpc.getHostDetail(hostId);
    if (!detail) return;
    setHostDetailsById((prev) => ({ ...prev, [hostId]: detail }));

    // Seamless 0ms handoff when an optimistic `opt_*` session becomes a real `ag_*` session!
    const curAgent = selectedAgentIdRef.current;
    if (
      selectedHostIdRef.current === hostId &&
      curAgent &&
      detail.replaced_optimistic_agents[curAgent]
    ) {
      const realId = detail.replaced_optimistic_agents[curAgent];
      setSelectedAgentId(realId);
      void orbitIpc.openAgentTimeline(hostId, realId);
      void orbitIpc.getAgentTimeline(hostId, realId).then(setTimeline);
    }
  }, []);

  const reloadFleet = useCallback(async () => {
    const snap = await orbitIpc.getFleetOverview();
    setFleet(snap);
    for (const h of snap.hosts) {
      void reloadHost(h.host_id);
    }
  }, [reloadHost]);

  useEffect(() => {
    void reloadFleet();
    void orbitIpc.listSshHosts().then(setSshHosts);
    void orbitIpc.subscribe((ev: CoreNotification) => {
      if (ev.kind === "fleet_changed") {
        void reloadFleet();
      } else if (ev.kind === "host_changed") {
        void reloadFleet();
        void reloadHost(ev.host_id);
      } else if (ev.kind === "timeline_changed") {
        if (ev.host_id === selectedHostIdRef.current && ev.agent_id === selectedAgentIdRef.current) {
          void orbitIpc.getAgentTimeline(ev.host_id, ev.agent_id).then(setTimeline);
        }
      } else if (ev.kind === "git_changed") {
        if (ev.host_id === selectedHostIdRef.current) {
          void orbitIpc.getGitReview(ev.host_id, ev.cwd, null).then(setGitReview);
        }
      } else if (ev.kind === "terminal_changed") {
        const seq = ++termSeqRef.current;
        const bytes = Uint8Array.from(ev.raw_bytes);
        setTermChunksByHost((prev) => {
          const existing = prev[ev.host_id] ?? [];
          return { ...prev, [ev.host_id]: [...existing.slice(-64), { seq, bytes }] };
        });
      }
    });
  }, [reloadFleet, reloadHost]);

  // Build Paseo-style `Project -> Workspace` hierarchy across ALL hosts
  const sidebarGroups = useMemo<SidebarProjectGroup[]>(() => {
    const hostMap = new Map(fleet.hosts.map((h) => [h.host_id, h]));
    const projMap = new Map<string, ClientProjectView>();

    for (const p of fleet.projects) {
      if (hostMap.has(p.host_id)) {
        projMap.set(p.project_id, p);
      }
    }

    // Ensure every workspace, agent, or top discovered project on each host has a Project group
    for (const host of fleet.hosts) {
      const detail = hostDetailsById[host.host_id];
      if (!detail) continue;

      for (const ws of detail.workspaces) {
        if (ws.is_worktree) continue;
        const exists = Array.from(projMap.values()).some(
          (p) => p.host_id === host.host_id && matchesProjectCwd(p, ws.cwd),
        );
        if (!exists) {
          const id = `${host.host_id}::${ws.cwd.replace(/\/+$/, "")}`;
          projMap.set(id, {
            project_id: id,
            host_id: host.host_id,
            name: ws.title || folderBasename(ws.cwd),
            directories: [ws.cwd],
            updated_at_ms: 0,
          });
        }
      }

      for (const ag of detail.agents) {
        const exists = Array.from(projMap.values()).some(
          (p) => p.host_id === host.host_id && matchesProjectCwd(p, ag.cwd),
        );
        if (!exists) {
          const id = `${host.host_id}::${ag.cwd.replace(/\/+$/, "")}`;
          projMap.set(id, {
            project_id: id,
            host_id: host.host_id,
            name: folderBasename(ag.cwd),
            directories: [ag.cwd],
            updated_at_ms: 0,
          });
        }
      }

      for (const dp of detail.discovered_projects.slice(0, 6)) {
        const cleanDp = dp.path.replace(/\/+$/, "");
        const parts = cleanDp.split("/");
        const isBareHome =
          (parts.length === 3 && (parts[1] === "home" || parts[1] === "Users")) ||
          cleanDp === "~";
        if (isBareHome && Array.from(projMap.values()).some((p) => p.host_id === host.host_id)) {
          continue;
        }
        const exists = Array.from(projMap.values()).some(
          (p) => p.host_id === host.host_id && matchesProjectCwd(p, cleanDp),
        );
        if (!exists) {
          const id = `${host.host_id}::${cleanDp}`;
          projMap.set(id, {
            project_id: id,
            host_id: host.host_id,
            name: dp.name,
            directories: [cleanDp],
            updated_at_ms: dp.last_modified_unix * 1000,
          });
        }
      }
    }

    const assignedAgents = new Set<string>();
    const groups: SidebarProjectGroup[] = [];

    for (const proj of projMap.values()) {
      const host = hostMap.get(proj.host_id);
      if (!host) continue;
      const detail = hostDetailsById[host.host_id];
      const wsById = new Map((detail?.workspaces ?? []).map((w) => [w.workspace_id, w]));
      const threads: SidebarProjectGroup["threads"] = [];

      for (const ag of detail?.agents ?? []) {
        const key = `${host.host_id}::${ag.agent_id}`;
        if (!assignedAgents.has(key) && matchesProjectCwd(proj, ag.cwd)) {
          assignedAgents.add(key);
          const ws = wsById.get(ag.workspace_id);
          threads.push({
            host,
            agent: ag,
            isWorktree: Boolean(ws?.is_worktree || ag.cwd.includes("/.orbit/worktrees/")),
            gitBranch: ws?.git_branch ?? null,
          });
        }
      }
      groups.push({ project: proj, host, threads });
    }

    groups.sort((a, b) => {
      const aParts = (a.project.directories[0] ?? "").replace(/\/+$/, "").split("/");
      const bParts = (b.project.directories[0] ?? "").replace(/\/+$/, "").split("/");
      const aHome = aParts.length <= 3 && (aParts[1] === "home" || aParts[1] === "Users") ? 1 : 0;
      const bHome = bParts.length <= 3 && (bParts[1] === "home" || bParts[1] === "Users") ? 1 : 0;
      if (aHome !== bHome) return aHome - bHome;
      return b.project.updated_at_ms - a.project.updated_at_ms;
    });

    return groups;
  }, [fleet, hostDetailsById]);

  const activeDraftProject = useMemo<ClientProjectView | null>(() => {
    if (sidebarGroups.length === 0) return null;
    if (draftProjectId) {
      const hit = sidebarGroups.find((g) => g.project.project_id === draftProjectId);
      if (hit) return hit.project;
    }
    return sidebarGroups[0]?.project ?? null;
  }, [sidebarGroups, draftProjectId]);

  const currentHostDetail = selectedHostId ? hostDetailsById[selectedHostId] ?? null : null;
  const currentAgent = useMemo(
    () => currentHostDetail?.agents.find((a) => a.agent_id === selectedAgentId) ?? null,
    [currentHostDetail, selectedAgentId],
  );
  const currentWorkspaceCwd =
    currentAgent?.cwd ?? activeDraftProject?.directories[0] ?? "~";

  // Smooth auto-follow scroll on new messages & streaming deltas
  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [timeline?.revision, currentAgent?.is_running]);

  useEffect(() => {
    if (!selectedHostId || !selectedAgentId) {
      setTimeline(null);
      return;
    }
    void orbitIpc.openAgentTimeline(selectedHostId, selectedAgentId);
    void orbitIpc.getAgentTimeline(selectedHostId, selectedAgentId).then(setTimeline);
    if (currentAgent?.cwd) {
      void orbitIpc.fetchGitStatus(selectedHostId, currentAgent.cwd);
    }
  }, [selectedHostId, selectedAgentId, currentAgent?.cwd]);

  useEffect(() => {
    if (activeTab === "git" && selectedHostId && currentWorkspaceCwd) {
      void orbitIpc.fetchGitStatus(selectedHostId, currentWorkspaceCwd);
      void orbitIpc.fetchGitDiff(selectedHostId, currentWorkspaceCwd, selectedDiffPath, false);
      void orbitIpc
        .getGitReview(selectedHostId, currentWorkspaceCwd, selectedDiffPath)
        .then(setGitReview);
    }
  }, [activeTab, selectedHostId, currentWorkspaceCwd, selectedDiffPath]);

  // Resolve available models & active model label for Composer
  const effectiveProvider: ProviderKind = currentAgent ? currentAgent.provider : draftProvider;
  const effectiveMode: SessionMode = currentAgent ? currentAgent.mode : draftMode;
  const effectiveHostId =
    (currentAgent ? selectedHostId : activeDraftProject?.host_id) ??
    fleet.hosts[0]?.host_id ??
    null;

  const availableModels = useMemo<ModelOption[]>(() => {
    const live = effectiveHostId
      ? hostDetailsById[effectiveHostId]?.models_by_provider[effectiveProvider]
      : undefined;
    return live && live.length > 0 ? live : FALLBACK_MODELS[effectiveProvider];
  }, [effectiveHostId, hostDetailsById, effectiveProvider]);

  const effectiveModelId = useMemo<string>(() => {
    if (currentAgent?.model) return currentAgent.model;
    const saved = draftModelByProv[effectiveProvider];
    if (saved) return saved;
    return (
      availableModels.find((m) => m.is_default)?.id ?? availableModels[0]?.id ?? ""
    );
  }, [currentAgent, draftModelByProv, effectiveProvider, availableModels]);

  const effectiveModelLabel = useMemo(() => {
    const hit = availableModels.find((m) => m.id === effectiveModelId);
    return hit ? shortModelLabel(hit.label) : effectiveModelId || "默认模型";
  }, [availableModels, effectiveModelId]);

  // --- Actions ---

  const openDirectoryBrowser = (hostId?: string, appendProj?: ClientProjectView | null) => {
    const targetHost =
      appendProj?.host_id ??
      hostId ??
      activeDraftProject?.host_id ??
      fleet.hosts[0]?.host_id ??
      null;
    if (!targetHost) {
      setMachinesModalOpen(true);
      return;
    }
    setAppendTargetProject(appendProj ?? null);
    setDirBrowserHostId(targetHost);
    setDirBrowserOpen(true);
    void orbitIpc.listDirectory(targetHost, null);
  };

  const handleConfirmDirectory = async (chosenPath: string) => {
    if (!dirBrowserHostId) return;
    if (appendTargetProject) {
      const merged = Array.from(new Set([...appendTargetProject.directories, chosenPath]));
      const updated = await orbitIpc.upsertProject({
        projectId: appendTargetProject.project_id,
        hostId: appendTargetProject.host_id,
        name: appendTargetProject.name,
        directories: merged,
      });
      setManagingProject(updated);
      setDirBrowserOpen(false);
      await reloadFleet();
      return;
    }
    const created = await orbitIpc.upsertProject({
      hostId: dirBrowserHostId,
      name: folderBasename(chosenPath),
      directories: [chosenPath],
    });
    setDraftProjectId(created.project_id);
    setSelectedAgentId(null);
    setDirBrowserOpen(false);
    await reloadFleet();
  };

  const handleStartNewConversation = async (prompt: string) => {
    const proj = activeDraftProject;
    const hostId = proj?.host_id ?? fleet.hosts[0]?.host_id;
    if (!hostId) {
      setMachinesModalOpen(true);
      return;
    }
    const primaryCwd = proj?.directories[0] ?? "~";
    const extraDirs = proj ? proj.directories.slice(1) : [];
    const optAgentId = await orbitIpc.createAgent({
      hostId,
      cwd: primaryCwd,
      extraDirs,
      provider: draftProvider,
      model: effectiveModelId || null,
      mode: draftMode,
      initialPrompt: prompt,
      worktreeBranch: draftWorktree ? "auto" : null,
    });
    setSelectedHostId(hostId);
    setSelectedAgentId(optAgentId);
    setActiveTab("chat");
    await reloadHost(hostId);
    const tl = await orbitIpc.getAgentTimeline(hostId, optAgentId);
    setTimeline(tl);
  };

  const handleSendFollowUp = async (prompt: string) => {
    if (!selectedHostId || !selectedAgentId) return;
    await orbitIpc.sendPrompt(selectedHostId, selectedAgentId, prompt);
    const tl = await orbitIpc.getAgentTimeline(selectedHostId, selectedAgentId);
    setTimeline(tl);
  };

  const handleConnectSshOneClick = async (entry: SshHostItem) => {
    setSshConnectingAlias(entry.alias);
    setMachineFeedback(`正在通过 SSH 自动部署并接入 ${entry.alias}…`);
    try {
      const card = await orbitIpc.pairSshHost(entry.alias, 6868);
      setSelectedHostId(card.host_id);
      setMachineFeedback(`已成功接入 ${card.display_name}`);
      await reloadFleet();
    } catch (e) {
      setMachineFeedback(`SSH 接入失败: ${String(e)}`);
    } finally {
      setSshConnectingAlias(null);
    }
  };

  const openHostSettings = (host: HostCardView) => {
    setEditingHost(host);
    setCfgName(host.custom_name ?? host.display_name);
    setCfgRouteMode(host.route_mode);
    setCfgSshAlias(host.ssh_alias ?? "");
    setCfgRemotePort(String(host.remote_daemon_port || 6868));
    setCfgDirectEp(host.custom_direct_endpoint ?? "");
    setCfgRelayUrl(host.relay_url ?? "");
    setCfgStatusMsg(null);
  };

  const handleSaveHostSettings = async () => {
    if (!editingHost) return;
    setCfgBusy(true);
    try {
      await orbitIpc.updateHostConfig({
        hostId: editingHost.host_id,
        customName: cfgName,
        routeMode: cfgRouteMode,
        sshAlias: cfgSshAlias,
        remoteDaemonPort: Number(cfgRemotePort) || 6868,
        customDirectEndpoint: cfgDirectEp,
        relayUrl: cfgRelayUrl,
        reconnect: true,
      });
      setEditingHost(null);
      await reloadFleet();
    } catch (e) {
      setCfgStatusMsg(`保存失败: ${String(e)}`);
    } finally {
      setCfgBusy(false);
    }
  };

  const handleUpgradeRemoteDaemon = async () => {
    if (!editingHost || !cfgSshAlias.trim()) {
      setCfgStatusMsg("请先填写 SSH Host 别名（如 devbox）再执行升级");
      return;
    }
    setCfgBusy(true);
    setCfgStatusMsg(`正在通过 SSH 升级 ${cfgSshAlias.trim()} 上的守护进程…`);
    try {
      const ver = await orbitIpc.upgradeRemoteDaemon(
        editingHost.host_id,
        cfgSshAlias.trim(),
        Number(cfgRemotePort) || 6868,
      );
      setCfgStatusMsg(`升级并重启成功 (${ver})`);
      await reloadFleet();
    } catch (e) {
      setCfgStatusMsg(`升级失败: ${String(e)}`);
    } finally {
      setCfgBusy(false);
    }
  };

  const onlineHostsCount = fleet.hosts.filter((h) => isHostOnline(h.connection_state)).length;
  const activeHostObj = selectedHostId
    ? fleet.hosts.find((h) => h.host_id === selectedHostId) ?? null
    : null;
  const changedFilesCount = gitReview?.status?.files.length ?? 0;

  const draftProjectPillLabel = useMemo(() => {
    if (!activeDraftProject) return "浏览添加项目";
    const hostName =
      fleet.hosts.find((h) => h.host_id === activeDraftProject.host_id)?.display_name ?? "";
    const extraCount = activeDraftProject.directories.length - 1;
    return extraCount > 0
      ? `${activeDraftProject.name} (+${extraCount}) · ${hostName}`
      : `${activeDraftProject.name} · ${hostName}`;
  }, [activeDraftProject, fleet.hosts]);

  const dirBrowserDetail = dirBrowserHostId ? hostDetailsById[dirBrowserHostId] : null;
  const dirListing = dirBrowserDetail?.directory_listing ?? null;

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-[#0B0D12] text-zinc-100">
      {/* =====================================================================
          PASEO-STYLE LEFT SIDEBAR (`Project -> Workspace` Hierarchy)
      ===================================================================== */}
      <aside className="flex w-72 shrink-0 flex-col border-r border-white/8 bg-[#0F1219]">
        {/* Brand + "+ 新对话" */}
        <div className="flex items-center justify-between px-3.5 pt-3.5 pb-2.5">
          <div className="flex min-w-0 items-center gap-2.5">
            <img
              src="/orbit-logo.png"
              alt="Orbit"
              className="size-7 shrink-0 rounded-lg ring-1 ring-white/15"
            />
            <span className="truncate text-sm font-bold tracking-tight">Orbit</span>
          </div>
          <Button
            size="xs"
            variant={selectedAgentId === null ? "secondary" : "outline"}
            onClick={() => {
              setSelectedAgentId(null);
              setActiveTab("chat");
            }}
          >
            <Plus className="size-3.5" /> 新对话
          </Button>
        </div>

        {/* Section Header + "+ 添加项目" */}
        {fleet.hosts.length > 0 && (
          <div className="flex items-center justify-between px-3.5 py-1.5 text-[11px]">
            <span className="font-semibold text-zinc-500">项目与工作区</span>
            <button
              type="button"
              onClick={() => openDirectoryBrowser(undefined, null)}
              className="flex cursor-pointer items-center gap-1 rounded px-1.5 py-0.5 font-medium text-sky-400 hover:bg-sky-500/10"
            >
              <FolderPlus className="size-3.5" /> 添加项目
            </button>
          </div>
        )}

        {/* Project -> Workspace Tree */}
        <div className="flex-1 space-y-2 overflow-y-auto px-2.5 py-1.5">
          {fleet.hosts.length === 0 ? (
            <div className="rounded-xl border border-dashed border-white/10 p-4 text-center text-xs text-zinc-500">
              先在右侧一键接入一台开发机器即可开启对话
            </div>
          ) : sidebarGroups.length === 0 ? (
            <div className="space-y-2 rounded-xl border border-dashed border-white/10 p-4 text-center text-xs text-zinc-500">
              <div>点击上方「添加项目」浏览机器上的工作目录</div>
              <Button
                size="xs"
                variant="outline"
                onClick={() => openDirectoryBrowser(undefined, null)}
              >
                浏览选择目录
              </Button>
            </div>
          ) : (
            sidebarGroups.map((group) => {
              const { project, host, threads } = group;
              const isCollapsed = Boolean(collapsedProjects[project.project_id]);
              const extraDirsCount = project.directories.length - 1;
              const isDraftSelected =
                selectedAgentId === null &&
                activeDraftProject?.project_id === project.project_id;

              return (
                <div key={project.project_id} className="space-y-0.5">
                  {/* Project Group Header */}
                  <div
                    className={cn(
                      "group flex items-center justify-between rounded-lg px-2 py-1.5 transition-colors",
                      isDraftSelected ? "bg-sky-500/10" : "hover:bg-white/[0.04]",
                    )}
                  >
                    <button
                      type="button"
                      onClick={() =>
                        setCollapsedProjects((prev) => ({
                          ...prev,
                          [project.project_id]: !isCollapsed,
                        }))
                      }
                      className="flex min-w-0 flex-1 cursor-pointer items-center gap-1.5 text-left"
                    >
                      {isCollapsed ? (
                        <ChevronRight className="size-3.5 shrink-0 text-zinc-500" />
                      ) : (
                        <ChevronDown className="size-3.5 shrink-0 text-zinc-500" />
                      )}
                      {extraDirsCount > 0 ? (
                        <FolderTree className="size-3.5 shrink-0 text-emerald-400" />
                      ) : (
                        <Folder className="size-3.5 shrink-0 text-sky-400" />
                      )}
                      <span className="truncate text-xs font-semibold text-zinc-200">
                        {project.name}
                      </span>
                      {extraDirsCount > 0 && (
                        <span className="shrink-0 rounded bg-emerald-500/15 px-1 py-0.2 text-[9.5px] font-medium text-emerald-300">
                          +{extraDirsCount}
                        </span>
                      )}
                      <span
                        className={cn(
                          "truncate text-[10.5px]",
                          isHostOnline(host.connection_state)
                            ? "text-zinc-500"
                            : "text-amber-400/80",
                        )}
                      >
                        {host.display_name}
                      </span>
                    </button>

                    <div className="flex shrink-0 items-center gap-0.5 opacity-80 group-hover:opacity-100">
                      <button
                        type="button"
                        title={`在 ${project.name} 开启新工作区会话`}
                        onClick={() => {
                          setDraftProjectId(project.project_id);
                          setSelectedAgentId(null);
                          setActiveTab("chat");
                        }}
                        className="cursor-pointer rounded p-1 text-zinc-400 hover:bg-white/10 hover:text-white"
                      >
                        <Plus className="size-3.5" />
                      </button>
                      <button
                        type="button"
                        title="管理多工作目录 / 重命名 / Worktree"
                        onClick={() => {
                          setManagingProject(project);
                          setEditingProjectName(project.name);
                        }}
                        className="cursor-pointer rounded p-1 text-zinc-400 hover:bg-white/10 hover:text-white"
                      >
                        <MoreHorizontal className="size-3.5" />
                      </button>
                    </div>
                  </div>

                  {/* Workspaces / Sessions under Project */}
                  {!isCollapsed && (
                    <div className="ml-3.5 space-y-0.5 border-l border-white/6 pl-2">
                      {threads.length === 0 ? (
                        <button
                          type="button"
                          onClick={() => {
                            setDraftProjectId(project.project_id);
                            setSelectedAgentId(null);
                            setActiveTab("chat");
                          }}
                          className="w-full cursor-pointer rounded-md px-2 py-1 text-left text-[11px] text-zinc-500 hover:bg-white/[0.03] hover:text-zinc-300"
                        >
                          + 开启新工作区会话…
                        </button>
                      ) : (
                        threads.map(({ agent: ag, isWorktree, gitBranch }) => {
                          const selected =
                            selectedHostId === host.host_id && selectedAgentId === ag.agent_id;
                          return (
                            <div
                              key={ag.agent_id}
                              onClick={() => {
                                setSelectedHostId(host.host_id);
                                setSelectedAgentId(ag.agent_id);
                                setActiveTab("chat");
                              }}
                              className={cn(
                                "group flex cursor-pointer items-center justify-between gap-1.5 rounded-lg px-2.5 py-1.5 transition-colors",
                                selected
                                  ? "bg-white/10 text-white"
                                  : "text-zinc-300 hover:bg-white/[0.04]",
                              )}
                            >
                              <div className="min-w-0 flex-1">
                                <div className="flex items-center gap-1.5">
                                  {ag.is_awaiting_permission ? (
                                    <span className="size-2 shrink-0 rounded-full bg-amber-400" />
                                  ) : ag.is_running ? (
                                    <Loader2 className="size-3 shrink-0 animate-spin text-sky-400" />
                                  ) : isWorktree ? (
                                    <GitBranch className="size-3 shrink-0 text-emerald-400" />
                                  ) : null}
                                  <span className="truncate text-xs font-medium">{ag.title}</span>
                                </div>
                                <div className="truncate text-[10.5px] text-zinc-500">
                                  {gitBranch ? `${gitBranch} · ` : ""}
                                  {ag.provider_label}
                                  {ag.model ? ` · ${shortModelLabel(ag.model)}` : ""}
                                </div>
                              </div>
                              <button
                                type="button"
                                title="归档移除会话"
                                onClick={(e) => {
                                  e.stopPropagation();
                                  if (selectedAgentId === ag.agent_id) {
                                    setSelectedAgentId(null);
                                  }
                                  void orbitIpc.archiveAgent(host.host_id, ag.agent_id);
                                }}
                                className="hidden cursor-pointer rounded p-1 text-zinc-500 hover:bg-rose-500/15 hover:text-rose-300 group-hover:block"
                              >
                                <Trash2 className="size-3" />
                              </button>
                            </div>
                          );
                        })
                      )}
                    </div>
                  )}
                </div>
              );
            })
          )}
        </div>

        {/* Machine Status & Tuning Footer */}
        <div className="border-t border-white/8 p-2.5">
          <button
            type="button"
            onClick={() => {
              void orbitIpc.listSshHosts().then(setSshHosts);
              setMachinesModalOpen(true);
            }}
            className="flex w-full cursor-pointer items-center justify-between rounded-xl px-3 py-2 text-xs transition-colors hover:bg-white/[0.05]"
          >
            <span className="flex min-w-0 items-center gap-2">
              <span
                className={cn(
                  "size-2 shrink-0 rounded-full",
                  onlineHostsCount > 0
                    ? "bg-emerald-400"
                    : fleet.hosts.length > 0
                      ? "bg-amber-400"
                      : "bg-zinc-600",
                )}
              />
              <span className="truncate text-zinc-300">
                {fleet.hosts.length === 0
                  ? "连接开发机器"
                  : fleet.hosts.length === 1
                    ? `${fleet.hosts[0].display_name} · ${
                        isHostOnline(fleet.hosts[0].connection_state)
                          ? `在线${fleet.hosts[0].last_ping_ms !== null ? ` ${fleet.hosts[0].last_ping_ms}ms` : ""}`
                          : "重连中…"
                      }`
                    : `${onlineHostsCount}/${fleet.hosts.length} 台机器在线`}
              </span>
            </span>
            <Settings2 className="size-3.5 shrink-0 text-zinc-400" />
          </button>
        </div>
      </aside>

      {/* =====================================================================
          MAIN STAGE (Welcome / Zero-Form New Chat / Active Workspace Stream)
      ===================================================================== */}
      <main className="flex min-w-0 flex-1 flex-col">
        {fleet.hosts.length === 0 ? (
          /* Zero-Form First-Launch Welcome Screen */
          <div className="flex flex-1 items-center justify-center p-8">
            <div className="w-full max-w-md space-y-5">
              <div className="flex flex-col items-center text-center">
                <img
                  src="/orbit-logo.png"
                  alt="Orbit"
                  className="mb-3 size-12 rounded-2xl shadow-lg ring-1 ring-white/15"
                />
                <h1 className="text-xl font-semibold">选择一台开发机器开始对话</h1>
                <p className="mt-1 text-xs text-zinc-400">
                  {sshHosts.length > 0
                    ? "已从 ~/.ssh/config 自动识别以下机器，点一下即可自动部署并建立加密连接"
                    : "也可粘贴开发机 `orbit pair` 配对码一键连入"}
                </p>
              </div>

              {sshHosts.length > 0 && (
                <div className="space-y-2">
                  {sshHosts.slice(0, 6).map((entry) => (
                    <button
                      key={entry.alias}
                      type="button"
                      disabled={sshConnectingAlias !== null}
                      onClick={() => void handleConnectSshOneClick(entry)}
                      className="flex w-full cursor-pointer items-center justify-between rounded-xl border border-white/10 bg-[#141822] px-4 py-3 text-left transition-colors hover:border-sky-500/40 hover:bg-white/[0.04]"
                    >
                      <div>
                        <div className="text-sm font-semibold">{entry.alias}</div>
                        <div className="font-mono text-[11px] text-zinc-500">
                          {entry.display_target}
                        </div>
                      </div>
                      {sshConnectingAlias === entry.alias ? (
                        <Loader2 className="size-4 animate-spin text-sky-400" />
                      ) : (
                        <Badge variant="secondary">一键接入</Badge>
                      )}
                    </button>
                  ))}
                </div>
              )}

              <div className="flex gap-2">
                <Input
                  placeholder="或粘贴 orbit://pair/... 配对码"
                  value={pairingInput}
                  onChange={(e) => setPairingInput(e.target.value)}
                />
                <Button
                  onClick={async () => {
                    if (!pairingInput.trim()) return;
                    try {
                      const c = await orbitIpc.pairHost(pairingInput.trim());
                      setSelectedHostId(c.host_id);
                      setPairingInput("");
                      await reloadFleet();
                    } catch (e) {
                      setMachineFeedback(String(e));
                    }
                  }}
                >
                  连接
                </Button>
              </div>
              {machineFeedback && (
                <div className="text-center text-xs text-sky-300">{machineFeedback}</div>
              )}
            </div>
          </div>
        ) : selectedAgentId === null ? (
          /* Zero-Form Centered New Chat Canvas */
          <div className="flex flex-1 flex-col items-center justify-center px-6 pb-10">
            <div className="w-full max-w-2xl space-y-6">
              <div className="space-y-1.5 text-center">
                <h1 className="text-2xl font-semibold tracking-tight">
                  今天想推进哪块代码？
                </h1>
                <p className="text-xs text-zinc-400">
                  直接输入需求按回车开聊；左下角可随时切换多目录项目、引擎、模型与协作方式
                </p>
              </div>

              <OrbitInlineComposer
                isNewChat
                isRunning={false}
                projectLabel={draftProjectPillLabel}
                hasExtraDirs={(activeDraftProject?.directories.length ?? 0) > 1}
                useWorktree={draftWorktree}
                provider={draftProvider}
                modelLabel={effectiveModelLabel}
                mode={draftMode}
                placeholder={
                  activeDraftProject
                    ? `在 ${activeDraftProject.name} 中直接输入你想做的事…`
                    : "直接输入你想推进的任务…"
                }
                onOpenProjectPicker={() => setProjectPickerOpen(true)}
                onOpenProviderPicker={() => {
                  if (effectiveHostId) void orbitIpc.refreshQuotas(effectiveHostId);
                  setProviderPickerOpen(true);
                }}
                onOpenModelPicker={() => setModelPickerOpen(true)}
                onOpenModePicker={() => setModePickerOpen(true)}
                onSend={(txt) => void handleStartNewConversation(txt)}
                onStop={() => {}}
              />
            </div>
          </div>
        ) : (
          /* Active Workspace Stream / Git Diff / PTY Terminal */
          <Tabs
            value={activeTab}
            onValueChange={setActiveTab}
            className="flex h-full flex-col overflow-hidden"
          >
            <header className="flex h-12 shrink-0 items-center justify-between border-b border-white/8 px-4">
              <div className="flex min-w-0 items-center gap-3">
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="truncate text-sm font-semibold">
                      {currentAgent?.title ?? "工作区对话"}
                    </span>
                    <span className="truncate text-xs text-zinc-500">
                      {folderBasename(currentWorkspaceCwd)} ·{" "}
                      {activeHostObj?.display_name ?? ""}
                    </span>
                  </div>
                </div>
              </div>

              <div className="flex shrink-0 items-center gap-2">
                {activeHostObj && <ConnectionBadge host={activeHostObj} />}
                <TabsList>
                  <TabsTrigger value="chat">
                    <Cpu className="size-3.5" /> 对话流
                  </TabsTrigger>
                  <TabsTrigger value="git">
                    <FolderGit2 className="size-3.5" /> 代码改动
                    {changedFilesCount > 0 && ` (${changedFilesCount})`}
                  </TabsTrigger>
                  <TabsTrigger value="terminal">
                    <TerminalIcon className="size-3.5" /> 终端 PTY
                  </TabsTrigger>
                </TabsList>
              </div>
            </header>

            {/* Non-destructive Reconnect Banner (Preserves chat & queues outgoing messages!) */}
            {activeHostObj && !isHostOnline(activeHostObj.connection_state) && (
              <div className="flex items-center justify-between bg-amber-500/12 px-4 py-1.5 text-xs text-amber-200">
                <span>
                  与 {activeHostObj.display_name}{" "}
                  的连接暂时中断（当前会话与历史已完整保留，发出消息将自动排队重连）
                </span>
                <Button
                  size="xs"
                  variant="outline"
                  onClick={() => void orbitIpc.connectHost(activeHostObj.host_id)}
                >
                  立即重连
                </Button>
              </div>
            )}

            <TabsContent value="chat" className="flex min-h-0 flex-1 flex-col">
              <div className="flex-1 overflow-y-auto px-6 py-5">
                <div className="mx-auto max-w-3xl space-y-3.5">
                  {timeline?.items.map((item, idx) => (
                    <TimelineEntry
                      key={`${item.item_type}-${item.seq}-${idx}`}
                      hostId={selectedHostId ?? ""}
                      agentId={selectedAgentId ?? ""}
                      item={item}
                      isLastItem={idx === (timeline.items.length - 1)}
                      isAgentRunning={Boolean(currentAgent?.is_running)}
                    />
                  ))}

                  {/* Unconditional Live Rotating Thinking / Tool Banner whenever Agent is Running! */}
                  {currentAgent?.is_running && (
                    <LiveTurnActivityBanner
                      agent={currentAgent}
                      items={timeline?.items ?? []}
                    />
                  )}
                  <div ref={chatEndRef} />
                </div>
              </div>

              <div className="px-6 pt-1 pb-4">
                <div className="mx-auto max-w-3xl">
                  <OrbitInlineComposer
                    isNewChat={false}
                    isRunning={Boolean(currentAgent?.is_running)}
                    provider={effectiveProvider}
                    modelLabel={effectiveModelLabel}
                    mode={effectiveMode}
                    placeholder={
                      currentAgent?.is_running
                        ? "正在执行中，随时输入可补充引导方向…"
                        : "继续对话…"
                    }
                    onOpenProviderPicker={() => {
                      if (selectedHostId) void orbitIpc.refreshQuotas(selectedHostId);
                      setProviderPickerOpen(true);
                    }}
                    onOpenModelPicker={() => setModelPickerOpen(true)}
                    onOpenModePicker={() => setModePickerOpen(true)}
                    onSend={(txt) => void handleSendFollowUp(txt)}
                    onStop={() => {
                      if (selectedHostId && selectedAgentId) {
                        void orbitIpc.interruptAgent(selectedHostId, selectedAgentId);
                      }
                    }}
                  />
                </div>
              </div>
            </TabsContent>

            <TabsContent value="git" className="flex min-h-0 flex-1 overflow-hidden">
              <div className="w-64 shrink-0 space-y-1 overflow-y-auto border-r border-white/8 p-3">
                <div className="mb-2 flex items-center justify-between text-xs font-medium">
                  <span className="flex items-center gap-1.5">
                    <GitBranch className="size-3.5 text-emerald-400" />
                    {gitReview?.status?.branch ?? "HEAD"}
                  </span>
                  <Button
                    variant="ghost"
                    size="xs"
                    onClick={() => setSelectedDiffPath(null)}
                  >
                    全部改动
                  </Button>
                </div>
                {gitReview?.status?.files.map((f) => (
                  <button
                    key={f.path}
                    type="button"
                    onClick={() => setSelectedDiffPath(f.path)}
                    className={cn(
                      "flex w-full cursor-pointer items-center justify-between rounded-md px-2.5 py-1.5 text-left font-mono text-xs",
                      selectedDiffPath === f.path
                        ? "bg-white/10 text-white"
                        : "text-zinc-300 hover:bg-white/[0.04]",
                    )}
                  >
                    <span className="truncate">{f.path}</span>
                    <Badge variant="outline" className="ml-1 px-1 py-0 text-[10px]">
                      {f.status}
                    </Badge>
                  </button>
                ))}
              </div>
              <div className="flex-1 overflow-y-auto p-4">
                <DiffFilesViewer files={gitReview?.diff_files ?? []} />
              </div>
            </TabsContent>

            <TabsContent value="terminal" className="min-h-0 flex-1 p-4">
              {selectedHostId && (
                <XtermPane
                  hostId={selectedHostId}
                  cwd={currentWorkspaceCwd}
                  chunks={termChunksByHost[selectedHostId] ?? []}
                />
              )}
            </TabsContent>
          </Tabs>
        )}
      </main>

      {/* =====================================================================
          MODAL 1: PROJECT & WORKTREE PICKER (Zero-Form Choices)
      ===================================================================== */}
      <Dialog open={projectPickerOpen} onOpenChange={setProjectPickerOpen}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center justify-between pr-6">
              <span>选择项目工作环境 (Project)</span>
              <Button
                size="xs"
                variant="outline"
                onClick={() => {
                  setProjectPickerOpen(false);
                  openDirectoryBrowser(activeDraftProject?.host_id, null);
                }}
              >
                <FolderPlus className="size-3.5" /> 浏览机器目录
              </Button>
            </DialogTitle>
            <DialogDescription>
              支持点选任意机器上的项目，也可为一个项目挂载多个关联代码目录 (--add-dir)
            </DialogDescription>
          </DialogHeader>

          <div className="max-h-72 space-y-1.5 overflow-y-auto">
            {sidebarGroups.map(({ project: p, host }) => {
              const selected = activeDraftProject?.project_id === p.project_id;
              return (
                <button
                  key={p.project_id}
                  type="button"
                  onClick={() => {
                    setDraftProjectId(p.project_id);
                    setProjectPickerOpen(false);
                  }}
                  className={cn(
                    "flex w-full cursor-pointer items-center justify-between rounded-xl border px-3.5 py-2.5 text-left transition-colors",
                    selected
                      ? "border-sky-500/50 bg-sky-500/15"
                      : "border-white/8 bg-white/[0.02] hover:bg-white/[0.05]",
                  )}
                >
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="truncate text-sm font-semibold">{p.name}</span>
                      {p.directories.length > 1 && (
                        <Badge variant="success" className="text-[10px]">
                          +{p.directories.length - 1} 目录
                        </Badge>
                      )}
                      <Badge variant="secondary" className="text-[10px]">
                        {host.display_name}
                      </Badge>
                    </div>
                    <div className="truncate font-mono text-[11px] text-zinc-400">
                      {shortenPath(p.directories[0] ?? "")}
                    </div>
                  </div>
                  {selected && <Check className="size-4 shrink-0 text-sky-400" />}
                </button>
              );
            })}
          </div>

          {activeDraftProject && (
            <Button
              size="sm"
              variant="outline"
              onClick={() => {
                setProjectPickerOpen(false);
                setManagingProject(activeDraftProject);
                setEditingProjectName(activeDraftProject.name);
              }}
            >
              <FolderTree className="size-3.5" /> 为「{activeDraftProject.name}
              」管理/挂载多个工作目录 ({activeDraftProject.directories.length})
            </Button>
          )}

          <div className="flex items-center justify-between rounded-xl border border-white/8 bg-white/[0.02] px-3.5 py-2.5">
            <div>
              <div className="text-xs font-semibold">独立分支隔离 (Git Worktree)</div>
              <div className="text-[11px] text-zinc-400">
                开启后自动生成独立分支工作区，不改动主目录
              </div>
            </div>
            <Button
              size="xs"
              variant={draftWorktree ? "default" : "outline"}
              onClick={() => setDraftWorktree((v) => !v)}
            >
              {draftWorktree ? "已开启" : "未开启"}
            </Button>
          </div>
        </DialogContent>
      </Dialog>

      {/* =====================================================================
          MODAL 2: INTERACTIVE REMOTE / LOCAL DIRECTORY BROWSER
      ===================================================================== */}
      <Dialog open={dirBrowserOpen} onOpenChange={setDirBrowserOpen}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>
              {appendTargetProject
                ? `为「${appendTargetProject.name}」添加关联工作目录`
                : "浏览选择工作环境目录"}
            </DialogTitle>
            <DialogDescription>
              直接点选机器上的文件夹，无需手动输入路径
            </DialogDescription>
          </DialogHeader>

          {/* Machine Switcher when adding a new Project */}
          {!appendTargetProject && fleet.hosts.length > 1 && (
            <div className="flex flex-wrap gap-1.5">
              {fleet.hosts.map((h) => (
                <Button
                  key={h.host_id}
                  size="xs"
                  variant={dirBrowserHostId === h.host_id ? "default" : "outline"}
                  onClick={() => {
                    setDirBrowserHostId(h.host_id);
                    void orbitIpc.listDirectory(h.host_id, null);
                  }}
                >
                  {h.display_name}
                </Button>
              ))}
            </div>
          )}

          {/* Auto-discovered quick repos on this host */}
          {dirBrowserDetail && dirBrowserDetail.discovered_projects.length > 0 && (
            <div className="space-y-1">
              <div className="text-[11px] text-zinc-400">自动发现的常用仓库（一键选定）：</div>
              <div className="flex flex-wrap gap-1.5">
                {dirBrowserDetail.discovered_projects.slice(0, 8).map((dp) => (
                  <Button
                    key={dp.path}
                    size="xs"
                    variant="secondary"
                    onClick={() => void handleConfirmDirectory(dp.path)}
                    title={dp.path}
                  >
                    <FolderGit2 className="size-3 text-emerald-400" /> {dp.name}
                  </Button>
                ))}
              </div>
            </div>
          )}

          {/* Current Path + Up / Home Toolbar */}
          <div className="flex items-center gap-1.5 rounded-lg border border-white/10 bg-black/30 px-2.5 py-1.5">
            <Button
              size="xs"
              variant="ghost"
              title="回到主目录 (~)"
              onClick={() => dirBrowserHostId && void orbitIpc.listDirectory(dirBrowserHostId, null)}
            >
              <Home className="size-3.5" />
            </Button>
            {dirListing?.parent_path && (
              <Button
                size="xs"
                variant="ghost"
                title="返回上一级目录"
                onClick={() =>
                  dirBrowserHostId &&
                  void orbitIpc.listDirectory(dirBrowserHostId, dirListing.parent_path)
                }
              >
                <ArrowUpFromLine className="size-3.5" /> 上一级
              </Button>
            )}
            <code className="flex-1 truncate font-mono text-xs text-sky-300">
              {dirListing?.current_path ?? "正在读取目录…"}
            </code>
          </div>

          {/* Subdirectory List */}
          <div className="max-h-64 space-y-1 overflow-y-auto rounded-xl border border-white/8 bg-[#0E121B] p-2">
            {!dirListing ? (
              <div className="flex items-center justify-center gap-2 py-10 text-xs text-zinc-400">
                <Loader2 className="size-4 animate-spin text-sky-400" />
                正在读取远端目录结构…
              </div>
            ) : dirListing.entries.length === 0 ? (
              <div className="py-8 text-center text-xs text-zinc-500">
                当前目录无子文件夹，可直接点击下方选定当前目录
              </div>
            ) : (
              dirListing.entries.map((entry) => (
                <div
                  key={entry.path}
                  className="flex items-center justify-between rounded-lg px-2.5 py-1.5 hover:bg-white/[0.05]"
                >
                  <button
                    type="button"
                    onClick={() =>
                      dirBrowserHostId && void orbitIpc.listDirectory(dirBrowserHostId, entry.path)
                    }
                    className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-left text-xs"
                  >
                    {entry.is_git_repo ? (
                      <FolderGit2 className="size-4 shrink-0 text-emerald-400" />
                    ) : (
                      <Folder className="size-4 shrink-0 text-sky-400" />
                    )}
                    <span className="truncate font-medium">{entry.name}</span>
                    {entry.is_git_repo && (
                      <Badge variant="success" className="px-1.5 py-0 text-[9.5px]">
                        Git
                      </Badge>
                    )}
                  </button>
                  <Button
                    size="xs"
                    variant="ghost"
                    onClick={() => void handleConfirmDirectory(entry.path)}
                  >
                    选定
                  </Button>
                </div>
              ))
            )}
          </div>

          <div className="flex justify-end gap-2">
            <Button variant="outline" onClick={() => setDirBrowserOpen(false)}>
              取消
            </Button>
            <Button
              disabled={!dirListing}
              onClick={() =>
                dirListing && void handleConfirmDirectory(dirListing.current_path)
              }
            >
              选定当前目录：{folderBasename(dirListing?.current_path ?? "")}
            </Button>
          </div>
        </DialogContent>
      </Dialog>

      {/* =====================================================================
          MODAL 3: PROJECT MULTI-DIRECTORY & RENAME SETTINGS
      ===================================================================== */}
      <Dialog
        open={Boolean(managingProject)}
        onOpenChange={(o) => !o && setManagingProject(null)}
      >
        <DialogContent className="max-w-md">
          {managingProject && (
            <>
              <DialogHeader>
                <DialogTitle>项目配置 · {managingProject.name}</DialogTitle>
                <DialogDescription>
                  支持重命名项目、挂载多个关联代码目录 (--add-dir)，或一键开启 Worktree 隔离会话
                </DialogDescription>
              </DialogHeader>

              <div className="space-y-1.5">
                <div className="text-xs font-medium text-zinc-300">项目显示名称</div>
                <div className="flex gap-2">
                  <Input
                    value={editingProjectName}
                    onChange={(e) => setEditingProjectName(e.target.value)}
                  />
                  <Button
                    size="sm"
                    onClick={async () => {
                      const updated = await orbitIpc.upsertProject({
                        projectId: managingProject.project_id,
                        hostId: managingProject.host_id,
                        name: editingProjectName,
                        directories: managingProject.directories,
                      });
                      setManagingProject(updated);
                      await reloadFleet();
                    }}
                  >
                    保存名称
                  </Button>
                </div>
              </div>

              <div className="space-y-2">
                <div className="flex items-center justify-between text-xs font-medium text-zinc-300">
                  <span>挂载的工作目录 ({managingProject.directories.length})</span>
                  <Button
                    size="xs"
                    variant="outline"
                    onClick={() => {
                      const target = managingProject;
                      setManagingProject(null);
                      openDirectoryBrowser(target.host_id, target);
                    }}
                  >
                    <Plus className="size-3.5" /> 添加关联目录
                  </Button>
                </div>
                <div className="max-h-48 space-y-1.5 overflow-y-auto">
                  {managingProject.directories.map((dir, idx) => (
                    <div
                      key={dir}
                      className="flex items-center justify-between gap-2 rounded-lg border border-white/8 bg-white/[0.03] px-3 py-2 text-xs"
                    >
                      <div className="min-w-0">
                        <div className="flex items-center gap-1.5 font-medium">
                          <span className="truncate">{folderBasename(dir)}</span>
                          <Badge variant="secondary" className="text-[9.5px]">
                            {idx === 0 ? "主目录" : "关联目录"}
                          </Badge>
                        </div>
                        <div className="truncate font-mono text-[11px] text-zinc-500">{dir}</div>
                      </div>
                      {idx > 0 && (
                        <Button
                          size="xs"
                          variant="ghost"
                          onClick={async () => {
                            const nextDirs = managingProject.directories.filter((d) => d !== dir);
                            const updated = await orbitIpc.upsertProject({
                              projectId: managingProject.project_id,
                              hostId: managingProject.host_id,
                              name: managingProject.name,
                              directories: nextDirs,
                            });
                            setManagingProject(updated);
                            await reloadFleet();
                          }}
                        >
                          <Trash2 className="size-3.5 text-rose-400" />
                        </Button>
                      )}
                    </div>
                  ))}
                </div>
              </div>

              <div className="flex items-center justify-between pt-2">
                <Button
                  size="sm"
                  variant="destructive"
                  onClick={async () => {
                    await orbitIpc.removeProject(managingProject.project_id);
                    setManagingProject(null);
                    await reloadFleet();
                  }}
                >
                  移除项目
                </Button>
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() => {
                    setDraftProjectId(managingProject.project_id);
                    setDraftWorktree(true);
                    setSelectedAgentId(null);
                    setManagingProject(null);
                  }}
                >
                  <GitBranch className="size-3.5" /> 新建 Worktree 隔离会话
                </Button>
              </div>
            </>
          )}
        </DialogContent>
      </Dialog>

      {/* =====================================================================
          MODAL 4: ENGINE / MODEL / MODE PICKERS
      ===================================================================== */}
      <Dialog open={providerPickerOpen} onOpenChange={setProviderPickerOpen}>
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>选择编程引擎</DialogTitle>
            <DialogDescription>对话中途也可随时切换引擎接力推进</DialogDescription>
          </DialogHeader>
          <div className="space-y-1.5">
            {PROVIDERS.map((p) => {
              const selected = effectiveProvider === p.id;
              const quota = effectiveHostId
                ? hostDetailsById[effectiveHostId]?.quotas.find((q) => q.provider === p.id)
                : undefined;
              const minRem =
                quota && quota.windows.length > 0
                  ? Math.round(
                      Math.min(...quota.windows.map((w) => Math.max(0, 100 - w.used_percent))),
                    )
                  : null;

              return (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => {
                    if (currentAgent && selectedHostId) {
                      void orbitIpc.switchAgentProvider(
                        selectedHostId,
                        currentAgent.agent_id,
                        p.id,
                      );
                    } else {
                      setDraftProvider(p.id);
                    }
                    setProviderPickerOpen(false);
                  }}
                  className={cn(
                    "flex w-full cursor-pointer items-center justify-between rounded-xl border px-3.5 py-2.5 text-left transition-colors",
                    selected
                      ? "border-sky-500/50 bg-sky-500/15"
                      : "border-white/8 bg-white/[0.02] hover:bg-white/[0.05]",
                  )}
                >
                  <div>
                    <div className="flex items-center gap-2 text-sm font-semibold">
                      <span>{p.label}</span>
                      {minRem !== null && (
                        <Badge variant="success" className="text-[10px]">
                          余 {minRem}%
                        </Badge>
                      )}
                    </div>
                    <div className="text-xs text-zinc-400">{p.desc}</div>
                  </div>
                  {selected && <Check className="size-4 text-sky-400" />}
                </button>
              );
            })}
          </div>
        </DialogContent>
      </Dialog>

      <Dialog open={modelPickerOpen} onOpenChange={setModelPickerOpen}>
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>选择模型 ({effectiveProvider})</DialogTitle>
            <DialogDescription>支持发消息前或对话中途实时切换推理模型</DialogDescription>
          </DialogHeader>
          <div className="max-h-80 space-y-1.5 overflow-y-auto">
            {availableModels.map((m) => {
              const selected = effectiveModelId === m.id;
              return (
                <button
                  key={m.id}
                  type="button"
                  onClick={() => {
                    setDraftModelByProv((prev) => ({ ...prev, [effectiveProvider]: m.id }));
                    if (currentAgent && selectedHostId) {
                      void orbitIpc.setAgentModel(selectedHostId, currentAgent.agent_id, m.id);
                    }
                    setModelPickerOpen(false);
                  }}
                  className={cn(
                    "flex w-full cursor-pointer items-center justify-between rounded-xl border px-3.5 py-2.5 text-left transition-colors",
                    selected
                      ? "border-violet-500/50 bg-violet-500/15"
                      : "border-white/8 bg-white/[0.02] hover:bg-white/[0.05]",
                  )}
                >
                  <div className="min-w-0 pr-2">
                    <div className="flex items-center gap-2 text-sm font-semibold">
                      <span className="truncate">{m.label}</span>
                      {m.is_default && (
                        <Badge variant="secondary" className="text-[10px]">
                          默认
                        </Badge>
                      )}
                    </div>
                    {m.description && (
                      <div className="truncate text-xs text-zinc-400">{m.description}</div>
                    )}
                  </div>
                  {selected && <Check className="size-4 shrink-0 text-violet-400" />}
                </button>
              );
            })}
          </div>
        </DialogContent>
      </Dialog>

      <Dialog open={modePickerOpen} onOpenChange={setModePickerOpen}>
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>选择协作方式</DialogTitle>
            <DialogDescription>对话中途可随时从「先出方案」切到「自动改码」</DialogDescription>
          </DialogHeader>
          <div className="space-y-1.5">
            {MODES.map((m) => {
              const selected = effectiveMode === m.id;
              return (
                <button
                  key={m.id}
                  type="button"
                  onClick={() => {
                    if (currentAgent && selectedHostId) {
                      void orbitIpc.setSessionMode(selectedHostId, currentAgent.agent_id, m.id);
                    } else {
                      setDraftMode(m.id);
                    }
                    setModePickerOpen(false);
                  }}
                  className={cn(
                    "flex w-full cursor-pointer items-center justify-between rounded-xl border px-3.5 py-2.5 text-left transition-colors",
                    selected
                      ? "border-sky-500/50 bg-sky-500/15"
                      : "border-white/8 bg-white/[0.02] hover:bg-white/[0.05]",
                  )}
                >
                  <div className="pr-2">
                    <div className="text-sm font-semibold">{m.label}</div>
                    <div className="text-xs leading-relaxed text-zinc-400">{m.desc}</div>
                  </div>
                  {selected && <Check className="size-4 shrink-0 text-sky-400" />}
                </button>
              );
            })}
          </div>
        </DialogContent>
      </Dialog>

      {/* =====================================================================
          MODAL 5: MACHINES MANAGEMENT & HOST TUNING / RENAMING
      ===================================================================== */}
      <Dialog open={machinesModalOpen} onOpenChange={setMachinesModalOpen}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>开发机器管理与连接配置</DialogTitle>
            <DialogDescription>
              点击任一机器的「配置」可重命名机器、调整 SSH 隧道 / 直连策略或一键升级远端守护进程
            </DialogDescription>
          </DialogHeader>

          <div className="max-h-72 space-y-2 overflow-y-auto">
            {fleet.hosts.map((h) => (
              <div
                key={h.host_id}
                className="flex items-center justify-between gap-2 rounded-xl border border-white/8 bg-white/[0.03] px-3.5 py-2.5"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <Server className="size-3.5 shrink-0 text-sky-400" />
                    <span className="truncate text-sm font-semibold">{h.display_name}</span>
                    {h.daemon_version && (
                      <span className="font-mono text-[10.5px] text-zinc-500">
                        v{h.daemon_version}
                      </span>
                    )}
                  </div>
                  <div className="mt-1 flex items-center gap-2">
                    <ConnectionBadge host={h} />
                    {h.last_error && (
                      <span className="truncate text-[11px] text-rose-400">{h.last_error}</span>
                    )}
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  <Button
                    size="xs"
                    variant="outline"
                    onClick={() => openHostSettings(h)}
                  >
                    <Settings2 className="size-3.5" /> 配置 / 重命名
                  </Button>
                  <Button
                    size="xs"
                    variant="ghost"
                    title="立即重连"
                    onClick={() => void orbitIpc.connectHost(h.host_id)}
                  >
                    <RefreshCw className="size-3.5" />
                  </Button>
                </div>
              </div>
            ))}
          </div>

          {sshHosts.length > 0 && (
            <div className="space-y-1.5">
              <div className="text-xs font-medium text-zinc-400">
                从 ~/.ssh/config 一键接入新机器
              </div>
              <div className="max-h-36 space-y-1 overflow-y-auto rounded-xl border border-white/8 p-2">
                {sshHosts.map((s) => (
                  <div
                    key={s.alias}
                    className="flex items-center justify-between rounded-lg px-2.5 py-1.5 text-xs hover:bg-white/[0.04]"
                  >
                    <div>
                      <span className="font-semibold">{s.alias}</span>
                      <span className="ml-2 font-mono text-[11px] text-zinc-500">
                        {s.display_target}
                      </span>
                    </div>
                    <Button
                      size="xs"
                      disabled={sshConnectingAlias !== null}
                      onClick={() => void handleConnectSshOneClick(s)}
                    >
                      {sshConnectingAlias === s.alias ? "接入中…" : "一键接入"}
                    </Button>
                  </div>
                ))}
              </div>
            </div>
          )}

          <div className="flex gap-2">
            <Input
              placeholder="粘贴 orbit://pair/... 配对码"
              value={pairingInput}
              onChange={(e) => setPairingInput(e.target.value)}
            />
            <Button
              onClick={async () => {
                if (!pairingInput.trim()) return;
                try {
                  await orbitIpc.pairHost(pairingInput.trim());
                  setPairingInput("");
                  await reloadFleet();
                } catch (e) {
                  setMachineFeedback(String(e));
                }
              }}
            >
              添加
            </Button>
          </div>
          {machineFeedback && <div className="text-xs text-sky-300">{machineFeedback}</div>}
        </DialogContent>
      </Dialog>

      {/* =====================================================================
          MODAL 6: PER-HOST CONFIGURATION, RENAMING & SSH TUNING
      ===================================================================== */}
      <Dialog open={Boolean(editingHost)} onOpenChange={(o) => !o && setEditingHost(null)}>
        <DialogContent className="max-w-lg">
          {editingHost && (
            <>
              <DialogHeader>
                <DialogTitle>机器配置与连接调优 · {editingHost.display_name}</DialogTitle>
                <DialogDescription className="font-mono text-[11px]">
                  主机名: {editingHost.hostname}
                  {editingHost.os_arch ? ` (${editingHost.os_arch})` : ""}
                </DialogDescription>
              </DialogHeader>

              <div className="max-h-[65vh] space-y-3 overflow-y-auto pr-1">
                <div className="space-y-1">
                  <div className="text-xs font-medium">机器显示名称（重命名）</div>
                  <Input
                    placeholder="例如：新加坡 DevBox / Mac mini"
                    value={cfgName}
                    onChange={(e) => setCfgName(e.target.value)}
                  />
                </div>

                <div className="space-y-1.5">
                  <div className="text-xs font-medium">连接通道策略</div>
                  {ROUTE_MODES.map((rm) => (
                    <button
                      key={rm.id}
                      type="button"
                      onClick={() => setCfgRouteMode(rm.id)}
                      className={cn(
                        "flex w-full cursor-pointer items-center justify-between rounded-xl border px-3 py-2 text-left text-xs",
                        cfgRouteMode === rm.id
                          ? "border-sky-500/50 bg-sky-500/15"
                          : "border-white/8 bg-white/[0.02]",
                      )}
                    >
                      <div>
                        <div className="font-semibold">{rm.label}</div>
                        <div className="text-[11px] text-zinc-400">{rm.desc}</div>
                      </div>
                      {cfgRouteMode === rm.id && <Check className="size-4 text-sky-400" />}
                    </button>
                  ))}
                </div>

                <div className="grid grid-cols-3 gap-2">
                  <div className="col-span-2 space-y-1">
                    <div className="text-xs font-medium">SSH Host 别名 (~/.ssh/config)</div>
                    <Input
                      placeholder="例如：devbox"
                      value={cfgSshAlias}
                      onChange={(e) => setCfgSshAlias(e.target.value)}
                    />
                  </div>
                  <div className="space-y-1">
                    <div className="text-xs font-medium">远端 Daemon 端口</div>
                    <Input
                      placeholder="6868"
                      value={cfgRemotePort}
                      onChange={(e) => setCfgRemotePort(e.target.value)}
                    />
                  </div>
                </div>

                <div className="space-y-1">
                  <div className="text-xs font-medium">
                    自定义直连地址（可选，如 Tailscale ws://100.x.y.z:6868/ws）
                  </div>
                  <Input
                    placeholder="ws://100.x.y.z:6868/ws"
                    value={cfgDirectEp}
                    onChange={(e) => setCfgDirectEp(e.target.value)}
                  />
                </div>

                <div className="space-y-1">
                  <div className="text-xs font-medium">外网加密中继地址（可选）</div>
                  <Input
                    placeholder="ws://your-relay:7878"
                    value={cfgRelayUrl}
                    onChange={(e) => setCfgRelayUrl(e.target.value)}
                  />
                </div>

                <Button
                  size="sm"
                  variant="outline"
                  className="w-full"
                  disabled={cfgBusy}
                  onClick={() => void handleUpgradeRemoteDaemon()}
                >
                  <RefreshCw className={cn("size-3.5", cfgBusy && "animate-spin")} />
                  SSH 一键升级并重启远端 Orbit Daemon
                </Button>

                {cfgStatusMsg && <div className="text-xs text-sky-300">{cfgStatusMsg}</div>}
              </div>

              <div className="flex items-center justify-between pt-1">
                <Button
                  size="sm"
                  variant="destructive"
                  onClick={async () => {
                    await orbitIpc.removeHost(editingHost.host_id);
                    setEditingHost(null);
                    await reloadFleet();
                  }}
                >
                  移除该机器
                </Button>
                <div className="flex gap-2">
                  <Button size="sm" variant="outline" onClick={() => setEditingHost(null)}>
                    取消
                  </Button>
                  <Button
                    size="sm"
                    disabled={cfgBusy}
                    onClick={() => void handleSaveHostSettings()}
                  >
                    保存配置并重连
                  </Button>
                </div>
              </div>
            </>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}
