import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertCircle, Check, Copy, QrCode, RefreshCw, ShieldCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useI18n } from "@/lib/i18n";
import { orbitIpc } from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";

export function PairingQrCard({
  hostId,
  hostName,
  compact = false,
  layout = "stacked",
  title,
  description,
  className,
}: {
  hostId: string;
  hostName: string;
  compact?: boolean;
  layout?: "stacked" | "inline";
  title?: string;
  description?: string;
  className?: string;
}) {
  const { t } = useI18n();
  const [pairingUri, setPairingUri] = useState<string | null>(null);
  const [svgMarkup, setSvgMarkup] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const loadQr = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const uri = await orbitIpc.exportHostPairingUri(hostId);
      const svg = await orbitIpc.renderQrSvg(uri);
      setPairingUri(uri);
      setSvgMarkup(svg);
    } catch (err) {
      setPairingUri(null);
      setSvgMarkup(null);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [hostId]);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError(null);
    void (async () => {
      try {
        const uri = await orbitIpc.exportHostPairingUri(hostId);
        const svg = await orbitIpc.renderQrSvg(uri);
        if (!active) return;
        setPairingUri(uri);
        setSvgMarkup(svg);
      } catch (err) {
        if (!active) return;
        setPairingUri(null);
        setSvgMarkup(null);
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        if (active) setLoading(false);
      }
    })();
    return () => {
      active = false;
    };
  }, [hostId]);

  const svgDataUri = useMemo(() => {
    if (!svgMarkup) return null;
    return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svgMarkup)}`;
  }, [svgMarkup]);

  const handleCopyUri = async () => {
    if (!pairingUri) return;
    try {
      await navigator.clipboard.writeText(pairingUri);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Ignore clipboard permission errors
    }
  };

  const qrBoxSize =
    layout === "inline" ? "size-[124px] p-2.5" : compact ? "size-[168px] p-3" : "size-[204px] p-3";

  const qrPad = (
    <div
      className={cn(
        "border-border-default flex shrink-0 items-center justify-center rounded-[12px] border bg-white shadow-2xs",
        qrBoxSize,
      )}
    >
      {loading ? (
        <div className="flex flex-col items-center gap-2 text-neutral-500">
          <OrbitSpinner size={18} />
          <span className="text-[11px]">{t("settings.pairingQrLoading")}</span>
        </div>
      ) : error || !svgDataUri ? (
        <div className="flex flex-col items-center gap-1.5 px-2 text-center text-neutral-600">
          <AlertCircle className="text-signal-danger size-[18px]" />
          <span className="text-[11px] leading-snug">{t("settings.pairingQrError")}</span>
          <Button
            type="button"
            variant="outline"
            size="xs"
            onClick={() => void loadQr()}
            className="mt-0.5 text-[11px]"
          >
            <RefreshCw className="size-[11px]" />
            <span>{t("common.retry")}</span>
          </Button>
        </div>
      ) : (
        <img
          src={svgDataUri}
          alt={t("settings.pairingQrAlt", { host: hostName })}
          draggable={false}
          className="size-full select-none object-contain [image-rendering:pixelated]"
        />
      )}
    </div>
  );

  if (layout === "inline") {
    return (
      <div
        className={cn(
          "flex flex-col gap-4 px-4 py-4 sm:flex-row sm:items-center sm:justify-between",
          className,
        )}
      >
        <div className="flex min-w-0 flex-1 flex-col gap-3">
          <div>
            {title && <div className="text-ink-primary text-[13px] font-medium">{title}</div>}
            {description && (
              <div className="text-ink-secondary mt-0.5 text-[13px] leading-[1.45]">
                {description}
              </div>
            )}
          </div>
          <div className="text-ink-muted inline-flex items-center gap-1.5 text-[11px]">
            <ShieldCheck className="size-[13px] shrink-0 opacity-80" />
            <span>{t("settings.pairingQrHint")}</span>
          </div>
          {pairingUri && (
            <div className="flex flex-wrap items-center gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => void handleCopyUri()}
                className="text-[13px]"
              >
                {copied ? (
                  <Check className="text-signal-success size-[13px]" />
                ) : (
                  <Copy className="text-ink-muted size-[13px]" />
                )}
                <span>{copied ? t("common.copied") : t("settings.copyUri")}</span>
              </Button>
            </div>
          )}
        </div>
        {qrPad}
      </div>
    );
  }

  return (
    <div className={cn("flex flex-col items-center gap-3.5", className)}>
      {qrPad}

      <div className="flex w-full flex-col items-center gap-2.5 text-center">
        <div className="text-ink-muted inline-flex items-center gap-1.5 text-[11px]">
          <ShieldCheck className="size-[13px] shrink-0 opacity-80" />
          <span>{t("settings.pairingQrHint")}</span>
        </div>

        {pairingUri && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void handleCopyUri()}
            className="text-[13px]"
          >
            {copied ? (
              <Check className="text-signal-success size-[13px]" />
            ) : (
              <Copy className="text-ink-muted size-[13px]" />
            )}
            <span>{copied ? t("common.copied") : t("settings.copyUri")}</span>
          </Button>
        )}
      </div>
    </div>
  );
}

export function PairingQrDialog({
  open,
  onOpenChange,
  hostId,
  hostName,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  hostId: string;
  hostName: string;
}) {
  const { t } = useI18n();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[380px] gap-5 p-5">
        <DialogHeader className="gap-1 pr-6">
          <DialogTitle className="flex items-center gap-2 text-[15px]">
            <QrCode className="text-ink-muted size-[15px] stroke-[1.75]" />
            <span>{t("settings.pairingQrTitle")}</span>
          </DialogTitle>
          <DialogDescription className="text-[13px] leading-[1.45]">
            {t("settings.pairingQrDesc", { host: hostName })}
          </DialogDescription>
        </DialogHeader>

        {open && <PairingQrCard hostId={hostId} hostName={hostName} />}
      </DialogContent>
    </Dialog>
  );
}
