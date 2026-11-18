import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  ArrowUp,
  AtSign,
  Check,
  ChevronDown,
  ChevronRight,
  Clock,
  FileSearch,
  Folder,
  FoldVertical,
  GitBranch,
  Laptop,
  ListTodo,
  Paperclip,
  Plus,
  Server,
  Shield,
  ShieldCheck,
  ShieldOff,
  SquareSlash,
  X,
  Zap,
} from "lucide-react";
import {
  defaultEffortDescription,
  formatCatalogModelLabel,
  formatEffortLabel,
  getCatalogForProvider,
  prettifyModelSlug,
  resolveActiveModelSpec,
  resolveSupportedEffortForModel,
  stripAgyEffortSuffix,
} from "@/lib/model-catalog";
import {
  type GitFileEntry,
  type ProviderKind,
  type ReasoningEffort,
  type SessionMode,
  type ThinkingOptionSpec,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { ContextTray } from "@/components/composer/context-tray";
import {
  type MentionCandidate,
  MentionMenu,
  buildMentionCandidates,
} from "@/components/composer/mention-menu";
import {
  type SlashCommandDef,
  SlashCommandMenu,
  filterSlashCommands,
} from "@/components/composer/slash-menu";
import { ProviderIcon } from "@/components/ui/provider-icons";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  Kbd,
  KbdGroup,
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/overlays";
import {
  type ContextMentionItem,
  type DiffQuoteSnippet,
  type PastedAttachment,
  type QueuedPromptItem,
  composerScopeKey,
  orbitActions,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

// Module-level frozen empty arrays for React 19 `useSyncExternalStore` snapshot stability
const EMPTY_CONTEXTS: readonly ContextMentionItem[] = [];
const EMPTY_PASTES: readonly PastedAttachment[] = [];
const EMPTY_QUOTES: readonly DiffQuoteSnippet[] = [];
const EMPTY_QUEUED: readonly QueuedPromptItem[] = [];
const EMPTY_FILES: readonly string[] = [];
const EMPTY_GIT_FILES: readonly GitFileEntry[] = [];

function collapseStrippedToken(
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

function conciseThinkingDescription(opt: ThinkingOptionSpec): string {
  const raw = (opt.description ?? "").trim();
  if (raw && raw.length <= 34) {
    return raw;
  }
  return defaultEffortDescription(opt.effort);
}

const PROVIDERS: { id: ProviderKind; label: string; shortLabel: string }[] = [
  { id: "claude_code", label: "Claude Code", shortLabel: "Claude" },
  { id: "codex", label: "Codex", shortLabel: "Codex" },
  { id: "antigravity", label: "Antigravity", shortLabel: "Antigravity" },
];

const PERMISSION_MODES: {
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

function normalizePermissionMode(mode: SessionMode): SessionMode {
  return mode === "plan" ? "default" : mode;
}

function modeLabel(mode: SessionMode): string {
  const normalized = normalizePermissionMode(mode);
  if (typeof normalized === "object") return normalized.custom;
  switch (normalized) {
    case "plan":
    case "default":
      return "Always Ask";
    case "accept_edits":
      return "Accept Edits";
    case "full_access":
      return "Full Access";
  }
}

function ModeIcon({ mode, className }: { mode: SessionMode; className?: string }) {
  const normalized = normalizePermissionMode(mode);
  if (typeof normalized === "object") {
    return <Shield className={cn("size-[15px] stroke-[1.75]", className)} />;
  }
  switch (normalized) {
    case "plan":
    case "default":
      return <Shield className={cn("size-[15px] stroke-[1.75]", className)} />;
    case "accept_edits":
      return <ShieldCheck className={cn("size-[15px] stroke-[1.75]", className)} />;
    case "full_access":
      return <ShieldOff className={cn("size-[15px] stroke-[1.75]", className)} />;
  }
}

function basenameOfCwd(cwd: string): string {
  const trimmed = cwd.replace(/\/+$/, "");
  if (!trimmed || trimmed === "~") return "~";
  const idx = trimmed.lastIndexOf("/");
  return idx >= 0 ? trimmed.slice(idx + 1) : trimmed;
}

export interface OmniComposerProps {
  variant?: "hero" | "docked";
  occluding?: boolean;
}

export function OmniComposer({ variant = "docked", occluding = false }: OmniComposerProps) {
  const fleet = useOrbitSelector((s) => s.fleet);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const isDraft = useOrbitSelector((s) => s.isDraftNewSession);
  const draftProvider = useOrbitSelector((s) => s.draftProvider);
  const draftModels = useOrbitSelector((s) => s.draftModels);
  const draftEffort = useOrbitSelector((s) => s.draftEffort);
  const draftMode = useOrbitSelector((s) => s.draftMode);
  const draftUseWorktree = useOrbitSelector((s) => s.draftUseWorktree);
  const draftWorktreeBranch = useOrbitSelector((s) => s.draftWorktreeBranch);
  const enterToSend = useOrbitSelector((s) => s.enterToSend);
  const promptHistory = useOrbitSelector((s) => s.promptHistory);

  const scopeKey = useOrbitSelector(composerScopeKey);
  const draftText = useOrbitSelector((s) => s.composerDrafts[composerScopeKey(s)] ?? "");
  const slashDirective = useOrbitSelector(
    (s) => s.slashDirectives[composerScopeKey(s)] ?? null,
  );
  const contexts = useOrbitSelector(
    (s) => s.contextItems[composerScopeKey(s)] ?? EMPTY_CONTEXTS,
  );
  const pastes = useOrbitSelector(
    (s) => s.pastedAttachments[composerScopeKey(s)] ?? EMPTY_PASTES,
  );
  const quotes = useOrbitSelector((s) => s.diffQuotes[composerScopeKey(s)] ?? EMPTY_QUOTES);
  const queued = useOrbitSelector((s) => s.queuedPrompts[composerScopeKey(s)] ?? EMPTY_QUEUED);

  const hostDetail = useOrbitSelector((s) =>
    s.selectedHostId ? (s.hostDetails[s.selectedHostId] ?? null) : null,
  );
  const workspaceFiles = useOrbitSelector((s) =>
    s.selectedHostId && s.selectedWorkspaceCwd
      ? (s.workspaceFilesCache[`${s.selectedHostId}::${s.selectedWorkspaceCwd}`] ?? EMPTY_FILES)
      : EMPTY_FILES,
  );
  const gitFiles = useOrbitSelector((s) =>
    s.selectedHostId && s.selectedWorkspaceCwd
      ? (s.gitReviews[`${s.selectedHostId}::${s.selectedWorkspaceCwd}`]?.status?.files ??
          EMPTY_GIT_FILES)
      : EMPTY_GIT_FILES,
  );

  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const directivePrefixRef = useRef<HTMLSpanElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const modelPopoverRef = useRef<HTMLDivElement | null>(null);
  const modelListRef = useRef<HTMLDivElement | null>(null);
  const hoveredRowElRef = useRef<HTMLButtonElement | null>(null);
  const thinkingSubmenuRef = useRef<HTMLDivElement | null>(null);

  const [cursorOffset, setCursorOffset] = useState<number>(0);
  const [directiveIndentPx, setDirectiveIndentPx] = useState<number>(0);
  const [menuHighlightIdx, setMenuHighlightIdx] = useState<number>(0);
  const [dismissedToken, setDismissedToken] = useState<string | null>(null);
  const [isDraggingOver, setIsDraggingOver] = useState<boolean>(false);
  const [historyCursor, setHistoryCursor] = useState<number | null>(null);
  const [modelPopoverOpen, setModelPopoverOpen] = useState<boolean>(false);
  const [browsedProvider, setBrowsedProvider] = useState<ProviderKind>("codex");
  const [hoveredModelKey, setHoveredModelKey] = useState<string | null>(null);
  const [submenuPos, setSubmenuPos] = useState<{ top: number; flipLeft: boolean }>({
    top: -1,
    flipLeft: false,
  });
  const savedDraftBeforeHistoryRef = useRef<string>("");

  const currentAgent = useMemo(
    () => hostDetail?.agents.find((a) => a.agent_id === selectedAgentId) ?? null,
    [hostDetail, selectedAgentId],
  );

  const availableProviders = hostDetail?.host.available_providers ?? [];
  const detectedProviders = useMemo(
    () => PROVIDERS.filter((p) => availableProviders.includes(p.id)),
    [availableProviders],
  );
  const resolvedDraftProvider: ProviderKind =
    detectedProviders.length > 0 && !detectedProviders.some((p) => p.id === draftProvider)
      ? detectedProviders[0].id
      : draftProvider;

  const activeProvider: ProviderKind =
    !isDraft && currentAgent ? currentAgent.provider : resolvedDraftProvider;
  const activeCatalog = useMemo(
    () => getCatalogForProvider(hostDetail?.provider_catalogs, activeProvider),
    [hostDetail?.provider_catalogs, activeProvider],
  );
  const rawActiveModel: string | null =
    !isDraft && currentAgent ? currentAgent.model : (draftModels[activeProvider] ?? null);
  const activeModelSpec = useMemo(
    () => resolveActiveModelSpec(activeCatalog, rawActiveModel),
    [activeCatalog, rawActiveModel],
  );

  const inferredAgyEffort =
    activeProvider === "antigravity" && rawActiveModel
      ? stripAgyEffortSuffix(rawActiveModel).inferredEffort
      : null;
  const rawDesiredEffort: ReasoningEffort | null =
    !isDraft && currentAgent
      ? (currentAgent.effort ?? inferredAgyEffort ?? draftEffort)
      : draftEffort;
  const effectiveActiveEffort: ReasoningEffort | null = resolveSupportedEffortForModel(
    activeModelSpec,
    rawDesiredEffort,
    activeCatalog.configured_default_effort,
  );
  const activeMode: SessionMode = normalizePermissionMode(
    !isDraft && currentAgent ? currentAgent.mode : draftMode,
  );
  const isRunning = Boolean(!isDraft && currentAgent?.is_running);

  // Warm workspace file list lazily so `@` mentions feel instantaneous
  useEffect(() => {
    if (!selectedHostId || !selectedCwd || selectedCwd === "~") return;
    const timer = window.setTimeout(() => {
      void orbitActions.ensureWorkspaceFiles(selectedHostId, selectedCwd);
    }, 180);
    return () => window.clearTimeout(timer);
  }, [selectedHostId, selectedCwd]);

  // Unified global Composer geometry (aligned 1:1 with Antigravity: `944px` width × `100px` base height)
  const minTextareaHeight = 54;
  const maxTextareaHeight = 220;

  useLayoutEffect(() => {
    if (!slashDirective) {
      setDirectiveIndentPx(0);
      return;
    }
    const el = directivePrefixRef.current;
    if (!el) return;
    const nextIndent = Math.ceil(el.getBoundingClientRect().width + 6);
    setDirectiveIndentPx((prev) => (prev === nextIndent ? prev : nextIndent));
  }, [slashDirective]);

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${Math.min(maxTextareaHeight, Math.max(minTextareaHeight, el.scrollHeight))}px`;
  }, [draftText, scopeKey, directiveIndentPx]);

  useEffect(() => {
    return orbitActions.onComposerFocusRequest(() => {
      textareaRef.current?.focus();
    });
  }, []);

  // Detect Slash (`/`) or Mention (`@`) trigger at cursor
  const slashTrigger = useMemo(() => {
    const safeOffset = Math.min(cursorOffset, draftText.length);
    const before = draftText.slice(0, safeOffset);
    const match = /(?:^|\s)\/([a-zA-Z0-9_-]*)$/.exec(before);
    if (!match) return null;
    const slashIndex = before.lastIndexOf("/");
    const query = match[1];
    const tokenKey = `slash:${slashIndex}:${query}`;
    if (dismissedToken === tokenKey) return null;
    const items = filterSlashCommands(query);
    if (items.length === 0) return null;
    return { slashIndex, endIndex: safeOffset, query, tokenKey, items };
  }, [draftText, cursorOffset, dismissedToken]);

  const mentionTrigger = useMemo(() => {
    if (slashTrigger) return null;
    const safeOffset = Math.min(cursorOffset, draftText.length);
    const before = draftText.slice(0, safeOffset);
    const match = /(?:^|\s)@([^\s@]*)$/.exec(before);
    if (!match) return null;
    const atIndex = before.lastIndexOf("@");
    const query = match[1];
    const tokenKey = `at:${atIndex}:${query}`;
    if (dismissedToken === tokenKey) return null;
    const items = buildMentionCandidates(query, workspaceFiles, gitFiles);
    if (items.length === 0) return null;
    return { atIndex, endIndex: safeOffset, query, tokenKey, items };
  }, [slashTrigger, draftText, cursorOffset, dismissedToken, workspaceFiles, gitFiles]);

  const repoProjects = useMemo(() => {
    if (!hostDetail) return [];
    const seen = new Set<string>();
    const list: { path: string; name: string; branch: string | null }[] = [];

    for (const w of hostDetail.workspaces) {
      if (!w.is_worktree && !seen.has(w.cwd)) {
        seen.add(w.cwd);
        list.push({ path: w.cwd, name: w.title || basenameOfCwd(w.cwd), branch: w.git_branch });
      }
    }
    for (const p of hostDetail.discovered_projects) {
      if (!seen.has(p.path)) {
        seen.add(p.path);
        list.push({ path: p.path, name: p.name, branch: p.git_branch });
      }
    }
    return list.slice(0, 12);
  }, [hostDetail]);

  const activeRepoBranch = useMemo(() => {
    const matched = repoProjects.find((p) => p.path === selectedCwd);
    return matched?.branch ?? null;
  }, [repoProjects, selectedCwd]);

  if (!selectedHostId) return null;

  const hasSubstantiveInput =
    draftText.trim().length > 0 ||
    contexts.length > 0 ||
    pastes.length > 0 ||
    quotes.length > 0;
  const hasPayload =
    hasSubstantiveInput || slashDirective === "review" || slashDirective === "compact";

  const canSwitchAgentEngine = isDraft || !currentAgent;

  const handleSelectModelAndEffort = (
    provider: ProviderKind,
    model: string | null,
    effort: ReasoningEffort | null,
  ) => {
    if (canSwitchAgentEngine) {
      orbitActions.setDraftConfig({
        provider,
        model,
        ...(effort ? { effort } : {}),
      });
      return;
    }
    if (currentAgent) {
      // Active conversations strictly retain their original Agent engine
      orbitActions.setDraftConfig({
        provider: currentAgent.provider,
        model,
        ...(effort ? { effort } : {}),
      });
      void orbitIpc.setAgentConfig(
        selectedHostId,
        currentAgent.agent_id,
        model,
        effort,
      );
    }
  };

  const handleSelectMode = (m: SessionMode) => {
    if (isDraft || !currentAgent) {
      orbitActions.setDraftConfig({ mode: m });
    } else {
      void orbitIpc.setSessionMode(selectedHostId, currentAgent.agent_id, m);
    }
  };

  const executeSlashCommand = (cmd: SlashCommandDef) => {
    const joined = slashTrigger
      ? collapseStrippedToken(
          draftText.slice(0, slashTrigger.slashIndex),
          draftText.slice(slashTrigger.endIndex),
        )
      : { text: draftText, cursor: draftText.length };
    const stripped = joined.text;
    orbitActions.setComposerDraft(stripped);
    setCursorOffset(joined.cursor);
    setDismissedToken(null);
    setMenuHighlightIdx(0);

    switch (cmd.id) {
      case "plan":
        orbitActions.setSlashDirective("plan");
        break;
      case "auto":
        handleSelectMode("accept_edits");
        break;
      case "supervised":
        handleSelectMode("default");
        break;
      case "full":
        handleSelectMode("full_access");
        break;
      case "review":
        orbitActions.setSlashDirective("review");
        break;
      case "diff":
        orbitActions.toggleInspector();
        break;
      case "compact":
        if (!isDraft && currentAgent) {
          orbitActions.setSlashDirective("compact");
        } else {
          orbitActions.showToast("Start an active session first to compact context");
        }
        break;
      case "terminal":
        orbitActions.toggleTerminalDock();
        break;
      case "new":
        orbitActions.startNewThreadDraft();
        break;
      case "settings":
        orbitActions.openSettings();
        break;
    }

    requestAnimationFrame(() => {
      const el = textareaRef.current;
      if (el) {
        el.focus();
        const pos = Math.min(joined.cursor, el.value.length);
        el.setSelectionRange(pos, pos);
      }
    });
  };

  const executeMentionSelect = (candidate: MentionCandidate) => {
    if (!mentionTrigger) return;
    const beforeAt = draftText.slice(0, mentionTrigger.atIndex);
    const afterToken = draftText.slice(mentionTrigger.endIndex);

    const insertInlineToken = (token: string) => {
      const needsLeadingSpace = beforeAt.length > 0 && !/\s$/.test(beforeAt);
      const inserted = `${needsLeadingSpace ? " " : ""}${token} `;
      const trimmedAfter = afterToken.replace(/^[ \t]+/, "");
      const nextDraft = `${beforeAt}${inserted}${trimmedAfter}`;
      const nextPos = beforeAt.length + inserted.length;
      orbitActions.setComposerDraft(nextDraft);
      setCursorOffset(nextPos);
      requestAnimationFrame(() => {
        const el = textareaRef.current;
        if (el) {
          el.focus();
          el.setSelectionRange(nextPos, nextPos);
        }
      });
    };

    if (candidate.kind === "file") {
      const token = `@${candidate.basename}`;
      orbitActions.registerInlineMention({
        token,
        kind: "file",
        path: candidate.path,
      });
      insertInlineToken(token);
    } else if (candidate.kind === "git_diff") {
      const token = candidate.staged ? "@staged-diff" : "@working-diff";
      orbitActions.registerInlineMention({
        token,
        kind: "git_diff",
        staged: candidate.staged,
        label: candidate.label,
      });
      insertInlineToken(token);
    } else if (candidate.kind === "terminal_output") {
      const snap = orbitActions.captureActiveTerminalTail();
      if (snap) {
        const token = "@terminal";
        orbitActions.registerInlineMention({
          token,
          kind: "terminal_output",
          label: snap.label,
          lineCount: snap.lineCount,
          content: snap.content,
        });
        insertInlineToken(token);
      } else {
        const joined = collapseStrippedToken(beforeAt, afterToken);
        orbitActions.setComposerDraft(joined.text);
        setCursorOffset(joined.cursor);
        requestAnimationFrame(() => textareaRef.current?.focus());
      }
    }

    setDismissedToken(null);
    setMenuHighlightIdx(0);
  };

  const triggerAtMentionFromButton = () => {
    if (selectedCwd && selectedCwd !== "~") {
      void orbitActions.ensureWorkspaceFiles(selectedHostId, selectedCwd);
    }
    const el = textareaRef.current;
    const pos = el?.selectionStart ?? draftText.length;
    const before = draftText.slice(0, pos);
    const after = draftText.slice(pos);
    const needsSpace = before.length > 0 && !/\s$/.test(before);
    const prefix = needsSpace ? " @" : "@";
    const next = `${before}${prefix}${after}`;
    const nextPos = before.length + prefix.length;
    setDismissedToken(null);
    setMenuHighlightIdx(0);
    orbitActions.setComposerDraft(next);
    setCursorOffset(nextPos);
    requestAnimationFrame(() => {
      if (textareaRef.current) {
        textareaRef.current.focus();
        textareaRef.current.setSelectionRange(nextPos, nextPos);
      }
    });
  };

  const triggerSlashActionsFromButton = () => {
    const el = textareaRef.current;
    const pos = el?.selectionStart ?? draftText.length;
    const before = draftText.slice(0, pos);
    const after = draftText.slice(pos);
    const needsSpace = before.length > 0 && !/\s$/.test(before);
    const prefix = needsSpace ? " /" : "/";
    const next = `${before}${prefix}${after}`;
    const nextPos = before.length + prefix.length;
    setDismissedToken(null);
    setMenuHighlightIdx(0);
    orbitActions.setComposerDraft(next);
    setCursorOffset(nextPos);
    requestAnimationFrame(() => {
      if (textareaRef.current) {
        textareaRef.current.focus();
        textareaRef.current.setSelectionRange(nextPos, nextPos);
      }
    });
  };

  const handlePaste = (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const items = Array.from(e.clipboardData.items ?? []);
    const imageFiles: File[] = [];
    for (const it of items) {
      if (it.kind === "file" && it.type.startsWith("image/")) {
        const f = it.getAsFile();
        if (f) imageFiles.push(f);
      }
    }
    if (imageFiles.length > 0) {
      e.preventDefault();
      for (const img of imageFiles) {
        void orbitActions.stageAndAttachFile(img);
      }
      return;
    }

    const text = e.clipboardData.getData("text/plain");
    if (!text) return;
    const lineCount = text.split("\n").length;
    if (lineCount >= 60 || text.length >= 4800) {
      e.preventDefault();
      orbitActions.addPastedAttachment(text);
    }
  };

  const handleDrop = (e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setIsDraggingOver(false);
    const files = Array.from(e.dataTransfer.files ?? []);
    for (const file of files) {
      void orbitActions.stageAndAttachFile(file);
    }
    textareaRef.current?.focus();
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.nativeEvent.isComposing) return;

    if (slashTrigger) {
      const len = slashTrigger.items.length;
      const safeIdx = Math.min(menuHighlightIdx, len - 1);
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setMenuHighlightIdx((safeIdx + 1) % len);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setMenuHighlightIdx((safeIdx - 1 + len) % len);
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        executeSlashCommand(slashTrigger.items[safeIdx]);
        return;
      }
      if (e.key === " ") {
        const exactCmd = slashTrigger.items.find(
          (c) =>
            (c.id === "plan" || c.id === "review" || c.id === "compact") &&
            c.command.slice(1).toLowerCase() === slashTrigger.query.toLowerCase(),
        );
        if (exactCmd) {
          e.preventDefault();
          executeSlashCommand(exactCmd);
          return;
        }
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setDismissedToken(slashTrigger.tokenKey);
        return;
      }
    }

    if (
      e.key === "Backspace" &&
      slashDirective &&
      textareaRef.current?.selectionStart === 0 &&
      textareaRef.current?.selectionEnd === 0
    ) {
      e.preventDefault();
      orbitActions.setSlashDirective(null);
      return;
    }

    if (mentionTrigger) {
      const len = mentionTrigger.items.length;
      const safeIdx = Math.min(menuHighlightIdx, len - 1);
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setMenuHighlightIdx((safeIdx + 1) % len);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setMenuHighlightIdx((safeIdx - 1 + len) % len);
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        executeMentionSelect(mentionTrigger.items[safeIdx]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setDismissedToken(mentionTrigger.tokenKey);
        return;
      }
    }

    if (
      e.key === "ArrowUp" &&
      !e.shiftKey &&
      !e.metaKey &&
      !e.altKey &&
      !e.ctrlKey &&
      (draftText.length === 0 || historyCursor !== null) &&
      promptHistory.length > 0
    ) {
      const el = textareaRef.current;
      const atFirstLine = !el || !draftText.slice(0, el.selectionStart).includes("\n");
      if (atFirstLine) {
        e.preventDefault();
        if (historyCursor === null) {
          savedDraftBeforeHistoryRef.current = draftText;
          const nextIdx = promptHistory.length - 1;
          setHistoryCursor(nextIdx);
          orbitActions.setComposerDraft(promptHistory[nextIdx]);
        } else if (historyCursor > 0) {
          const nextIdx = historyCursor - 1;
          setHistoryCursor(nextIdx);
          orbitActions.setComposerDraft(promptHistory[nextIdx]);
        }
        return;
      }
    }

    if (
      e.key === "ArrowDown" &&
      historyCursor !== null &&
      !e.shiftKey &&
      !e.metaKey &&
      !e.altKey &&
      !e.ctrlKey
    ) {
      const el = textareaRef.current;
      const atLastLine = !el || !draftText.slice(el.selectionEnd).includes("\n");
      if (atLastLine) {
        e.preventDefault();
        if (historyCursor < promptHistory.length - 1) {
          const nextIdx = historyCursor + 1;
          setHistoryCursor(nextIdx);
          orbitActions.setComposerDraft(promptHistory[nextIdx]);
        } else {
          setHistoryCursor(null);
          orbitActions.setComposerDraft(savedDraftBeforeHistoryRef.current);
        }
        return;
      }
    }

    if (e.key === "Enter" && e.altKey) {
      e.preventDefault();
      setHistoryCursor(null);
      void orbitActions.submitComposer(true);
      return;
    }

    if (enterToSend) {
      if (e.key === "Enter" && !e.shiftKey && !e.metaKey && !e.ctrlKey) {
        e.preventDefault();
        setHistoryCursor(null);
        void orbitActions.submitComposer(false);
        return;
      }
    } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      setHistoryCursor(null);
      void orbitActions.submitComposer(false);
    }
  };

  const activeHostLabel =
    hostDetail?.host.display_name ??
    fleet.hosts.find((h) => h.host_id === selectedHostId)?.display_name ??
    "Host";

  const rawBrowsedProvider: ProviderKind = canSwitchAgentEngine
    ? modelPopoverOpen
      ? browsedProvider
      : activeProvider
    : activeProvider;
  const effectiveBrowsedProvider: ProviderKind =
    canSwitchAgentEngine &&
    detectedProviders.length > 0 &&
    !detectedProviders.some((p) => p.id === rawBrowsedProvider)
      ? detectedProviders[0].id
      : rawBrowsedProvider;
  const browsedCatalog = getCatalogForProvider(
    hostDetail?.provider_catalogs,
    effectiveBrowsedProvider,
  );
  const resolvedSelectedModelIdOnActive = activeModelSpec?.id ?? rawActiveModel;
  const isCustomModelActiveOnBrowsed =
    effectiveBrowsedProvider === activeProvider &&
    rawActiveModel !== null &&
    activeModelSpec === null;

  const defaultCustomThinkingOptions: ThinkingOptionSpec[] = (
    ["low", "medium", "high", "xhigh", "max"] as ReasoningEffort[]
  ).map((eff) => ({
    effort: eff,
    label: formatEffortLabel(eff),
    description: defaultEffortDescription(eff),
  }));

  const browsedRows: {
    key: string;
    id: string;
    label: string;
    desc: string;
    defaultEffort: ReasoningEffort | null;
    thinkingOptions: ThinkingOptionSpec[];
  }[] = [
    ...(isCustomModelActiveOnBrowsed && rawActiveModel
      ? [
          {
            key: `custom:${rawActiveModel}`,
            id: rawActiveModel,
            label: prettifyModelSlug(rawActiveModel, effectiveBrowsedProvider),
            desc: rawActiveModel,
            defaultEffort: "medium" as ReasoningEffort,
            thinkingOptions: defaultCustomThinkingOptions,
          },
        ]
      : []),
    ...browsedCatalog.models.map((m) => ({
      key: m.id,
      id: m.id,
      label:
        effectiveBrowsedProvider === "claude_code"
          ? m.label.replace(/^Claude\s+/i, "")
          : m.label,
      desc: m.description,
      defaultEffort: m.default_effort,
      thinkingOptions: m.thinking_options,
    })),
  ];

  const hoveredRow = hoveredModelKey
    ? (browsedRows.find((r) => r.key === hoveredModelKey) ?? null)
    : null;

  // Align the top of the Level-2 Thinking Mode flyout flush with the currently hovered model item,
  // clamping only when it would otherwise overflow outside the window viewport.
  useLayoutEffect(() => {
    if (!modelPopoverOpen || !hoveredRow || hoveredRow.thinkingOptions.length === 0) return;
    const popEl = modelPopoverRef.current;
    const rowEl = hoveredRowElRef.current;
    const subEl = thinkingSubmenuRef.current;
    if (!popEl || !rowEl || !subEl) return;

    const popRect = popEl.getBoundingClientRect();
    const rowRect = rowEl.getBoundingClientRect();
    const originY = popRect.top + popEl.clientTop;
    const subHeight = subEl.offsetHeight;
    const subWidth = subEl.offsetWidth;
    const edgeMargin = 12;

    const desiredTop = rowRect.top - originY;
    const maxTopRelative = window.innerHeight - edgeMargin - originY - subHeight;
    const minTopRelative = edgeMargin - originY;
    const clampedTop = Math.round(
      Math.max(minTopRelative, Math.min(desiredTop, maxTopRelative)),
    );

    const flipLeft =
      popRect.right + subWidth + 8 > window.innerWidth - edgeMargin &&
      popRect.left - subWidth - 8 >= edgeMargin;

    setSubmenuPos((prev) =>
      prev.top === clampedTop && prev.flipLeft === flipLeft
        ? prev
        : { top: clampedTop, flipLeft },
    );
  }, [modelPopoverOpen, hoveredRow]);

  return (
    <div
      className={cn(
        "w-full",
        variant === "docked" ? "shrink-0 px-[24px] pt-0 pb-4" : "px-0",
      )}
    >
      <div className="mx-auto flex w-full max-w-[768px] flex-col gap-2">
        <input
          ref={fileInputRef}
          type="file"
          multiple
          className="hidden"
          onChange={(e) => {
            const selected = Array.from(e.target.files ?? []);
            for (const f of selected) {
              void orbitActions.stageAndAttachFile(f);
            }
            e.target.value = "";
            textareaRef.current?.focus();
          }}
        />

        {/* Queued Follow-Up Strip when Agent is Running */}
        {queued.length > 0 && (
          <div className="border-border bg-elevated/95 space-y-1 rounded-2xl border px-4 py-2 text-[12.5px]">
            {queued.map((qItem, idx) => (
              <div key={qItem.id} className="flex items-center justify-between gap-2">
                <div className="text-muted-foreground flex min-w-0 items-center gap-1.5">
                  <Clock className="size-3.5 shrink-0" />
                  <span className="shrink-0 text-[12px] font-medium">Queued</span>
                  {qItem.badges.map((b, bIdx) => (
                    <span
                      key={`${b.kind}-${bIdx}`}
                      className="bg-muted text-foreground/80 shrink-0 rounded px-1.5 py-0.5 font-mono text-[11px]"
                    >
                      {b.label}
                    </span>
                  ))}
                  {qItem.displayText && (
                    <span className="text-foreground/85 truncate">{qItem.displayText}</span>
                  )}
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  <button
                    type="button"
                    onClick={() => void orbitActions.flushQueuedPromptImmediately(idx)}
                    className="text-foreground hover:bg-accent inline-flex cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[11.5px] font-medium"
                  >
                    <Zap className="size-3" /> Steer
                  </button>
                  <button
                    type="button"
                    onClick={() => orbitActions.popQueuedToDraft(idx)}
                    className="text-muted-foreground hover:text-foreground cursor-pointer rounded-md px-2 py-0.5 text-[11.5px]"
                  >
                    Edit
                  </button>
                  <button
                    type="button"
                    onClick={() => orbitActions.removeQueuedPrompt(idx)}
                    className="text-muted-foreground hover:text-foreground cursor-pointer rounded-md p-1"
                  >
                    <X className="size-3.5" />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}

        {/* Relative Anchor for Floating `/` and `@` Menus + Single-Rim Tactile Composer Card */}
        <div
          onDragOver={(e) => {
            e.preventDefault();
            if (!isDraggingOver) setIsDraggingOver(true);
          }}
          onDragLeave={(e) => {
            if (!e.currentTarget.contains(e.relatedTarget as Node | null)) {
              setIsDraggingOver(false);
            }
          }}
          onDrop={handleDrop}
          className="relative"
        >
          {slashTrigger && (
            <SlashCommandMenu
              items={slashTrigger.items}
              activeIndex={Math.min(menuHighlightIdx, slashTrigger.items.length - 1)}
              onHoverIndex={setMenuHighlightIdx}
              onSelect={executeSlashCommand}
            />
          )}

          {mentionTrigger && (
            <MentionMenu
              items={mentionTrigger.items}
              activeIndex={Math.min(menuHighlightIdx, mentionTrigger.items.length - 1)}
              onHoverIndex={setMenuHighlightIdx}
              onSelect={executeMentionSelect}
            />
          )}

          {/* New-Session Composer Top Bar (Codex / Antigravity style: Project + Local/Worktree at Top of Composer) */}
          {variant === "hero" && (
            <div className="flex flex-wrap items-center justify-between gap-2 px-1 pb-2">
              <div className="flex min-w-0 items-center gap-1">
                {/* 1. Project / Workspace Repository Selector */}
                <Popover>
                  <PopoverTrigger asChild>
                    <button
                      type="button"
                      title={selectedCwd}
                      className="text-foreground/72 hover:text-foreground inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[13px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
                    >
                      <Folder className="size-3.5 shrink-0 stroke-[1.75] opacity-75" />
                      <span className="max-w-48 truncate font-normal">
                        {basenameOfCwd(selectedCwd)}
                      </span>
                      <ChevronDown className="text-foreground/45 size-3.5 shrink-0 stroke-[1.75]" />
                    </button>
                  </PopoverTrigger>
                  <PopoverContent side="bottom" align="start" className="w-80 space-y-2 p-2">
                    <div className="text-muted-foreground px-2.5 pt-1 pb-0.5 text-[12px] font-medium">
                      Project Workspace
                    </div>
                    {repoProjects.length > 0 && (
                      <div className="max-h-56 space-y-0.5 overflow-y-auto">
                        {repoProjects.map((proj) => {
                          const active = proj.path === selectedCwd;
                          return (
                            <button
                              key={proj.path}
                              type="button"
                              onClick={() => orbitActions.setSelectedWorkspaceCwd(proj.path)}
                              className={cn(
                                "flex w-full cursor-pointer items-center justify-between gap-2 rounded-[9px] px-2.5 py-1.5 text-left text-[13px] transition-colors",
                                active
                                  ? "bg-accent text-accent-foreground font-medium"
                                  : "text-foreground/90 hover:bg-accent/60",
                              )}
                            >
                              <div className="min-w-0">
                                <div className="flex items-center gap-1.5">
                                  <span className="truncate">{proj.name}</span>
                                  {proj.branch && (
                                    <span className="text-muted-foreground shrink-0 font-mono text-[11px]">
                                      {proj.branch}
                                    </span>
                                  )}
                                </div>
                                <div className="text-muted-foreground truncate font-mono text-[11px]">
                                  {proj.path}
                                </div>
                              </div>
                              {active && <Check className="size-3.5 shrink-0" />}
                            </button>
                          );
                        })}
                      </div>
                    )}
                    <div className="border-border border-t pt-2">
                      <input
                        type="text"
                        value={selectedCwd}
                        onChange={(e) => orbitActions.setSelectedWorkspaceCwd(e.target.value)}
                        placeholder="/path/to/repository"
                        className="border-border bg-background text-foreground h-8 w-full rounded-[9px] border px-2.5 font-mono text-[12px] outline-none focus:border-foreground/30"
                      />
                    </div>
                  </PopoverContent>
                </Popover>

                {/* Optional Multi-Host Fleet Target Selector (when >1 host paired) */}
                {fleet.hosts.length > 1 && (
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <button
                        type="button"
                        className="text-foreground/65 hover:text-foreground inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[13px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
                      >
                        <Server className="size-3.5 shrink-0 stroke-[1.75] opacity-75" />
                        <span className="max-w-36 truncate">{activeHostLabel}</span>
                        <ChevronDown className="text-foreground/45 size-3.5 shrink-0 stroke-[1.75]" />
                      </button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="start" side="bottom" className="w-54">
                      <DropdownMenuLabel>Target Host</DropdownMenuLabel>
                      {fleet.hosts.map((h) => {
                        const active = h.host_id === selectedHostId;
                        const online = h.connection_state.startsWith("online");
                        return (
                          <DropdownMenuItem
                            key={h.host_id}
                            onSelect={() => orbitActions.selectHost(h.host_id)}
                          >
                            <span className="flex min-w-0 items-center gap-2">
                              <span
                                className={cn(
                                  "size-1.5 shrink-0 rounded-full",
                                  online ? "bg-emerald-500" : "bg-zinc-400",
                                )}
                              />
                              <span className="truncate">{h.display_name}</span>
                            </span>
                            {active && <Check className="size-3.5 shrink-0" />}
                          </DropdownMenuItem>
                        );
                      })}
                      <DropdownMenuSeparator />
                      <DropdownMenuItem onSelect={() => orbitActions.openSettings("hosts")}>
                        <span className="text-muted-foreground">Manage Hosts…</span>
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                )}
              </div>

              {/* 2. Right: Local Checkout vs Isolated Git Worktree + Base Branch */}
              <Popover>
                <PopoverTrigger asChild>
                  <button
                    type="button"
                    className="text-foreground/72 hover:text-foreground inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[13px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
                  >
                    {draftUseWorktree ? (
                      <GitBranch className="size-3.5 shrink-0 stroke-[1.75] opacity-75" />
                    ) : (
                      <Laptop className="size-3.5 shrink-0 stroke-[1.75] opacity-75" />
                    )}
                    <span>
                      {draftUseWorktree
                        ? draftWorktreeBranch.trim() || "Worktree"
                        : "Local"}
                    </span>
                    {activeRepoBranch && (
                      <span className="text-foreground/45 max-w-28 truncate font-mono text-[12px]">
                        · {activeRepoBranch}
                      </span>
                    )}
                    <ChevronDown className="text-foreground/45 size-3.5 shrink-0 stroke-[1.75]" />
                  </button>
                </PopoverTrigger>
                <PopoverContent side="bottom" align="end" className="w-74 space-y-2 p-2.5">
                  <div className="text-muted-foreground px-1.5 pt-0.5 text-[12px] font-medium">
                    Execution Environment
                  </div>
                  <div className="space-y-1">
                    <button
                      type="button"
                      onClick={() => orbitActions.setDraftConfig({ useWorktree: false })}
                      className={cn(
                        "flex w-full cursor-pointer items-start justify-between gap-2 rounded-[9px] p-2 text-left transition-colors",
                        !draftUseWorktree ? "bg-accent" : "hover:bg-accent/50",
                      )}
                    >
                      <div className="flex items-start gap-2">
                        <Laptop className="text-foreground/70 mt-0.5 size-3.5 shrink-0 stroke-[1.75]" />
                        <div>
                          <div className="text-[13px] font-medium">Local</div>
                          <div className="text-muted-foreground text-[11.5px]">
                            Run directly inside current checkout
                          </div>
                        </div>
                      </div>
                      {!draftUseWorktree && <Check className="mt-0.5 size-3.5 shrink-0" />}
                    </button>

                    <button
                      type="button"
                      onClick={() => orbitActions.setDraftConfig({ useWorktree: true })}
                      className={cn(
                        "flex w-full cursor-pointer items-start justify-between gap-2 rounded-[9px] p-2 text-left transition-colors",
                        draftUseWorktree ? "bg-accent" : "hover:bg-accent/50",
                      )}
                    >
                      <div className="flex items-start gap-2">
                        <GitBranch className="text-foreground/70 mt-0.5 size-3.5 shrink-0 stroke-[1.75]" />
                        <div>
                          <div className="text-[13px] font-medium">Worktree</div>
                          <div className="text-muted-foreground text-[11.5px]">
                            Isolated git worktree for parallel tasks
                          </div>
                        </div>
                      </div>
                      {draftUseWorktree && <Check className="mt-0.5 size-3.5 shrink-0" />}
                    </button>
                  </div>

                  {draftUseWorktree && (
                    <div className="border-border space-y-1 border-t pt-2">
                      <label className="text-muted-foreground block px-0.5 text-[11.5px]">
                        Branch name (optional)
                      </label>
                      <input
                        type="text"
                        value={draftWorktreeBranch}
                        onChange={(e) =>
                          orbitActions.setDraftConfig({ worktreeBranch: e.target.value })
                        }
                        placeholder={activeRepoBranch ? `from ${activeRepoBranch} (auto)` : "auto"}
                        className="border-border bg-background text-foreground h-8 w-full rounded-[9px] border px-2.5 font-mono text-[12px] outline-none focus:border-foreground/30"
                      />
                    </div>
                  )}
                </PopoverContent>
              </Popover>
            </div>
          )}

          <div
            data-occluding={occluding ? "true" : undefined}
            className={cn(
              "composer-surface relative rounded-[20px]",
              isDraggingOver && "ring-primary/25 border-primary ring-2",
            )}
          >
            {isDraggingOver && (
              <div className="bg-background/85 text-foreground pointer-events-none absolute inset-0 z-20 flex items-center justify-center rounded-[20px] text-[13px] font-medium backdrop-blur-[2px]">
                Drop images or files to attach
              </div>
            )}

            {/* Unified Context Tray — strictly for uploaded files/images, diff quotes & large pastes */}
            <ContextTray
              contexts={contexts}
              quotes={quotes}
              pastes={pastes}
            />

            {/* Auto-growing Prose Textarea + Inline Slash Token (`[icon] plan |`) + Placeholder */}
            <div className="relative">
              {slashDirective && (
                <span
                  ref={directivePrefixRef}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    textareaRef.current?.focus();
                  }}
                  className="pointer-events-auto absolute top-[17px] left-[18px] z-10 inline-flex h-[20px] cursor-text items-center gap-[5.5px] select-none"
                >
                  {slashDirective === "plan" ? (
                    <ListTodo className="text-muted-foreground size-[15px] shrink-0 stroke-[1.75]" />
                  ) : slashDirective === "review" ? (
                    <FileSearch className="text-muted-foreground size-[15px] shrink-0 stroke-[1.75]" />
                  ) : (
                    <FoldVertical className="text-muted-foreground size-[15px] shrink-0 stroke-[1.75]" />
                  )}
                  <span className="text-foreground text-[15px] leading-[20px] font-normal">
                    {slashDirective}
                  </span>
                </span>
              )}

              {!slashDirective && draftText.length === 0 && (
                <div
                  aria-hidden="true"
                  className="text-foreground/45 pointer-events-none absolute top-[17px] right-[18px] left-[18px] truncate text-[15px] leading-[20px] select-none"
                >
                  {isRunning
                    ? "Ask a follow-up, @ to mention, or ⌥↵ to steer"
                    : "Ask anything, @ to mention, / for actions"}
                </div>
              )}

              <textarea
                ref={textareaRef}
                value={draftText}
                aria-label="Prompt input"
                style={
                  slashDirective && directiveIndentPx > 0
                    ? { textIndent: `${directiveIndentPx}px` }
                    : undefined
                }
                onChange={(e) => {
                  const val = e.target.value;
                  const pos = e.target.selectionStart ?? val.length;
                  setHistoryCursor(null);
                  setDismissedToken(null);
                  setMenuHighlightIdx(0);
                  setCursorOffset(pos);
                  orbitActions.setComposerDraft(val);
                  if (val.includes("@") && selectedCwd && selectedCwd !== "~") {
                    void orbitActions.ensureWorkspaceFiles(selectedHostId, selectedCwd);
                  }
                }}
                onSelect={(e) => setCursorOffset(e.currentTarget.selectionStart ?? 0)}
                onClick={(e) => setCursorOffset(e.currentTarget.selectionStart ?? 0)}
                onPaste={handlePaste}
                onKeyDown={handleKeyDown}
                rows={1}
                className="text-foreground block w-full resize-none bg-transparent px-[18px] pt-[17px] pb-2.5 text-[15px] leading-[20px] outline-none"
              />
            </div>

            {/* Paseo / Antigravity Bottom Control Bar (`34px` uniform control rhythm) */}
            <div className="flex items-center justify-between gap-2 px-3 pt-0 pb-2.5">
              {/* Left Cluster: `[+]` -> `Model Picker` -> `Thinking Effort` */}
              <div className="flex min-w-0 items-center gap-0.5">
                {/* 1. `[+]` Add Context Menu (`34×34` -> Files, Mentions, Actions) */}
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <button
                      type="button"
                      title="Add context"
                      className="text-foreground/70 hover:text-foreground inline-flex size-8.5 shrink-0 cursor-pointer items-center justify-center rounded-full transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
                    >
                      <Plus className="size-[18px] stroke-[1.75]" />
                    </button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent
                    align="start"
                    side="top"
                    onCloseAutoFocus={(e) => e.preventDefault()}
                    className="w-[180px] p-1.5"
                  >
                    <DropdownMenuLabel>Add Context</DropdownMenuLabel>
                    <DropdownMenuItem
                      onSelect={() => fileInputRef.current?.click()}
                      className="py-1.5"
                    >
                      <span className="flex items-center gap-2.5">
                        <Paperclip className="text-muted-foreground size-4 stroke-[1.75]" />
                        <span className="text-[13px]">Files</span>
                      </span>
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      onSelect={triggerAtMentionFromButton}
                      className="py-1.5"
                    >
                      <span className="flex items-center gap-2.5">
                        <AtSign className="text-muted-foreground size-4 stroke-[1.75]" />
                        <span className="text-[13px]">Mentions</span>
                      </span>
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      onSelect={triggerSlashActionsFromButton}
                      className="py-1.5"
                    >
                      <span className="flex items-center gap-2.5">
                        <SquareSlash className="text-muted-foreground size-4 stroke-[1.75]" />
                        <span className="text-[13px]">Actions</span>
                      </span>
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>

                {/* 2. Paseo Catalog + Antigravity-Style Cascading Model -> Thinking Mode Selector */}
                <Popover
                  open={modelPopoverOpen}
                  onOpenChange={(open) => {
                    setModelPopoverOpen(open);
                    if (open) {
                      setBrowsedProvider(activeProvider);
                      setHoveredModelKey(null);
                      if (!hostDetail?.provider_catalogs?.length) {
                        void orbitIpc.refreshProviderModels(selectedHostId, false);
                      }
                    }
                  }}
                >
                  <PopoverTrigger asChild>
                    <button
                      type="button"
                      className="hover:text-foreground inline-flex h-8.5 max-w-76 cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[13.5px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
                    >
                      <ProviderIcon provider={activeProvider} className="size-4" />
                      <span className="text-foreground/88 truncate font-normal">
                        {formatCatalogModelLabel(activeCatalog, rawActiveModel)}
                      </span>
                      {effectiveActiveEffort && (
                        <span className="text-foreground/48 shrink-0 font-normal">
                          {formatEffortLabel(effectiveActiveEffort)}
                        </span>
                      )}
                      <ChevronDown className="text-foreground/45 size-3.5 shrink-0 stroke-[1.75]" />
                    </button>
                  </PopoverTrigger>
                  <PopoverContent
                    ref={modelPopoverRef}
                    side="top"
                    align="start"
                    collisionPadding={12}
                    className="relative w-[280px] overflow-visible p-1.5"
                  >
                    <div className="flex flex-col gap-1">
                      {/* Segmented Agent Engine Switcher — strictly New Session & detected CLIs only */}
                      {canSwitchAgentEngine && detectedProviders.length > 1 ? (
                        <div
                          onMouseEnter={() => setHoveredModelKey(null)}
                          style={{
                            gridTemplateColumns: `repeat(${detectedProviders.length}, minmax(0, 1fr))`,
                          }}
                          className="bg-muted/85 grid gap-0.5 rounded-[9px] p-0.5"
                        >
                          {detectedProviders.map((p) => {
                            const isTabSelected = p.id === effectiveBrowsedProvider;
                            return (
                              <button
                                key={p.id}
                                type="button"
                                onClick={() => {
                                  setBrowsedProvider(p.id);
                                  setHoveredModelKey(null);
                                }}
                                className={cn(
                                  "flex h-[26px] cursor-pointer items-center justify-center gap-1.5 rounded-[7px] px-2 text-[12px] font-medium transition-colors",
                                  isTabSelected
                                    ? "bg-elevated text-foreground shadow-2xs"
                                    : "text-muted-foreground hover:text-foreground",
                                )}
                              >
                                <ProviderIcon provider={p.id} className="size-3.5" />
                                <span className="truncate">{p.shortLabel}</span>
                              </button>
                            );
                          })}
                        </div>
                      ) : (
                        <div
                          onMouseEnter={() => setHoveredModelKey(null)}
                          className="text-foreground/55 flex items-center px-2 pt-0.5 pb-0.5 text-[12px] font-medium"
                        >
                          <span className="flex items-center gap-1.5">
                            <ProviderIcon provider={activeProvider} className="size-3.5" />
                            <span>
                              {PROVIDERS.find((p) => p.id === activeProvider)?.label ??
                                activeProvider}
                            </span>
                          </span>
                        </div>
                      )}

                      {/* Level 1: Compact Antigravity-Styled Model List */}
                      <div
                        ref={modelListRef}
                        onScroll={() => setHoveredModelKey(null)}
                        className="max-h-[304px] overflow-y-auto"
                      >
                        {browsedRows.map((row) => {
                          const isModelActive =
                            effectiveBrowsedProvider === activeProvider &&
                            resolvedSelectedModelIdOnActive?.toLowerCase() === row.id.toLowerCase();
                          const isSubOpen = hoveredModelKey === row.key;
                          const hasThinkingSubmenu = row.thinkingOptions.length > 0;
                          const rowEffort: ReasoningEffort | null = hasThinkingSubmenu
                            ? isModelActive
                              ? effectiveActiveEffort
                              : (row.defaultEffort ?? row.thinkingOptions[0]?.effort ?? null)
                            : null;

                          return (
                            <button
                              key={row.key}
                              type="button"
                              onMouseEnter={(e) => {
                                hoveredRowElRef.current = e.currentTarget;
                                setHoveredModelKey(row.key);
                              }}
                              onClick={() => {
                                const nextEffort = hasThinkingSubmenu
                                  ? (row.thinkingOptions.some(
                                      (o) => o.effort === effectiveActiveEffort,
                                    )
                                      ? effectiveActiveEffort
                                      : (row.defaultEffort ??
                                        row.thinkingOptions[0]?.effort ??
                                        null))
                                  : null;
                                handleSelectModelAndEffort(
                                  effectiveBrowsedProvider,
                                  row.id,
                                  nextEffort,
                                );
                                setModelPopoverOpen(false);
                              }}
                              className={cn(
                                "group/model flex h-[28px] w-full cursor-pointer items-center justify-between gap-2 rounded-[7px] px-2 text-left transition-colors",
                                isSubOpen ? "bg-accent/75" : "hover:bg-accent/65",
                              )}
                            >
                              <div className="flex min-w-0 items-baseline gap-1.5">
                                <span
                                  className={cn(
                                    "truncate text-[12.5px] leading-normal",
                                    isModelActive
                                      ? "text-foreground font-medium"
                                      : "text-foreground/82 group-hover/model:text-foreground font-normal",
                                  )}
                                >
                                  {row.label}
                                </span>
                                {rowEffort && (
                                  <span className="text-muted-foreground shrink-0 text-[11.5px] leading-normal font-normal">
                                    {formatEffortLabel(rowEffort)}
                                  </span>
                                )}
                              </div>

                              <div className="flex shrink-0 items-center">
                                {isModelActive ? (
                                  <Check className="text-foreground size-3.5 shrink-0 stroke-[1.9]" />
                                ) : hasThinkingSubmenu ? (
                                  <ChevronRight className="text-muted-foreground/75 size-3.5 shrink-0 stroke-[1.75]" />
                                ) : null}
                              </div>
                            </button>
                          );
                        })}
                      </div>
                    </div>

                    {/* Zero-Footprint Overlay Anchor for Level-2 Thinking Mode Flyout */}
                    <div className="pointer-events-none absolute inset-0 overflow-visible">
                      {hoveredRow && hoveredRow.thinkingOptions.length > 0 && (
                        <div
                          ref={thinkingSubmenuRef}
                          style={{ top: `${submenuPos.top}px` }}
                          className={cn(
                            "pointer-events-auto absolute z-50",
                            submenuPos.flipLeft ? "right-full pr-1.5" : "left-full pl-1.5",
                          )}
                        >
                          <div className="bg-popover text-popover-foreground floating-surface w-[264px] rounded-[14px] border border-black/[0.09] p-1.5 dark:border-white/[0.10]">
                            <div className="text-muted-foreground px-2 pt-0.5 pb-1 text-[12px] font-medium">
                              Thinking Mode
                            </div>
                            {hoveredRow.thinkingOptions.map((opt) => {
                              const isHoveredModelActive =
                                effectiveBrowsedProvider === activeProvider &&
                                resolvedSelectedModelIdOnActive?.toLowerCase() ===
                                  hoveredRow.id.toLowerCase();
                              const isEffortSelected = isHoveredModelActive
                                ? opt.effort === effectiveActiveEffort
                                : opt.effort === hoveredRow.defaultEffort;
                              return (
                                <button
                                  key={opt.effort}
                                  type="button"
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    handleSelectModelAndEffort(
                                      effectiveBrowsedProvider,
                                      hoveredRow.id,
                                      opt.effort,
                                    );
                                    setModelPopoverOpen(false);
                                  }}
                                  className="hover:bg-accent/65 flex w-full cursor-pointer items-center justify-between gap-2 rounded-[7px] px-2 py-1 text-left transition-colors"
                                >
                                  <div className="min-w-0">
                                    <div
                                      className={cn(
                                        "text-[12px] leading-snug",
                                        isEffortSelected && isHoveredModelActive
                                          ? "text-foreground font-medium"
                                          : "text-foreground/85",
                                      )}
                                    >
                                      {opt.label}
                                    </div>
                                    <div className="text-muted-foreground truncate text-[10.5px] leading-normal">
                                      {conciseThinkingDescription(opt)}
                                    </div>
                                  </div>
                                  {isEffortSelected && isHoveredModelActive && (
                                    <Check className="text-foreground size-3.5 shrink-0 stroke-[1.9]" />
                                  )}
                                </button>
                              );
                            })}
                          </div>
                        </div>
                      )}
                    </div>
                  </PopoverContent>
                </Popover>
              </div>

              {/* Right Cluster: Access Permissions Selector + `34×34` Primary Action Circle */}
              <div className="flex shrink-0 items-center gap-1.5">
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <button
                      type="button"
                      title="Access permissions (⇧Tab)"
                      className="text-foreground/78 hover:text-foreground inline-flex h-8.5 cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[13.5px] font-normal transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
                    >
                      <ModeIcon mode={activeMode} className="opacity-75" />
                      <span className="hidden sm:inline">{modeLabel(activeMode)}</span>
                      <ChevronDown className="text-foreground/45 size-3.5 shrink-0 stroke-[1.75]" />
                    </button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent
                    align="end"
                    side="top"
                    className="w-[368px] p-1.5"
                  >
                    <DropdownMenuLabel className="flex items-center justify-between">
                      <span>Access Permissions</span>
                      <KbdGroup>
                        <Kbd>⇧</Kbd>
                        <Kbd>Tab</Kbd>
                      </KbdGroup>
                    </DropdownMenuLabel>
                    {PERMISSION_MODES.map((m) => {
                      const selected = m.id === activeMode;
                      return (
                        <DropdownMenuItem
                          key={m.id}
                          onSelect={() => handleSelectMode(m.id)}
                          className="items-start gap-3 px-2.5 py-2"
                        >
                          <div className="flex min-w-0 flex-1 items-start gap-2.5">
                            <ModeIcon
                              mode={m.id}
                              className={cn(
                                "mt-0.5 size-4 shrink-0",
                                selected ? "text-foreground" : "text-muted-foreground",
                              )}
                            />
                            <div className="flex min-w-0 flex-col gap-0.5">
                              <span className="text-foreground text-[13px] leading-tight font-medium whitespace-nowrap">
                                {m.label}
                              </span>
                              <span className="text-muted-foreground text-[11.5px] leading-snug whitespace-nowrap">
                                {m.desc}
                              </span>
                            </div>
                          </div>
                          {selected && (
                            <Check className="text-foreground mt-0.5 size-3.5 shrink-0" />
                          )}
                        </DropdownMenuItem>
                      );
                    })}
                  </DropdownMenuContent>
                </DropdownMenu>

                {/* Primary Morphing Action Circle (`34px × 34px` = `size-8.5`): Stop | Steer + Queue | Send */}
                {isRunning && !hasPayload ? (
                  <button
                    type="button"
                    title="Stop generation"
                    onClick={() => {
                      if (currentAgent) {
                        void orbitIpc.interruptAgent(selectedHostId, currentAgent.agent_id);
                      }
                    }}
                    className="inline-flex size-8.5 cursor-pointer items-center justify-center rounded-full bg-black/[0.06] transition-colors hover:bg-black/[0.10] dark:bg-white/[0.09] dark:hover:bg-white/[0.14]"
                  >
                    <span className="size-[11px] rounded-[2.5px] bg-[#EF4444]" />
                  </button>
                ) : isRunning && hasPayload ? (
                  <div className="flex items-center gap-1.5">
                    <button
                      type="button"
                      title="Interrupt and steer immediately (⌥↵)"
                      onClick={() => void orbitActions.submitComposer(true)}
                      className="border-border bg-secondary/85 text-foreground hover:bg-accent inline-flex h-8.5 cursor-pointer items-center gap-1 rounded-full border px-3 text-[13px] font-medium transition-colors"
                    >
                      <Zap className="size-3.5" />
                      <span>Steer</span>
                    </button>
                    <button
                      type="button"
                      title="Queue follow-up"
                      onClick={() => void orbitActions.submitComposer(false)}
                      className="inline-flex size-8.5 cursor-pointer items-center justify-center rounded-full bg-[#141416] text-white transition-colors hover:bg-[#27272A] dark:bg-[#F4F4F6] dark:text-[#141416] dark:hover:bg-white"
                    >
                      <ArrowUp className="size-[17px] stroke-[2.2]" />
                    </button>
                  </div>
                ) : (
                  <button
                    type="button"
                    disabled={!hasPayload}
                    title="Send"
                    onClick={() => void orbitActions.submitComposer(false)}
                    className={cn(
                      "inline-flex size-8.5 cursor-pointer items-center justify-center rounded-full transition-colors",
                      hasPayload
                        ? "bg-[#141416] text-white hover:bg-[#27272A] dark:bg-[#F4F4F6] dark:text-[#141416] dark:hover:bg-white"
                        : "text-foreground/30 pointer-events-none bg-black/[0.055] dark:bg-white/[0.08] dark:text-white/30",
                    )}
                  >
                    <ArrowUp className="size-[17px] stroke-[2.15]" />
                  </button>
                )}
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
