import { defineConfig, mergeConfig } from "vite";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import desktop from "../desktop/vite.config";

export default mergeConfig(
  desktop,
  defineConfig({
    resolve: { dedupe: ["react", "react-dom"] },
    plugins: [
      {
        name: "orbit-offline-shell",
        enforce: "post",
        generateBundle(_, bundle) {
          const logo = readFileSync(
            new URL("../desktop/public/orbit-logo.png", import.meta.url),
          );
          this.emitFile({
            type: "asset",
            fileName: "orbit-logo.png",
            source: logo,
          });
          for (const [fileName, path] of [
            ["icon-512.png", "../desktop/src-tauri/icons/icon.png"],
            [
              "icon-192.png",
              "../android/app/src/main/res/mipmap-xxxhdpi/ic_launcher.png",
            ],
          ])
            this.emitFile({
              type: "asset",
              fileName,
              source: readFileSync(new URL(path, import.meta.url)),
            });
          const assets = [
            ...Object.keys(bundle),
            "orbit-logo.png",
            "icon-512.png",
            "icon-192.png",
            "manifest.webmanifest",
          ].sort();
          const manifest = readFileSync(
            new URL("./public/manifest.webmanifest", import.meta.url),
          );
          const hash = createHash("sha256").update(logo).update(manifest);
          for (const item of Object.values(bundle))
            hash.update(item.type === "chunk" ? item.code : item.source);
          const worker = readFileSync(
            new URL("./public/sw.js", import.meta.url),
            "utf8",
          );
          this.emitFile({
            type: "asset",
            fileName: "sw.js",
            source: worker
              .replace(
                "BUILD_VERSION",
                hash.update(worker).digest("hex").slice(0, 16),
              )
              .replace(
                '["BUILD_ASSETS"]',
                JSON.stringify([...new Set([...assets, "index.html"])]),
              ),
          });
        },
      },
    ],
  }),
);
