import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Check,
  ExternalLink,
  MessageSquarePlus,
  Send,
  ShieldAlert,
  Sparkles,
  X,
} from "lucide-react";
import {
  type PendingMessageRequest,
  type PendingModelContextRequest,
  type PendingOpenLinkRequest,
  type PendingToolCallRequest,
  type SandboxTheme,
  createMcpAppHostBridge,
  injectSandboxCsp,
} from "@/components/artifacts/mcp-app-bridge";
import { orbitIpc } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const CONFIRM_CLICKJACK_DELAY_MS = 300;

export interface ArtifactSandboxFrameProps {
  artifactId: string;
  title: string;
  version: number;
  html: string;
  isMcpApp?: boolean;
  resourceUri?: string | null;
  hostId?: string | null;
  mcpServerName?: string | null;
}

/**
 * Strictly sandboxed HTML & SEP-1865 MCP Apps runtime frame.
 *
 * Isolation guarantees:
 * - `sandbox="allow-scripts"` ONLY (no `allow-same-origin`, no `allow-top-navigation`, no `allow-popups`).
 * - Unbypassable byte-0 CSP (`default-src 'none'; base-uri 'none'; form-action 'none'; frame-src 'none'; ...`).
 * - Every `postMessage` validates `event.source === iframeRef.current?.contentWindow` AND `event.origin === "null"`.
 * - Every host-facing request (`tools/call`, `ui/open-link`, `ui/message`, `ui/update-model-context`) requires
 *   explicit user confirmation, refuses bait-and-switch overwrites (`-32002`), and disables confirm buttons
 *   for 300ms after opening to defeat clickjacking.
 * - `resources/read` and `tools/call` are strictly bound to `(artifactId, version)` and resolved on the
 *   daemon in the artifact's persisted workspace (never using active-agent cwd or cross-server overrides).
 * - Any iframe load/navigation increments the bridge navigation epoch and drops in-flight replies.
 */
