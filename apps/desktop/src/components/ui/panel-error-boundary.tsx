import * as React from "react";
import { AlertTriangle, Check, ChevronDown, ChevronRight, Copy, RotateCw } from "lucide-react";
import { subscribeLocale, t } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export interface PanelErrorBoundaryProps {
  /** Human-readable panel name displayed in the fallback card */
  panelName: string;
  /** Changing this value automatically clears a previously trapped error on persistent panels */
  resetKey?: string | number | null;
  /** Compact padding for narrower panes such as sidebar or slim tabs */
  compact?: boolean;
  /** Optional className applied to the fallback wrapper */
  className?: string;
  children: React.ReactNode;
}

interface PanelErrorBoundaryState {
  hasError: boolean;
  error: unknown;
  componentStack: string | null;
  detailsOpen: boolean;
  copied: boolean;
}

const INITIAL_STATE: PanelErrorBoundaryState = {
  hasError: false,
  error: null,
  componentStack: null,
  detailsOpen: false,
  copied: false,
};

function formatBoundaryError(
  error: unknown,
  componentStack: string | null,
): { message: string; details: string } {
  let name = "Error";
  let rawMessage = t("errorBoundary.unexpectedError");
  let stack = "";

  if (error instanceof Error) {
    name = error.name || "Error";
    if (error.message.trim()) rawMessage = error.message.trim();
    stack = error.stack?.trim() ?? "";
  } else if (typeof error === "string" && error.trim()) {
    rawMessage = error.trim();
  } else if (error != null) {
    try {
      const serialized = String(error).trim();
      if (serialized) rawMessage = serialized;
    } catch {
      // Keep default fallback text
    }
  }

  const message = rawMessage.length > 220 ? `${rawMessage.slice(0, 217)}…` : rawMessage;
  const parts: string[] = [stack || `${name}: ${rawMessage}`];
  const trimmedStack = componentStack?.trim();
  if (trimmedStack) parts.push(`Component stack:\n${trimmedStack}`);

  return { message, details: parts.join("\n\n") };
}

/**
 * Restrained per-panel React Error Boundary.
 * Isolates rendering faults to an individual workspace region (sidebar, stage,
 * side-panel tab, settings, automations) so one bad payload never whitescreens
 * the entire application. Automatically clears an existing error when `resetKey` changes.
 */
export class PanelErrorBoundary extends React.Component<
  PanelErrorBoundaryProps,
  PanelErrorBoundaryState
