---
title: 跨引擎记忆与 WASM 插件
description: 在 Claude Code、Codex、Antigravity 与 34+ ACP 代理间共享 Markdown + FTS5 + RRF 项目记忆，并通过 wasmi WebAssembly 沙箱安全扩展 Orbit。
---

项目的工程约定、踩坑结论与架构边界应当沉淀在你的代码仓库中，而不是被锁死在某一家 AI 厂商的黑盒云端里。Orbit 将透明的 Markdown 项目记忆与能力隔离的 WASM 插件运行时合二为一。

## 跨 Provider 项目记忆（Markdown-First）

Daemon 按本机规范化工作目录隔离项目记忆，每条记忆直接落盘为人类可读、可用任意编辑器修改甚至提交 Git 的 Markdown 文件：

```text
<project-root>/
└── .orbit/
    └── memory/
        ├── noise-rekey-invariant.md
        └── worktree-handoff-rules.md
```

文件顶部的 YAML frontmatter 记录 `id`、分类、审核状态、置信度、绑定的源码相对路径（`bound_paths`）、标签、来源与时间戳。

### 离线混合检索（FTS5 + 256 维向量 RRF）

- **默认完全离线运行**：SQLite 结合 **FTS5 BM25 词法打分**与内置 **256 维特征哈希余弦相似度**（同时覆盖英文词、代码标识符与中文字符片段），经 **Reciprocal Rank Fusion（`k = 60`）** 融合排序，并结合审核状态、近期更新与访问频次微调。
- **自动去重与源码变更失效感知**：新建记忆与已有本地记录余弦相似度高于 `0.92` 时自动合并。防抖 `notify` 文件监听器在 `bound_paths` 关联源码发生变化时，会自动把对应已确认记忆标记为 `needs_revalidation` 并写回 Markdown。
- **确认后才参与首轮注入**：AI 自动生成的记忆默认处于 `hypothesis` 状态；仅经人工审核或显式 `confirm` 确认的记录，才会在 Provider 首轮输入时受预算控制地注入 `<orbit_project_memory>`。
- **历史记忆兼容**：既有 `~/.hermes/memories/*.md` 会按 `workspace_root` 自动以只读候选记忆（`hypothesis`）形式导入。

### 可选本地或远端 Embedding 增强

如需叠加 OpenAI 兼容的向量端点（例如本机 Ollama 的 `nomic-embed-text`），在 `$ORBIT_HOME/config.json` 中配置并重启 Daemon：

```json
{
  "memory_embedding": {
    "provider": "open_ai_compatible",
    "endpoint_url": "http://127.0.0.1:11434/v1/embeddings",
    "model": "nomic-embed-text",
    "auto_inject_budget_tokens": 1200
  }
}
```

密钥从 `api_key_env` 指定的环境变量读取（本地无鉴权端点可省略）；超时上限严格设为 3 秒，端点不可用时自动无缝回退至内置离线向量。

### 统一 Core 工具：`orbit_memory`

运行在 Orbit 宿主机上的全部引擎（Claude Code、Codex、Antigravity、Pi 及 34+ ACP 代理）共用同一个按当前工作目录隔离的 `orbit_memory` 工具，提供四个动作：

- `recall`（支持 `include_unconfirmed: true` 检索待核验假设）
- `remember`（新建 `hypothesis` 记忆条目）
- `confirm`（确认记忆使其参与首轮自动注入）
- `forget`（从磁盘与索引中移除过期记忆）

已配对客户端同样可通过 `ListProjectMemories`、`UpsertProjectMemory`、`ConfirmProjectMemory`、`DeleteProjectMemory` 与 `RecallProjectMemories` 直接管理记忆。

## `wasmi` 沙箱 WASM 插件生态

传统 IDE 插件直接以当前用户身份执行任意原生代码。Orbit 将插件运行在纯 Rust 嵌入式的 **`wasmi` WebAssembly 沙箱**中：

- **零原生提权风险**：插件运行在内存与计算配额（fuel）受限的确定性 WASM 实例内，仅能调用显式授权的能力接口，无法私自拉起任意子进程或越界读取宿主凭据。
- **统一 `orbit_plugin` 工具面**：插件注册的强类型工具与钩子对所有已接入的 AI 编码引擎一致可见。
- **跨端声明式 UI**：插件无需分别为 React、SwiftUI、Compose 和 ArkTS 编写四套前端，只需输出声明式 UI 描述树（指标卡、表格、操作按钮与表单），即可在桌面端、Web WASM 与三大移动端原生渲染。
- **SEP-1865 MCP Apps 桥接**：桌面端 Artifacts 标签页同时支持遵循 SEP-1865 协议的 MCP Server `ui://` 交互资源（`ReadMcpResource`）与关联工具调用（`CallArtifactMcpTool`）。
