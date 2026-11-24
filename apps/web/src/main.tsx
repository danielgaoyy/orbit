import { useState, type ChangeEvent, type FormEvent } from "react";
import { createRoot } from "react-dom/client";
import jsQR from "jsqr";
import App from "@/App";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { installGlobalAnimationSync } from "@/lib/animation-sync";
import { installWebBackend } from "@/lib/orbit-ipc";
import { WasmWebBackend } from "@/lib/wasm-web-backend";
import { t } from "@/lib/i18n";
import init, * as bindings from "./generated/orbit_client_wasm";
import "./shell.css";
import { markOnboardingCompleted } from "@/state/store-helpers";

function Pairing({ backend }: { backend: WasmWebBackend }) {
  const [ready, setReady] = useState(backend.paired);
  const [uri, setUri] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function pair(event: FormEvent) {
    event.preventDefault();
    setError("");
    setBusy(true);
    try {
      await backend.pair(uri);
      setUri("");
      setReady(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function readQR(event: ChangeEvent<HTMLInputElement>) {
    const file = event.currentTarget.files?.[0];
    if (!file) return;
    setError("");
    try {
      const image = await createImageBitmap(file);
      try {
        const scale = Math.min(1, 1600 / Math.max(image.width, image.height));
        const canvas = document.createElement("canvas");
        canvas.width = Math.max(1, Math.round(image.width * scale));
        canvas.height = Math.max(1, Math.round(image.height * scale));
        const ctx = canvas.getContext("2d");
        if (!ctx) throw new Error(t("web.qrError"));
        ctx.drawImage(image, 0, 0, canvas.width, canvas.height);
        const pixels = ctx.getImageData(0, 0, canvas.width, canvas.height);
        const code = jsQR(pixels.data, pixels.width, pixels.height);
        if (!code?.data.startsWith("orbit://pair/"))
          throw new Error(t("web.qrError"));
        setUri(code.data);
      } finally {
        image.close();
      }
    } catch (e) {
      setError(String(e));
    }
  }
  if (ready) return <App />;
  return (
    <main className="bg-surface-base text-ink-primary grid min-h-dvh place-items-center p-6">
      <form
        onSubmit={pair}
        className="border-border-subtle bg-surface-raised flex w-full max-w-sm flex-col gap-4 rounded-xl border p-6"
      >
        <img src="./orbit-logo.png" width="40" height="40" alt="Orbit" />
        <h1 className="text-xl font-semibold">{t("web.pairTitle")}</h1>
        <p className="text-ink-secondary">{t("web.pairHelp")}</p>
        <label>
          {t("web.uri")}
          <Input
            type="password"
            autoComplete="off"
            spellCheck={false}
            autoCapitalize="none"
            required
            value={uri}
            onChange={(e) => setUri(e.target.value)}
            placeholder="orbit://pair/…"
          />
        </label>
        <label>
          {t("web.qr")}
          <Input type="file" accept="image/*" onChange={readQR} />
        </label>
        {error && (
          <p role="alert" className="text-signal-danger">
            {error}
          </p>
        )}
        <Button type="submit" disabled={busy || !uri.trim()}>
          {busy ? t("web.connecting") : t("web.connect")}
        </Button>
      </form>
    </main>
  );
}

const root = createRoot(document.getElementById("root")!);
async function start() {
  await init();
  const backend = await WasmWebBackend.open(bindings);
  installWebBackend(backend);
  markOnboardingCompleted();
  installGlobalAnimationSync();
  root.render(<Pairing backend={backend} />);
  if (import.meta.env.PROD && "serviceWorker" in navigator)
    await navigator.serviceWorker.register("./sw.js");
}
void start().catch((e) =>
  root.render(
    <main role="alert">
      {t("web.startError")} {String(e)}
    </main>,
  ),
);
