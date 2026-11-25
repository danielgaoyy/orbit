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
  Mic,
  Paperclip,
  Plus,
  SquareSlash,
  X,
} from "lucide-react";
import {
  collapseStrippedToken,
  modeLabel,
  normalizePermissionMode,
} from "@/components/composer/composer-constants";
import { ComposerDirectiveToken } from "@/components/composer/composer-directive-token";
import { isComposerInlineDirective, resolveComposerSubmission } from "@/lib/slash-commands";
import { ComposerHeroHeader } from "@/components/composer/composer-hero-header";
import { ComposerModePicker } from "@/components/composer/composer-mode-picker";
import { ComposerModelPicker } from "@/components/composer/composer-model-picker";
import { ComposerQueueStrip } from "@/components/composer/composer-queue-strip";
import { ContextOccupancyPill } from "@/components/composer/context-occupancy-pill";
import { ContextTray } from "@/components/composer/context-tray";
import {
  MENTION_MENU_LISTBOX_ID,
  MentionMenu,
  mentionOptionId,
} from "@/components/composer/mention-menu";
import {
  SLASH_MENU_LISTBOX_ID,
  SlashCommandMenu,
  filterSlashCommands,
  isSlashCommandSessionRequired,
  slashOptionId,
} from "@/components/composer/slash-menu";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import { detectActiveTriggerKind } from "@/lib/composer-input";
import { translate, useI18n, useLocale } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import { modSymbol } from "@/lib/keybindings";
import {
  getCatalogForProvider,
  modelSupportsFastMode,
  resolveActiveModelSpec,
} from "@/lib/model-catalog";
import {
  type ClipboardImagePayload,
  type ManagedSkillRecord,
  type MentionCandidateView,
  type ProviderKind,
  type ProviderModelCatalog,
  type SessionMode,
  type SlashCommandView,
  type VoiceTranscriptionResult,
  isAgentForegroundRunning,
  isMacDesktop,
  isTauriRuntime,
  orbitIpc,
  parseVoiceRpcError,
} from "@/lib/orbit-ipc";
import {
  VoiceRecorderSession,
  captureBrowserSpeechFallback,
  speakVoiceText,
} from "@/lib/voice-recorder";
import {
  buildFolderTreeSummary,
  findInlineMentionSpans,
  isValidHttpsReferenceUrl,
  resolveAtomicMentionBackspace,
  resolveAtomicMentionDelete,
  shortWebUrlLabel,
} from "@/lib/inline-mentions";
import {
  containsInlineMentionToken,
  mergeMentionCandidates,
  parseSymbolMentionQuery,
  resolveDisambiguatedSymbolToken,
  workspaceSymbolToMentionCandidate,
} from "@/lib/symbol-mentions";
import {
  type ActiveTurnSendBehavior,
  type ContextMentionItem,
  type DiffQuoteSnippet,
  type InlineMentionTarget,
  type PastedAttachment,
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
const EMPTY_MENTIONS: MentionCandidateView[] = [];
const EMPTY_MANAGED_SKILLS: readonly ManagedSkillRecord[] = [];
const EMPTY_INLINE_MENTIONS: readonly InlineMentionTarget[] = [];
const EMPTY_CATALOGS: readonly ProviderModelCatalog[] = [];

const BASE_PAD_LEFT = 14;
const TOKEN_PAD_X = 6;
const TOKEN_GAP = 4;

const IMAGE_URI_EXT_RE = /\.(png|jpe?g|webp|gif|bmp|svg|avif|ico)(?:[?#].*)?$/i;

export function resolveComposerEnterAction(
  e: Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "shiftKey" | "altKey">,
  enterToSend: boolean,
  isTurnBusy: boolean,
  activeTurnSendBehavior: ActiveTurnSendBehavior = "queue",
): { submit: boolean; steerImmediately: boolean } {
  if (e.key !== "Enter" || e.altKey) {
    return { submit: false, steerImmediately: false };
  }
  const hasMod = e.metaKey || e.ctrlKey;
  const steerOnPrimary = isTurnBusy && activeTurnSendBehavior === "steer";
  const steerOnAlternate = isTurnBusy && activeTurnSendBehavior !== "steer";
  if (enterToSend) {
    if (hasMod && !e.shiftKey) {
      return { submit: true, steerImmediately: steerOnAlternate };
    }
    if (!hasMod && !e.shiftKey) {
      return { submit: true, steerImmediately: steerOnPrimary };
    }
    return { submit: false, steerImmediately: false };
  }
  // enterToSend === false: Mod+Enter is the primary shortcut; Mod+Shift+Enter triggers the opposite active-turn mode.
  if (hasMod && e.shiftKey) {
    return { submit: true, steerImmediately: steerOnAlternate };
  }
  if (hasMod && !e.shiftKey) {
    return { submit: true, steerImmediately: steerOnPrimary };
  }
  return { submit: false, steerImmediately: false };
}

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

function formatVoiceDuration(ms: number): string {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const mins = Math.floor(totalSeconds / 60);
  const secs = totalSeconds % 60;
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

function focusVisualPermissionCard(requestId?: string | null): void {
  if (typeof document === "undefined") return;
  const selector = requestId
    ? `[data-permission-request-id="${CSS.escape(requestId)}"], .approval-dock-surface`
    : ".approval-dock-surface";
  const el = document.querySelector<HTMLElement>(selector);
  if (!el) return;
  el.scrollIntoView({ behavior: "smooth", block: "nearest" });
  if (typeof el.focus === "function") {
    el.focus({ preventScroll: true });
  }
  el.animate?.(
    [
      { boxShadow: "0 0 0 0 rgba(245, 183, 73, 0)" },
      { boxShadow: "0 0 0 3px rgba(245, 183, 73, 0.55)" },
      { boxShadow: "0 0 0 0 rgba(245, 183, 73, 0)" },
    ],
    { duration: 900, iterations: 1 },
  );
}

function resolveVoiceModeAlias(query: string): SessionMode | null {
  const q = query.trim().toLowerCase();
  if (q === "plan" || q === "planning" || q === "计划" || q === "プラン") return "plan";
  if (q === "build" || q === "auto" || q === "agent" || q === "构建" || q === "ビルド") return "auto";
  if (q === "default" || q === "ask" || q === "默认" || q === "询问") return "default";
  if (q === "accept edits" || q === "accept_edits" || q === "edits") return "accept_edits";
  if (q === "full access" || q === "full_access" || q === "全权限") return "full_access";
  return null;
}

export interface OmniComposerProps {
  variant?: "hero" | "docked";
  occluding?: boolean;
  hasPendingDecisions?: boolean;
}

export const OmniComposer = memo(function OmniComposer({
  variant = "docked",
  occluding = false,
  hasPendingDecisions = false,
}: OmniComposerProps) {
  const locale = useLocale();
  const t = useMemo(() => translate.bind(null, locale), [locale]);
  const mod = modSymbol();
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const activePathRulesCount = useOrbitSelector((s) =>
    s.selectedHostId
      ? (s.pathRulesByScope[`${s.selectedHostId}::${s.selectedWorkspaceCwd || "~"}`]?.rules.length ??
        0)
      : 0,
  );
  const isDraft = useOrbitSelector((s) => s.isDraftNewSession);
  const draftMode = useOrbitSelector((s) => s.draftMode);
  const draftProvider = useOrbitSelector((s) => s.draftProvider);
  const draftModels = useOrbitSelector((s) => s.draftModels);
  const draftFastMode = useOrbitSelector((s) => s.draftFastMode);
  const providerCatalogs = useOrbitSelector((s) =>
    s.selectedHostId
      ? (s.hostDetails[s.selectedHostId]?.provider_catalogs ?? EMPTY_CATALOGS)
      : EMPTY_CATALOGS,
  );
  const enterToSend = useOrbitSelector((s) => s.enterToSend);
  const activeTurnSendBehavior = useOrbitSelector((s) => s.activeTurnSendBehavior);
  const promptHistory = useOrbitSelector((s) => s.promptHistory);
  const slashCommands = useOrbitSelector((s) => s.slashCommands);
  const managedSkills = useOrbitSelector((s) =>
    s.selectedHostId
      ? (s.hostDetails[s.selectedHostId]?.managed_skills ?? EMPTY_MANAGED_SKILLS)
      : EMPTY_MANAGED_SKILLS,
  );
  const ime = useImeCompositionGuard();

  // Keep host/project managed skills hydrated for slash command discovery
  useEffect(() => {
    if (!selectedHostId) return;
    const scopedCwd = selectedCwd && selectedCwd !== "~" ? selectedCwd : null;
    void orbitIpc.listManagedSkills(selectedHostId, scopedCwd).catch(() => {});
  }, [selectedHostId, selectedCwd]);

  const effectiveSlashCommands = useMemo<SlashCommandView[]>(() => {
    const disabledSlugs = new Set(
      managedSkills.filter((s) => !s.enabled).map((s) => s.slug.toLowerCase()),
    );
    const visibleBuiltins =
      disabledSlugs.size > 0
        ? slashCommands.filter((c) => !disabledSlugs.has(c.id.toLowerCase()))
        : slashCommands;
    const existingCommands = new Set(visibleBuiltins.map((c) => c.command.toLowerCase()));
    const skillEntries: SlashCommandView[] = [];
    for (const skill of managedSkills) {
      if (!skill.enabled) continue;
      const cmd = `/${skill.slug}`;
      if (existingCommands.has(cmd.toLowerCase())) continue;
      existingCommands.add(cmd.toLowerCase());
      if (skill.is_command) {
        skillEntries.push({
          id: `cmd:${skill.id}`,
          command: cmd,
          label: skill.name,
          description: skill.description || `Run ${cmd} command`,
          shortcut_key: null,
          icon_key: "command",
          keywords: ["command", "claude", skill.slug, skill.name, ...skill.triggers],
          desktop_only: false,
          argument_hint: skill.argument_hint ?? null,
          skill_id: skill.id,
        });
      } else {
        skillEntries.push({
          id: `skill:${skill.slug}`,
          command: cmd,
          label: skill.name,
          description: skill.description || `Invoke ${skill.name} skill`,
          shortcut_key: null,
          icon_key: "skill",
          keywords: ["skills", "skill", skill.slug, skill.name, ...skill.triggers],
          desktop_only: false,
        });
      }
    }
    return skillEntries.length > 0 ? [...visibleBuiltins, ...skillEntries] : visibleBuiltins;
  }, [slashCommands, managedSkills]);

  // Single primitive-serialized selector replaces repeated `.agents.find(...)` scans on host heartbeats
  const activeAgentMetaKey = useOrbitSelector((s): string => {
    if (s.isDraftNewSession || !s.selectedHostId || !s.selectedAgentId) return "";
    const ag = s.hostDetails[s.selectedHostId]?.agents.find(
      (a) => a.agent_id === s.selectedAgentId,
    );
    if (!ag) return "";
    const modeStr = typeof ag.mode === "string" ? ag.mode : `custom:${ag.mode.custom}`;
    return `1\0${isAgentForegroundRunning(ag) ? "1" : "0"}\0${ag.is_awaiting_permission ? "1" : "0"}\0${modeStr}\0${ag.workspace_id ?? ""}\0${ag.provider}\0${ag.model ?? ""}\0${ag.fast_mode ? "1" : "0"}`;
  });

  const {
    hasCurrentAgent,
    isForegroundRunning,
    isTurnBusy,
    activeAgentMode,
    activeWorkspaceId,
    activeAgentProvider,
    activeAgentModel,
    activeAgentFastMode,
  } = useMemo(() => {
    if (!activeAgentMetaKey) {
      return {
        hasCurrentAgent: false,
        isForegroundRunning: false,
        isTurnBusy: false,
        activeAgentMode: null as SessionMode | null,
        activeWorkspaceId: null as string | null,
        activeAgentProvider: null as ProviderKind | null,
        activeAgentModel: null as string | null,
        activeAgentFastMode: false,
      };
    }
    const [, runningFlag, permFlag, modeRaw, wsId, prov, modelRaw, fastFlag] =
      activeAgentMetaKey.split("\0");
    const parsedMode: SessionMode | null = !modeRaw
      ? null
      : modeRaw.startsWith("custom:")
        ? { custom: modeRaw.slice("custom:".length) }
        : (modeRaw as SessionMode);
    return {
      hasCurrentAgent: true,
      isForegroundRunning: runningFlag === "1",
      isTurnBusy: runningFlag === "1" || permFlag === "1",
      activeAgentMode: parsedMode,
      activeWorkspaceId: wsId || null,
      activeAgentProvider: (prov || null) as ProviderKind | null,
      activeAgentModel: modelRaw || null,
      activeAgentFastMode: fastFlag === "1",
    };
  }, [activeAgentMetaKey]);

  const effectiveProvider: ProviderKind =
    !isDraft && activeAgentProvider ? activeAgentProvider : draftProvider;
  const effectiveModelRaw: string | null =
    !isDraft && activeAgentProvider ? activeAgentModel : (draftModels[effectiveProvider] ?? null);
  const { fastModeSupported, fastModeActive } = useMemo(() => {
    const cat = getCatalogForProvider(
      providerCatalogs as ProviderModelCatalog[],
      effectiveProvider,
    );
    const spec = resolveActiveModelSpec(cat, effectiveModelRaw);
    const supported = modelSupportsFastMode(effectiveProvider, spec);
    const active = Boolean(
      supported && (!isDraft && hasCurrentAgent ? activeAgentFastMode : draftFastMode),
    );
    return { fastModeSupported: supported, fastModeActive: active };
  }, [
    providerCatalogs,
    effectiveProvider,
    effectiveModelRaw,
    isDraft,
    hasCurrentAgent,
    activeAgentFastMode,
    draftFastMode,
  ]);

  const scopeKey = useOrbitSelector(composerScopeKey);
  const draftText = useComposerDraft(scopeKey);
  const slashDirective = useOrbitSelector(
    (s) => s.slashDirectives[composerScopeKey(s)] ?? null,
  );
  const contexts = useOrbitSelector(
    (s) => s.contextItems[composerScopeKey(s)] ?? EMPTY_CONTEXTS,
  );
  const inlineMentions = useOrbitSelector(
    (s) => s.inlineMentions[composerScopeKey(s)] ?? EMPTY_INLINE_MENTIONS,
  );
  const pastes = useOrbitSelector(
    (s) => s.pastedAttachments[composerScopeKey(s)] ?? EMPTY_PASTES,
  );
  const quotes = useOrbitSelector((s) => s.diffQuotes[composerScopeKey(s)] ?? EMPTY_QUOTES);

  // Primitive counts (never array refs) so mention re-ranking only runs when counts actually change
  const workspaceFilesCount = useOrbitSelector((s) =>
    s.selectedHostId && s.selectedWorkspaceCwd
      ? (s.workspaceFilesCache[`${s.selectedHostId}::${s.selectedWorkspaceCwd}`]?.length ?? 0)
      : 0,
  );
  const gitFilesCount = useOrbitSelector((s) =>
    s.selectedHostId && s.selectedWorkspaceCwd
      ? (s.gitReviews[`${s.selectedHostId}::${s.selectedWorkspaceCwd}`]?.status?.files.length ?? 0)
      : 0,
  );

  const scrollContainerRef = useRef<HTMLDivElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const mirrorTextareaRef = useRef<HTMLDivElement | null>(null);
  const lastMeasuredHeightRef = useRef<number>(40);
  const [composerWidthPx, setComposerWidthPx] = useState(0);
  const [isCompactBar, setIsCompactBar] = useState(false);
  const draftTextRef = useRef<string>(draftText);
  draftTextRef.current = draftText;
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const savedDraftBeforeHistoryRef = useRef<string>("");
  const pasteHandledForCtrlVRef = useRef<boolean>(false);
  const nativeClipboardInFlightRef = useRef<boolean>(false);

  const [cursor, setCursor] = useState({ text: "", offset: 0 });
  const setCursorOffset = useCallback((offset: number, text: string) => {
    setCursor((prev) =>
      prev.offset === offset && prev.text === text ? prev : { text, offset },
    );
  }, []);
  const tokenOverlayRef = useRef<HTMLDivElement>(null);
  const [tokenIndentPx, setTokenIndentPx] = useState(0);
  const indent = slashDirective ? tokenIndentPx : 0;
  const hasDraftText = draftText.length > 0;
  const inputInset = useMemo(() => ({
    paddingLeft: BASE_PAD_LEFT + (hasDraftText ? 0 : indent),
    textIndent: hasDraftText ? indent : 0,
  }), [hasDraftText, indent]);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [menuHighlightIdx, setMenuHighlightIdx] = useState<number>(0);
  const [dismissedToken, setDismissedToken] = useState<string | null>(null);
  const [isDraggingOver, setIsDraggingOver] = useState<boolean>(false);
  const [historyCursor, setHistoryCursor] = useState<number | null>(null);
  const [rankedMentions, setRankedMentions] = useState<MentionCandidateView[]>(EMPTY_MENTIONS);
  const [symbolMentions, setSymbolMentions] = useState<MentionCandidateView[]>(EMPTY_MENTIONS);
  const [isSearchingSymbols, setIsSearchingSymbols] = useState<boolean>(false);
  const mentionReqSeqRef = useRef<number>(0);
  const symbolReqSeqRef = useRef<number>(0);

  useLayoutEffect(() => {
    const overlay = tokenOverlayRef.current;
    if (!overlay) return;
    const measure = () =>
      setTokenIndentPx(Math.max(0, overlay.getBoundingClientRect().width - TOKEN_PAD_X + TOKEN_GAP));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(overlay);
    return () => observer.disconnect();
  }, [slashDirective, selectedHostId]);

  const activeMode: SessionMode = normalizePermissionMode(
    !isDraft && activeAgentMode ? activeAgentMode : draftMode,
  );

  // Warm path-scoped rules lazily.
  useEffect(() => {
    if (!selectedHostId) return;
    const effectiveCwd = selectedCwd || "~";
    const timer = window.setTimeout(() => {
      void orbitActions.refreshPathRules(selectedHostId, effectiveCwd);
    }, 180);
    return () => window.clearTimeout(timer);
  }, [selectedHostId, selectedCwd]);

  const minTextareaHeight = 40;
  const maxTextareaHeight = 200;

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => {
      const width = entry.borderBoxSize[0]?.inlineSize ?? entry.contentRect.width;
      setComposerWidthPx(width);
      setIsCompactBar(width < 440);
    });
    ro.observe(el, { box: "border-box" });
    return () => ro.disconnect();
  }, [selectedHostId]);

  // Measure full wrapped textarea height from the off-flow `<div>` mirror BEFORE paint (`useLayoutEffect`).
  // The outer `scrollContainerRef` owns vertical scrolling (`max-h-[200px] overflow-y-auto`) while `<textarea>`
  // stays full-height (`overflow-y: hidden`), so the inline directive token and textarea prose scroll together
  // on a single GPU compositor layer with zero JS scroll lag or trackpad jitter.
  useLayoutEffect(() => {
    const el = textareaRef.current;
    const mirror = mirrorTextareaRef.current;
    if (!el || !mirror) return;

    const prevHeight = lastMeasuredHeightRef.current;
    let nextHeight = minTextareaHeight;
    if (draftText.length === 0) {
      if (mirror.textContent) mirror.textContent = "";
      if (scrollContainerRef.current && scrollContainerRef.current.scrollTop !== 0) {
        scrollContainerRef.current.scrollTop = 0;
      }
    } else {
      mirror.textContent = draftText + "\u200b";
      const measured = Math.max(mirror.offsetHeight, mirror.scrollHeight);
      nextHeight = Math.max(minTextareaHeight, measured);
    }

    const heightCss = `${nextHeight}px`;
    if (el.style.height !== heightCss) {
      el.style.height = heightCss;
    }
    lastMeasuredHeightRef.current = nextHeight;

    // When typing appends a new line past `maxTextareaHeight` at the end of the draft, keep the caret in view.
    const scroller = scrollContainerRef.current;
    if (
      scroller &&
      nextHeight > maxTextareaHeight &&
      nextHeight > prevHeight &&
      document.activeElement === el &&
      el.selectionStart === draftText.length &&
      el.selectionEnd === draftText.length
    ) {
      scroller.scrollTop = scroller.scrollHeight - scroller.clientHeight;
    }
  }, [draftText, scopeKey, composerWidthPx, indent, slashDirective, selectedHostId]);

  useEffect(() => {
    return orbitActions.onComposerFocusRequest((opts) => {
      const el = textareaRef.current;
      if (!el) return;
      const active = document.activeElement;
      if (
        opts?.onlyIfIdle &&
        active instanceof HTMLElement &&
        active !== el &&
        (active.tagName === "INPUT" ||
          active.tagName === "TEXTAREA" ||
          active.tagName === "SELECT" ||
          active.isContentEditable)
      ) {
        return;
      }
      const wasFocused = active === el;
      el.focus({ preventScroll: true });
      if (!wasFocused) {
        const end = el.value.length;
        el.setSelectionRange(end, end);
      }
    });
  }, []);

  const availableSlashCommands = useMemo(() => {
    const hasSession = !isDraft && hasCurrentAgent;
    return effectiveSlashCommands.filter((cmd) => {
      if (!hasSession && isSlashCommandSessionRequired(cmd)) return false;
      if (cmd.id === "fast" && !fastModeSupported && !fastModeActive) return false;
      return true;
    });
  }, [effectiveSlashCommands, isDraft, hasCurrentAgent, fastModeSupported, fastModeActive]);

  const cursorOffset = cursor.text === draftText ? cursor.offset : 0;
  const triggerKind = useMemo(() => detectActiveTriggerKind(draftText, cursorOffset), [draftText, cursorOffset]);
  const slashTrigger = useMemo(() => {
    if (triggerKind !== "slash") return null;
    const slashIndex = draftText.lastIndexOf("/", cursorOffset - 1);
    const query = draftText.slice(slashIndex + 1, cursorOffset);
    const tokenKey = `slash:${slashIndex}:${query}`;
    if (dismissedToken === tokenKey) return null;
    // An explicitly typed /goal still gives the session-required feedback in draft mode.
    const commands = query.toLowerCase() === "goal" ? effectiveSlashCommands : availableSlashCommands;
    const items = filterSlashCommands(commands, query, t);
    if (items.length === 0) return null;
    return { slashIndex, endIndex: cursorOffset, query, tokenKey, items };
  }, [triggerKind, draftText, cursorOffset, dismissedToken, availableSlashCommands, effectiveSlashCommands, t]);

  const mentionTrigger = useMemo(() => {
    if (triggerKind !== "mention") return null;
    const atIndex = draftText.lastIndexOf("@", cursorOffset - 1);
    const query = draftText.slice(atIndex + 1, cursorOffset);
    const tokenKey = `at:${atIndex}:${query}`;
    if (dismissedToken === tokenKey) return null;
    return { atIndex, endIndex: cursorOffset, query, tokenKey };
  }, [triggerKind, draftText, cursorOffset, dismissedToken]);

  const trackTriggerCursor = (el: HTMLTextAreaElement) => {
    const pos = el.selectionStart;
    if (detectActiveTriggerKind(el.value, pos) || slashTrigger || mentionTrigger) {
      setCursorOffset(pos, el.value);
    }
  };

  const hasMentionTrigger = Boolean(mentionTrigger);
  useEffect(() => {
    if (hasMentionTrigger && selectedHostId && selectedCwd && selectedCwd !== "~") {
      void orbitActions.ensureWorkspaceFiles(selectedHostId, selectedCwd);
    }
  }, [hasMentionTrigger, selectedHostId, selectedCwd]);

  const parsedSymbolTrigger = useMemo(
    () => (mentionTrigger ? parseSymbolMentionQuery(mentionTrigger.query) : null),
    [mentionTrigger],
  );

  useEffect(() => {
    if (!mentionTrigger || !selectedHostId || parsedSymbolTrigger?.symbolOnly) {
      mentionReqSeqRef.current += 1;
      setRankedMentions(EMPTY_MENTIONS);
      return;
    }
    const reqId = mentionReqSeqRef.current + 1;
    mentionReqSeqRef.current = reqId;
    const hostId = selectedHostId;
    const cwd = selectedCwd || "~";
    const query = mentionTrigger.query;
    const rank = () => {
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
    };
    const timer = query.length ? window.setTimeout(rank, 35) : undefined;
    if (!query.length) rank();
    return () => {
      window.clearTimeout(timer);
      mentionReqSeqRef.current += 1;
    };
  }, [
    mentionTrigger,
    parsedSymbolTrigger,
    selectedHostId,
    selectedCwd,
    workspaceFilesCount,
    gitFilesCount,
    activePathRulesCount,
    activeWorkspaceId,
    isDraft,
    selectedAgentId,
  ]);

  // Immediately clear any cached symbol results when switching hosts, workspaces, or threads
  useEffect(() => {
    symbolReqSeqRef.current += 1;
    setSymbolMentions(EMPTY_MENTIONS);
    setIsSearchingSymbols(false);
  }, [selectedHostId, selectedCwd, scopeKey]);

  // Debounced (`120ms`) workspace symbol fuzzy search with stale-response & unmount cancellation guard
  useEffect(() => {
    if (
      !mentionTrigger ||
      !parsedSymbolTrigger ||
      !selectedHostId ||
      !selectedCwd ||
      selectedCwd === "~" ||
      (!parsedSymbolTrigger.symbolOnly && parsedSymbolTrigger.query.length === 0)
    ) {
      symbolReqSeqRef.current += 1;
      setSymbolMentions(EMPTY_MENTIONS);
      setIsSearchingSymbols(false);
      return;
    }

    const reqId = symbolReqSeqRef.current + 1;
    symbolReqSeqRef.current = reqId;
    const hostId = selectedHostId;
    const cwd = selectedCwd;
    const query = parsedSymbolTrigger.query;
    const limit = parsedSymbolTrigger.symbolOnly ? 20 : 8;
    setIsSearchingSymbols(true);

    const timer = window.setTimeout(() => {
      void orbitIpc
        .searchWorkspaceSymbols(hostId, cwd, query, limit)
        .then((res) => {
          if (symbolReqSeqRef.current !== reqId) return;
          const mapped = (res.symbols ?? []).map((entry, idx) =>
            workspaceSymbolToMentionCandidate(entry, idx),
          );
          setSymbolMentions(mapped);
          setIsSearchingSymbols(false);
        })
        .catch(() => {
          if (symbolReqSeqRef.current === reqId) {
            setSymbolMentions(EMPTY_MENTIONS);
            setIsSearchingSymbols(false);
          }
        });
    }, 120);

    return () => {
      symbolReqSeqRef.current += 1;
      window.clearTimeout(timer);
    };
  }, [mentionTrigger, parsedSymbolTrigger, selectedHostId, selectedCwd]);

  const combinedMentions = useMemo(() => {
    if (!mentionTrigger) return EMPTY_MENTIONS;
    return mergeMentionCandidates(
      rankedMentions,
      symbolMentions,
      Boolean(parsedSymbolTrigger?.symbolOnly),
      16,
    );
  }, [mentionTrigger, rankedMentions, symbolMentions, parsedSymbolTrigger]);

  const hasUploadingAttachment = useMemo(
    () => contexts.some((c) => (c.kind === "image" || c.kind === "file") && c.uploading),
    [contexts],
  );
  const hasSubstantiveInput =
    draftText.trim().length > 0 ||
    contexts.length > 0 ||
    pastes.length > 0 ||
    quotes.length > 0;
  const localSubmission = resolveComposerSubmission(slashDirective, draftText).kind !== "prompt";
  const hasPayload =
    localSubmission || hasSubstantiveInput || slashDirective === "review" || slashDirective === "compact";

  const triggerSubmit = useCallback(
    (steerImmediately: boolean) => {
      if (!hasPayload) return;
      const reportSubmitRejection = (err: unknown) => {
        const detail = err instanceof Error ? err.message : String(err);
        const trimmed = detail.trim();
        orbitActions.showToast(
          trimmed ? `${t("common.actionFailed")} (${trimmed})` : t("common.actionFailed"),
        );
      };
      if (localSubmission) {
        setHistoryCursor(null);
        void orbitActions.submitComposer().catch(reportSubmitRejection);
        return;
      }
      if (isSubmitting) return;
      setIsSubmitting(true);
      setHistoryCursor(null);
      void orbitActions
        .submitComposer(steerImmediately)
        .catch(reportSubmitRejection)
        .finally(() => {
          setIsSubmitting(false);
        });
    },
    [hasPayload, isSubmitting, localSubmission, t],
  );

  const handleSelectMode = useCallback(
    (m: SessionMode, announceToast = false) => {
      if (isDraft || !hasCurrentAgent || !selectedHostId || !selectedAgentId) {
        orbitActions.setDraftConfig({ mode: m });
        if (announceToast) {
          orbitActions.showToast(t("composer.modeSwitchedToast", { mode: modeLabel(m, t) }));
        }
      } else {
        void orbitIpc
          .setSessionMode(selectedHostId, selectedAgentId, m)
          .then(() => {
            if (announceToast) {
              orbitActions.showToast(t("composer.modeSwitchedToast", { mode: modeLabel(m, t) }));
            }
          })
          .catch((err) => {
            orbitActions.showToast(t("composer.modeSwitchFailed", { error: String(err) }));
          });
      }
    },
    [isDraft, hasCurrentAgent, selectedHostId, selectedAgentId, t],
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
    setCursorOffset(joined.cursor, joined.text);
    setDismissedToken(null);
    setMenuHighlightIdx(0);

    switch (cmd.id) {
      case "fork":
        if (!isDraft && hasCurrentAgent && selectedHostId && selectedAgentId) {
          const nextText = stripped ? `/fork ${stripped}` : "/fork ";
          orbitActions.setComposerDraft(nextText);
          setCursorOffset(nextText.length, nextText);
        } else {
          orbitActions.showToast(t("composer.toastSessionRequiredFork"));
          return;
        }
        break;
      case "goal":
        if (isDraft || !hasCurrentAgent) {
          orbitActions.showToast(t("composer.toastSessionRequiredGoal"));
          break;
        }
        orbitActions.setSlashDirective(cmd.id);
        break;
      case "btw":
      case "plan":
      case "review":
      case "orbit":
      case "orbit-handoff":
      case "orbit-committee":
      case "orbit-advisor":
        orbitActions.setSlashDirective(cmd.id);
        break;
      case "supervised":
        handleSelectMode("default", true);
        break;
      case "auto":
        handleSelectMode("auto", true);
        break;
      case "edits":
        handleSelectMode("accept_edits", true);
        break;
      case "full":
        handleSelectMode("full_access", true);
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
          orbitActions.showToast(t("composer.toastSessionRequiredCompact"));
        }
        break;
      case "export":
        if (!isDraft && hasCurrentAgent && selectedHostId && selectedAgentId) {
          orbitActions.openExportDialog(selectedHostId, selectedAgentId);
          return;
        }
        orbitActions.showToast(t("composer.toastSessionRequiredExport"));
        return;
      case "copy":
        if (!isDraft && hasCurrentAgent && selectedHostId && selectedAgentId) {
          void orbitActions.openCopyCodeBlocksModal(selectedHostId, selectedAgentId);
          return;
        }
        orbitActions.showToast(t("composer.toastSessionRequiredCopy"));
        return;
      case "fast":
        orbitActions.toggleActiveFastMode();
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
      default:
        if (cmd.id.startsWith("cmd:")) {
          const nextText = stripped ? `${cmd.command} ${stripped}` : `${cmd.command} `;
          orbitActions.setComposerDraft(nextText);
          setCursorOffset(nextText.length, nextText);
          requestAnimationFrame(() => {
            const el = textareaRef.current;
            if (el) {
              el.focus();
              el.setSelectionRange(nextText.length, nextText.length);
            }
          });
          return;
        }
        if (cmd.id.startsWith("skill:")) {
          const slug = cmd.id.slice("skill:".length);
          orbitActions.addContextMention({
            id: `skill_${slug}`,
            kind: "skill",
            slug,
            label: cmd.label,
            description: cmd.description,
          });
          break;
        }
        break;
    }

    requestAnimationFrame(() => {
      const el = textareaRef.current;
      if (el) {
        el.focus();
        const pos = isComposerInlineDirective(cmd.id) ? 0 : Math.min(joined.cursor, el.value.length);
        el.setSelectionRange(pos, pos);
      }
    });
  };

  const executeMentionSelect = (candidate: MentionCandidateView) => {
    if (!mentionTrigger) return;
    const beforeAt = draftText.slice(0, mentionTrigger.atIndex);
    const afterToken = draftText.slice(mentionTrigger.endIndex);

    const insertInlineToken = (token: string, appendTrailingSpace = true) => {
      const needsLeadingSpace = beforeAt.length > 0 && !/\s$/.test(beforeAt);
      const inserted = `${needsLeadingSpace ? " " : ""}${token}${appendTrailingSpace ? " " : ""}`;
      const trimmedAfter = appendTrailingSpace ? afterToken.replace(/^[ \t]+/, "") : afterToken;
      const nextDraft = `${beforeAt}${inserted}${trimmedAfter}`;
      const nextPos = beforeAt.length + inserted.length;
      orbitActions.setComposerDraft(nextDraft);
      setCursorOffset(nextPos, nextDraft);
      requestAnimationFrame(() => {
        const el = textareaRef.current;
        if (el) {
          el.focus();
          el.setSelectionRange(nextPos, nextPos);
        }
      });
    };

    if (candidate.kind === "code_symbol") {
      const symbolName = candidate.title;
      const startLine = candidate.start_line ?? 1;
      const token = resolveDisambiguatedSymbolToken(
        {
          symbolName,
          path: candidate.file_path,
          startLine,
          container: candidate.container ?? null,
        },
        inlineMentions,
        draftText,
      );
      orbitActions.registerInlineMention({
        token,
        kind: "code_symbol",
        path: candidate.file_path,
        startLine,
        endLine: candidate.end_line ?? startLine,
        symbolName,
        symbolKind: candidate.symbol_kind ?? "symbol",
        signature: candidate.signature ?? null,
        container: candidate.container ?? null,
        language: candidate.language ?? "",
      });
      insertInlineToken(token);
    } else if (candidate.kind === "file") {
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
    } else if (candidate.kind === "git_diff" || candidate.kind === "staged") {
      const staged = candidate.kind === "staged" || candidate.key.includes("staged");
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
    } else if (candidate.kind === "path_rule") {
      const token = candidate.token_label || candidate.title;
      insertInlineToken(token);
    } else if (candidate.kind === "folder_hint") {
      insertInlineToken("@folder:", false);
      if (selectedHostId && selectedCwd && selectedCwd !== "~") {
        void orbitActions.ensureWorkspaceFiles(selectedHostId, selectedCwd);
      }
    } else if (candidate.kind === "folder") {
      const folderPath = candidate.file_path.replace(/\/+$/, "");
      const token = candidate.token_label || `@folder:${folderPath}`;
      const cachedFiles =
        selectedHostId && selectedCwd
          ? (orbitActions.getSnapshot().workspaceFilesCache[`${selectedHostId}::${selectedCwd}`] ??
            [])
          : [];
      const { fileCount } = buildFolderTreeSummary(folderPath, cachedFiles);
      orbitActions.registerInlineMention({
        token,
        kind: "folder",
        path: folderPath,
        fileCount,
      });
      insertInlineToken(token);
    } else if (candidate.kind === "web_hint") {
      insertInlineToken("@web:https://", false);
    } else if (candidate.kind === "web") {
      const cleanUrl = (candidate.file_path || candidate.subtitle).trim();
      if (isValidHttpsReferenceUrl(cleanUrl)) {
        const token = candidate.token_label || `@web:${cleanUrl}`;
        orbitActions.registerInlineMention({
          token,
          kind: "web",
          url: cleanUrl,
          label: shortWebUrlLabel(cleanUrl),
        });
        insertInlineToken(token);
      } else {
        orbitActions.showToast(t("composer.mentionInvalidHttpsUrl"));
      }
    } else if (candidate.kind === "pr") {
      const joined = collapseStrippedToken(beforeAt, afterToken);
      orbitActions.setComposerDraft(joined.text);
      setCursorOffset(joined.cursor, joined.text);
      const hostId = selectedHostId;
      const cwd = selectedCwd;
      if (!hostId || !cwd || cwd === "~") {
        orbitActions.showToast(t("composer.mentionPrUnavailable"));
      } else {
        void orbitIpc
          .gitPrStatus({ hostId, cwd })
          .then((pr) => {
            if (pr.availability !== "available" || !pr.number) {
              orbitActions.showToast(pr.reason || t("composer.mentionPrNotFound"));
              return;
            }
            const token = `@pr#${pr.number}`;
            const title = pr.title?.trim() || `Pull Request #${pr.number}`;
            const summaryParts: string[] = [];
            if (pr.head_ref && pr.base_ref) {
              summaryParts.push(`Branch: ${pr.head_ref} -> ${pr.base_ref}`);
            }
            if (pr.state) {
              summaryParts.push(`State: ${pr.state}${pr.is_draft ? " (Draft)" : ""}`);
            }
            if (pr.checks.length > 0) {
              summaryParts.push(
                `CI checks: ${pr.passing_checks} passing, ${pr.failing_checks} failing, ${pr.pending_checks} pending`,
              );
            }
            if (pr.unresolved_comment_count > 0) {
              summaryParts.push(`Unresolved review comments: ${pr.unresolved_comment_count}`);
            }
            orbitActions.registerInlineMention({
              token,
              kind: "pr",
              number: pr.number,
              title,
              url: pr.url ?? null,
              state: pr.state ?? null,
              headRef: pr.head_ref ?? null,
              baseRef: pr.base_ref ?? null,
              summary: summaryParts.join("\n") || null,
            });
            const latest = draftTextRef.current;
            const sep = latest.length > 0 && !/\s$/.test(latest) ? " " : "";
            const next = `${latest}${sep}${token} `;
            orbitActions.setComposerDraft(next);
            setCursorOffset(next.length, next);
            requestAnimationFrame(() => {
              const el = textareaRef.current;
              if (el) {
                el.focus();
                el.setSelectionRange(next.length, next.length);
              }
            });
          })
          .catch(() => {
            orbitActions.showToast(t("composer.mentionPrUnavailable"));
          });
      }
    } else if (candidate.kind === "terminal" || candidate.kind === "terminal_output") {
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
        setCursorOffset(joined.cursor, joined.text);
        requestAnimationFrame(() => textareaRef.current?.focus());
      }
    }

    setDismissedToken(null);
    setMenuHighlightIdx(0);
  };

  const triggerAtMentionFromButton = useCallback(() => {
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
    setCursorOffset(nextPos, next);
    requestAnimationFrame(() => {
      if (textareaRef.current) {
        textareaRef.current.focus();
        textareaRef.current.setSelectionRange(nextPos, nextPos);
      }
    });
  }, [setCursorOffset]);

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
    setCursorOffset(nextPos, next);
    requestAnimationFrame(() => {
      if (textareaRef.current) {
        textareaRef.current.focus();
        textareaRef.current.setSelectionRange(nextPos, nextPos);
      }
    });
  }, [setCursorOffset]);

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
    setCursorOffset(nextPos, next);
    requestAnimationFrame(() => {
      if (textareaRef.current) {
        textareaRef.current.focus();
        textareaRef.current.setSelectionRange(nextPos, nextPos);
      }
    });
  }, [setCursorOffset]);

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
    const pastedFiles: File[] = [];
    for (const it of items) {
      if (it.kind === "file") {
        const f = it.getAsFile();
        if (f) pastedFiles.push(f);
      }
    }
    if (pastedFiles.length === 0) {
      for (const f of Array.from(e.clipboardData.files ?? [])) {
        pastedFiles.push(f);
      }
    }
    if (pastedFiles.length > 0) {
      e.preventDefault();
      for (const file of pastedFiles) {
        void orbitActions.stageAndAttachFile(file);
      }
      return;
    }

    const text = e.clipboardData.getData("text/plain");
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
            isComposerInlineDirective(c.id) &&
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
      !e.metaKey &&
      !e.ctrlKey &&
      !e.altKey &&
      !slashTrigger &&
      !mentionTrigger &&
      inlineMentions.length > 0 &&
      textareaRef.current
    ) {
      const atomicBack = resolveAtomicMentionBackspace(
        draftText,
        textareaRef.current.selectionStart,
        textareaRef.current.selectionEnd,
        inlineMentions,
      );
      if (atomicBack) {
        e.preventDefault();
        orbitActions.setComposerDraft(atomicBack.nextText);
        if (!containsInlineMentionToken(atomicBack.nextText, atomicBack.removedToken)) {
          orbitActions.unregisterInlineMention(atomicBack.removedToken);
        }
        setCursorOffset(atomicBack.nextCursor, atomicBack.nextText);
        requestAnimationFrame(() => {
          const el = textareaRef.current;
          if (el) {
            el.setSelectionRange(atomicBack.nextCursor, atomicBack.nextCursor);
          }
        });
        return;
      }
    }

    if (
      e.key === "Delete" &&
      !e.metaKey &&
      !e.ctrlKey &&
      !e.altKey &&
      !slashTrigger &&
      !mentionTrigger &&
      inlineMentions.length > 0 &&
      textareaRef.current
    ) {
      const atomicDel = resolveAtomicMentionDelete(
        draftText,
        textareaRef.current.selectionStart,
        textareaRef.current.selectionEnd,
        inlineMentions,
      );
      if (atomicDel) {
        e.preventDefault();
        orbitActions.setComposerDraft(atomicDel.nextText);
        if (!containsInlineMentionToken(atomicDel.nextText, atomicDel.removedToken)) {
          orbitActions.unregisterInlineMention(atomicDel.removedToken);
        }
        setCursorOffset(atomicDel.nextCursor, atomicDel.nextText);
        requestAnimationFrame(() => {
          const el = textareaRef.current;
          if (el) {
            el.setSelectionRange(atomicDel.nextCursor, atomicDel.nextCursor);
          }
        });
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

    if (mentionTrigger && combinedMentions.length > 0) {
      const len = combinedMentions.length;
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
        executeMentionSelect(combinedMentions[safeIdx]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setDismissedToken(mentionTrigger.tokenKey);
        return;
      }
    } else if (mentionTrigger && e.key === "Escape") {
      e.preventDefault();
      setDismissedToken(mentionTrigger.tokenKey);
      return;
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

    const enterAction = resolveComposerEnterAction(e, enterToSend, isTurnBusy, activeTurnSendBehavior);
    if (enterAction.submit) {
      e.preventDefault();
      triggerSubmit(enterAction.steerImmediately);
      return;
    }
  };

  const handleAttach = useCallback(() => fileInputRef.current?.click(), []);
  const handleFocusAfterDirective = useCallback(() => {
    textareaRef.current?.focus();
    textareaRef.current?.setSelectionRange(0, 0);
  }, []);
  const handleSelectModeSilent = useCallback((mode: SessionMode) => handleSelectMode(mode, false), [handleSelectMode]);
  const steerOnPrimarySubmit = isTurnBusy && activeTurnSendBehavior === "steer";
  const handleSubmitClick = useCallback(
    () => triggerSubmit(steerOnPrimarySubmit),
    [triggerSubmit, steerOnPrimarySubmit],
  );

  // --- M6 Codex-style Voice Control (zero extra toolbar chrome) ---
  const voiceRecorderRef = useRef<VoiceRecorderSession | null>(null);
  const voiceElapsedTimerRef = useRef<number | null>(null);
  const voiceStartMsRef = useRef<number>(0);
  const [voiceRecording, setVoiceRecording] = useState(false);
  const [voiceTranscribing, setVoiceTranscribing] = useState(false);
  const [voiceElapsedMs, setVoiceElapsedMs] = useState(0);
  const [voiceLevel, setVoiceLevel] = useState(0);
  const [voiceInterimText, setVoiceInterimText] = useState("");

  const stopVoiceTimer = useCallback(() => {
    if (voiceElapsedTimerRef.current !== null) {
      window.clearInterval(voiceElapsedTimerRef.current);
      voiceElapsedTimerRef.current = null;
    }
  }, []);

  const handleCancelVoice = useCallback(() => {
    stopVoiceTimer();
    voiceRecorderRef.current?.cancel();
    voiceRecorderRef.current = null;
    setVoiceRecording(false);
    setVoiceElapsedMs(0);
    setVoiceLevel(0);
    setVoiceInterimText("");
  }, [stopVoiceTimer]);

  // Cleanly release mic stream on unmount or host switch
  useEffect(() => {
    return () => {
      stopVoiceTimer();
      voiceRecorderRef.current?.cancel();
      voiceRecorderRef.current = null;
    };
  }, [selectedHostId, stopVoiceTimer]);

  // Escape cancels an active recording without disturbing surrounding panels
  useEffect(() => {
    if (!voiceRecording) return;
    const onKeyDown = (ev: KeyboardEvent) => {
      if (ev.key === "Escape") {
        ev.preventDefault();
        handleCancelVoice();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [voiceRecording, handleCancelVoice]);

  const executeVoiceResult = useCallback(
    async (
      hostId: string,
      agentId: string | null,
      result: VoiceTranscriptionResult,
      voiceLang: string | null,
    ) => {
      const intent = result.parsed_intent;
      if (!intent) {
        if (result.text.trim()) {
          const cur = draftTextRef.current;
          const next = cur.trim() ? `${cur.trimEnd()} ${result.text.trim()}` : result.text.trim();
          orbitActions.setComposerDraft(next);
          requestAnimationFrame(() => textareaRef.current?.focus());
        } else {
          orbitActions.showToast(t("composer.voiceNoSpeechDetected"));
        }
        return;
      }

      switch (intent.kind) {
        case "send_prompt": {
          const spoken = intent.text.trim();
          if (!spoken) {
            orbitActions.showToast(t("composer.voiceNoSpeechDetected"));
            return;
          }
          const cur = draftTextRef.current;
          const explicitSend =
            !cur.trim() && spoken.toLowerCase() !== result.text.trim().toLowerCase();
          const next = cur.trim() ? `${cur.trimEnd()} ${spoken}` : spoken;
          orbitActions.setComposerDraft(next);
          if (explicitSend) {
            void orbitActions.submitComposer().catch((err) => {
              orbitActions.showToast(err instanceof Error ? err.message : String(err));
            });
          } else {
            requestAnimationFrame(() => {
              textareaRef.current?.focus();
              textareaRef.current?.setSelectionRange(next.length, next.length);
            });
          }
          return;
        }
        case "switch_agent": {
          const modeAlias = resolveVoiceModeAlias(intent.query);
          if (modeAlias) {
            handleSelectModeSilent(modeAlias);
            orbitActions.showToast(
              t("composer.voiceSwitchedMode", { mode: modeLabel(modeAlias) }),
            );
            return;
          }
          const snap = orbitActions.getSnapshot();
          const agents = snap.hostDetails[hostId]?.agents ?? [];
          const q = intent.query.trim().toLowerCase();
          const matched = agents.find(
            (a) =>
              a.title.toLowerCase().includes(q) ||
              a.provider_label.toLowerCase().includes(q) ||
              a.agent_id.toLowerCase() === q,
          );
          if (matched) {
            orbitActions.selectAgent(hostId, matched.agent_id);
            orbitActions.showToast(
              t("composer.voiceSwitchedAgent", { title: matched.title || matched.provider_label }),
            );
          } else {
            orbitActions.showToast(t("composer.voiceAgentNotFound", { query: intent.query }));
          }
          return;
        }
        case "read_last_reply": {
          if (!agentId) {
            orbitActions.showToast(t("composer.voiceNoReplyToRead"));
            return;
          }
          const snap = orbitActions.getSnapshot();
          const stage = snap.stages[`${hostId}::${agentId}`];
          const turns = stage?.turns ?? [];
          let lastText = "";
          for (let i = turns.length - 1; i >= 0; i -= 1) {
            const termItem = turns[i]?.terminal_reply;
            const lastItem = turns[i]?.last_assistant_reply;
            const candidate =
              (termItem?.item_type === "assistant_message" ? termItem.text : "") ||
              (lastItem?.item_type === "assistant_message" ? lastItem.text : "");
            if (candidate.trim()) {
              lastText = candidate.trim();
              break;
            }
          }
          if (!lastText) {
            orbitActions.showToast(t("composer.voiceNoReplyToRead"));
            return;
          }
          try {
            const spoken = await speakVoiceText(hostId, lastText, { language: voiceLang });
            if (spoken.reason === "disabled") {
              orbitActions.showToast(t("composer.voiceTtsDisabledToast"));
            } else if (!spoken.spoken) {
              orbitActions.showToast(t("composer.voiceTtsUnavailable"));
            }
          } catch (err) {
            orbitActions.showToast(parseVoiceRpcError(err).message);
          }
          return;
        }
        case "queue_status": {
          const snap = orbitActions.getSnapshot();
          const scope = composerScopeKey(snap);
          const count = snap.queuedPrompts[scope]?.length ?? 0;
          orbitActions.showToast(
            count === 0
              ? t("composer.voiceQueueEmpty")
              : t("composer.voiceQueueCount", { count }),
          );
          return;
        }
        case "approve_permission": {
          const safety = result.approval_safety ?? "requires_visual_confirmation";
          if (safety === "requires_visual_confirmation") {
            focusVisualPermissionCard(intent.request_id);
            orbitActions.showToast(
              t("composer.voiceVisualConfirmRequired", { id: intent.request_id }),
            );
            return;
          }
          if (safety === "requires_spoken_confirmation" || !intent.confirmed) {
            focusVisualPermissionCard(intent.request_id);
            orbitActions.showToast(
              t("composer.voiceSpokenConfirmRequired", { id: intent.request_id }),
            );
            return;
          }
          if (!agentId) {
            focusVisualPermissionCard(intent.request_id);
            orbitActions.showToast(
              t("composer.voiceVisualConfirmRequired", { id: intent.request_id }),
            );
            return;
          }
          try {
            const finalSafety = await orbitIpc.approveVoicePermission(
              hostId,
              agentId,
              intent.request_id,
              true,
            );
            if (finalSafety === "allowed") {
              orbitActions.showToast(t("composer.voicePermissionApproved", { id: intent.request_id }));
            } else {
              focusVisualPermissionCard(intent.request_id);
              orbitActions.showToast(
                t("composer.voiceVisualConfirmRequired", { id: intent.request_id }),
              );
            }
          } catch (err) {
            orbitActions.showToast(parseVoiceRpcError(err).message);
          }
          return;
        }
        case "deny_permission": {
          if (!agentId) {
            focusVisualPermissionCard(intent.request_id);
            return;
          }
          try {
            await orbitIpc.resolvePermission(hostId, agentId, intent.request_id, "deny");
            orbitActions.showToast(t("composer.voicePermissionDenied", { id: intent.request_id }));
          } catch (err) {
            orbitActions.showToast(err instanceof Error ? err.message : String(err));
          }
          return;
        }
        case "cancel": {
          if (agentId && isTurnBusy) {
            void orbitIpc.interruptAgent(hostId, agentId);
            orbitActions.showToast(t("composer.voiceTurnStopped"));
          } else {
            orbitActions.showToast(t("composer.voiceCanceled"));
          }
          return;
        }
      }
    },
    [handleSelectModeSilent, isTurnBusy, t],
  );

  const handleStartVoice = useCallback(async () => {
    if (!selectedHostId || voiceRecording || voiceTranscribing) return;
    let voiceLang: string | null = null;
    try {
      const voiceState = await orbitIpc.getVoiceConfig(selectedHostId);
      voiceLang = voiceState.config.language;
      if (!voiceState.config.enabled) {
        orbitActions.showToast(t("composer.voiceDisabledOnHost"), {
          label: t("composer.voiceOpenSettings"),
          onClick: () => orbitActions.openSettings(`host:${selectedHostId}:voice`),
        });
        return;
      }
    } catch {
      // Proceed to mic capture and let transcribe report host errors cleanly
    }

    const session = new VoiceRecorderSession();
    try {
      await session.start({
        language: voiceLang,
        deviceId: orbitActions.getSnapshot().voiceInputDeviceId,
        onLevel: (lvl) => setVoiceLevel(lvl),
        onInterimTranscript: (txt) => setVoiceInterimText(txt),
      });
      voiceRecorderRef.current = session;
      voiceStartMsRef.current = Date.now();
      setVoiceElapsedMs(0);
      setVoiceLevel(0);
      setVoiceInterimText("");
      setVoiceRecording(true);
      stopVoiceTimer();
      voiceElapsedTimerRef.current = window.setInterval(() => {
        setVoiceElapsedMs(Date.now() - voiceStartMsRef.current);
      }, 200);
    } catch (err) {
      session.cancel();
      const domErr = err as { name?: string; message?: string };
      if (domErr?.name === "NotAllowedError" || domErr?.name === "PermissionDeniedError") {
        orbitActions.showToast(t("composer.voiceMicDenied"));
      } else {
        orbitActions.showToast(domErr?.message || t("composer.voiceUnavailable"));
      }
    }
  }, [selectedHostId, stopVoiceTimer, t, voiceRecording, voiceTranscribing]);

  const handleStopVoice = useCallback(async () => {
    const session = voiceRecorderRef.current;
    voiceRecorderRef.current = null;
    stopVoiceTimer();
    setVoiceRecording(false);
    setVoiceLevel(0);
    setVoiceInterimText("");
    if (!session || !selectedHostId) return;

    setVoiceTranscribing(true);
    try {
      const payload = await session.stop();
      let voiceLang: string | null = null;
      try {
        const cfgState = await orbitIpc.getVoiceConfig(selectedHostId);
        voiceLang = cfgState.config.language;
      } catch {
        // Optional language hint
      }
      let result = await orbitIpc.transcribeVoiceAudio(
        selectedHostId,
        payload.pcm16LeBase64,
        payload.sampleRateHz,
        {
          language: voiceLang,
          agentId: selectedAgentId,
        },
      );
      if (result.backend_used === "system_client_fallback" && !result.text.trim()) {
        const fallbackText =
          payload.browserTranscript || (await captureBrowserSpeechFallback(voiceLang));
        if (fallbackText.trim()) {
          result = await orbitIpc.parseVoiceTranscript(selectedHostId, fallbackText, {
            language: voiceLang,
            agentId: selectedAgentId,
          });
        } else {
          orbitActions.showToast(t("composer.voiceClientFallbackUnavailable"), {
            label: t("composer.voiceOpenSettings"),
            onClick: () => orbitActions.openSettings(`host:${selectedHostId}:voice`),
          });
          return;
        }
      }
      await executeVoiceResult(selectedHostId, selectedAgentId, result, voiceLang);
    } catch (err) {
      const parsed = parseVoiceRpcError(err);
      if (parsed.message.toLowerCase().includes("disabled")) {
        orbitActions.showToast(t("composer.voiceDisabledOnHost"), {
          label: t("composer.voiceOpenSettings"),
          onClick: () => orbitActions.openSettings(`host:${selectedHostId}:voice`),
        });
      } else {
        orbitActions.showToast(parsed.message || t("composer.voiceUnavailable"));
      }
    } finally {
      setVoiceTranscribing(false);
      setVoiceElapsedMs(0);
    }
  }, [executeVoiceResult, selectedAgentId, selectedHostId, stopVoiceTimer, t]);
  const handleRemoveInlineMention = useCallback((token: string) => {
    orbitActions.removeInlineMention(token);
    requestAnimationFrame(() => {
      textareaRef.current?.focus();
    });
  }, []);

  const activeInlineMentionSpans = useMemo(
    () => findInlineMentionSpans(draftText, inlineMentions),
    [draftText, inlineMentions],
  );

  const activeInlineMentions = useMemo(() => {
    if (inlineMentions.length === 0 || !draftText) return EMPTY_INLINE_MENTIONS;
    const matched = inlineMentions.filter((m) => containsInlineMentionToken(draftText, m.token));
    return matched.length > 0 ? matched : EMPTY_INLINE_MENTIONS;
  }, [draftText, inlineMentions]);

  const inlineBackdropSegments = useMemo(() => {
    if (activeInlineMentionSpans.length === 0) return null;
    const segs: Array<{ text: string; isMention: boolean }> = [];
    let cursorPos = 0;
    for (const span of activeInlineMentionSpans) {
      if (span.start > cursorPos) {
        segs.push({ text: draftText.slice(cursorPos, span.start), isMention: false });
      }
      segs.push({ text: draftText.slice(span.start, span.end), isMention: true });
      cursorPos = span.end;
    }
    if (cursorPos < draftText.length) {
      segs.push({ text: draftText.slice(cursorPos), isMention: false });
    }
    return segs;
  }, [draftText, activeInlineMentionSpans]);

  if (!selectedHostId) return null;

  const showMentionMenu =
    Boolean(mentionTrigger) &&
    (combinedMentions.length > 0 ||
      isSearchingSymbols ||
      Boolean(parsedSymbolTrigger?.symbolOnly && parsedSymbolTrigger.query.length > 0));

  const activeListboxId = slashTrigger
    ? SLASH_MENU_LISTBOX_ID
    : showMentionMenu
      ? MENTION_MENU_LISTBOX_ID
      : undefined;
  const activeDescendantId = slashTrigger
    ? slashOptionId(Math.min(menuHighlightIdx, slashTrigger.items.length - 1))
    : showMentionMenu && combinedMentions.length > 0
      ? mentionOptionId(Math.min(menuHighlightIdx, combinedMentions.length - 1))
      : undefined;

  const showStopButton = isTurnBusy && !hasPayload && Boolean(selectedAgentId);
  const waitingForUpload = hasUploadingAttachment && !localSubmission;
  const submitBusy = !localSubmission && (isSubmitting || hasUploadingAttachment);
  const alternateSendShortcut = enterToSend ? `${mod}↵` : mod === "⌘" ? "⌘⇧↵" : `${mod}Shift+↵`;
  const submitTooltipLabel = waitingForUpload
    ? t("composer.uploadingAttachments")
    : isSubmitting && !localSubmission
      ? t("composer.sendingPrompt")
      : isTurnBusy && !localSubmission
        ? activeTurnSendBehavior === "steer"
          ? t("composer.steerFollowUpHint", { shortcut: alternateSendShortcut })
          : t("composer.queueFollowUpHint", { shortcut: alternateSendShortcut })
        : t("common.send");
  const submitShortcut = submitBusy ? undefined : enterToSend ? "↵" : `${mod}↵`;

  const dynamicPlaceholder = (() => {
    switch (slashDirective) {
      case "btw":
        return t("composer.placeholderBtw");
      case "goal":
        return t("composer.placeholderGoal");
      case "plan":
        return t("composer.placeholderPlan");
      case "review":
        return t("composer.placeholderReview");
      case "compact":
        return t("composer.placeholderCompact");
      case "orbit":
        return t("composer.placeholderOrbit");
      case "orbit-handoff":
        return t("composer.placeholderOrbitHandoff");
      case "orbit-committee":
        return t("composer.placeholderOrbitCommittee");
      case "orbit-advisor":
        return t("composer.placeholderOrbitAdvisor");
      default:
        break;
    }
    if (isForegroundRunning) {
      return activeTurnSendBehavior === "steer"
        ? t("composer.placeholderRunningSteer", { shortcut: alternateSendShortcut })
        : t("composer.placeholderRunning", { shortcut: alternateSendShortcut });
    }
    if (!isDraft && hasCurrentAgent) {
      return t("composer.placeholderFollowUp");
    }
    return t("composer.placeholder");
  })();

  return (
    <div
      className={cn(
        "w-full",
        variant === "docked" ? "shrink-0 px-[24px] pt-0 pb-4" : "px-0",
      )}
    >
      <div className="mx-auto flex w-full max-w-[var(--orbit-stage-max-w,740px)] flex-col gap-2">
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

        {/* Isolated Queued Follow-Up Strip — zero keystroke re-renders */}
        <ComposerQueueStrip />

        {/* Relative Anchor for Floating `/` & `@` Menus + Single-Rim Tactile Composer Card */}
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
              query={slashTrigger.query}
              activeIndex={Math.min(menuHighlightIdx, slashTrigger.items.length - 1)}
              activeMode={activeMode}
              fastModeActive={fastModeActive}
              onHoverIndex={setMenuHighlightIdx}
              onSelect={executeSlashCommand}
            />
          )}

          {showMentionMenu && (
            <MentionMenu
              items={combinedMentions}
              activeIndex={Math.min(menuHighlightIdx, Math.max(0, combinedMentions.length - 1))}
              loading={isSearchingSymbols}
              showEmpty={Boolean(parsedSymbolTrigger?.symbolOnly && parsedSymbolTrigger.query.length > 0)}
              onHoverIndex={setMenuHighlightIdx}
              onSelect={executeMentionSelect}
            />
          )}

          {variant === "hero" && <ComposerHeroHeader />}

          <div
            data-occluding={occluding ? "true" : undefined}
            data-plan={slashDirective === "plan" ? "true" : undefined}
            data-dock-active={hasPendingDecisions ? "true" : undefined}
            className={cn(
              "composer-surface relative rounded-[20px]",
              isDraggingOver && "ring-signal-info/25 border-signal-info ring-2",
            )}
          >
            <div
              aria-hidden={!isDraggingOver}
              className={cn(
                "bg-surface-base/85 text-ink-primary pointer-events-none absolute inset-0 z-20 flex items-center justify-center rounded-[20px] text-[13px] font-medium backdrop-blur-[2px] transition-opacity duration-[140ms] ease-[var(--ease-out-quart)] select-none",
                isDraggingOver ? "opacity-100" : "opacity-0",
              )}
            >
              {t("composer.dropToAttach")}
            </div>

            {/* Unified Context Tray — attachments, active atomic `@` mention chips, diff quotes & large pastes */}
            <ContextTray
              contexts={contexts}
              inlineMentions={activeInlineMentions}
              quotes={quotes}
              pastes={pastes}
              onRemoveInlineMention={handleRemoveInlineMention}
            />

            {/* Shared Native Scroll Container (`max-h-[200px]`): inline directive token + inline mention pill backdrop + full-height textarea scroll as one compositor layer */}
            <div
              ref={scrollContainerRef}
              className="orbit-native-scroll relative max-h-[200px] overflow-x-hidden overflow-y-auto rounded-t-[20px]"
            >
              {slashDirective && (
                <div
                  ref={tokenOverlayRef}
                  className="pointer-events-none absolute z-10 flex w-max"
                  style={{ top: 11, left: BASE_PAD_LEFT - TOKEN_PAD_X }}
                >
                  <ComposerDirectiveToken directive={slashDirective} onClick={handleFocusAfterDirective} />
                </div>
              )}
              {draftText.length === 0 && (
                <div
                  aria-hidden="true"
                  style={{ left: BASE_PAD_LEFT + indent }}
                  className="text-ink-faint pointer-events-none absolute top-[12px] right-[14px] truncate text-[15px] leading-[22px] select-none"
                >
                  {dynamicPlaceholder}
                </div>
              )}

              {/* Pixel-aligned monochrome hairline pill backdrop behind bound `@` mention spans */}
              {inlineBackdropSegments && (
                <div
                  aria-hidden="true"
                  style={inputInset}
                  className="pointer-events-none absolute top-0 right-0 left-0 z-0 block w-full px-[14px] pt-[12px] pb-[6px] text-[15px] leading-[22px] whitespace-pre-wrap break-words text-transparent select-none"
                >
                  {inlineBackdropSegments.map((seg, idx) =>
                    seg.isMention ? (
                      <mark
                        key={idx}
                        className="bg-foreground/[0.065] ring-border-default/80 rounded-[5px] text-transparent ring-1 ring-inset"
                      >
                        {seg.text}
                      </mark>
                    ) : (
                      <span key={idx}>{seg.text}</span>
                    ),
                  )}
                </div>
              )}

              {/* Off-flow div mirror sizer: deterministically measures wrapped height on both typing and large pastes */}
              <div
                ref={mirrorTextareaRef}
                style={inputInset}
                aria-hidden="true"
                className="pointer-events-none invisible absolute top-0 right-0 left-0 -z-10 block w-full px-[14px] pt-[12px] pb-[6px] text-[15px] leading-[22px] whitespace-pre-wrap break-words transition-none"
              />

              <textarea
                ref={textareaRef}
                style={inputInset}
                value={draftText}
                data-orbit-composer-input="true"
                role="combobox"
                aria-label={t("composer.promptInputAria")}
                aria-autocomplete="list"
                aria-expanded={Boolean(activeListboxId)}
                aria-controls={activeListboxId}
                aria-activedescendant={activeDescendantId}
                onChange={(e) => {
                  const val = e.target.value;
                  setHistoryCursor(null);
                  setDismissedToken(null);
                  setMenuHighlightIdx(0);
                  trackTriggerCursor(e.currentTarget);
                  orbitActions.setComposerDraft(val);
                }}
                onSelect={(e) => trackTriggerCursor(e.currentTarget)}
                onClick={(e) => trackTriggerCursor(e.currentTarget)}
                onPaste={handlePaste}
                onBlur={ime.reset}
                {...ime.compositionProps}
                onKeyDown={handleKeyDown}
                rows={1}
                className="text-ink-primary relative z-[1] block min-h-[40px] w-full resize-none overflow-y-hidden bg-transparent px-[14px] pt-[12px] pb-[6px] text-[15px] leading-[22px] transition-none outline-none"
              />
            </div>

            {/* Minimal Inline Voice Recording Strip — rendered ONLY while actively recording */}
            {voiceRecording && (
              <div
                role="status"
                aria-live="polite"
                className="border-border-subtle/75 flex items-center justify-between gap-3 border-t px-3.5 py-2"
              >
                <div className="flex min-w-0 flex-1 items-center gap-2.5">
                  <span className="bg-signal-danger size-2 shrink-0 animate-pulse rounded-full motion-reduce:animate-none" />
                  <span
                    aria-hidden="true"
                    className="flex h-[14px] shrink-0 items-center gap-[2.5px]"
                  >
                    {[0.45, 0.78, 1.0, 0.72, 0.42].map((weight, idx) => {
                      const scale = Math.max(0.22, Math.min(1, 0.22 + voiceLevel * weight * 0.95));
                      return (
                        <span
                          key={idx}
                          style={{ transform: `scaleY(${scale.toFixed(2)})` }}
                          className="bg-ink-secondary/80 h-[12px] w-[2px] origin-center rounded-full transition-transform duration-100 ease-out"
                        />
                      );
                    })}
                  </span>
                  <span className="text-ink-secondary shrink-0 font-mono text-[12px] tabular-nums">
                    {formatVoiceDuration(voiceElapsedMs)}
                  </span>
                  <span className="text-ink-muted min-w-0 truncate text-[12px]">
                    {voiceInterimText || t("composer.voiceListening")}
                  </span>
                </div>
                <SimpleTooltip label={t("composer.voiceCancel")} shortcut="Esc" side="top">
                  <button
                    type="button"
                    aria-label={t("composer.voiceCancel")}
                    onClick={handleCancelVoice}
                    className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-6 shrink-0 cursor-pointer items-center justify-center rounded-full transition-colors"
                  >
                    <X className="size-[13px] stroke-[2]" />
                  </button>
                </SimpleTooltip>
              </div>
            )}

            <ComposerBottomBar
              hasPayload={hasPayload}
              submitBusy={submitBusy}
              showStopButton={showStopButton}
              voiceRecording={voiceRecording}
              voiceTranscribing={voiceTranscribing}
              submitTooltipLabel={submitTooltipLabel}
              submitShortcut={submitShortcut}
              activeMode={activeMode}
              effectiveProvider={effectiveProvider}
              isCompactBar={isCompactBar}
              selectedHostId={selectedHostId}
              selectedAgentId={selectedAgentId}
              onAttach={handleAttach}
              onMention={triggerAtMentionFromButton}
              onSlash={triggerSlashActionsFromButton}
              onSelectMode={handleSelectModeSilent}
              onSubmit={handleSubmitClick}
              onStartVoice={handleStartVoice}
              onStopVoice={handleStopVoice}
            />
          </div>
        </div>
      </div>
    </div>
  );
});

interface ComposerBottomBarProps {
  hasPayload: boolean;
  submitBusy: boolean;
  showStopButton: boolean;
  voiceRecording: boolean;
  voiceTranscribing: boolean;
  submitTooltipLabel: string;
  submitShortcut?: string;
  activeMode: SessionMode;
  effectiveProvider: ProviderKind;
  isCompactBar: boolean;
  selectedHostId: string;
  selectedAgentId: string | null;
  onAttach: () => void;
  onMention: () => void;
  onSlash: () => void;
  onSelectMode: (mode: SessionMode) => void;
  onSubmit: () => void;
  onStartVoice: () => void;
  onStopVoice: () => void;
}

const ComposerBottomBar = memo(function ComposerBottomBar({
  hasPayload, submitBusy, showStopButton, voiceRecording, voiceTranscribing,
  submitTooltipLabel, submitShortcut,
  activeMode, effectiveProvider, isCompactBar, selectedHostId, selectedAgentId,
  onAttach, onMention, onSlash, onSelectMode, onSubmit, onStartVoice, onStopVoice,
}: ComposerBottomBarProps) {
  const { t } = useI18n();
  const actionMode: "stop" | "voice_recording" | "submitting" | "send" | "voice_idle" =
    voiceRecording
      ? "voice_recording"
      : showStopButton
        ? "stop"
        : submitBusy || voiceTranscribing
          ? "submitting"
          : hasPayload
            ? "send"
            : "voice_idle";
  const actionLabel =
    actionMode === "voice_recording"
      ? t("composer.voiceStopAndTranscribe")
      : actionMode === "stop"
        ? t("composer.stopGeneration")
        : actionMode === "voice_idle"
          ? t("composer.voiceStartDictation")
          : voiceTranscribing
            ? t("composer.voiceTranscribing")
            : submitTooltipLabel;
  const actionShortcut =
    actionMode === "stop"
      ? "Esc Esc"
      : actionMode === "send"
        ? submitShortcut
        : undefined;
  const actionDisabled = actionMode === "submitting";

  return (
    <div className="flex items-center justify-between gap-2 px-2.5 pt-2 pb-2">
      <div className="flex min-w-0 flex-1 items-center gap-0.5 overflow-hidden">
        {/* 1. `[+]` Add Context Menu (`34×34` -> Files, Mentions, Actions) */}
        <DropdownMenu>
          <SimpleTooltip label={t("composer.addContextTitle")} side="top">
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                aria-label={t("composer.addContextTitle")}
                className="text-ink-secondary hover:text-ink-primary hover:bg-accent inline-flex size-8.5 shrink-0 cursor-pointer items-center justify-center rounded-full transition-colors"
              >
                <Plus className="size-[18px] stroke-[1.75]" />
              </button>
            </DropdownMenuTrigger>
          </SimpleTooltip>
          <DropdownMenuContent
            align="start"
            side="top"
            finalFocus={false}
            className="min-w-[168px]"
          >
            <DropdownMenuGroup>
              <DropdownMenuItem onSelect={onAttach}>
                <span className="flex items-center gap-2 whitespace-nowrap">
                  <Paperclip className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                  <span>{t("composer.files")}</span>
                </span>
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={onMention}>
                <span className="flex items-center gap-2 whitespace-nowrap">
                  <AtSign className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                  <span>{t("composer.mentions")}</span>
                </span>
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={onSlash}>
                <span className="flex items-center gap-2 whitespace-nowrap">
                  <SquareSlash className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                  <span>{t("composer.actions")}</span>
                </span>
              </DropdownMenuItem>
            </DropdownMenuGroup>
          </DropdownMenuContent>
        </DropdownMenu>

        {/* 2. Isolated Agent + Model + Thinking / Fast Selector */}
        <ComposerModelPicker />
      </div>

      {/* Right Cluster: Ambient Context Ring + Access Permissions + Stationary Single `size-8.5` Action Circle */}
      <div className="flex shrink-0 items-center gap-1">
        <ContextOccupancyPill />
        <ComposerModePicker
          mode={activeMode}
          onSelectMode={onSelectMode}
          provider={effectiveProvider}
          iconOnly={isCompactBar}
        />

        <SimpleTooltip
          label={actionLabel}
          shortcut={actionShortcut}
          side="top"
        >
          <button
            type="button"
            disabled={actionDisabled}
            aria-busy={actionMode === "submitting"}
            aria-pressed={actionMode === "voice_recording" ? true : undefined}
            aria-label={actionLabel}
            onClick={() => {
              if (actionMode === "voice_recording") {
                onStopVoice();
                return;
              }
              if (actionMode === "stop") {
                if (selectedAgentId) {
                  void orbitIpc.interruptAgent(selectedHostId, selectedAgentId);
                }
                return;
              }
              if (actionMode === "voice_idle") {
                onStartVoice();
                return;
              }
              if (actionMode === "send") {
                onSubmit();
              }
            }}
            className={cn(
              "orbit-press inline-flex size-8.5 shrink-0 cursor-pointer items-center justify-center rounded-full transition-colors duration-150 ease-[var(--ease-out-expo)]",
              actionMode === "stop"
                ? "bg-surface-recessed hover:bg-accent"
                : actionMode === "voice_recording"
                  ? "bg-action-primary-bg text-action-primary-fg hover:bg-action-primary-hover"
                  : actionMode === "send"
                    ? "bg-action-primary-bg text-action-primary-fg hover:bg-action-primary-hover"
                    : actionMode === "submitting"
                      ? "bg-action-primary-bg text-action-primary-fg pointer-events-none opacity-80"
                      : "text-ink-secondary bg-surface-recessed hover:bg-accent hover:text-ink-primary",
            )}
          >
            <span className="relative flex size-4 items-center justify-center">
              <Mic
                aria-hidden="true"
                className={cn(
                  "absolute size-[15.5px] stroke-[1.9] transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)]",
                  actionMode === "voice_idle"
                    ? "scale-100 opacity-100"
                    : "pointer-events-none scale-75 opacity-0",
                )}
              />
              <ArrowUp
                aria-hidden="true"
                className={cn(
                  "absolute size-[17px] stroke-[2.15] transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)]",
                  actionMode === "send"
                    ? "scale-100 opacity-100"
                    : "pointer-events-none scale-75 opacity-0",
                )}
              />
              <span
                aria-hidden="true"
                className={cn(
                  "bg-signal-danger absolute size-[11px] rounded-[2.5px] transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)]",
                  actionMode === "stop"
                    ? "scale-100 opacity-100"
                    : "pointer-events-none scale-75 opacity-0",
                )}
              />
              <span
                aria-hidden="true"
                className={cn(
                  "bg-action-primary-fg absolute size-[10px] rounded-[2.5px] transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)]",
                  actionMode === "voice_recording"
                    ? "scale-100 opacity-100"
                    : "pointer-events-none scale-75 opacity-0",
                )}
              />
              <span
                aria-hidden="true"
                className={cn(
                  "absolute flex items-center justify-center transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)]",
                  actionMode === "submitting"
                    ? "scale-100 opacity-100"
                    : "pointer-events-none scale-75 opacity-0",
                )}
              >
                <OrbitSpinner
                  size={15}
                  className={cn(
                    "shrink-0",
                    actionMode !== "submitting" && "[animation-play-state:paused]",
                  )}
                />
              </span>
            </span>
          </button>
        </SimpleTooltip>
      </div>
    </div>
  );
});
