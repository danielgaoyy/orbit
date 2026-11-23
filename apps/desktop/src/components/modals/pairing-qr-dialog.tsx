import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertCircle, Check, Copy, Loader2, QrCode, RefreshCw, ShieldCheck } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { useI18n } from "@/lib/i18n";
import { orbitIpc } from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";

export function PairingQrCard({
  hostId,
  hostName,
  compact = false,
  className,
}: {
  hostId: string;
  hostName: string;
  compact?: boolean;
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

  const qrBoxSize = compact ? "h-[168px] w-[168px]" : "h-[208px] w-[208px]";

  return (
    <div className={cn("flex flex-col items-center gap-3.5", className)}>
      {/* Crisp #FFFFFF quiet-zone pad ensures effortless mobile scanning in Dark & Light mode */}
      <div
        className={cn(
          "flex items-center justify-center rounded-[14px] border border-black/[0.08] bg-white p-3 shadow-2xs dark:border-white/[0.14]",
          qrBoxSize,
        )}
      >
        {loading ? (
          <div className="flex flex-col items-center gap-2 text-zinc-500">
            <Loader2 className="h-5 w-5 animate-spin" />
            <span className="text-[11.5px]">{t("settings.pairingQrLoading")}</span>
          </div>
        ) : error || !svgDataUri ? (
          <div className="flex flex-col items-center gap-2 px-3 text-center text-zinc-600">
            <AlertCircle className="h-5 w-5 text-rose-500" />
            <span className="text-[11.5px] leading-snug">{t("settings.pairingQrError")}</span>
            <button
              type="button"
              onClick={() => void loadQr()}
              className="mt-0.5 inline-flex cursor-pointer items-center gap-1 rounded-md border border-zinc-200 px-2 py-1 text-[11px] font-medium text-zinc-700 hover:bg-zinc-100"
            >
              <RefreshCw className="h-3 w-3" />
              <span>{t("common.retry")}</span>
            </button>
          </div>
        ) : (
          <img
            src={svgDataUri}
            alt={t("settings.pairingQrAlt", { host: hostName })}
            draggable={false}
            className="h-full w-full select-none object-contain [image-rendering:pixelated]"
          />
        )}
      </div>

      <div className="flex w-full flex-col items-center gap-2 text-center">
        <div className="text-muted-foreground inline-flex items-center gap-1.5 text-[11.5px]">
          <ShieldCheck className="h-[13px] w-[13px] shrink-0 opacity-75" />
          <span>{t("settings.pairingQrHint")}</span>
        </div>

        {pairingUri && (
          <button
            type="button"
            onClick={() => void handleCopyUri()}
            className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-3 text-[12px] font-medium transition-colors"
          >
            {copied ? (
              <Check className="h-[13px] w-[13px] text-emerald-500" />
            ) : (
              <Copy className="text-muted-foreground h-[13px] w-[13px]" />
            )}
            <span>{copied ? t("common.copied") : t("settings.copyUri")}</span>
          </button>
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
            <QrCode className="text-muted-foreground h-[16px] w-[16px] stroke-[1.75]" />
            <span>{t("settings.pairingQrTitle")}</span>
          </DialogTitle>
          <DialogDescription className="text-[12.5px] leading-relaxed">
            {t("settings.pairingQrDesc", { host: hostName })}
          </DialogDescription>
        </DialogHeader>

        {open && <PairingQrCard hostId={hostId} hostName={hostName} />}
      </DialogContent>
    </Dialog>
  );
}