> {
  private copiedTimer: number | null = null;
  private unsubscribeLocale: (() => void) | null = null;

  constructor(props: PanelErrorBoundaryProps) {
    super(props);
    this.state = INITIAL_STATE;
  }

  componentDidMount(): void {
    this.unsubscribeLocale = subscribeLocale(() => {
      if (this.state.hasError) {
        this.forceUpdate();
      }
    });
  }

  static getDerivedStateFromError(error: unknown): Partial<PanelErrorBoundaryState> {
    return {
      hasError: true,
      error,
      detailsOpen: false,
      copied: false,
    };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo): void {
    this.setState({ componentStack: info.componentStack ?? null });
    console.error(`[Orbit PanelErrorBoundary:${this.props.panelName}]`, error, info.componentStack);
  }

  componentDidUpdate(
    prevProps: PanelErrorBoundaryProps,
    prevState: PanelErrorBoundaryState,
  ): void {
    if (
      prevState.hasError &&
      this.state.hasError &&
      !Object.is(prevProps.resetKey, this.props.resetKey)
    ) {
      this.clearCopiedTimer();
      this.setState(INITIAL_STATE);
    }
  }

  componentWillUnmount(): void {
    this.clearCopiedTimer();
    this.unsubscribeLocale?.();
    this.unsubscribeLocale = null;
  }

  private clearCopiedTimer(): void {
    if (this.copiedTimer !== null && typeof window !== "undefined") {
      window.clearTimeout(this.copiedTimer);
      this.copiedTimer = null;
    }
  }

  private handleReload = (): void => {
    this.clearCopiedTimer();
    this.setState(INITIAL_STATE);
  };

  private handleCopyDetails = (details: string): void => {
    if (typeof navigator === "undefined" || !navigator.clipboard) return;
    void navigator.clipboard
      .writeText(`${this.props.panelName}\n\n${details}`)
      .then(() => {
        this.clearCopiedTimer();
        this.setState({ copied: true });
        if (typeof window !== "undefined") {
          this.copiedTimer = window.setTimeout(() => {
            this.copiedTimer = null;
            this.setState({ copied: false });
          }, 1500);
        }
      })
      .catch(() => undefined);
  };

  render(): React.ReactNode {
    if (!this.state.hasError) {
      return this.props.children;
    }

    const summary = formatBoundaryError(this.state.error, this.state.componentStack);
    const { panelName, compact = false, className } = this.props;
    const { detailsOpen, copied } = this.state;

    return (
      <div
        role="alert"
        data-slot="panel-error-boundary"
        className={cn(
          "flex h-full min-h-[160px] w-full flex-1 items-center justify-center overflow-auto",
          compact ? "p-3" : "p-6",
          className,
        )}
      >
        <div
          className={cn(
            "border-border bg-elevated/80 w-full rounded-[13px] border shadow-2xs",
            compact ? "max-w-[260px] space-y-3 p-3.5" : "max-w-[380px] space-y-3.5 p-4.5",
          )}
        >
          <div className="flex items-start gap-2.5">
            <div className="border-border bg-muted/75 text-muted-foreground mt-0.5 flex size-[26px] shrink-0 items-center justify-center rounded-lg border">
              <AlertTriangle className="size-[13.5px] stroke-[1.85]" />
            </div>
            <div className="min-w-0 flex-1 space-y-1">
              <h3 className="text-foreground text-[13px] leading-snug font-semibold tracking-[-0.01em]">
                {t("errorBoundary.couldNotRender", { panel: panelName })}
              </h3>
              <p className="text-muted-foreground text-[12px] leading-[1.5] break-words">
                {summary.message}
              </p>
            </div>
          </div>

          <div className="space-y-1.5">
            <button
              type="button"
              onClick={() => this.setState((s) => ({ detailsOpen: !s.detailsOpen }))}
              className="text-muted-foreground hover:text-foreground inline-flex cursor-pointer items-center gap-1 text-[11.5px] font-medium transition-colors"
            >
              {detailsOpen ? (
                <ChevronDown className="size-[13px]" />
              ) : (
                <ChevronRight className="size-[13px]" />
              )}
              <span>{t("errorBoundary.technicalDetails")}</span>
            </button>

            {detailsOpen && (
              <pre className="selectable-text border-border/80 bg-canvas/85 text-muted-foreground max-h-36 overflow-auto rounded-lg border p-2.5 font-mono text-[10.5px] leading-[1.45] whitespace-pre-wrap break-words">
                {summary.details}
              </pre>
            )}
          </div>

          <div className="flex flex-wrap items-center justify-between gap-2 pt-0.5">
            <button
              type="button"
              onClick={this.handleReload}
              className="bg-foreground text-background inline-flex h-[28px] cursor-pointer items-center gap-1.5 rounded-lg px-2.5 text-[12px] font-medium transition-opacity hover:opacity-90"
            >
              <RotateCw className="size-[12px]" />
              <span>{t("errorBoundary.reloadPanel")}</span>
            </button>

            <button
              type="button"
              onClick={() => this.handleCopyDetails(summary.details)}
              className="border-border bg-background text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-[28px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[11.5px] font-medium transition-colors"
            >
              {copied ? (
                <>
                  <Check className="size-[12px] text-emerald-500" />
                  <span>{t("errorBoundary.copied")}</span>
                </>
              ) : (
                <>
                  <Copy className="size-[12px]" />
                  <span>{t("errorBoundary.copyDetails")}</span>
                </>
              )}
            </button>
          </div>
        </div>
      </div>
    );
  }
}