export const ArtifactSandboxFrame = memo(function ArtifactSandboxFrame({
  artifactId,
  title,
  version,
  html,
  isMcpApp = false,
  resourceUri = null,
  hostId = null,
  mcpServerName = null,
}: ArtifactSandboxFrameProps) {
  const resolvedTheme = useOrbitSelector((s) => s.resolvedTheme);
  const theme: SandboxTheme = resolvedTheme === "dark" ? "dark" : "light";

  const iframeRef = useRef<HTMLIFrameElement | null>(null);
  const themeRef = useRef<SandboxTheme>(theme);
  themeRef.current = theme;
  const hostIdRef = useRef<string | null>(hostId);
  hostIdRef.current = hostId;
  const artifactIdRef = useRef<string>(artifactId);
  artifactIdRef.current = artifactId;
  const versionRef = useRef<number>(version);
  versionRef.current = version;
  const mcpServerNameRef = useRef<string | null>(mcpServerName);
  mcpServerNameRef.current = mcpServerName;
  const artifactGenRef = useRef<number>(0);

  const [frameHeight, setFrameHeight] = useState<number>(460);
  const [bridgeReady, setBridgeReady] = useState<boolean>(false);
  const [pendingToolCall, setPendingToolCall] = useState<PendingToolCallRequest | null>(null);
  const [pendingLink, setPendingLink] = useState<PendingOpenLinkRequest | null>(null);
  const [pendingMessage, setPendingMessage] = useState<PendingMessageRequest | null>(null);
  const [pendingContext, setPendingContext] = useState<PendingModelContextRequest | null>(null);
  const [confirmLocked, setConfirmLocked] = useState<boolean>(false);

  // Build srcDoc once per artifact content/version; theme updates after load flow via SEP-1865 notification
  const initialThemeRef = useRef<SandboxTheme>(theme);
  const srcDoc = useMemo(() => {
    initialThemeRef.current = themeRef.current;
    return injectSandboxCsp(html, initialThemeRef.current);
  }, [artifactId, version, html]);

  // Brief clickjacking protection lock whenever a new confirmation bar appears
  const activePromptKey = pendingToolCall
    ? `tool:${String(pendingToolCall.id)}:${pendingToolCall.name}`
    : pendingLink
      ? `link:${String(pendingLink.id)}:${pendingLink.url}`
      : pendingMessage
        ? `msg:${String(pendingMessage.id)}`
        : pendingContext
          ? `ctx:${String(pendingContext.id)}`
          : null;

  useEffect(() => {
    if (!activePromptKey) {
      setConfirmLocked(false);
      return;
    }
    setConfirmLocked(true);
    const timer = window.setTimeout(() => setConfirmLocked(false), CONFIRM_CLICKJACK_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [activePromptKey]);

  const bridge = useMemo(
    () =>
      createMcpAppHostBridge({
        getExpectedSource: () => iframeRef.current?.contentWindow ?? null,
        getTheme: () => themeRef.current,
        getOwningMcpServerName: () => mcpServerNameRef.current?.trim() || null,
        onInitialized: () => {
          setBridgeReady(true);
        },
        onSizeChanged: (nextHeight) => {
          setFrameHeight(nextHeight);
        },
        onOpenLinkRequest: (req) => {
          setPendingLink(req);
        },
        onMessageRequest: (req) => {
          setPendingMessage(req);
        },
        onUpdateModelContextRequest: (req) => {
          // NEVER silently mutate composer — always stage for explicit Attach / Dismiss
          setPendingContext(req);
        },
        onToolCallRequest: (req) => {
          setPendingToolCall(req);
        },
        onReadResourceRequest: (req) => {
          const activeHost = hostIdRef.current?.trim() || "";
          const activeArtifactId = artifactIdRef.current;
          const activeVersion = versionRef.current;
          const gen = artifactGenRef.current;
          const reqEpoch = req.navigationEpoch;
          if (!activeHost) {
            bridge.resolveResourceRead(
              req.id,
              null,
              "No active host connected for resources/read",
              reqEpoch,
            );
            return;
          }
          void orbitIpc
            .readMcpResource(activeHost, activeArtifactId, activeVersion, req.uri)
            .then((res) => {
              if (artifactGenRef.current !== gen || bridge.getNavigationEpoch() !== reqEpoch) {
                return;
              }
              bridge.resolveResourceRead(
                req.id,
                {
                  uri: res.uri,
                  mimeType: res.mime_type,
                  text: res.text,
                  blob: res.blob_base64,
                },
                undefined,
                reqEpoch,
              );
            })
            .catch(() => {
              if (artifactGenRef.current !== gen || bridge.getNavigationEpoch() !== reqEpoch) {
                return;
              }
              // Never echo raw host/process stderr or internal transport traces into iframe JS.
              bridge.resolveResourceRead(
                req.id,
                null,
                "Failed to read MCP resource from owning server",
                reqEpoch,
              );
            });
        },
      }),
    [],
  );

  // Reset per-artifact transient prompts & invalidate pending async replies on artifact/version switch
  useEffect(() => {
    artifactGenRef.current += 1;
    bridge.invalidateNavigation();
    setBridgeReady(false);
    setPendingToolCall(null);
    setPendingLink(null);
    setPendingMessage(null);
    setPendingContext(null);
  }, [bridge, artifactId, version]);

  // Listen for SEP-1865 JSON-RPC 2.0 messages originating strictly from our sandboxed iframe
  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      bridge.handleMessageEvent(event);
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [bridge]);

  // Automatically push `ui/notifications/host-context-changed` when Orbit switches Light <-> Dark
  useEffect(() => {
    bridge.pushThemeChange(theme);
  }, [bridge, theme]);

  const handleConfirmToolCall = useCallback(
    async (approved: boolean) => {
      if (!pendingToolCall) return;
      if (approved && confirmLocked) return;
      const req = pendingToolCall;
      const reqEpoch = req.navigationEpoch;
      setPendingToolCall(null);
      if (!approved) {
        bridge.resolveToolCall(req.id, false, "Tool request denied by user", reqEpoch);
        return;
      }

      const activeHost = hostIdRef.current?.trim() || "";
      const boundServer = mcpServerNameRef.current?.trim() || "";
      const gen = artifactGenRef.current;

      if (activeHost && boundServer) {
        try {
          const outcome = await orbitIpc.callArtifactMcpTool(
            activeHost,
            artifactId,
            version,
            req.name,
            req.arguments,
          );
          if (artifactGenRef.current !== gen || bridge.getNavigationEpoch() !== reqEpoch) return;
          bridge.resolveToolCallResult(
            req.id,
            {
              ok: outcome.ok,
              isError: outcome.is_error,
              content: outcome.content,
              structuredContent: outcome.structured_content,
              error: outcome.error,
            },
            reqEpoch,
          );
          if (!outcome.ok && outcome.error) {
            orbitActions.showToast(`MCP tool ${req.name} failed: ${outcome.error}`);
          }
        } catch (err) {
          if (artifactGenRef.current !== gen || bridge.getNavigationEpoch() !== reqEpoch) return;
          // Return concise protocol-safe error to iframe; show toast only in trusted host UI.
          bridge.resolveToolCallResult(
            req.id,
            {
              ok: false,
              isError: true,
              content: [],
              error: "MCP tool execution failed on owning server",
            },
            reqEpoch,
          );
          orbitActions.showToast(`MCP tool ${req.name} failed: ${String(err)}`);
        }
        return;
      }

      const hasArgs = Object.keys(req.arguments).length > 0;
      const promptLine = hasArgs
        ? `Run tool \`${req.name}\` requested by Artifact "${title}" (v${version}):\n\`\`\`json\n${req.argsPreview}\n\`\`\``
        : `Run tool \`${req.name}\` requested by Artifact "${title}" (v${version})`;
      orbitActions.quoteDiffLineToComposer(
        `Artifact "${title}" (v${version}, tools/call)`,
        null,
        promptLine,
      );
      bridge.resolveToolCall(
        req.id,
        true,
        `Tool request "${req.name}" attached to active conversation composer.`,
        reqEpoch,
      );
      orbitActions.showToast(`Attached tool request: ${req.name}`);
    },
    [artifactId, bridge, confirmLocked, pendingToolCall, title, version],
  );

  const handleConfirmOpenLink = useCallback(
    async (approved: boolean) => {
      if (!pendingLink) return;
      if (approved && confirmLocked) return;
      const req = pendingLink;
      setPendingLink(null);
      if (!approved) {
        bridge.resolveOpenLink(req.id, false, "Link open cancelled by user");
        return;
      }
      try {
        await orbitIpc.openExternalBrowserUrl(req.url);
        bridge.resolveOpenLink(req.id, true);
      } catch (err) {
        bridge.resolveOpenLink(req.id, false, `Unable to open link: ${String(err)}`);
        orbitActions.showToast(`Unable to open link: ${String(err)}`);
      }
    },
    [bridge, confirmLocked, pendingLink],
  );

  const handleResolveMessage = useCallback(
    (action: "insert" | "send" | "dismiss") => {
      if (!pendingMessage) return;
      if (action !== "dismiss" && confirmLocked) return;
      const req = pendingMessage;
      setPendingMessage(null);
      if (action === "dismiss") {
        bridge.resolveMessage(req.id, false, "Message dismissed by user");
        return;
      }
      orbitActions.appendToComposerDraft(req.text);
      if (action === "send") {
        void orbitActions.submitComposer();
      } else {
        orbitActions.focusComposer();
      }
      bridge.resolveMessage(req.id, true);
    },
    [bridge, confirmLocked, pendingMessage],
  );

  const handleResolveContext = useCallback(
    (approved: boolean) => {
      if (!pendingContext) return;
      if (approved && confirmLocked) return;
      const req = pendingContext;
      setPendingContext(null);
      if (!approved) {
        bridge.resolveModelContext(req.id, false, "Context update dismissed by user");
        return;
      }
      orbitActions.quoteDiffLineToComposer(
        `Artifact "${title}" (v${version}, App Context)`,
        null,
        req.note,
      );
      bridge.resolveModelContext(req.id, true);
      orbitActions.showToast("Attached app context to composer");
    },
    [bridge, confirmLocked, pendingContext, title, version],
  );

  const hasToolArgs = Boolean(pendingToolCall && Object.keys(pendingToolCall.arguments).length > 0);

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Quiet SEP-1865 status strip (only when MCP App URI or bridge active) */}
      {(isMcpApp || resourceUri || bridgeReady) && (
        <div className="border-border/60 bg-elevated/35 text-muted-foreground flex h-[28px] shrink-0 items-center justify-between gap-2 border-b px-3 text-[11px]">
          <span className="flex min-w-0 items-center gap-1.5">
            <span
              className={cn(
                "size-[6px] shrink-0 rounded-full",
                bridgeReady ? "bg-emerald-500" : "bg-muted-foreground/45",
              )}
            />
            <span className="truncate font-mono">
              {resourceUri || (isMcpApp ? "MCP App (SEP-1865)" : "Sandboxed HTML")}
            </span>
          </span>
          <span className="text-muted-foreground/70 shrink-0">
            {mcpServerName ? `MCP · ${mcpServerName} · Sandboxed` : "Sandboxed · No network"}
          </span>
        </div>
      )}

      {/* Restrained inline confirmation bar: tools/call (with bounded arguments preview) */}
      {pendingToolCall && (
        <div className="border-border bg-amber-500/[0.08] flex shrink-0 flex-col gap-1.5 border-b px-3 py-2 text-[12px]">
          <div className="flex items-center justify-between gap-2">
            <div className="flex min-w-0 items-center gap-2 text-amber-800 dark:text-amber-200">
              <ShieldAlert className="h-[14px] w-[14px] shrink-0" />
              <span className="truncate">
                App requests tool:{" "}
                <code className="font-mono font-medium">{pendingToolCall.name}</code>
              </span>
            </div>
            <div className="flex shrink-0 items-center gap-1.5">
              <button
                type="button"
                onClick={() => void handleConfirmToolCall(false)}
                className="border-border hover:bg-accent text-foreground/80 cursor-pointer rounded-md border px-2 py-0.5 text-[11.5px]"
              >
                Deny
              </button>
              <button
                type="button"
                disabled={confirmLocked}
                onClick={() => void handleConfirmToolCall(true)}
                className="bg-foreground text-background inline-flex cursor-pointer items-center gap-1 rounded-md px-2.5 py-0.5 text-[11.5px] font-medium hover:opacity-90 disabled:cursor-default disabled:opacity-45"
              >
                <Check className="h-3 w-3" />
                <span>Allow</span>
              </button>
            </div>
          </div>
          {hasToolArgs && (
            <pre className="border-border/70 bg-background/80 text-foreground/85 m-0 max-h-28 overflow-auto rounded-md border px-2 py-1.5 font-mono text-[11px] leading-snug whitespace-pre-wrap break-all select-text">
              {pendingToolCall.argsPreview}
            </pre>
          )}
        </div>
      )}

      {/* Restrained inline confirmation bar: ui/open-link (https:// only) */}
      {pendingLink && (
        <div className="border-border bg-elevated/70 flex shrink-0 items-center justify-between gap-2 border-b px-3 py-1.5 text-[12px]">
          <div className="flex min-w-0 items-center gap-1.5">
            <ExternalLink className="text-muted-foreground h-[13px] w-[13px] shrink-0" />
            <span className="text-foreground/85 truncate font-mono text-[11.5px]">
              {pendingLink.url}
            </span>
          </div>
          <div className="flex shrink-0 items-center gap-1.5">
            <button
              type="button"
              onClick={() => void handleConfirmOpenLink(false)}
              className="text-muted-foreground hover:text-foreground cursor-pointer px-1.5 py-0.5 text-[11.5px]"
            >
              Cancel
            </button>
            <button
              type="button"
              disabled={confirmLocked}
              onClick={() => void handleConfirmOpenLink(true)}
              className="bg-foreground text-background cursor-pointer rounded-md px-2 py-0.5 text-[11.5px] font-medium disabled:cursor-default disabled:opacity-45"
            >
              Open
            </button>
          </div>
        </div>
      )}

      {/* Restrained user-visible bar: ui/message */}
      {pendingMessage && (
        <div className="border-border bg-elevated/70 flex shrink-0 items-center justify-between gap-2 border-b px-3 py-1.5 text-[12px]">
          <div className="flex min-w-0 items-center gap-1.5">
            <MessageSquarePlus className="text-muted-foreground h-[13px] w-[13px] shrink-0" />
            <span className="text-foreground/85 truncate">{pendingMessage.text}</span>
          </div>
          <div className="flex shrink-0 items-center gap-1">
            <button
              type="button"
              disabled={confirmLocked}
              onClick={() => handleResolveMessage("insert")}
              className="border-border hover:bg-accent cursor-pointer rounded-md border px-2 py-0.5 text-[11.5px] disabled:cursor-default disabled:opacity-45"
            >
              Insert
            </button>
            <button
              type="button"
              disabled={confirmLocked}
              onClick={() => handleResolveMessage("send")}
              className="bg-foreground text-background inline-flex cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[11.5px] font-medium disabled:cursor-default disabled:opacity-45"
            >
              <Send className="h-3 w-3" />
              <span>Send</span>
            </button>
            <button
              type="button"
              aria-label="Dismiss app message"
              onClick={() => handleResolveMessage("dismiss")}
              className="text-muted-foreground hover:text-foreground inline-flex h-5 w-5 cursor-pointer items-center justify-center rounded"
            >
              <X className="h-3 w-3" />
            </button>
          </div>
        </div>
      )}

      {/* Restrained confirmation bar: ui/update-model-context (never auto-attaches silently) */}
      {pendingContext && (
        <div className="border-border bg-elevated/70 flex shrink-0 items-center justify-between gap-2 border-b px-3 py-1.5 text-[12px]">
          <div className="flex min-w-0 items-center gap-1.5">
            <Sparkles className="text-muted-foreground h-[13px] w-[13px] shrink-0" />
            <span className="text-foreground/85 truncate">
              App suggests context: <span className="text-muted-foreground">{pendingContext.note}</span>
            </span>
          </div>
          <div className="flex shrink-0 items-center gap-1.5">
            <button
              type="button"
              onClick={() => handleResolveContext(false)}
              className="text-muted-foreground hover:text-foreground cursor-pointer px-1.5 py-0.5 text-[11.5px]"
            >
              Dismiss
            </button>
            <button
              type="button"
              disabled={confirmLocked}
              onClick={() => handleResolveContext(true)}
              className="bg-foreground text-background cursor-pointer rounded-md px-2 py-0.5 text-[11.5px] font-medium disabled:cursor-default disabled:opacity-45"
            >
              Attach
            </button>
          </div>
        </div>
      )}

      {/* Strictly Sandboxed iframe (`allow-scripts` ONLY — never `allow-same-origin`) */}
      <div className="min-h-0 flex-1 overflow-auto bg-white dark:bg-[#0f1115]">
        <iframe
          ref={iframeRef}
          key={`${artifactId}::v${version}`}
          title={title}
          sandbox="allow-scripts"
          referrerPolicy="no-referrer"
          srcDoc={srcDoc}
          onLoad={() => {
            // Invalidate any in-flight MCP replies whenever the iframe loads or navigates.
            bridge.invalidateNavigation();
            setBridgeReady(false);
            setPendingToolCall(null);
            setPendingLink(null);
            setPendingMessage(null);
            setPendingContext(null);
            bridge.pushThemeChange(themeRef.current);
          }}
          style={{ minHeight: `${frameHeight}px` }}
          className="block h-full w-full border-0"
        />
      </div>
    </div>
  );
});
