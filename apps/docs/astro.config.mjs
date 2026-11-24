import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

export default defineConfig({
  site: "https://danielgaoyy.github.io",
  integrations: [
    starlight({
      title: {
        en: "Orbit",
        "zh-CN": "Orbit",
      },
      description:
        "One Personal Agent. Every Machine. Zero-Knowledge Multi-Host Orchestration for AI Coding Agents.",
      logo: {
        light: "./src/assets/orbit-logo-light.svg",
        dark: "./src/assets/orbit-logo-dark.svg",
        replacesTitle: false,
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
            content: "/images/orbit-desktop-dark.png",
          },
        },
        {
          tag: "meta",
          attrs: {
            name: "theme-color",
            content: "#17181E",
            media: "(prefers-color-scheme: dark)",
          },
        },
        {
          tag: "meta",
          attrs: {
            name: "theme-color",
            content: "#FCFCFE",
            media: "(prefers-color-scheme: light)",
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
