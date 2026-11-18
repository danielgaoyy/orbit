import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  Bot,
  CheckCircle2,
  ChevronRight,
  Cpu,
  FolderGit2,
  GitBranch,
  HelpCircle,
  Loader2,
  Plus,
  Radio,
  RefreshCw,
  Send,
  Server,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  Square,
  Terminal as TerminalIcon,
  Trash2,
  Wifi,
  WifiOff,
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
} from "@/components/ui/dialog-tabs";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/dialog-tabs";
import { Input, Textarea } from "@/components/ui/input";
import { DiffFilesViewer } from "@/components/diff-viewer";
import { MarkdownBlocksView } from "@/components/markdown-blocks";
import { XtermPane, type XtermStreamChunk } from "@/components/xterm-pane";
import {
  type AgentTimelineView,
  type CoreNotification,
  type FleetOverviewView,
  type GitReviewView,
  type HostCardView,
  type HostDetailView,
  type ProviderKind,
  type QuestionItem,
  type SessionMode,
  type SshHostItem,
  type TimelineItemView,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";

function ConnectionBadge({ state }: { state: HostCardView["connection_state"] }) {
  switch (state) {
    case "online_direct":
      return (
        <Badge variant="success">
          <Wifi className="size-3" /> LAN Direct
        </Badge>
      );
    case "online_relay":
      return (
        <Badge variant="relay">
          <ShieldCheck className="size-3" /> Noise E2EE
        </Badge>
      );
    case "connecting_direct":
    case "connecting_relay":
      return (
        <Badge variant="warning">
          <Loader2 className="size-3 animate-spin" /> Connecting
        </Badge>
      );
    default:
      return (
        <Badge variant="outline" className="text-zinc-500">
          <WifiOff className="size-3" /> Offline
        </Badge>
      );
  }
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
  const [selections, setSelections] = useState<Record<number, string[]>>(() => {
    const init: Record<number, string[]> = {};
    questions.forEach((q, idx) => {
      if (q.options.length > 0) init[idx] = [q.options[0]];
    });
    return init;
  });

  const toggleOption = (qIdx: number, opt: string, multi: boolean) => {
    setSelections((prev) => {
      const cur = prev[qIdx] ?? [];
      if (!multi) return { ...prev, [qIdx]: [opt] };
      return {
        ...prev,
        [qIdx]: cur.includes(opt) ? cur.filter((x) => x !== opt) : [...cur, opt],
      };
    });
  };

  const handleSubmit = () => {
    const ordered = questions.map((_, idx) => selections[idx] ?? []);
    void orbitIpc.answerQuestion(hostId, agentId, requestId, ordered);
  };

  return (
    <Card className="border-sky-500/35 bg-sky-500/[0.06]">
      <CardHeader className="pb-1">
        <CardTitle className="flex items-center gap-2 text-sky-300">
          <HelpCircle className="size-4" /> Clarifying Question
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        {questions.map((q, qIdx) => (
          <div key={q.id || qIdx} className="space-y-2">
            <div className="text-sm font-medium">{q.question}</div>
            <div className="flex flex-wrap gap-1.5">
              {q.options.map((opt) => {
                const chosen = resolvedAnswers
                  ? resolvedAnswers[qIdx]?.includes(opt)
                  : selections[qIdx]?.includes(opt);
                return (
                  <Button
                    key={opt}
                    size="xs"
                    variant={chosen ? "default" : "outline"}
                    disabled={Boolean(resolvedAnswers)}
                    onClick={() => toggleOption(qIdx, opt, q.is_multi_select)}
                  >
                    {opt}
                  </Button>
                );
              })}
            </div>
          </div>
        ))}
        {resolvedAnswers ? (
          <Badge variant="success">Submitted</Badge>
        ) : (
          <Button size="sm" onClick={handleSubmit}>
            Submit Answers
          </Button>
        )}
      </CardContent>
    </Card>
  );
}

function TimelineEntry({
  hostId,
  agentId,
  item,
}: {
  hostId: string;
  agentId: string;
  item: TimelineItemView;
}) {
  switch (item.item_type) {
    case "user_prompt":
      return (
        <div className="flex justify-end">
          <div className="bg-primary/15 border-primary/25 max-w-[82%] rounded-2xl rounded-br-sm border px-4 py-2.5">
            <MarkdownBlocksView blocks={item.blocks} />
          </div>
        </div>
      );

    case "assistant_thinking":
      return (
        <details className="group rounded-lg border border-white/8 bg-white/[0.02] px-3 py-2 text-xs text-zinc-400">
          <summary className="flex cursor-pointer items-center gap-1.5 font-medium select-none">
            <ChevronRight className="size-3.5 transition-transform group-open:rotate-90" />
            Reasoning trace
          </summary>
          <pre className="mt-2 font-mono text-[11.5px] leading-relaxed whitespace-pre-wrap text-zinc-400">
            {item.text}
          </pre>
        </details>
      );

    case "assistant_message":
      return (
        <div className="rounded-xl border border-white/8 bg-card/70 px-4 py-3">
          <div className="mb-1.5 flex items-center gap-2 text-xs text-zinc-400">
            <Sparkles className="text-primary size-3.5" />
            <span className="font-medium text-zinc-200">Agent</span>
            {item.streaming && (
              <Badge variant="default" className="h-4 px-1.5 text-[10px]">
                streaming
              </Badge>
            )}
          </div>
          <MarkdownBlocksView blocks={item.blocks} />
        </div>
      );

    case "tool_call":
      return (
        <div className="rounded-lg border border-white/8 bg-[#0B0F19] px-3.5 py-2.5 text-xs">
          <div className="flex items-center justify-between gap-2">
            <code className="truncate font-mono text-zinc-200">{item.summary_title}</code>
            <Badge
              variant={
                item.status === "completed"
                  ? "success"
                  : item.status === "running"
                    ? "warning"
                    : "destructive"
              }
            >
              {item.status}
            </Badge>
          </div>
          {item.detail.kind === "shell" && item.detail.output_preview && (
            <pre className="mt-2 max-h-40 overflow-auto rounded bg-black/40 p-2 font-mono text-[11px] text-zinc-400">
              {item.detail.output_preview}
            </pre>
          )}
          {item.diff_files.length > 0 && (
            <div className="mt-2">
              <DiffFilesViewer files={item.diff_files} />
            </div>
          )}
        </div>
      );

    case "permission_request": {
      const decisiones =
        item.allowed_decisions.length > 0 ? item.allowed_decisions : ["allow", "deny"];
      return (
        <Card className="border-amber-500/40 bg-amber-500/[0.07]">
          <CardHeader className="pb-1">
            <CardTitle className="flex items-center gap-2 text-amber-300">
              <ShieldAlert className="size-4" /> {item.title}
            </CardTitle>
            <CardDescription className="font-mono text-xs text-zinc-300">
              {item.detail.kind === "shell"
                ? `$ ${item.detail.command}`
                : item.detail.kind === "file_edit" || item.detail.kind === "file_read"
                  ? item.detail.path
                  : JSON.stringify(item.detail)}
            </CardDescription>
          </CardHeader>
          <CardContent>
            {item.resolved_decision ? (
              <Badge variant="secondary">Decision: {item.resolved_decision}</Badge>
            ) : (
              <div className="flex flex-wrap gap-2">
                {decisiones.map((dec) => (
                  <Button
                    key={dec}
                    size="sm"
                    variant={dec.toLowerCase().includes("deny") ? "destructive" : "default"}
                    onClick={() =>
                      void orbitIpc.resolvePermission(hostId, agentId, item.request_id, dec)
                    }
                  >
                    {dec}
                  </Button>
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      );
    }

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
        <div className="text-muted-foreground flex items-center gap-2 rounded-md bg-white/[0.03] px-3 py-1.5 text-xs">
          <AlertTriangle className="size-3.5 text-amber-400" />
          <span>{item.message}</span>
        </div>
      );
  }
}

export default function App() {
  const [fleet, setFleet] = useState<FleetOverviewView>({
    revision: 0,
    total_pending_approvals: 0,
    hosts: [],
  });
  const [selectedHostId, setSelectedHostId] = useState<string | null>(null);
  const [hostDetail, setHostDetail] = useState<HostDetailView | null>(null);
  const [selectedWorkspaceCwd, setSelectedWorkspaceCwd] = useState<string>("");
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);
  const [timeline, setTimeline] = useState<AgentTimelineView | null>(null);
  const [gitReview, setGitReview] = useState<GitReviewView | null>(null);
  const [selectedDiffPath, setSelectedDiffPath] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<string>("chat");

  // Terminal raw stream buffer per host
  const termSeqRef = useRef<number>(0);
  const [termChunksByHost, setTermChunksByHost] = useState<Record<string, XtermStreamChunk[]>>({});

  // Modals
  const [pairDialogOpen, setPairDialogOpen] = useState(false);
  const [pairingInput, setPairingInput] = useState("");
  const [pairError, setPairError] = useState<string | null>(null);

  const [relayDialogOpen, setRelayDialogOpen] = useState(false);
  const [sshHosts, setSshHosts] = useState<SshHostItem[]>([]);
  const [manualRelayUrl, setManualRelayUrl] = useState("");
  const [relayBusy, setRelayBusy] = useState<string | null>(null);

  const [newAgentOpen, setNewAgentOpen] = useState(false);
  const [newCwd, setNewCwd] = useState("");
  const [newProvider, setNewProvider] = useState<ProviderKind>("antigravity");
  const [newMode, setNewMode] = useState<SessionMode>("accept_edits");
  const [newWorktreeBranch, setNewWorktreeBranch] = useState("");
  const [newPrompt, setNewPrompt] = useState("");

  const [composerText, setComposerText] = useState("");

  const activeHostIdRef = useRef<string | null>(null);
  const activeAgentIdRef = useRef<string | null>(null);
  const activeCwdRef = useRef<string>("");
  const activeDiffPathRef = useRef<string | null>(null);

  activeHostIdRef.current = selectedHostId;
  activeAgentIdRef.current = selectedAgentId;
  activeCwdRef.current = selectedWorkspaceCwd;
  activeDiffPathRef.current = selectedDiffPath;

  const reloadFleet = useCallback(async () => {
    const snap = await orbitIpc.getFleetOverview();
    setFleet(snap);
    setSelectedHostId((prev) => prev ?? snap.hosts[0]?.host_id ?? null);
  }, []);

  const reloadHost = useCallback(async (hostId: string) => {
    const detail = await orbitIpc.getHostDetail(hostId);
    setHostDetail(detail);
    if (detail) {
      setSelectedWorkspaceCwd(
        (prev) => prev || detail.workspaces[0]?.cwd || detail.agents[0]?.cwd || "~",
      );
      setSelectedAgentId((prev) => {
        if (prev && detail.agents.some((a) => a.agent_id === prev)) return prev;
        return detail.agents[0]?.agent_id ?? null;
      });
    }
  }, []);

  useEffect(() => {
    void reloadFleet();
    void orbitIpc.subscribe((ev: CoreNotification) => {
      if (ev.kind === "fleet_changed") {
        void reloadFleet();
        if (activeHostIdRef.current) void reloadHost(activeHostIdRef.current);
      } else if (ev.kind === "host_changed") {
        void reloadFleet();
        if (ev.host_id === activeHostIdRef.current) void reloadHost(ev.host_id);
      } else if (ev.kind === "timeline_changed") {
        if (ev.host_id === activeHostIdRef.current && ev.agent_id === activeAgentIdRef.current) {
          void orbitIpc
            .getAgentTimeline(ev.host_id, ev.agent_id)
            .then((tl) => setTimeline(tl));
        }
      } else if (ev.kind === "git_changed") {
        if (ev.host_id === activeHostIdRef.current && ev.cwd === activeCwdRef.current) {
          void orbitIpc
            .getGitReview(ev.host_id, ev.cwd, activeDiffPathRef.current)
            .then((gr) => setGitReview(gr));
        }
      } else if (ev.kind === "terminal_changed") {
        const seq = ++termSeqRef.current;
        const bytes = Uint8Array.from(ev.raw_bytes);
        setTermChunksByHost((prev) => {
          const existing = prev[ev.host_id] ?? [];
          const next = [...existing.slice(-64), { seq, bytes }];
          return { ...prev, [ev.host_id]: next };
        });
      }
    });
  }, [reloadFleet, reloadHost]);

  useEffect(() => {
    if (!selectedHostId) {
      setHostDetail(null);
      return;
    }
    void reloadHost(selectedHostId);
    void orbitIpc.refreshQuotas(selectedHostId);
  }, [selectedHostId, reloadHost]);

  useEffect(() => {
    if (!selectedHostId || !selectedAgentId) {
      setTimeline(null);
      return;
    }
    void orbitIpc.openAgentTimeline(selectedHostId, selectedAgentId);
    void orbitIpc.getAgentTimeline(selectedHostId, selectedAgentId).then(setTimeline);
  }, [selectedHostId, selectedAgentId]);

  useEffect(() => {
    if (activeTab === "git" && selectedHostId && selectedWorkspaceCwd) {
      void orbitIpc.fetchGitStatus(selectedHostId, selectedWorkspaceCwd);
      void orbitIpc.fetchGitDiff(selectedHostId, selectedWorkspaceCwd, selectedDiffPath, false);
      void orbitIpc
        .getGitReview(selectedHostId, selectedWorkspaceCwd, selectedDiffPath)
        .then(setGitReview);
    }
  }, [activeTab, selectedHostId, selectedWorkspaceCwd, selectedDiffPath]);

  const currentAgent = useMemo(
    () => hostDetail?.agents.find((a) => a.agent_id === selectedAgentId) ?? null,
    [hostDetail, selectedAgentId],
  );

  const handlePairSubmit = async () => {
    setPairError(null);
    try {
      const card = await orbitIpc.pairHost(pairingInput.trim());
      setSelectedHostId(card.host_id);
      setPairingInput("");
      setPairDialogOpen(false);
    } catch (e) {
      setPairError(String(e));
    }
  };

  const openRelayConfigurator = async () => {
    if (!hostDetail) return;
    setManualRelayUrl(hostDetail.host.relay_url ?? "");
    setRelayDialogOpen(true);
    const discovered = await orbitIpc.listSshHosts();
    setSshHosts(discovered);
  };

  const handleProvisionSshRelay = async (entry: SshHostItem) => {
    if (!selectedHostId) return;
    setRelayBusy(`Deploying zero-knowledge Relay to ${entry.alias}...`);
    try {
      const url = await orbitIpc.provisionSshRelay({
        targetHostId: selectedHostId,
        sshAlias: entry.alias,
        fallbackHostname: entry.host_name,
        relayPort: 7878,
      });
      setManualRelayUrl(url);
      setRelayBusy(null);
      setRelayDialogOpen(false);
    } catch (e) {
      setRelayBusy(`Error: ${String(e)}`);
    }
  };

  const handleSaveManualRelay = async () => {
    if (!selectedHostId) return;
    await orbitIpc.updateHostRelay(
      selectedHostId,
      manualRelayUrl.trim() === "" ? null : manualRelayUrl.trim(),
    );
    setRelayDialogOpen(false);
  };

  const handleCreateAgent = async () => {
    if (!selectedHostId || !newPrompt.trim()) return;
    const targetCwd = newCwd.trim() || selectedWorkspaceCwd || "~";
    await orbitIpc.createAgent({
      hostId: selectedHostId,
      cwd: targetCwd,
      provider: newProvider,
      mode: newMode,
      initialPrompt: newPrompt.trim(),
      worktreeBranch: newWorktreeBranch.trim() || null,
    });
    setNewPrompt("");
    setNewWorktreeBranch("");
    setNewAgentOpen(false);
  };

  const handleSendPrompt = async () => {
    if (!selectedHostId || !selectedAgentId || !composerText.trim()) return;
    const msg = composerText.trim();
    setComposerText("");
    await orbitIpc.sendPrompt(selectedHostId, selectedAgentId, msg);
  };

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      {/* Column 1: Multi-Host Fleet Rail */}
      <aside className="flex w-72 shrink-0 flex-col border-r border-border bg-card/40">
        <div className="flex items-center justify-between border-b border-border px-4 py-3.5">
          <div className="flex items-center gap-2.5">
            <img
              src="/orbit-logo.png"
              alt="Orbit"
              className="size-7 shrink-0 rounded-lg shadow-sm ring-1 ring-white/15"
            />
            <div>
              <div className="text-sm font-semibold tracking-tight">Orbit Fleet</div>
              <div className="text-muted-foreground text-[11px]">Multi-Host Control Plane</div>
            </div>
          </div>
          {fleet.total_pending_approvals > 0 && (
            <Badge variant="warning">{fleet.total_pending_approvals} pending</Badge>
          )}
        </div>

        <div className="flex-1 space-y-1.5 overflow-y-auto p-3">
          {fleet.hosts.length === 0 ? (
            <div className="text-muted-foreground rounded-lg border border-dashed p-4 text-center text-xs">
              No hosts paired yet. Run <code className="font-mono">orbit pair</code> on your
              Devbox, Linux workstation, or Mac mini.
            </div>
          ) : (
            fleet.hosts.map((h) => {
              const active = h.host_id === selectedHostId;
              return (
                <div
                  key={h.host_id}
                  onClick={() => setSelectedHostId(h.host_id)}
                  className={cn(
                    "group flex cursor-pointer flex-col gap-2 rounded-xl border p-3 transition-colors",
                    active
                      ? "border-primary/50 bg-primary/10"
                      : "border-transparent bg-white/[0.02] hover:bg-white/[0.05]",
                  )}
                >
                  <div className="flex items-center justify-between gap-2">
                    <div className="flex items-center gap-2 truncate font-medium text-sm">
                      <Server className="text-muted-foreground size-3.5 shrink-0" />
                      <span className="truncate">{h.display_name}</span>
                    </div>
                    {h.pending_approval_count > 0 && (
                      <Badge variant="warning">{h.pending_approval_count}</Badge>
                    )}
                  </div>
                  <div className="flex items-center justify-between">
                    <ConnectionBadge state={h.connection_state} />
                    <span className="text-muted-foreground text-[11px]">
                      {h.agent_count} agents
                    </span>
                  </div>
                  {h.last_error && (
                    <div className="truncate text-[11px] text-rose-400">{h.last_error}</div>
                  )}
                </div>
              );
            })
          )}
        </div>

        {/* Quotas Summary Footer */}
        {hostDetail && hostDetail.quotas.length > 0 && (
          <div className="space-y-2 border-t border-border p-3">
            <div className="flex items-center justify-between text-xs font-medium text-zinc-400">
              <span>Provider Quotas</span>
              <button
                onClick={() => selectedHostId && void orbitIpc.refreshQuotas(selectedHostId)}
                className="hover:text-foreground cursor-pointer"
              >
                <RefreshCw className="size-3" />
              </button>
            </div>
            {hostDetail.quotas.map((q) => (
              <div key={q.provider} className="space-y-1 rounded-lg bg-white/[0.03] p-2 text-xs">
                <div className="flex items-center justify-between">
                  <span className="font-medium capitalize">
                    {q.provider.replace("_", " ")}
                  </span>
                  {q.credits_remaining !== null && (
                    <span className="font-mono text-[11px] text-emerald-400">
                      {q.credits_remaining.toFixed(1)} cr
                    </span>
                  )}
                </div>
                {q.windows.map((w) => (
                  <div key={w.label} className="space-y-0.5">
                    <div className="text-muted-foreground flex justify-between text-[10px]">
                      <span>{w.label}</span>
                      <span>{Math.round(w.used_percent)}%</span>
                    </div>
                    <div className="h-1 w-full overflow-hidden rounded-full bg-white/10">
                      <div
                        className="bg-primary h-full transition-all"
                        style={{ width: `${Math.min(100, Math.max(0, w.used_percent))}%` }}
                      />
                    </div>
                  </div>
                ))}
              </div>
            ))}
          </div>
        )}

        <div className="border-t border-border p-3">
          <Button className="w-full" size="sm" onClick={() => setPairDialogOpen(true)}>
            <Plus className="size-4" /> Pair Host (`orbit://pair`)
          </Button>
        </div>
      </aside>

      {/* Column 2: Host Sessions & Workspaces Sidebar */}
      <section className="flex w-80 shrink-0 flex-col border-r border-border bg-background/50">
        {hostDetail ? (
          <>
            <div className="flex items-center justify-between border-b border-border px-4 py-3">
              <div className="min-w-0">
                <div className="truncate text-sm font-semibold">
                  {hostDetail.host.display_name}
                </div>
                <div className="text-muted-foreground truncate font-mono text-[11px]">
                  {hostDetail.host.relay_url ?? "Direct LAN / Tailscale"}
                </div>
              </div>
              <div className="flex items-center gap-1">
                <Button
                  variant="ghost"
                  size="icon"
                  title="Configure E2EE Relay"
                  onClick={() => void openRelayConfigurator()}
                >
                  <Radio className="size-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  title="Reconnect & Refresh"
                  onClick={() => {
                    void orbitIpc.connectHost(hostDetail.host.host_id);
                    void orbitIpc.refreshHost(hostDetail.host.host_id);
                  }}
                >
                  <RefreshCw className="size-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  title="Remove Host"
                  onClick={() => void orbitIpc.removeHost(hostDetail.host.host_id)}
                >
                  <Trash2 className="size-4 text-rose-400" />
                </Button>
              </div>
            </div>

            <div className="flex items-center justify-between px-3 pt-3 pb-1.5">
              <span className="text-muted-foreground text-xs font-medium">Agent Sessions</span>
              <Button
                size="xs"
                onClick={() => {
                  setNewCwd(selectedWorkspaceCwd || hostDetail.workspaces[0]?.cwd || "");
                  setNewAgentOpen(true);
                }}
              >
                <Plus className="size-3.5" /> New Agent
              </Button>
            </div>

            <div className="flex-1 space-y-1.5 overflow-y-auto p-3">
              {hostDetail.agents.length === 0 ? (
                <div className="text-muted-foreground rounded-lg border border-dashed p-4 text-center text-xs">
                  No active agent sessions on this host. Click <b>New Agent</b> to launch
                  Antigravity, Codex, or Claude Code.
                </div>
              ) : (
                hostDetail.agents.map((ag) => {
                  const selected = ag.agent_id === selectedAgentId;
                  return (
                    <div
                      key={ag.agent_id}
                      onClick={() => {
                        setSelectedAgentId(ag.agent_id);
                        setSelectedWorkspaceCwd(ag.cwd);
                      }}
                      className={cn(
                        "flex cursor-pointer flex-col gap-1.5 rounded-lg border p-2.5 transition-colors",
                        selected
                          ? "border-primary/50 bg-accent"
                          : "border-transparent hover:bg-white/[0.04]",
                      )}
                    >
                      <div className="flex items-center justify-between gap-2">
                        <span className="truncate text-xs font-semibold">{ag.title}</span>
                        <Badge
                          variant={
                            ag.is_awaiting_permission
                              ? "warning"
                              : ag.is_running
                                ? "default"
                                : "secondary"
                          }
                          className="text-[10px]"
                        >
                          {ag.status_label}
                        </Badge>
                      </div>
                      <div className="text-muted-foreground flex items-center justify-between text-[11px]">
                        <span className="flex items-center gap-1">
                          <Bot className="size-3" /> {ag.provider_label}
                        </span>
                        <span className="max-w-36 truncate font-mono">{ag.cwd}</span>
                      </div>
                    </div>
                  );
                })
              )}
            </div>
          </>
        ) : (
          <div className="text-muted-foreground flex flex-1 items-center justify-center p-6 text-center text-xs">
            Select or pair a host from the left rail.
          </div>
        )}
      </section>

      {/* Column 3: Main Workspace Canvas (Timeline / Git Diff / WebGL PTY) */}
      <main className="flex min-w-0 flex-1 flex-col">
        {selectedHostId && hostDetail ? (
          <Tabs
            value={activeTab}
            onValueChange={setActiveTab}
            className="flex h-full flex-col overflow-hidden"
          >
            <header className="flex items-center justify-between border-b border-border px-5 py-2.5">
              <div className="flex items-center gap-3 min-w-0">
                <TabsList>
                  <TabsTrigger value="chat">
                    <Cpu className="size-3.5" /> Agent Stream
                  </TabsTrigger>
                  <TabsTrigger value="git">
                    <FolderGit2 className="size-3.5" /> Git Diff
                  </TabsTrigger>
                  <TabsTrigger value="terminal">
                    <TerminalIcon className="size-3.5" /> PTY Terminal
                  </TabsTrigger>
                </TabsList>
                {currentAgent && (
                  <span className="text-muted-foreground hidden truncate font-mono text-xs xl:inline">
                    {currentAgent.cwd}
                  </span>
                )}
              </div>

              {currentAgent && activeTab === "chat" && (
                <div className="flex items-center gap-2">
                  {(["default", "plan", "accept_edits", "full_access"] as const).map((m) => (
                    <Button
                      key={m}
                      size="xs"
                      variant={currentAgent.mode === m ? "secondary" : "ghost"}
                      onClick={() =>
                        void orbitIpc.setSessionMode(selectedHostId, currentAgent.agent_id, m)
                      }
                    >
                      {m.replace("_", " ")}
                    </Button>
                  ))}
                  {currentAgent.is_running && (
                    <Button
                      size="xs"
                      variant="destructive"
                      onClick={() =>
                        void orbitIpc.interruptAgent(selectedHostId, currentAgent.agent_id)
                      }
                    >
                      <Square className="size-3" /> Stop
                    </Button>
                  )}
                </div>
              )}
            </header>

            <TabsContent value="chat" className="flex min-h-0 flex-1 flex-col">
              <div className="flex-1 space-y-3 overflow-y-auto p-5">
                {timeline && timeline.items.length > 0 ? (
                  timeline.items.map((item) => (
                    <TimelineEntry
                      key={`${item.item_type}-${item.seq}`}
                      hostId={selectedHostId}
                      agentId={selectedAgentId ?? ""}
                      item={item}
                    />
                  ))
                ) : (
                  <div className="text-muted-foreground flex h-full flex-col items-center justify-center gap-2 text-sm">
                    <CheckCircle2 className="size-8 opacity-30" />
                    <span>Select an agent session or launch a new task to stream events.</span>
                  </div>
                )}
              </div>

              {selectedAgentId && (
                <div className="border-t border-border p-4">
                  <div className="flex items-end gap-2">
                    <Textarea
                      value={composerText}
                      onChange={(e) => setComposerText(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                          e.preventDefault();
                          void handleSendPrompt();
                        }
                      }}
                      placeholder="Send follow-up instructions or steer active turn (Cmd/Ctrl + Enter)..."
                      className="max-h-36 min-h-12 resize-none"
                    />
                    <Button onClick={() => void handleSendPrompt()}>
                      <Send className="size-4" /> Send
                    </Button>
                  </div>
                </div>
              )}
            </TabsContent>

            <TabsContent value="git" className="flex min-h-0 flex-1 overflow-hidden">
              <div className="w-64 shrink-0 space-y-1 overflow-y-auto border-r border-border p-3">
                <div className="mb-2 flex items-center justify-between text-xs font-medium">
                  <span className="flex items-center gap-1.5">
                    <GitBranch className="size-3.5" />
                    {gitReview?.status?.branch ?? "HEAD"}
                  </span>
                  <Button
                    variant="ghost"
                    size="xs"
                    onClick={() => setSelectedDiffPath(null)}
                  >
                    All
                  </Button>
                </div>
                {gitReview?.status?.files.map((f) => (
                  <button
                    key={f.path}
                    onClick={() => setSelectedDiffPath(f.path)}
                    className={cn(
                      "flex w-full cursor-pointer items-center justify-between rounded-md px-2.5 py-1.5 text-left font-mono text-xs",
                      selectedDiffPath === f.path
                        ? "bg-accent text-accent-foreground"
                        : "hover:bg-white/[0.04]",
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
              <XtermPane
                hostId={selectedHostId}
                cwd={selectedWorkspaceCwd || "~"}
                chunks={termChunksByHost[selectedHostId] ?? []}
              />
            </TabsContent>
          </Tabs>
        ) : (
          <div className="text-muted-foreground flex flex-1 items-center justify-center text-sm">
            Orbit Desktop Ready — Pair or select a host to begin.
          </div>
        )}
      </main>

      {/* Pair Host Dialog */}
      <Dialog open={pairDialogOpen} onOpenChange={setPairDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Pair Orbit Host</DialogTitle>
            <DialogDescription>
              Run <code className="font-mono">orbit pair</code> on your remote server or Mac mini
              and paste the <code className="font-mono">orbit://pair/...</code> URI below.
            </DialogDescription>
          </DialogHeader>
          <Input
            placeholder="orbit://pair/eyJob3N0X2lkIjoi..."
            value={pairingInput}
            onChange={(e) => setPairingInput(e.target.value)}
          />
          {pairError && <div className="text-xs text-rose-400">{pairError}</div>}
          <div className="flex justify-end gap-2">
            <Button variant="outline" onClick={() => setPairDialogOpen(false)}>
              Cancel
            </Button>
            <Button onClick={() => void handlePairSubmit()}>Pair & Connect</Button>
          </div>
        </DialogContent>
      </Dialog>

      {/* One-Click SSH Relay Provisioning Dialog */}
      <Dialog open={relayDialogOpen} onOpenChange={setRelayDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Zero-Knowledge Noise E2EE Relay</DialogTitle>
            <DialogDescription>
              One-click deploy an allowlisted <code className="font-mono">orbit relay</code> to any
              server in your <code className="font-mono">~/.ssh/config</code>, or enter an existing
              Relay endpoint.
            </DialogDescription>
          </DialogHeader>
          {sshHosts.length > 0 && (
            <div className="max-h-44 space-y-1.5 overflow-y-auto rounded-lg border p-2">
              {sshHosts.map((s) => (
                <div
                  key={s.alias}
                  className="flex items-center justify-between rounded-md px-2.5 py-1.5 hover:bg-white/[0.04]"
                >
                  <div className="text-xs">
                    <div className="font-semibold">{s.alias}</div>
                    <div className="text-muted-foreground font-mono text-[11px]">
                      {s.display_target}
                    </div>
                  </div>
                  <Button size="xs" onClick={() => void handleProvisionSshRelay(s)}>
                    Deploy & Attach
                  </Button>
                </div>
              ))}
            </div>
          )}
          {relayBusy && <div className="text-xs text-sky-300">{relayBusy}</div>}
          <Input
            placeholder="ws://your-relay-host:7878 or wss://relay.domain.com"
            value={manualRelayUrl}
            onChange={(e) => setManualRelayUrl(e.target.value)}
          />
          <div className="flex justify-end gap-2">
            <Button variant="outline" onClick={() => setRelayDialogOpen(false)}>
              Close
            </Button>
            <Button onClick={() => void handleSaveManualRelay()}>Save Relay URL</Button>
          </div>
        </DialogContent>
      </Dialog>

      {/* Create New Agent Session Dialog */}
      <Dialog open={newAgentOpen} onOpenChange={setNewAgentOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Launch Coding Agent Session</DialogTitle>
            <DialogDescription>
              Runs inside an isolated <code className="font-mono">orbit shim</code> so daemon
              reloads never interrupt active tasks.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <Input
              placeholder="Workspace path (e.g. /home/dev/project)"
              value={newCwd}
              onChange={(e) => setNewCwd(e.target.value)}
            />
            <div className="flex gap-2">
              {(["antigravity", "codex", "claude_code"] as const).map((p) => (
                <Button
                  key={p}
                  size="sm"
                  variant={newProvider === p ? "default" : "outline"}
                  onClick={() => setNewProvider(p)}
                  className="flex-1 capitalize"
                >
                  {p.replace("_", " ")}
                </Button>
              ))}
            </div>
            <div className="flex gap-1.5">
              {(["default", "plan", "accept_edits", "full_access"] as const).map((m) => (
                <Button
                  key={m}
                  size="xs"
                  variant={newMode === m ? "secondary" : "ghost"}
                  onClick={() => setNewMode(m)}
                  className="flex-1"
                >
                  {m.replace("_", " ")}
                </Button>
              ))}
            </div>
            <Input
              placeholder="Optional isolated Git worktree branch (e.g. feat/auth-refactor)"
              value={newWorktreeBranch}
              onChange={(e) => setNewWorktreeBranch(e.target.value)}
            />
            <Textarea
              placeholder="Initial task prompt..."
              value={newPrompt}
              onChange={(e) => setNewPrompt(e.target.value)}
            />
          </div>
          <div className="flex justify-end gap-2">
            <Button variant="outline" onClick={() => setNewAgentOpen(false)}>
              Cancel
            </Button>
            <Button onClick={() => void handleCreateAgent()}>Start Agent</Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
