import { type ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { AlertCircle, ExternalLink, RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useI18n } from "@/lib/i18n";
import {
  type OrbitPluginTone,
  type OrbitPluginUiNode,
  type OrbitPluginUiSlot,
  type OrbitPluginUiSpec,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  type SafeMarkdownInlineToken,
  isSafePluginUrl,
  tokenizeSafePluginMarkdownLine,
} from "@/components/plugins/plugin-ui-safety";
import { cn } from "@/lib/utils";

export { type SafeMarkdownInlineToken, isSafePluginUrl, tokenizeSafePluginMarkdownLine };

function toneTextClass(tone?: OrbitPluginTone): string {
  switch (tone) {
    case "info":
      return "text-signal-info";
    case "success":
      return "text-signal-success";
    case "warning":
      return "text-signal-warning";
    case "danger":
      return "text-signal-danger";
    default:
      return "text-ink-primary";
  }
}

function toneBadgeClass(tone?: OrbitPluginTone): string {
  switch (tone) {
    case "info":
      return "border-signal-info/30 bg-signal-info/10 text-signal-info";
    case "success":
      return "border-signal-success/30 bg-signal-success/10 text-signal-success";
    case "warning":
      return "border-signal-warning/30 bg-signal-warning/10 text-signal-warning";
    case "danger":
      return "border-signal-danger/30 bg-signal-danger/10 text-signal-danger";
    default:
      return "border-border-subtle bg-surface-recessed text-ink-secondary";
  }
}

function SafePluginMarkdown({ content }: { content: string }) {
  const lines = useMemo(() => content.split(/\r?\n/), [content]);

  const renderInline = (line: string, keyPrefix: string): ReactNode =>
    tokenizeSafePluginMarkdownLine(line).map((tok, idx) => {
      const key = `${keyPrefix}-${idx}`;
      switch (tok.kind) {
        case "code":
          return (
            <code
              key={key}
              className="bg-surface-recessed text-ink-primary rounded px-1 py-0.5 font-mono text-[11.5px]"
            >
              {tok.text}
            </code>
          );
        case "strong":
          return (
            <strong key={key} className="text-ink-primary font-semibold">
              {tok.text}
            </strong>
          );
        case "link":
          return (
            <a
              key={key}
              href={tok.href}
              target="_blank"
              rel="noopener noreferrer"
              className="text-signal-info inline-flex items-center gap-0.5 underline underline-offset-2 hover:opacity-85"
            >
              <span>{tok.text}</span>
              <ExternalLink className="size-3 shrink-0" />
            </a>
          );
        default:
          return <span key={key}>{tok.text}</span>;
      }
    });

  return (
    <div className="text-ink-secondary flex flex-col gap-1.5 text-[12.5px] leading-[1.5]">
      {lines.map((rawLine, idx) => {
        const trimmed = rawLine.trim();
        if (!trimmed) return <div key={idx} className="h-1" />;
        const headingMatch = trimmed.match(/^(#{1,3})\s+(.*)$/);
        if (headingMatch) {
          return (
            <div key={idx} className="text-ink-primary text-[13px] font-semibold">
              {renderInline(headingMatch[2], `h-${idx}`)}
            </div>
          );
        }
        const bulletMatch = trimmed.match(/^[-*]\s+(.*)$/);
        if (bulletMatch) {
          return (
            <div key={idx} className="flex items-start gap-2 pl-1">
              <span className="text-ink-faint select-none">•</span>
              <span className="min-w-0 flex-1">{renderInline(bulletMatch[1], `b-${idx}`)}</span>
            </div>
          );
        }
        return (
          <p key={idx} className="break-words">
            {renderInline(rawLine, `p-${idx}`)}
          </p>
        );
      })}
    </div>
  );
}

function collectInitialFormValues(
  node: OrbitPluginUiNode,
  out: Record<string, string>,
  depth = 0,
): void {
  if (depth > 8) return;
  if (node.type === "form_input") {
    out[node.field_id] = node.value ?? "";
    return;
  }
  if (node.type === "stack" || node.type === "card") {
    for (const child of node.children) {
      collectInitialFormValues(child, out, depth + 1);
    }
  }
}

export interface PluginUiRendererProps {
  spec: OrbitPluginUiSpec;
  busyActionId?: string | null;
  onAction?: (actionId: string, payload: unknown, formValues: Record<string, string>) => void;
  className?: string;
}

export function PluginUiRenderer({
  spec,
  busyActionId = null,
  onAction,
  className,
}: PluginUiRendererProps) {
  const [formValues, setFormValues] = useState<Record<string, string>>(() => {
    const init: Record<string, string> = {};
    collectInitialFormValues(spec.root, init);
    return init;
  });

  useEffect(() => {
    const nextInit: Record<string, string> = {};
    collectInitialFormValues(spec.root, nextInit);
    setFormValues((prev) => ({ ...nextInit, ...prev }));
  }, [spec]);

  const renderNode = useCallback(
    (node: OrbitPluginUiNode, key: string, depth: number): ReactNode => {
      if (depth > 8) return null;

      switch (node.type) {
        case "stack": {
          const gapPx = Math.max(0, Math.min(32, node.gap ?? 8));
          return (
            <div
              key={key}
              style={{ gap: `${gapPx}px` }}
              className={cn(
                "flex min-w-0",
                node.direction === "horizontal" ? "flex-row flex-wrap items-center" : "flex-col",
              )}
            >
              {node.children.map((child, idx) => renderNode(child, `${key}-${idx}`, depth + 1))}
            </div>
          );
        }
        case "card":
          return (
            <div
              key={key}
              className="border-border-subtle bg-surface-raised flex min-w-0 flex-col gap-3 rounded-[12px] border p-3.5"
            >
              <div className="flex flex-col gap-0.5">
                <div className="text-ink-primary text-[13px] font-semibold">{node.title}</div>
                {node.subtitle && (
                  <div className="text-ink-secondary text-[12px]">{node.subtitle}</div>
                )}
              </div>
              {node.children.length > 0 && (
                <div className="flex min-w-0 flex-col gap-2.5">
                  {node.children.map((child, idx) => renderNode(child, `${key}-${idx}`, depth + 1))}
                </div>
              )}
            </div>
          );
        case "key_value":
          return (
            <div
              key={key}
              className="border-border-subtle/70 flex items-baseline justify-between gap-3 border-b py-1.5 last:border-b-0"
            >
              <span className="text-ink-secondary text-[12px]">{node.label}</span>
              <span
                className={cn(
                  "text-right font-mono text-[12px] font-medium break-all tabular-nums",
                  toneTextClass(node.tone),
                )}
              >
                {node.value}
              </span>
            </div>
          );
        case "badge":
          return (
            <Badge
              key={key}
              variant="outline"
              className={cn("text-[11px] font-medium", toneBadgeClass(node.tone))}
            >
              {node.text}
            </Badge>
          );
        case "table":
          return (
            <div
              key={key}
              className="border-border-subtle overflow-x-auto rounded-[8px] border"
            >
              <table className="w-full border-collapse text-left text-[12px]">
                <thead>
                  <tr className="border-border-subtle bg-surface-recessed/70 border-b">
                    {node.columns.map((col, ci) => (
                      <th
                        key={ci}
                        className="text-ink-muted px-2.5 py-1.5 text-[11px] font-semibold tracking-[0.02em] whitespace-nowrap"
                      >
                        {col}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody className="divide-border-subtle/70 divide-y">
                  {node.rows.map((row, ri) => (
                    <tr key={ri} className="hover:bg-surface-recessed/35 transition-colors">
                      {row.map((cell, ci) => (
                        <td
                          key={ci}
                          className="text-ink-primary px-2.5 py-1.5 align-top font-mono text-[11.5px] tabular-nums"
                        >
                          {cell}
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          );
        case "markdown":
          return <SafePluginMarkdown key={key} content={node.content} />;
        case "button": {
          const isBusy = busyActionId === node.action_id;
          return (
            <Button
              key={key}
              type="button"
              size="sm"
              variant={node.variant === "primary" ? "default" : "outline"}
              disabled={Boolean(busyActionId)}
              onClick={() => onAction?.(node.action_id, node.payload ?? {}, formValues)}
            >
              {isBusy && <OrbitSpinner size={12} />}
              {node.label}
            </Button>
          );
        }
        case "form_input": {
          const inputId = `plugin-field-${key}-${node.field_id}`;
          return (
            <Field key={key} className="gap-1.5">
              <FieldLabel htmlFor={inputId} className="text-ink-secondary text-[12px]">
                {node.label}
              </FieldLabel>
              <Input
                id={inputId}
                value={formValues[node.field_id] ?? ""}
                placeholder={node.placeholder ?? undefined}
                onChange={(e) => {
                  const val = e.target.value;
                  setFormValues((prev) => ({ ...prev, [node.field_id]: val }));
                }}
                className="h-8 text-[12.5px]"
              />
            </Field>
          );
        }
      }
    },
    [busyActionId, formValues, onAction],
  );

  return <div className={cn("flex min-w-0 flex-col gap-2.5", className)}>{renderNode(spec.root, "root", 1)}</div>;
}

export interface PluginSlotCardProps {
  hostId: string;
  pluginId: string;
  title?: string;
  slot: OrbitPluginUiSlot;
  cwd?: string | null;
  mcpOpNames?: readonly string[];
  className?: string;
}

export function PluginSlotCard({
  hostId,
  pluginId,
  title,
  slot,
  cwd = null,
  mcpOpNames,
  className,
}: PluginSlotCardProps) {
  const { t } = useI18n();
  const [spec, setSpec] = useState<OrbitPluginUiSpec | null>(null);
  const [loading, setLoading] = useState(true);
  const [busyActionId, setBusyActionId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [lastOpOutput, setLastOpOutput] = useState<string | null>(null);

  const loadSlot = useCallback(
    async (actionId?: string | null, payload?: unknown) => {
      if (actionId) setBusyActionId(actionId);
      else setLoading(true);
      setError(null);
      try {
        const nextSpec = await orbitIpc.renderPluginUi(hostId, pluginId, slot, {
          cwd,
          actionId: actionId ?? null,
          payload: payload ?? null,
        });
        setSpec(nextSpec);
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setLoading(false);
        setBusyActionId(null);
      }
    },
    [cwd, hostId, pluginId, slot],
  );

  useEffect(() => {
    void loadSlot();
  }, [loadSlot]);

  const handleAction = useCallback(
    async (actionId: string, payload: unknown, formValues: Record<string, string>) => {
      const mergedPayload =
        payload && typeof payload === "object" && !Array.isArray(payload)
          ? { ...(payload as Record<string, unknown>), fields: formValues }
          : { payload, fields: formValues };

      if (mcpOpNames?.includes(actionId)) {
        setBusyActionId(actionId);
        setError(null);
        try {
          const out = await orbitIpc.invokePluginOp(hostId, pluginId, actionId, mergedPayload, cwd);
          setLastOpOutput(typeof out === "string" ? out : JSON.stringify(out, null, 2));
        } catch (err) {
          setError(err instanceof Error ? err.message : String(err));
        } finally {
          setBusyActionId(null);
        }
        return;
      }

      await loadSlot(actionId, mergedPayload);
    },
    [cwd, hostId, loadSlot, mcpOpNames, pluginId],
  );

  return (
    <div
      className={cn(
        "border-border-subtle bg-surface-base flex min-w-0 flex-col gap-2.5 rounded-[12px] border p-3",
        className,
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="text-ink-primary truncate text-[12.5px] font-medium">
          {title || pluginId}
        </span>
        <button
          type="button"
          aria-label={t("common.refresh")}
          disabled={loading || Boolean(busyActionId)}
          onClick={() => void loadSlot()}
          className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-6 cursor-pointer items-center justify-center rounded-md transition-colors disabled:opacity-50"
        >
          <RefreshCw className={cn("size-3.5", loading && "animate-spin")} />
        </button>
      </div>

      {error && (
        <div className="border-signal-danger/30 bg-signal-danger/8 text-signal-danger flex items-start gap-2 rounded-[8px] border px-2.5 py-2 text-[12px]">
          <AlertCircle className="mt-0.5 size-3.5 shrink-0" />
          <span className="min-w-0 break-words">{error}</span>
        </div>
      )}

      {loading && !spec ? (
        <div className="text-ink-muted flex items-center gap-2 py-3 text-[12px]">
          <OrbitSpinner size={13} />
          <span>{t("common.loading")}</span>
        </div>
      ) : spec ? (
        <PluginUiRenderer
          spec={spec}
          busyActionId={busyActionId}
          onAction={(actionId, payload, formValues) =>
            void handleAction(actionId, payload, formValues)
          }
        />
      ) : null}

      {lastOpOutput && (
        <pre className="border-border-subtle bg-surface-recessed text-ink-secondary max-h-36 overflow-auto rounded-[8px] border p-2 font-mono text-[11px]">
          {lastOpOutput}
        </pre>
      )}
    </div>
  );
}
