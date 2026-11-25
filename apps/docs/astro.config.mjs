import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

const docsSite = (process.env.ORBIT_DOCS_SITE || "https://danielgaoyy.github.io").replace(
  /\/+$/,
  "",
);
const rawBase = process.env.ORBIT_DOCS_BASE ?? "/orbit";
const docsBase =
  rawBase === "/" || rawBase === ""
    ? "/"
    : `/${rawBase.replace(/^\/+|\/+$/g, "")}`;
const basePrefix = docsBase === "/" ? "" : docsBase;

export default defineConfig({
  site: docsSite,
  base: docsBase,
  trailingSlash: "always",
  devToolbar: {
    enabled: false,
  },
  integrations: [
    starlight({
      title: {
        en: "Orbit",
        "zh-CN": "Orbit",
      },
      description:
        "Pure-Rust multi-host control plane for Claude Code, Codex, and 34+ coding agents with a persistent Personal Agent and zero-knowledge Noise E2EE.",
      logo: {
        src: "./src/assets/orbit-logo-dark.svg",
        replacesTitle: false,
      },
      components: {
        Head: "./src/components/OrbitSeoHead.astro",
        ThemeProvider: "./src/components/DarkThemeProvider.astro",
        ThemeSelect: "./src/components/EmptyThemeSelect.astro",
        Header: "./src/components/OrbitDocsHeader.astro",
        PageTitle: "./src/components/OrbitDocsTitle.astro",
      },
      expressiveCode: {
        themes: ["github-dark-default"],
        styleOverrides: {
          borderRadius: "10px",
          borderWidth: "1px",
          borderColor: "rgba(255, 255, 255, 0.1)",
          codeBackground: "#0d0e12",
          codeFontFamily:
            '"JetBrains Mono", "SF Mono", ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
          codeFontSize: "14px",
          codeLineHeight: "1.62",
          codePaddingBlock: "0.95rem",
          codePaddingInline: "1.15rem",
          uiFontSize: "14px",
          frames: {
            frameBoxShadowCssValue: "none",
            editorBackground: "#0d0e12",
            editorTabBarBackground: "#13141a",
            editorTabBarBorderBottomColor: "rgba(255, 255, 255, 0.072)",
            terminalBackground: "#0d0e12",
            terminalTitlebarBackground: "#13141a",
            terminalTitlebarBorderBottomColor: "rgba(255, 255, 255, 0.072)",
            terminalTitlebarDotsForeground: "transparent",
            terminalTitlebarDotsOpacity: "0",
            inlineButtonBackground: "#16171c",
            inlineButtonBorder: "rgba(255, 255, 255, 0.11)",
            inlineButtonForeground: "#9a9da8",
            inlineButtonBackgroundHoverOrFocusOpacity: "1",
            tooltipSuccessBackground: "#162b24",
            tooltipSuccessForeground: "#34d399",
          },
        },
      },
      favicon: "/favicon.svg",
      defaultLocale: "root",
      locales: {
        root: {
          label: "English",
          lang: "en",
        },
        "zh-cn": {
          label: "简体中文",
          lang: "zh-CN",
        },
      },
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/danielgaoyy/orbit",
        },
      ],
      customCss: ["./src/styles/orbit-theme.css"],
      head: [
        {
          tag: "meta",
          attrs: {
            property: "og:image",
            content: `${docsSite}${basePrefix}/images/orbit-desktop-dark.png`,
          },
        },
        {
          tag: "meta",
          attrs: {
            name: "theme-color",
            content: "#09090b",
          },
        },
      ],
      sidebar: [
        {
          label: "Getting Started",
          translations: { "zh-CN": "快速开始" },
          items: [
            {
              label: "Quickstart",
              translations: { "zh-CN": "五分钟上手" },
              slug: "getting-started/quickstart",
            },
          ],
        },
        {
          label: "Core Capabilities",
          translations: { "zh-CN": "核心能力指南" },
          items: [
            {
              label: "Personal Agent & IM Dispatch",
              translations: { "zh-CN": "Personal Agent 与全局调度" },
              slug: "guides/personal-agent",
            },
            {
              label: "Multi-Host Fleet & E2EE Relay",
              translations: { "zh-CN": "多主机舰队与零知识中继" },
              slug: "guides/multi-host-fleet",
            },
            {
              label: "Security, Sandbox & Audit Chain",
              translations: { "zh-CN": "安全边界、内核沙箱与审计链" },
              slug: "guides/security-sandbox",
            },
            {
              label: "Cross-Provider Memory & WASM Plugins",
              translations: { "zh-CN": "跨引擎记忆与 WASM 插件" },
              slug: "guides/memory-and-plugins",
            },
            {
              label: "Five Native Clients & Offline Voice",
              translations: { "zh-CN": "五端原生客户端与离线语音" },
              slug: "guides/clients-and-voice",
            },
            {
              label: "Code, Git Rewind & Orbit Patrol",
              translations: { "zh-CN": "代码编辑、时间回溯与 Orbit Patrol" },
              slug: "guides/code-and-review",
            },
          ],
        },
        {
          label: "Reference",
          translations: { "zh-CN": "架构与参考" },
          items: [
            {
              label: "CLI, Architecture & Telemetry",
              translations: { "zh-CN": "CLI 命令、五端架构与可观测性" },
              slug: "reference/cli-and-architecture",
            },
          ],
        },
      ],
      pagefind: true,
      lastUpdated: false,
      pagination: true,
    }),
  ],
});
