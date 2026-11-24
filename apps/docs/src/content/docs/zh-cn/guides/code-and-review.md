---
title: 代码编辑、时间回溯与 Orbit Patrol
description: 使用 CodeMirror 6 + LSP + FIM 幽灵补全直接编辑远端文件，一键回溯会话与文件状态，可视化解决三方 Git 冲突，并运行两阶段 Orbit Patrol PR 审阅。
---

把任务交给 AI 代理只是研发闭环的前半程，**高效验证、微调、回溯并安全合入代码**才是决定工程质量的关键。Orbit 将完整的代码编辑与 Git 审阅闭环直接融入对话舞台旁。

## CodeMirror 6 + LSP + FIM 幽灵补全

按下 `⌘P`（`Ctrl+P`）快速打开或点击时间线中任意文件路径，即可在右侧面板浏览与编辑本地或远端工作区代码：

- **零卡顿远端打开体验**：结合 Rust 250 行前缀语法高亮、悬停/按下意图预取（Promise 去重）与 stale-while-revalidate 缓存，翻阅远端代码如本地般丝滑。
- **完整 Language Server Protocol（LSP）支持**：直接连接远端主机的语言服务器，提供工作区符号搜索、类型悬停、跳转定义、引用查找、内联诊断、Code Action 与跨文件安全重命名（`lsp_hover`、`lsp_goto_definition`、`lsp_references`、`lsp_rename`、`lsp_apply_workspace_edit`）。
- **FIM（Fill-in-the-Middle）幽灵文本预测**：低延迟行内编辑预测以克制的半透明幽灵文本直接呈现在 CodeMirror 6 中——按 `Tab` 接受、`Esc` 忽略，绝不弹出遮挡视线的浮窗。

## 交互式会话与文件回溯（Rewind）

当 Agent 沿着错误的方向修改了多个文件时，不再需要手动 `git checkout` 清理残局：

- 在时间线任意历史轮次触发 **回溯（`rewind_agent`）**。
- Orbit 同步将**对话时间线**与**该轮次之后发生变更的工作区文件**一并恢复至检查点（返回 `restored_files` 数量与恢复的提示词草稿），助你迅速换一条更优路径继续推进。
- 若希望保留当前尝试并同时验证另一套方案，可使用 **分叉至 Worktree（`orbit fork --worktree`）**，在任意 `seq` 节点把对话与代码同步克隆到独立 Git Worktree。

## 完整 Git 写入闭环与 Worktree 移交

按下 `⌘D`（`Ctrl+D`）呼出统一 **Review** 面板，支持在 **Working Tree（工作区）** 与 **Session Turns（本会话改动）** 间一键切换：

- **单 Hunk 精细操作**：按单个代码块或整文件暂存、取消暂存或丢弃修改；在任意代码行留下行内批注并一键回传给当前 Agent 修复。
- **Stash、Rebase 与三方冲突解决器**：内置 Git 储藏管理（`git_stash_*`）、变基状态控制（`git_rebase_*`）与可视化逐块三方冲突解决（`git_list_conflicts` / `git_resolve_conflict`），告别手改冲突标记。
- **双向 Worktree Handoff**：支持自动创建安全备份 ref 与冲突感知的 `dry_run` / `apply` / `move` 策略，把并行子代理在独立工作树中完成的改动安全合回主工作区。
- **提交、推送与 PR 检查**：内置确定性或 AI 辅助提交信息生成、分支推送、创建 PR 及实时 GitHub CI 检查状态条。

## 两阶段 Orbit Patrol PR 审阅

在把多个并行 Agent 产出的代码合入主干前，使用 **Orbit Patrol** 执行高信噪比自动化审阅：

1. **第一阶段 — 自动化广度巡检与不变量筛查**：扫描变更 Hunk、关联文件、LSP 诊断、测试输出及仓库规则（`AGENTS.md` / 路径规则），快速定位潜在回归、安全边界违背与跨端/平行视图漏改。
2. **第二阶段 — 深度核验与可执行建议**：围绕候选问题回溯上下文调用点与运行时不变量，过滤低质量误报，输出精准锚定到代码行的修复建议，或一键派发给专职子代理自动修补并复验。
