import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  ArrowUp,
  AtSign,
  CornerDownRight,
  FileSearch,
  FoldVertical,
  GripVertical,
  Layers,
  ListTodo,
  Paperclip,
  Plus,
  Send,
  Sparkles,
  SquareSlash,
  Users,
  X,
} from "lucide-react";
import {
  collapseStrippedToken,
  normalizePermissionMode,
} from "@/components/composer/composer-constants";
import { ComposerHeroHeader } from "@/components/composer/composer-hero-header";
import { ComposerModePicker } from "@/components/composer/composer-mode-picker";
import { ComposerModelPicker } from "@/components/composer/composer-model-picker";
import { ContextTray } from "@/components/composer/context-tray";
import {
  MentionMenu,
} from "@/components/composer/mention-menu";
import {
  SlashCommandMenu,
  filterSlashCommands,
} from "@/components/composer/slash-menu";
import { useImeCompositionGuard } from "@/lib/ime";
import { modSymbol } from "@/lib/keybindings";
import {
  type ClipboardImagePayload,
  type GitFileEntry,
  type MentionCandidateView,
  type QueuedPromptView,
  type SessionMode,
  type SlashCommandView,
  isAgentForegroundRunning,
  isMacDesktop,
  isTauriRuntime,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import {
  type ContextMentionItem,
  type DiffQuoteSnippet,
  type PastedAttachment,
  type QueuedPromptItem,
  composerScopeKey,
  orbitActions,
  useComposerDraft,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

// Module-level frozen empty arrays for React 19 `useSyncExternalStore` snapshot stability
const EMPTY_CONTEXTS: readonly ContextMentionItem[] = [];
const EMPTY_PASTES: readonly PastedAttachment[] = [];
const EMPTY_QUOTES: readonly DiffQuoteSnippet[] = [];
const EMPTY_QUEUED: readonly QueuedPromptItem[] = [];
const EMPTY_SERVER_QUEUED: readonly QueuedPromptView[] = [];
const EMPTY_FILES: readonly string[] = [];
const EMPTY_GIT_FILES: readonly GitFileEntry[] = [];
const EMPTY_MENTIONS: MentionCandidateView[] = [];

const IMAGE_URI_EXT_RE = /\.(png|jpe?g|webp|gif|bmp|svg|avif|ico)(?:[?#].*)?$/i;

function clipboardPayloadToFile(payload: ClipboardImagePayload): File {
  const binary = window.atob(payload.data_b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return new File([bytes], payload.filename || `screenshot-${Date.now()}.png`, {
    type: payload.mime_type || "image/png",
  });
}

function uriListContainsImageFile(uriList: string): boolean {
  for (const rawLine of uriList.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;
    if (line.startsWith("file://") && IMAGE_URI_EXT_RE.test(line)) {
      return true;
    }
  }
  return false;
}

function isStandaloneHtmlImageCopy(html: string): boolean {
  const stripped = html
    .replace(/<!--[\s\S]*?-->/g, "")
    .replace(/<\/?(?:html|head|body|meta)\b[^>]*>/gi, "")
    .trim();
  return /^<img\b[^>]*\/?>$/i.test(stripped);
}

export interface OmniComposerProps {
  variant?: "hero" | "docked";
  occluding?: boolean;
}

export const OmniComposer = memo(function OmniComposer({
  variant = "docked",
  occluding = false,
}: OmniComposerProps) {
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const isDraft = useOrbitSelector((s) => s.isDraftNewSession);
  const draftMode = useOrbitSelector((s) => s.draftMode);
  const enterToSend = useOrbitSelector((s) => s.enterToSend);
  const promptHistory = useOrbitSelector((s) => s.promptHistory);
  const slashCommands = useOrbitSelector((s) => s.slashCommands);
  const ime = useImeCompositionGuard();

  // Primitive/narrow selectors instead of subscribing to full `HostDetailView`
  const hasCurrentAgent = useOrbitSelector((s) =>
    Boolean(
      !s.isDraftNewSession &&
        s.selectedHostId &&
        s.selectedAgentId &&
        s.hostDetails[s.selectedHostId]?.agents.some((a) => a.agent_id === s.selectedAgentId),
    ),
  );
  const isRunning = useOrbitSelector((s) =>
    Boolean(
      !s.isDraftNewSession &&
        s.selectedHostId &&
        s.selectedAgentId &&
        isAgentForegroundRunning(
          s.hostDetails[s.selectedHostId]?.agents.find((a) => a.agent_id === s.selectedAgentId),
        ),
    ),
  );
  const isAwaitingPermission = useOrbitSelector((s) =>
    Boolean(
      !s.isDraftNewSession &&
        s.selectedHostId &&
        s.selectedAgentId &&
        s.hostDetails[s.selectedHostId]?.agents.find((a) => a.agent_id === s.selectedAgentId)
          ?.is_awaiting_permission,
    ),
  );
  const isTurnBusy = isRunning || isAwaitingPermission;
  const activeAgentMode = useOrbitSelector((s) => {
    if (s.isDraftNewSession || !s.selectedHostId || !s.selectedAgentId) return null;
    return (
      s.hostDetails[s.selectedHostId]?.agents.find((a) => a.agent_id === s.selectedAgentId)
        ?.mode ?? null
    );
  });
  const activeWorkspaceId = useOrbitSelector((s) => {
    if (s.isDraftNewSession || !s.selectedHostId || !s.selectedAgentId) return null;
    return (
      s.hostDetails[s.selectedHostId]?.agents.find((a) => a.agent_id === s.selectedAgentId)
        ?.workspace_id ?? null
    );
  });

  const scopeKey = useOrbitSelector(composerScopeKey);
  const draftText = useComposerDraft(scopeKey);
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
  const localQueued = useOrbitSelector((s) => s.queuedPrompts[composerScopeKey(s)] ?? EMPTY_QUEUED);
  const hostSupportsServerQueue = useOrbitSelector((s) =>
    Boolean(s.selectedHostId && s.hostDetails[s.selectedHostId]?.host.supports_server_queue),
  );
  const serverQueued = useOrbitSelector((s) => {
    if (s.isDraftNewSession || !s.selectedHostId || !s.selectedAgentId) return EMPTY_SERVER_QUEUED;
    return (
      s.hostDetails[s.selectedHostId]?.agents.find((a) => a.agent_id === s.selectedAgentId)
        ?.queued_prompts ?? EMPTY_SERVER_QUEUED
    );
  });
  const queued = useMemo(() => {
    if (hostSupportsServerQueue) {
      return serverQueued.map((q) => ({
        id: q.queue_id,
        displayText: q.display_text,
        badges: q.badges ?? [],
      }));
    }
    return localQueued;
  }, [hostSupportsServerQueue, serverQueued, localQueued]);

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
  const mirrorTextareaRef = useRef<HTMLDivElement | null>(null);
  const lastTextareaHeightRef = useRef<number>(46);
  const [composerWidthPx, setComposerWidthPx] = useState(0);
  const draftTextRef = useRef<string>(draftText);
  draftTextRef.current = draftText;
  const directivePrefixRef = useRef<HTMLSpanElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const savedDraftBeforeHistoryRef = useRef<string>("");
  const pasteHandledForCtrlVRef = useRef<boolean>(false);
  const nativeClipboardInFlightRef = useRef<boolean>(false);

  const [cursorOffset, setCursorOffset] = useState<number>(0);
  const [directiveIndentPx, setDirectiveIndentPx] = useState<number>(0);
  const [queueDrag, setQueueDrag] = useState<{
    fromIndex: number;
    overIndex: number;
    deltaY: number;
    rowStep: number;
  } | null>(null);
  const queueRowRefs = useRef<(HTMLDivElement | null)[]>([]);
  const queueDragCleanupRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    // Abort any in-flight drag cleanly if the queue shrinks or swaps underneath us
    if (queueDrag && queueDrag.fromIndex >= queued.length) {
      queueDragCleanupRef.current?.();
    }
  }, [queueDrag, queued.length]);

  useEffect(() => {
    return () => {
      queueDragCleanupRef.current?.();
    };
  }, []);

  const handleQueueRowPointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>, fromIndex: number) => {
      if (e.button !== 0 || queued.length < 2) return;
      if ((e.target as HTMLElement | null)?.closest("button")) return;

      const firstRect = queueRowRefs.current[0]?.getBoundingClientRect();
      const secondRect = queueRowRefs.current[1]?.getBoundingClientRect();
      const measuredStep =
        firstRect && secondRect && secondRect.top > firstRect.top
          ? secondRect.top - firstRect.top
          : 30;
      const startY = e.clientY;
      const count = queued.length;
      const minDeltaY = -fromIndex * measuredStep;
      const maxDeltaY = (count - 1 - fromIndex) * measuredStep;

      let active = false;
      let overIndex = fromIndex;

      const cleanup = () => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        window.removeEventListener("pointercancel", onCancel);
        window.removeEventListener("keydown", onKeyDown);
        queueDragCleanupRef.current = null;
        setQueueDrag(null);
      };

      const onMove = (ev: PointerEvent) => {
        const rawDelta = ev.clientY - startY;
        if (!active) {
          if (Math.abs(rawDelta) < 4) return;
          active = true;
        }
        const clampedDeltaY = Math.max(minDeltaY, Math.min(maxDeltaY, rawDelta));
        const slotShift = Math.round(clampedDeltaY / measuredStep);
        overIndex = Math.max(0, Math.min(count - 1, fromIndex + slotShift));
        setQueueDrag({
          fromIndex,
          overIndex,
          deltaY: clampedDeltaY,
          rowStep: measuredStep,
        });
      };

      const onUp = () => {
        const shouldCommit = active && overIndex !== fromIndex;
        const targetIndex = overIndex;
        cleanup();
        if (shouldCommit) {
          orbitActions.reorderQueuedPrompts(fromIndex, targetIndex);
        }
      };

      const onCancel = () => {
        cleanup();
      };

      const onKeyDown = (ev: KeyboardEvent) => {
        if (ev.key === "Escape") {
          cleanup();
        }
      };

      queueDragCleanupRef.current?.();
      queueDragCleanupRef.current = cleanup;
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
      window.addEventListener("pointercancel", onCancel);
      window.addEventListener("keydown", onKeyDown);
    },
    [queued.length],
  );
  const [menuHighlightIdx, setMenuHighlightIdx] = useState<number>(0);
  const [dismissedToken, setDismissedToken] = useState<string | null>(null);
  const [isDraggingOver, setIsDraggingOver] = useState<boolean>(false);
  const [historyCursor, setHistoryCursor] = useState<number | null>(null);
  const [rankedMentions, setRankedMentions] = useState<MentionCandidateView[]>(EMPTY_MENTIONS);
  const mentionReqSeqRef = useRef<number>(0);

  const activeMode: SessionMode = normalizePermissionMode(
    !isDraft && activeAgentMode ? activeAgentMode : draftMode,
  );

  // Warm workspace file list lazily so `@` mentions feel instantaneous
  useEffect(() => {
    if (!selectedHostId || !selectedCwd || selectedCwd === "~") return;
    const timer = window.setTimeout(() => {
      void orbitActions.ensureWorkspaceFiles(selectedHostId, selectedCwd);
    }, 180);
    return () => window.clearTimeout(timer);
  }, [selectedHostId, selectedCwd]);

  const minTextareaHeight = 46;
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
    let lastW = Math.round(el.clientWidth);
    if (lastW > 0) setComposerWidthPx(lastW);
    const ro = new ResizeObserver((entries) => {
      const nextW = Math.round(entries[0]?.contentRect.width ?? 0);
      if (nextW > 0 && nextW !== lastW) {
        lastW = nextW;
        // Avoid re-rendering the entire composer tree on every sidebar animation frame when empty.
        if (el.value.length > 0) {
          setComposerWidthPx(nextW);
        }
      }
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Measure wrapped textarea height from the invisible off-flow `<div>` mirror BEFORE paint (`useLayoutEffect`),
  // ensuring both incremental typing and large multi-line pastes expand deterministically in WKWebView.
  useLayoutEffect(() => {
    const el = textareaRef.current;
    if (!el) return;

    if (draftText.length === 0 && !slashDirective) {
      if (lastTextareaHeightRef.current !== minTextareaHeight || el.style.height !== "46px") {
        lastTextareaHeightRef.current = minTextareaHeight;
        el.style.height = "46px";
      }
      if (el.style.overflowY !== "hidden") {
        el.style.overflowY = "hidden";
      }
      return;
    }

    const mirror = mirrorTextareaRef.current;
    if (!mirror) return;
    const measured = Math.max(mirror.offsetHeight, mirror.scrollHeight);
    const nextHeight = Math.min(maxTextareaHeight, Math.max(minTextareaHeight, measured));
    if (lastTextareaHeightRef.current !== nextHeight || el.style.height !== `${nextHeight}px`) {
      lastTextareaHeightRef.current = nextHeight;
      el.style.height = `${nextHeight}px`;
    }
    const nextOverflow = measured > maxTextareaHeight ? "auto" : "hidden";
    if (el.style.overflowY !== nextOverflow) {
      el.style.overflowY = nextOverflow;
    }
  }, [draftText, scopeKey, slashDirective, directiveIndentPx, composerWidthPx]);

  useEffect(() => {
    return orbitActions.onComposerFocusRequest(() => {
      textareaRef.current?.focus();
    });
  }, []);

  const slashTrigger = useMemo(() => {
    if (!draftText.includes("/")) return null;
    const safeOffset = Math.min(cursorOffset, draftText.length);
    const before = draftText.slice(0, safeOffset);
    const match = /(?:^|\s)\/([a-zA-Z0-9_-]*)$/.exec(before);
    if (!match) return null;
    const slashIndex = before.lastIndexOf("/");
    const query = match[1];
    const tokenKey = `slash:${slashIndex}:${query}`;
    if (dismissedToken === tokenKey) return null;
    const items = filterSlashCommands(slashCommands, query);
    if (items.length === 0) return null;
    return { slashIndex, endIndex: safeOffset, query, tokenKey, items };
  }, [draftText, cursorOffset, dismissedToken, slashCommands]);

  const mentionTrigger = useMemo(() => {
    if (slashTrigger || !draftText.includes("@")) return null;
    const safeOffset = Math.min(cursorOffset, draftText.length);
    const before = draftText.slice(0, safeOffset);
    const match = /(?:^|\s)@([^\s@]*)$/.exec(before);
    if (!match) return null;
    const atIndex = before.lastIndexOf("@");
    const query = match[1];
    const tokenKey = `at:${atIndex}:${query}`;
    if (dismissedToken === tokenKey) return null;
    return { atIndex, endIndex: safeOffset, query, tokenKey };
  }, [slashTrigger, draftText, cursorOffset, dismissedToken]);

  useEffect(() => {
    if (!mentionTrigger || !selectedHostId) {
      mentionReqSeqRef.current += 1;
      setRankedMentions(EMPTY_MENTIONS);
      return;
    }
    const reqId = mentionReqSeqRef.current + 1;
    mentionReqSeqRef.current = reqId;
    const hostId = selectedHostId;
    const cwd = selectedCwd || "~";
    const query = mentionTrigger.query;
    void orbitIpc
      .rankMentionCandidates({
        host_id: hostId,
        cwd,
        query,
        limit: 12,
        workspace_id: activeWorkspaceId,
        active_agent_id: isDraft ? null : selectedAgentId,
      })
      .then((items) => {
        if (mentionReqSeqRef.current === reqId) {
          setRankedMentions(items);
        }
      })
      .catch(() => {
        if (mentionReqSeqRef.current === reqId) {
          setRankedMentions(EMPTY_MENTIONS);
        }
      });
  }, [
    mentionTrigger,
    selectedHostId,
    selectedCwd,
    workspaceFiles,
    gitFiles,
    activeWorkspaceId,
    isDraft,
    selectedAgentId,
  ]);

  const hasSubstantiveInput =
    draftText.trim().length > 0 ||
    contexts.length > 0 ||
    pastes.length > 0 ||
    quotes.length > 0;
  const hasPayload =
    hasSubstantiveInput || slashDirective === "review" || slashDirective === "compact";

  const handleSelectMode = useCallback(
    (m: SessionMode) => {
      if (isDraft || !hasCurrentAgent || !selectedHostId || !selectedAgentId) {
        orbitActions.setDraftConfig({ mode: m });
      } else {
        void orbitIpc.setSessionMode(selectedHostId, selectedAgentId, m);
      }
    },
    [isDraft, hasCurrentAgent, selectedHostId, selectedAgentId],
  );

  const executeSlashCommand = (cmd: SlashCommandView) => {
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
      case "btw":
        orbitActions.openSideQuestionTab();
        return;
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
        orbitActions.toggleReviewTab();
        break;
      case "files":
        orbitActions.openFilesExplorerTab();
        return;
      case "browser":
        orbitActions.openBrowserTab();
        return;
      case "compact":
        if (!isDraft && hasCurrentAgent) {
          orbitActions.setSlashDirective("compact");
        } else {
          orbitActions.showToast("Start an active session first to compact context");
        }
        break;
      case "orbit":
      case "orbit-handoff":
      case "orbit-committee":
      case "orbit-advisor":
        orbitActions.setSlashDirective(cmd.id);
        break;
      case "terminal":
        orbitActions.toggleTerminalTab();
        return;
      case "new":
        orbitActions.startNewThreadDraft();
        break;
      case "settings":
        orbitActions.openSettings();
        return;
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

  const executeMentionSelect = (candidate: MentionCandidateView) => {
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
      const filePath = candidate.file_path || candidate.title;
      const slashIdx = filePath.lastIndexOf("/");
      const basename = slashIdx >= 0 ? filePath.slice(slashIdx + 1) : filePath;
      const token = candidate.token_label || `@${basename}`;
      orbitActions.registerInlineMention({
        token,
        kind: "file",
        path: filePath,
      });
      insertInlineToken(token);
    } else if (candidate.kind === "git_diff") {
      const staged = candidate.key.includes("staged");
      const token = candidate.token_label || (staged ? "@staged-diff" : "@working-diff");
      orbitActions.registerInlineMention({
        token,
        kind: "git_diff",
        staged,
        label: candidate.title,
      });
      insertInlineToken(token);
    } else if (candidate.kind === "workspace_peer") {
      const token = candidate.token_label || candidate.title;
      insertInlineToken(token);
    } else if (candidate.kind === "terminal_output") {
      const snap = orbitActions.captureActiveTerminalTail();
      if (snap) {
        const token = candidate.token_label || "@terminal";
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

  const triggerAtMentionFromButton = useCallback(() => {
    if (selectedHostId && selectedCwd && selectedCwd !== "~") {
      void orbitActions.ensureWorkspaceFiles(selectedHostId, selectedCwd);
    }
    const curDraft = draftTextRef.current;
    const el = textareaRef.current;
    const pos = el?.selectionStart ?? curDraft.length;
    const before = curDraft.slice(0, pos);
    const after = curDraft.slice(pos);
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
  }, [selectedHostId, selectedCwd]);

  const triggerSlashActionsFromButton = useCallback(() => {
    const curDraft = draftTextRef.current;
    const el = textareaRef.current;
    const pos = el?.selectionStart ?? curDraft.length;
    const before = curDraft.slice(0, pos);
    const after = curDraft.slice(pos);
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
  }, []);

  const insertPastedTextAtCursor = useCallback((text: string) => {
    if (!text) return;
    const lineCount = text.split("\n").length;
    if (lineCount >= 60 || text.length >= 4800) {
      orbitActions.addPastedAttachment(text);
      return;
    }
    const cur = draftTextRef.current;
    const el = textareaRef.current;
    const start = el?.selectionStart ?? cur.length;
    const end = el?.selectionEnd ?? start;
    const next = `${cur.slice(0, start)}${text}${cur.slice(end)}`;
    const nextPos = start + text.length;
    orbitActions.setComposerDraft(next);
    setCursorOffset(nextPos);
    requestAnimationFrame(() => {
      if (textareaRef.current) {
        textareaRef.current.focus();
        textareaRef.current.setSelectionRange(nextPos, nextPos);
      }
    });
  }, []);

  const tryAttachNativeClipboardImages = useCallback(async (): Promise<boolean> => {
    if (!isTauriRuntime() || isMacDesktop()) return false;
    if (nativeClipboardInFlightRef.current) return false;
    nativeClipboardInFlightRef.current = true;
    try {
      const payloads = await orbitIpc.readClipboardImages();
      if (payloads.length === 0) return false;
      for (const payload of payloads) {
        void orbitActions.stageAndAttachFile(clipboardPayloadToFile(payload));
      }
      return true;
    } catch {
      return false;
    } finally {
      nativeClipboardInFlightRef.current = false;
    }
  }, []);

  const handlePaste = (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    pasteHandledForCtrlVRef.current = true;
    const items = Array.from(e.clipboardData.items ?? []);
    const imageFiles: File[] = [];
    for (const it of items) {
      if (it.kind === "file" && it.type.startsWith("image/")) {
        const f = it.getAsFile();
        if (f) imageFiles.push(f);
      }
    }
    if (imageFiles.length === 0) {
      for (const f of Array.from(e.clipboardData.files ?? [])) {
        if (f.type.startsWith("image/")) {
          imageFiles.push(f);
        }
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
    // On Linux WebKitGTK (CVE-2025-13947 / DataTransfer::allowsFileAccess() == false),
    // `e.clipboardData.items` & `.files` drop pasted images completely. Fall back to Tauri's
    // native GTK clipboard reader when pasting raw images, browser "Copy Image", or file-manager
    // copied image URIs.
    if (isTauriRuntime() && !isMacDesktop()) {
      const uriList = e.clipboardData.getData("text/uri-list");
      const html = e.clipboardData.getData("text/html");
      const likelyImageCopy =
        !text ||
        uriListContainsImageFile(uriList) ||
        isStandaloneHtmlImageCopy(html);
      if (likelyImageCopy) {
        e.preventDefault();
        void tryAttachNativeClipboardImages().then((attached) => {
          if (!attached && text) {
            insertPastedTextAtCursor(text);
          }
        });
        return;
      }
    }

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
    if (ime.shouldIgnoreKeyDown(e)) return;

    // Fallback for Linux WebKitGTK builds where `Ctrl+V` into a plain-text `<textarea>`
    // with an image-only clipboard suppresses the DOM `paste` event completely.
    if (
      isTauriRuntime() &&
      !isMacDesktop() &&
      e.ctrlKey &&
      !e.metaKey &&
      !e.altKey &&
      !e.shiftKey &&
      e.key.toLowerCase() === "v"
    ) {
      pasteHandledForCtrlVRef.current = false;
      window.setTimeout(() => {
        if (!pasteHandledForCtrlVRef.current) {
          void tryAttachNativeClipboardImages();
        }
      }, 40);
    }

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
            (c.id === "plan" ||
              c.id === "review" ||
              c.id === "compact" ||
              c.id === "orbit" ||
              c.id === "orbit-handoff" ||
              c.id === "orbit-committee" ||
              c.id === "orbit-advisor") &&
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

    if (mentionTrigger && rankedMentions.length > 0) {
      const len = rankedMentions.length;
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
        executeMentionSelect(rankedMentions[safeIdx]);
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

    if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey) {
      e.preventDefault();
      setHistoryCursor(null);
      void orbitActions.submitComposer(isTurnBusy);
      return;
    }

    if (enterToSend) {
      if (e.key === "Enter" && !e.shiftKey && !e.metaKey && !e.ctrlKey && !e.altKey) {
        e.preventDefault();
        setHistoryCursor(null);
        void orbitActions.submitComposer(false);
        return;
      }
    }
  };

  if (!selectedHostId) return null;

  return (
    <div
      className={cn(
        "w-full",
        variant === "docked" ? "shrink-0 px-[24px] pt-0 pb-4" : "px-0",
      )}
    >
      <div className="mx-auto flex w-full max-w-[var(--orbit-stage-max-w,768px)] flex-col gap-2">
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
          <div className="approval-dock-surface orbit-stage-enter space-y-0.5 rounded-[14px] px-2 py-1.5 text-[12.5px]">
            {queued.map((qItem, idx) => {
              const canDrag = queued.length > 1;
              const isDragged = queueDrag !== null && queueDrag.fromIndex === idx;
              let translateY = 0;
              if (queueDrag) {
                if (isDragged) {
                  translateY = queueDrag.deltaY;
                } else if (
                  queueDrag.fromIndex < queueDrag.overIndex &&
                  idx > queueDrag.fromIndex &&
                  idx <= queueDrag.overIndex
                ) {
                  translateY = -queueDrag.rowStep;
                } else if (
                  queueDrag.fromIndex > queueDrag.overIndex &&
                  idx >= queueDrag.overIndex &&
                  idx < queueDrag.fromIndex
                ) {
                  translateY = queueDrag.rowStep;
                }
              }

              return (
                <div
                  key={qItem.id}
                  ref={(el) => {
                    queueRowRefs.current[idx] = el;
                  }}
                  onPointerDown={canDrag ? (e) => handleQueueRowPointerDown(e, idx) : undefined}
                  style={
                    queueDrag
                      ? { transform: `translate3d(0, ${translateY}px, 0)` }
                      : undefined
                  }
                  className={cn(
                    "group/qrow flex h-[28px] items-center justify-between gap-2 rounded-[8px] px-2 select-none",
                    isDragged
                      ? "bg-elevated/95 ring-border/75 relative z-10 cursor-grabbing shadow-[0_3px_10px_rgba(0,0,0,0.08)] ring-1 dark:shadow-[0_4px_14px_rgba(0,0,0,0.34)]"
                      : cn(
                          queueDrag
                            ? "transition-transform duration-150 ease-out"
                            : "transition-colors hover:bg-black/[0.025] dark:hover:bg-white/[0.035]",
                          canDrag && !queueDrag && "cursor-grab",
                        ),
                  )}
                >
                  <div className="flex min-w-0 flex-1 items-center gap-2">
                    {canDrag ? (
                      <>
                        <CornerDownRight
                          className={cn(
                            "text-muted-foreground/60 size-3.5 shrink-0 stroke-[1.8]",
                            isDragged ? "hidden" : "group-hover/qrow:hidden",
                          )}
                        />
                        <GripVertical
                          className={cn(
                            "text-muted-foreground/75 size-3.5 shrink-0 stroke-[1.8]",
                            isDragged ? "block" : "hidden group-hover/qrow:block",
                          )}
                        />
                      </>
                    ) : (
                      <CornerDownRight className="text-muted-foreground/60 size-3.5 shrink-0 stroke-[1.8]" />
                    )}
                    {qItem.badges.map((b, bIdx) => (
                      <span
                        key={`${b.kind}-${bIdx}`}
                        className="bg-muted text-foreground/80 shrink-0 rounded px-1.5 py-0.5 font-mono text-[11px]"
                      >
                        {b.label}
                      </span>
                    ))}
                    {qItem.displayText && (
                      <span className="text-foreground/86 truncate">{qItem.displayText}</span>
                    )}
                  </div>
                  <div
                    className={cn(
                      "flex shrink-0 items-center gap-0.5",
                      queueDrag && "pointer-events-none opacity-45",
                    )}
                  >
                    <SimpleTooltip label="Send now (interrupts current turn)">
                      <button
                        type="button"
                        onClick={() => void orbitActions.flushQueuedPromptImmediately(idx, qItem.id)}
                        className="text-foreground/82 hover:text-foreground inline-flex h-[22px] cursor-pointer items-center rounded-[6px] px-2 text-[11.5px] font-medium transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
                      >
                        Send
                      </button>
                    </SimpleTooltip>
                    <button
                      type="button"
                      aria-label="Edit queued message"
                      onClick={() => orbitActions.popQueuedToDraft(idx, qItem.id)}
                      className="text-muted-foreground hover:text-foreground inline-flex h-[22px] cursor-pointer items-center rounded-[6px] px-2 text-[11.5px] font-medium transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
                    >
                      Edit
                    </button>
                    <SimpleTooltip label="Remove">
                      <button
                        type="button"
                        aria-label="Remove queued message"
                        onClick={() => orbitActions.removeQueuedPrompt(idx, qItem.id)}
                        className="text-muted-foreground hover:text-foreground inline-flex size-[22px] cursor-pointer items-center justify-center rounded-[6px] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
                      >
                        <X className="size-3.5 stroke-[1.8]" />
                      </button>
                    </SimpleTooltip>
                  </div>
                </div>
              );
            })}
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

          {mentionTrigger && rankedMentions.length > 0 && (
            <MentionMenu
              items={rankedMentions}
              activeIndex={Math.min(menuHighlightIdx, rankedMentions.length - 1)}
              onHoverIndex={setMenuHighlightIdx}
              onSelect={executeMentionSelect}
            />
          )}

          {variant === "hero" && <ComposerHeroHeader />}

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
            <ContextTray contexts={contexts} quotes={quotes} pastes={pastes} />

            {/* Auto-growing Prose Textarea + Off-Flow Mirror Sizer + Inline Slash Token + Placeholder */}
            <div className="relative">
              {slashDirective && (
                <span
                  ref={directivePrefixRef}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    textareaRef.current?.focus();
                  }}
                  className="orbit-pill-enter pointer-events-auto absolute top-[13px] left-[14px] z-10 inline-flex h-[20px] cursor-text items-center gap-[5.5px] select-none"
                >
                  {slashDirective === "plan" ? (
                    <ListTodo className="text-muted-foreground size-[15px] shrink-0 stroke-[1.75]" />
                  ) : slashDirective === "review" ? (
                    <FileSearch className="text-muted-foreground size-[15px] shrink-0 stroke-[1.75]" />
                  ) : slashDirective === "orbit" ? (
                    <Layers className="text-muted-foreground size-[15px] shrink-0 stroke-[1.75]" />
                  ) : slashDirective === "orbit-handoff" ? (
                    <Send className="text-muted-foreground size-[15px] shrink-0 stroke-[1.75]" />
                  ) : slashDirective === "orbit-committee" ? (
                    <Users className="text-muted-foreground size-[15px] shrink-0 stroke-[1.75]" />
                  ) : slashDirective === "orbit-advisor" ? (
                    <Sparkles className="text-muted-foreground size-[15px] shrink-0 stroke-[1.75]" />
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
                  className="text-foreground/45 pointer-events-none absolute top-[13px] right-[14px] left-[14px] truncate text-[15px] leading-[20px] select-none"
                >
                  Ask anything, @ to mention, / for actions
                </div>
              )}

              {/* Off-flow div mirror sizer: deterministically measures wrapped height on both typing and large pastes */}
              <div
                ref={mirrorTextareaRef}
                aria-hidden="true"
                style={
                  slashDirective && directiveIndentPx > 0
                    ? { textIndent: `${directiveIndentPx}px` }
                    : undefined
                }
                className="pointer-events-none invisible absolute top-0 right-0 left-0 -z-10 block w-full px-[14px] py-[13px] text-[15px] leading-[20px] whitespace-pre-wrap break-words"
              >
                {draftText}
                {"\u200b"}
              </div>

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
                  if (val.includes("/") || val.includes("@")) {
                    setCursorOffset(pos);
                  }
                  orbitActions.setComposerDraft(val);
                  if (val.includes("@") && selectedCwd && selectedCwd !== "~") {
                    void orbitActions.ensureWorkspaceFiles(selectedHostId, selectedCwd);
                  }
                }}
                onSelect={(e) => {
                  const val = e.currentTarget.value;
                  if (val.includes("/") || val.includes("@")) {
                    setCursorOffset(e.currentTarget.selectionStart ?? 0);
                  }
                }}
                onClick={(e) => {
                  const val = e.currentTarget.value;
                  if (val.includes("/") || val.includes("@")) {
                    setCursorOffset(e.currentTarget.selectionStart ?? 0);
                  }
                }}
                onPaste={handlePaste}
                {...ime.compositionProps}
                onKeyDown={handleKeyDown}
                rows={1}
                className="text-foreground block min-h-[46px] w-full resize-none overflow-y-hidden bg-transparent px-[14px] py-[13px] text-[15px] leading-[20px] outline-none"
              />
            </div>

            {/* Bottom Control Bar (`34px` uniform control rhythm) */}
            <div className="flex items-center justify-between gap-2 px-2.5 pt-0 pb-2">
              <div className="flex min-w-0 flex-1 items-center gap-0.5 overflow-hidden">
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
                    finalFocus={false}
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

                {/* 2. Isolated Agent + Model + Thinking Mode Selector */}
                <ComposerModelPicker />
              </div>

              {/* Right Cluster: Isolated Access Permissions Selector + `34×34` Primary Action Circle */}
              <div className="flex shrink-0 items-center gap-1.5">
                <ComposerModePicker
                  mode={activeMode}
                  onSelectMode={handleSelectMode}
                  iconOnly={composerWidthPx > 0 && composerWidthPx < 440}
                />

                {isTurnBusy && !hasPayload ? (
                  <button
                    type="button"
                    title="Stop generation"
                    onClick={() => {
                      if (selectedAgentId) {
                        void orbitIpc.interruptAgent(selectedHostId, selectedAgentId);
                      }
                    }}
                    className="orbit-press inline-flex size-8.5 cursor-pointer items-center justify-center rounded-full bg-black/[0.06] hover:bg-black/[0.10] dark:bg-white/[0.09] dark:hover:bg-white/[0.14]"
                  >
                    <span className="size-[11px] rounded-[2.5px] bg-[#EF4444]" />
                  </button>
                ) : (
                  <button
                    type="button"
                    disabled={!hasPayload}
                    title={isTurnBusy ? `Queue follow-up (${modSymbol()}↵ to send now)` : "Send"}
                    onClick={() => void orbitActions.submitComposer(false)}
                    className={cn(
                      "orbit-press inline-flex size-8.5 cursor-pointer items-center justify-center rounded-full",
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
});
