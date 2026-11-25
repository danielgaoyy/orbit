export type LandingLang = "en" | "zh-CN";

export const ORBIT_VERSION = "v0.11.0";
export const ORBIT_VERSION_SHORT = "v0.11";

export interface AgentPill {
  name: string;
  mark: string;
  row: 1 | 2 | 3;
}

export interface PaWorker {
  agent: string;
  host: string;
  stage: string;
  reason: string;
  result: string;
  tone: "cyan" | "ok";
}

export interface PaScenario {
  id: "feature" | "incident" | "refactor";
  label: string;
  channel: string;
  userPrompt: string;
  hubSteps: string[];
  workers: PaWorker[];
  returnedSummary: string;
  gateCmd: string;
}

export interface BentoFeature {
  id: "shim" | "im" | "worktree" | "sandbox" | "patrol" | "e2ee";
  span: "span-4" | "span-3" | "span-2";
  eyebrow: string;
  title: string;
  desc: string;
  hrefSlug: string;
}

export interface TestimonialItem {
  quote: string;
  role: string;
  setup: string;
  tag: string;
  avatarBg: string;
  avatarAccent: string;
  avatarShape: 0 | 1 | 2 | 3;
  row: 1 | 2;
}

export interface CompareRow {
  capability: string;
  orbit: string;
  others: string;
}

export interface FaqItem {
  q: string;
  a: string;
}

export interface LandingCopy {
  nav: {
    docs: string;
    changelog: string;
    community: string;
    communityItems: {
      id: "discord" | "x" | "reddit" | "feishu" | "xiaohongshu" | "wechat";
      label: string;
      href: string;
    }[];
    download: string;
    langCurrentCode: string;
    enHref: string;
    zhHref: string;
    starFallback: string;
  };
  hero: {
    pillTag: string;
    pillText: string;
    title: string;
    subtitle: string;
    dlNeutral: string;
    dlMac: string;
    dlWin: string;
    dlLinux: string;
    dlMobile: string;
    viewGithub: string;
    metaLine: string;
    alsoFor: string;
  };
  pa: {
    title: string;
    subtitle: string;
    ctaPrimary: string;
    ctaSecondary: string;
    colYou: string;
    colHub: string;
    colFleet: string;
    hubTagline: string;
    returnBadge: string;
    youChannels: string[];
    approveBtn: string;
    denyBtn: string;
    approvedNote: string;
    deniedNote: string;
    resetBtn: string;
    scenarios: PaScenario[];
  };
  agents: {
    title: string;
    subtitle: string;
    anyOther: string;
    migrateHint: string;
    list: AgentPill[];
  };
  devices: {
    title: string;
    subtitle: string;
    cards: {
      img: string;
      alt: string;
      title: string;
      sub: string;
      center?: boolean;
    }[];
    platformStrip: string;
    alsoLink: string;
  };
  bento: {
    title: string;
    subtitle: string;
    items: BentoFeature[];
  };
  testimonials: {
    title: string;
    subtitle: string;
    items: TestimonialItem[];
  };
  compare: {
    title: string;
    subtitle: string;
    colCap: string;
    colOrbit: string;
    colOthers: string;
    rows: CompareRow[];
  };
  faq: {
    title: string;
    subtitle: string;
    items: FaqItem[];
  };
  cta: {
    title: string;
    subtitle: string;
    readDocs: string;
    alsoFor: string;
  };
  footer: {
    blurb: string;
    cols: { title: string; links: { label: string; href: string; external?: boolean }[] }[];
    copyright: string;
    rightNote: string;
  };
}

const SHARED_AGENTS: AgentPill[] = [
  // Row 1
  { name: "Claude Code", mark: "CC", row: 1 },
  { name: "Codex", mark: "CX", row: 1 },
  { name: "Antigravity", mark: "AG", row: 1 },
  { name: "Gemini CLI", mark: "GM", row: 1 },
  { name: "OpenCode", mark: "OC", row: 1 },
  { name: "Cursor", mark: "CR", row: 1 },
  { name: "GitHub Copilot", mark: "GH", row: 1 },
  { name: "Amp", mark: "AM", row: 1 },
  // Row 2
  { name: "Pi", mark: "PI", row: 2 },
  { name: "Cline", mark: "CL", row: 2 },
  { name: "Goose", mark: "GS", row: 2 },
  { name: "Aider", mark: "AD", row: 2 },
  { name: "Continue", mark: "CN", row: 2 },
  { name: "Qwen Code", mark: "QW", row: 2 },
  { name: "Kiro", mark: "KR", row: 2 },
  { name: "Auggie", mark: "AU", row: 2 },
  // Row 3
  { name: "Mistral Vibe", mark: "MV", row: 3 },
  { name: "Kimi", mark: "KM", row: 3 },
  { name: "Droid", mark: "DR", row: 3 },
  { name: "Kilocode", mark: "KC", row: 3 },
  { name: "Charm", mark: "CH", row: 3 },
  { name: "Codebuff", mark: "CB", row: 3 },
  { name: "Rovo Dev", mark: "RD", row: 3 },
];

const DISCUSSIONS_URL = "https://github.com/danielgaoyy/orbit/discussions";

export function getLandingCopy(lang: LandingLang, base: string, prefix: string): LandingCopy {
  const isZh = lang === "zh-CN";
  const enHref = `${base}/`;
  const zhHref = `${base}/zh-cn/`;

  if (isZh) {
    return {
      nav: {
        docs: "文档",
        changelog: "更新日志",
        community: "社区",
        communityItems: [
          { id: "feishu", label: "飞书群", href: DISCUSSIONS_URL },
          { id: "xiaohongshu", label: "小红书", href: DISCUSSIONS_URL },
          { id: "wechat", label: "微信公众号", href: DISCUSSIONS_URL },
        ],
        download: "下载",
        langCurrentCode: "中文",
        enHref,
        zhHref,
        starFallback: "Star",
      },
      hero: {
        pillTag: ORBIT_VERSION_SHORT,
        pillText: "开源纯 Rust 多主机控制平面",
        title: "多主机 Agent 控制平面",
        subtitle:
          "一个 Personal Agent 统一驱动你所有 Mac、Linux 与 GPU 服务器上的 Claude Code、Codex 与 Gemini CLI，高危操作在手机或 Slack 一键拍板。",
        dlNeutral: "下载 Orbit",
        dlMac: "下载 Mac 版",
        dlWin: "下载 Windows 版",
        dlLinux: "下载 Linux 版",
        dlMobile: "查看全平台下载",
        viewGithub: "在 GitHub 查看",
        metaLine: "MIT / Apache-2.0 免费开源 · 直接复用现有 Claude / Codex 订阅",
        alsoFor: "支持 Apple Silicon · Windows · Linux · iOS · Android · HarmonyOS",
      },
      pa: {
        title: "一个 Personal Agent，统筹全部主机",
        subtitle:
          "你始终只需要对它说话。它会自动拆解目标、调度最合适的专长 Agent 跨机协作接力，只把最终结果和关键确认交回给你。",
        ctaPrimary: "了解 Personal Agent",
        ctaSecondary: "阅读文档",
        colYou: "1 · 你（唯一交互入口）",
        colHub: "2 · PERSONAL AGENT",
        colFleet: "3 · 跨主机专长 AGENT 协作",
        hubTagline: "拆解任务 · 接力调度 · 汇总交付",
        returnBadge: "汇总结果交回给你",
        youChannels: ["Desktop", "Mobile", "Slack", "Discord", "飞书"],
        approveBtn: "批准执行",
        denyBtn: "拒绝",
        approvedNote: "✓ 已批准（118ms），Personal Agent 自动完成最终发布",
        deniedNote: "✕ 已拒绝，工作区保持原状",
        resetBtn: "重置",
        scenarios: [
          {
            id: "feature",
            label: "发版接力",
            channel: "来自 Slack DM",
            userPrompt: "补齐 Noise 密钥轮换并跑完 Cargo E2E，做一轮安全审码后准备发 v0.11.0。",
            hubSteps: [
              "1. 拆解为编码测试、安全审码与产物验收三阶段",
              "2. 在独立 Worktree 串联 Codex → Claude Code → Antigravity",
              "3. 汇总三端结果，仅将推送 Tag 的不可逆动作交你确认",
            ],
            workers: [
              {
                agent: "Codex",
                host: "linux-build-01",
                stage: "01 · 编码实现与全量单测",
                reason: "高并发构建机 · 独立 Git Worktree",
                result: "✓ 补丁落地 · 1,061 项单测全绿 (4.18s)",
                tone: "ok",
              },
              {
                agent: "Claude Code",
                host: "Mac Studio",
                stage: "02 · 深度安全审码与加固",
                reason: "长上下文代码推理 · 审阅 Codex 产出",
                result: "✓ 发现并补齐 X25519 低阶点校验",
                tone: "ok",
              },
              {
                agent: "Antigravity",
                host: "gpu-h100",
                stage: "03 · WASM 与 Patrol 终检",
                reason: "端到端多端验证 · 不可变 Blob 复核",
                result: "✓ WASM 握手通过 · 0 阻断缺陷",
                tone: "cyan",
              },
            ],
            returnedSummary:
              "全部三阶段已完成，1,061 项测试全绿并加固 1 处边界校验。推送生产 Tag 等待你拍板：",
            gateCmd: "git push origin v0.11.0  # linux-build-01",
          },
          {
            id: "incident",
            label: "安全修复",
            channel: "来自 iOS 语音",
            userPrompt: "查一下 Android 深度链接越权风险，写好修复和回归单测，再做一次安全复核。",
            hubSteps: [
              "1. 派 Claude Code 追踪 SyncLink 越权根因",
              "2. 交接给 Codex 在 Linux 隔离 Worktree 落地修复与单测",
              "3. 交由 Orbit Patrol 对抗复核后汇总交回给你",
            ],
            workers: [
              {
                agent: "Claude Code",
                host: "Mac Studio",
                stage: "01 · 跨仓库根因追踪",
                reason: "深度权限边界与 Intent 链路分析",
                result: "✓ 定位 2 处未受保护的外部入口",
                tone: "ok",
              },
              {
                agent: "Codex",
                host: "linux-build-01",
                stage: "02 · 修复实现与回归测试",
                reason: "独立 Worktree 补丁与 Gradle 回归",
                result: "✓ 补齐确认弹窗 · 64 项单测全绿",
                tone: "ok",
              },
              {
                agent: "Orbit Patrol",
                host: "gpu-h100",
                stage: "03 · 二次对抗安全复核",
                reason: "不可变 Git Blob 无绕过路径校验",
                result: "✓ 0 遗留风险 · 可创建 Hotfix PR",
                tone: "cyan",
              },
            ],
            returnedSummary:
              "根因定位、修复补丁与 64 项单测均已通过，安全复核无绕过路径。确认后自动创建 Hotfix PR：",
            gateCmd: "gh pr create --base main --title fix(android): guard sync deep-link",
          },
          {
            id: "refactor",
            label: "跨端重构",
            channel: "来自 Desktop",
            userPrompt: "把五端认证状态统一收敛到 Rust Core，更新各端绑定并回放老版本协议兼容。",
            hubSteps: [
              "1. Claude Code 重构共享 Rust 认证状态机",
              "2. Antigravity 同步升级 Swift / Kotlin / ArkTS / WASM",
              "3. Codex 回放 v0.9–v0.11 加密帧验证零回归",
            ],
            workers: [
              {
                agent: "Claude Code",
                host: "Mac Studio",
                stage: "01 · Rust Core 架构重构",
                reason: "零 unsafe 共享状态机设计",
                result: "✓ orbit-client-core · clippy 全绿",
                tone: "ok",
              },
              {
                agent: "Antigravity",
                host: "gpu-h100",
                stage: "02 · 五端 FFI 联动升级",
                reason: "多端绑定与界面冒烟并行验证",
                result: "✓ 5 大原生客户端全部编译通过",
                tone: "ok",
              },
              {
                agent: "Codex",
                host: "linux-build-01",
                stage: "03 · 线协议兼容性回放",
                reason: "历史版本加密帧自动化压测",
                result: "✓ 100% 向后兼容 · 0 回归",
                tone: "cyan",
              },
            ],
            returnedSummary:
              "五端绑定更新与老版本线协议兼容压测全部通过。确认后合并独立 Worktree 分支：",
            gateCmd: "git merge --no-ff wt/unified-auth-core",
          },
        ],
      },
      agents: {
        title: "自带你喜欢的任何 Agent 与订阅",
        subtitle:
          "原生接入 Claude Code、Codex、Antigravity、Gemini CLI 与 30+ ACP 代理。沿用现有订阅，统一交由 Personal Agent 协同调度。",
        anyOther: "+ 任意 ACP / CLI 代理（Registry 内置 34+）",
        migrateHint: "支持从 Claude Code、Codex、Antigravity、Cursor 与 VS Code 一键导入配置",
        list: SHARED_AGENTS,
      },
      devices: {
        title: "在每一台设备上掌控你的 Agent",
        subtitle: "一套纯 Rust ViewModel 驱动全端原生客户端，真实 iPhone 18 Pro 实机截图，Noise E2EE 实时同步。",
        cards: [
          {
            img: "/images/orbit-mobile-models.webp",
            alt: "Orbit 移动端多引擎与思考深度选择器实机截图",
            title: "多引擎与思考深度秒切",
            sub: "Claude Code · Codex · Antigravity 随时切换",
          },
          {
            img: "/images/orbit-mobile-stage.webp",
            alt: "Orbit 移动端原生深色工作台实机截图",
            title: "原生舰队工作台",
            sub: "LAN 直连 · 实时 Diff 计数 · /plan 与一键审码",
            center: true,
          },
          {
            img: "/images/orbit-mobile-pty.webp",
            alt: "Orbit 移动端原生远程终端实机截图",
            title: "全功能远程二进制终端",
            sub: "原生键盘扩展条（ESC / TAB / CTRL / ^C）",
          },
        ],
        platformStrip: "统一原生体验覆盖 iOS (SwiftUI) · Android (Jetpack Compose) · HarmonyOS NEXT (ArkTS V2)",
        alsoLink: "同时提供 macOS · Windows · Linux 原生桌面端与零安装 Web WASM PWA",
      },
      bento: {
        title: "Agent-first，硬核底层全栈原生",
        subtitle: "只放工程师真正每天依赖的核心能力——全部内聚于单一静态 Rust 二进制。",
        items: [
          {
            id: "shim",
            span: "span-4",
            eyebrow: "ZERO-DOWNTIME",
            title: "热升级不断线，永不杀掉运行中的 Agent",
            desc: "每个有状态 Agent 与 PTY 托管在独立 setsid orbit shim 子进程中。升级或重启 daemon 后自动无感重连，任务零中断、日志零丢失。",
            hrefSlug: "/reference/cli-and-architecture/",
          },
          {
            id: "im",
            span: "span-2",
            eyebrow: "ZERO-WEBHOOK IM",
            title: "穿透内网的 Slack / Discord 审批",
            desc: "纯 Rust 主动出站长连接，无需公网 IP 或 ngrok，手机端直接点卡批准高危操作。",
            hrefSlug: "/guides/personal-agent/",
          },
          {
            id: "worktree",
            span: "span-2",
            eyebrow: "PARALLEL ISOLATION",
            title: "隔离 Git Worktree 与任意轮次回溯",
            desc: "并行代理自动运行在独立分支与 Worktree，内置三方冲突解决与一键文件 Rewind。",
            hrefSlug: "/guides/code-and-review/",
          },
          {
            id: "sandbox",
            span: "span-4",
            eyebrow: "KERNEL SECURITY",
            title: "操作系统内核沙箱与防篡改审计链",
            desc: "macOS Seatbelt 与 Linux bubblewrap 默认封锁 ~/.ssh 与出站域名，并将每条高危操作写入 SHA-256 + Ed25519 签名哈希链。",
            hrefSlug: "/guides/security-sandbox/",
          },
          {
            id: "patrol",
            span: "span-3",
            eyebrow: "CODE & REVIEW",
            title: "Orbit Patrol 对抗审码与行内补全",
            desc: "基于不可变 Git Blob 执行两阶段深度审码并支持一键 Fix with Agent，搭配 CodeMirror 6 + LSP 与 IME 安全行内幽灵补全。",
            hrefSlug: "/guides/code-and-review/",
          },
          {
            id: "e2ee",
            span: "span-3",
            eyebrow: "ZERO-KNOWLEDGE MESH",
            title: "全程端到端加密，中继节点零明文",
            desc: "优先直连 LAN / SSH / Tailscale，跨网自动切入 <6MB 自托管盲中继；双向 Noise_NKpsk0 加密确保中继永远读不到任何字节。",
            hrefSlug: "/guides/multi-host-fleet/",
          },
        ],
      },
      testimonials: {
        title: "深受多主机工程师喜爱",
        subtitle: "来自在 Mac Studio、Linux 构建机与云端 GPU 之间日常协同编码的真实工作流反馈。",
        items: [
          {
            quote:
              "「再也不用同时盯四个 SSH 窗口看 Claude Code 和 Codex 了。给 Personal Agent 下一个目标，编码测试与审码接力完成后在 Slack 点一下就合完 PR。」",
            role: "基础设施工程师",
            setup: "Mac Studio + 3 台 Linux 构建机 · Rust / K8s",
            tag: "Slack",
            avatarBg: "#1b2838",
            avatarAccent: "#67c3ff",
            avatarShape: 0,
            row: 1,
          },
          {
            quote:
              "「独立 setsid shim 隔离非常稳——升级本机 daemon 时，正在跑 20 分钟集成测试的三个 Agent 纹丝不动，PTY 输出一帧没丢。」",
            role: "资深系统工程师",
            setup: "Apple Silicon + 自建 CI 集群 · Systems",
            tag: "Shim",
            avatarBg: "#162b24",
            avatarAccent: "#34d399",
            avatarShape: 1,
            row: 1,
          },
          {
            quote:
              "「公司防火墙严禁开任何入站端口，Orbit 的纯 Rust Socket Mode 主动出站长连接三分钟配通，买杯咖啡都能在手机上批高危操作。」",
            role: "全栈技术负责人",
            setup: "企业内网 · 零公网 IP / 零 Webhook",
            tag: "NAT",
            avatarBg: "#2c2238",
            avatarAccent: "#c084fc",
            avatarShape: 2,
            row: 1,
          },
          {
            quote:
              "「iOS、Android 折叠屏和鸿蒙原生全部由同一套 Rust ViewModel 驱动，从 Wi-Fi 切到 5G 回来时间线秒开，完全是原生客户端的顺滑质感。」",
            role: "移动端架构师",
            setup: "iOS + Android 折叠屏 + HarmonyOS NEXT",
            tag: "Mobile",
            avatarBg: "#2e2418",
            avatarAccent: "#fbbf24",
            avatarShape: 3,
            row: 1,
          },
          {
            quote:
              "「Orbit Patrol 基于不可变 Git Blob 做两阶段审查，直接帮我们拦掉一个 Android Deep-Link 越权漏洞，点一下 Fix with Agent 自动补齐单测。」",
            role: "安全工程负责人",
            setup: "多仓库 PR 审计 · 金融级安全边界",
            tag: "Patrol",
            avatarBg: "#311c24",
            avatarAccent: "#fb7185",
            avatarShape: 1,
            row: 2,
          },
          {
            quote:
              "「单静态 Rust 二进制，不装 Node 也不装 Python。扔进裸 Linux H100 服务器三秒配对上线，直接从 Discord 或 Mac 桌面派活。」",
            role: "AI 基础设施工程师",
            setup: "裸金属 Linux H100 节点 · 训练与评测",
            tag: "GPU",
            avatarBg: "#192930",
            avatarAccent: "#38bdf8",
            avatarShape: 2,
            row: 2,
          },
          {
            quote:
              "「跨引擎共享 .orbit/memory 终于解决了 Claude Code 和 Codex 各自失忆的问题——踩过一次平台边界坑，全舰队所有 Agent 永远记得。」",
            role: "分布式存储核心维护者",
            setup: "macOS + Linux + Windows 跨平台构建",
            tag: "Memory",
            avatarBg: "#1c2b20",
            avatarAccent: "#4ade80",
            avatarShape: 0,
            row: 2,
          },
          {
            quote:
              "「自托管盲中继跑在 5 美元 VPS 上只占不到 6MB 内存，Noise_NKpsk0 全程端到端加密，安全团队看完架构文档直接全组放行。」",
            role: "平台安全架构师",
            setup: "自托管盲中继 · 零知识 E2EE 网格",
            tag: "E2EE",
            avatarBg: "#252036",
            avatarAccent: "#a78bfa",
            avatarShape: 3,
            row: 2,
          },
        ],
      },
      compare: {
        title: "为多主机而生，而非单机外壳拼凑",
        subtitle:
          "传统编辑器为单台电脑敲代码设计，终端外壳止步于本机。Orbit 是完整的纯 Rust 多主机工程控制平面。",
        colCap: "核心能力",
        colOrbit: "Orbit",
        colOthers: "传统 IDE 与终端外壳",
        rows: [
          { capability: "常驻 Personal Agent 跨主机串联多个专长 Agent（编码 → 审码 → 验收）", orbit: "多步接力", others: "—" },
          { capability: "零公网 IP Slack / Discord / 飞书双向长连接审批", orbit: "✓", others: "—" },
          { capability: "独立 setsid shim，热升级 daemon 永不杀运行中的 Agent", orbit: "✓", others: "—" },
          { capability: "桌面 + 移动 + Web 原生客户端（macOS / Win / Linux / iOS / Android / 鸿蒙 / WASM）", orbit: "5 大原生端", others: "仅桌面端" },
          { capability: "单一静态 Rust 多主机编排二进制（零 Node.js / Python 运行时）", orbit: "✓", others: "—" },
          { capability: "Noise_NKpsk0 端到端加密与自托管零知识盲中继", orbit: "<6MB 中继", others: "明文云中转" },
          { capability: "OS 内核沙箱（Seatbelt / bubblewrap）+ Ed25519 防篡改审计链", orbit: "✓", others: "—" },
          { capability: "并行代理自动隔离 Git Worktree + 三方冲突与任意轮次 Rewind", orbit: "✓", others: "因工具而异" },
          { capability: "CodeMirror 6 + LSP + 行内 FIM + Orbit Patrol 两阶段审码", orbit: "✓", others: "部分支持" },
          { capability: "跨引擎共享 Markdown 记忆 + 零闲置 Token Wasm 插件", orbit: "✓", others: "绑定单一厂商" },
          { capability: "34+ 原生与 ACP 代理接入，MIT / Apache-2.0 双协议开源", orbit: "34+ 代理", others: "1–2 个 / 闭源" },
        ],
      },
      faq: {
        title: "常见问题",
        subtitle: "关于多主机编排、零知识安全边界与订阅兼容的常见疑问。",
        items: [
          {
            q: "Orbit 和普通单机 AI 编辑器或 SSH + tmux 有什么本质区别？",
            a: "单机工具默认你只在一台电脑上盯一个窗口。Orbit 将你的 Mac、Linux 构建机和云端 GPU 连接成一个统一工作区：你只需面向 Personal Agent 说话，它会自动拆解任务、在多个子 Agent 间串联接力（如 Codex 编码测试 → Claude Code 安全审码 → Antigravity 验证），并把最终结果与关键审批交回给你。",
          },
          {
            q: "我的代码、提示词或密钥会经过第三方云服务器吗？",
            a: "不会。orbit daemon 默认仅监听本机回环 127.0.0.1:6868；跨网连接优先直连（LAN / SSH / Tailscale），需要中继时通过你自托管的 <6MB 盲中继并使用 Noise_NKpsk0_25519_ChaChaPoly_SHA256 全程端到端加密，中继节点永远无法解密任何明文。",
          },
          {
            q: "没有公网 IP 和域名，Slack、Discord 或飞书怎么连上我本机的 Agent？",
            a: "Orbit 在纯 Rust 中实现了 Slack Socket Mode、Discord Gateway v10 与飞书/Lark WebSocket 长连接，由内网机器主动向外建连，不需要公网 IP、不需要配置 Webhook URL，也不需要开放任何入站端口，同时强制 Default-Deny 用户/群组白名单。",
          },
          {
            q: "我需要重新购买模型 API 吗，还是可以直接用现有订阅？",
            a: "完全无需重复付费。你可以直接接入本机已安装的 Claude Code、Codex、Antigravity、Gemini CLI、OpenCode 及 34+ ACP 代理与现有订阅，并支持从 Claude Code、Codex、Antigravity、Cursor 和 VS Code 一键迁移配置。",
          },
          {
            q: "重启或热升级 orbit daemon 时，正在干活的 Agent 会被杀掉吗？",
            a: "绝不会。所有有状态 AI CLI 会话和交互式 PTY 都托管在 setsid 脱离会话组的独立 orbit shim 子进程中，通过 Unix Socket 与 daemon 通信；升级重启 daemon 后自动无缝重连，正在运行的任务零中断。",
          },
          {
            q: "Orbit 免费且开源吗？支持哪些操作系统与移动端？",
            a: "Orbit 采用 MIT OR Apache-2.0 双协议完全开源免费。宿主 CLI/Daemon 与桌面端支持 macOS（Apple Silicon）、Linux（x64/arm64）与 Windows；客户端同时覆盖 iOS、Android、HarmonyOS NEXT 以及零知识 Web WASM PWA。",
          },
        ],
      },
      cta: {
        title: "获取 Orbit",
        subtitle: "MIT / Apache-2.0 免费开源。原生桌面端与全端移动客户端，几分钟内把你的所有机器连成一个统一工作区。",
        readDocs: "阅读快速上手",
        alsoFor: "同时支持 Apple Silicon · Windows · Linux · iOS · Android · HarmonyOS",
      },
      footer: {
        blurb: "多主机 AI 编码代理控制平面。免费开源，支持 macOS、Windows、Linux、iOS、Android 与 HarmonyOS。",
        cols: [
          {
            title: "产品",
            links: [
              { label: "下载安装", href: `${prefix}/download/` },
              { label: "更新日志", href: "https://github.com/danielgaoyy/orbit/releases", external: true },
              { label: "插件与记忆", href: `${prefix}/guides/memory-and-plugins/` },
              { label: "安全与沙箱", href: `${prefix}/guides/security-sandbox/` },
              { label: "完整文档", href: `${prefix}/getting-started/quickstart/` },
            ],
          },
          {
            title: "社区",
            links: [
              { label: "GitHub", href: "https://github.com/danielgaoyy/orbit", external: true },
              { label: "飞书群", href: DISCUSSIONS_URL, external: true },
              { label: "小红书", href: DISCUSSIONS_URL, external: true },
              { label: "微信公众号", href: DISCUSSIONS_URL, external: true },
            ],
          },
          {
            title: "文档与资源",
            links: [
              { label: "Personal Agent", href: `${prefix}/guides/personal-agent/` },
              { label: "多主机舰队", href: `${prefix}/guides/multi-host-fleet/` },
              { label: "CLI 与架构", href: `${prefix}/reference/cli-and-architecture/` },
              { label: "安全策略", href: "https://github.com/danielgaoyy/orbit/blob/main/SECURITY.md", external: true },
            ],
          },
        ],
        copyright: "© 2026 Orbit Contributors · MIT / Apache-2.0",
        rightNote: "Built in Pure Rust · Zero-Knowledge E2EE",
      },
    };
  }

  return {
    nav: {
      docs: "Docs",
      changelog: "Changelog",
      community: "Community",
      communityItems: [
        { id: "discord", label: "Discord", href: DISCUSSIONS_URL },
        { id: "x", label: "X (Twitter)", href: DISCUSSIONS_URL },
        { id: "reddit", label: "Reddit", href: DISCUSSIONS_URL },
      ],
      download: "Download",
      langCurrentCode: "EN",
      enHref,
      zhHref,
      starFallback: "Star",
    },
    hero: {
      pillTag: ORBIT_VERSION_SHORT,
      pillText: "Open-source pure-Rust multi-host control plane",
      title: "Run your coding agents on every machine you own.",
      subtitle:
        "One Personal Agent drives Claude Code, Codex, Gemini CLI, and 30+ others across your Mac, Linux servers, and cloud GPUs. Approve risky steps from your phone or Slack.",
      dlNeutral: "Download Orbit",
      dlMac: "Download for Mac",
      dlWin: "Download for Windows",
      dlLinux: "Download for Linux",
      dlMobile: "All Platform Downloads",
      viewGithub: "View on GitHub",
      metaLine: "Free & open source (MIT / Apache-2.0) · Bring your own Claude or Codex subscription",
      alsoFor: "Also on Windows, Linux, iOS, Android, and HarmonyOS",
    },
    pa: {
      title: "One Personal Agent for your entire fleet",
      subtitle:
        "You only ever talk to one assistant. It breaks down your goal, coordinates the right specialist agents across your hosts, and returns one verified answer.",
      ctaPrimary: "Meet Personal Agent",
      ctaSecondary: "Read the docs",
      colYou: "1 · YOU (SINGLE CONTACT)",
      colHub: "2 · PERSONAL AGENT",
      colFleet: "3 · SPECIALIST AGENTS ACROSS HOSTS",
      hubTagline: "Plans · Chains specialists · Synthesizes",
      returnBadge: "Returned to you",
      youChannels: ["Desktop", "Mobile", "Slack", "Discord", "Lark"],
      approveBtn: "Approve",
      denyBtn: "Deny",
      approvedNote: "✓ Approved (118ms) — Personal Agent finalized the release",
      deniedNote: "✕ Denied — workspace left untouched",
      resetBtn: "Reset",
      scenarios: [
        {
          id: "feature",
          label: "Release Handoff",
          channel: "via Slack DM",
          userPrompt:
            "Finish Noise key rotation, pass Cargo E2E on Linux, run a security review, and prep v0.11.0.",
          hubSteps: [
            "1. Split into build, adversarial review, and release check",
            "2. Chain Codex → Claude Code → Antigravity across 3 hosts",
            "3. Synthesize results; ping you only for irreversible push",
          ],
          workers: [
            {
              agent: "Codex",
              host: "linux-build-01",
              stage: "01 · Implement & Run E2E",
              reason: "Fast Linux build box · isolated Git worktree",
              result: "✓ Patched noise.rs · 1,061 tests green (4.18s)",
              tone: "ok",
            },
            {
              agent: "Claude Code",
              host: "Mac Studio",
              stage: "02 · Adversarial Security Review",
              reason: "Deep reasoning audit on Codex's patch",
              result: "✓ Hardened low-order X25519 key check",
              tone: "ok",
            },
            {
              agent: "Antigravity",
              host: "gpu-h100",
              stage: "03 · WASM & Patrol Verify",
              reason: "Immutable Git blob audit & client check",
              result: "✓ WASM E2EE verified · 0 blocking issues",
              tone: "cyan",
            },
          ],
          returnedSummary:
            "All three stages passed — 1,061 Cargo tests green and one key edge case hardened. Tagging v0.11.0 awaits your approval:",
          gateCmd: "git push origin v0.11.0  # linux-build-01",
        },
        {
          id: "incident",
          label: "Security Fix",
          channel: "via iOS Voice",
          userPrompt:
            "Trace the Android deep-link sync overwrite bug, ship the fix with tests, and re-audit it.",
          hubSteps: [
            "1. Send Claude Code to trace SyncLink intent trust boundary",
            "2. Hand off fix spec to Codex for patch + Gradle tests",
            "3. Re-audit with Orbit Patrol before requesting PR creation",
          ],
          workers: [
            {
              agent: "Claude Code",
              host: "Mac Studio",
              stage: "01 · Root Cause Trace",
              reason: "Cross-repo auth & intent boundary audit",
              result: "✓ Isolated 2 unguarded deep-link paths",
              tone: "ok",
            },
            {
              agent: "Codex",
              host: "linux-build-01",
              stage: "02 · Patch & Regression Suite",
              reason: "Isolated worktree implementation & Gradle",
              result: "✓ Added consent guard · 64/64 tests green",
              tone: "ok",
            },
            {
              agent: "Orbit Patrol",
              host: "gpu-h100",
              stage: "03 · Security Re-Audit",
              reason: "Immutable blob check for bypass vectors",
              result: "✓ Zero bypass risk · Hotfix PR ready",
              tone: "cyan",
            },
          ],
          returnedSummary:
            "Root cause fixed with 64 passing Android regression tests and clean security re-audit. Ready to open the PR:",
          gateCmd: "gh pr create --base main --title fix(android): guard sync deep-link",
        },
        {
          id: "refactor",
          label: "Core Refactor",
          channel: "via Desktop",
          userPrompt:
            "Unify auth state across all 5 clients into shared Rust core and verify backwards wire compatibility.",
          hubSteps: [
            "1. Claude Code refactors shared Rust auth state machine",
            "2. Antigravity updates Swift, Kotlin, ArkTS & WASM bindings",
            "3. Codex replays v0.9–v0.11 encrypted frames for zero regression",
          ],
          workers: [
            {
              agent: "Claude Code",
              host: "Mac Studio",
              stage: "01 · Rust Core Refactor",
              reason: "Zero-unsafe shared state architecture",
              result: "✓ orbit-client-core · clippy -D clean",
              tone: "ok",
            },
            {
              agent: "Antigravity",
              host: "gpu-h100",
              stage: "02 · 5-Client Binding Sync",
              reason: "Parallel native builds & UI smoke",
              result: "✓ All 5 native apps built & verified",
              tone: "ok",
            },
            {
              agent: "Codex",
              host: "linux-build-01",
              stage: "03 · Wire Compatibility Fuzz",
              reason: "Encrypted frame replay across versions",
              result: "✓ 100% backwards wire compatible",
              tone: "cyan",
            },
          ],
          returnedSummary:
            "Shared Rust core, all five native clients, and backwards wire replay all passed. Ready to merge into main:",
          gateCmd: "git merge --no-ff wt/unified-auth-core",
        },
      ],
    },
    agents: {
      title: "Bring your own agent and subscription",
      subtitle:
        "Works with Claude Code, Codex, Antigravity, Gemini CLI, and 30+ ACP agents. Plug in the subscriptions you already pay for and let your Personal Agent chain them together.",
      anyOther: "+ any other ACP agent — 34 in the registry",
      migrateHint: "One-click config migration from Claude Code, Codex, Antigravity, Cursor & VS Code",
      list: SHARED_AGENTS,
    },
    devices: {
      title: "Run your agents on every device",
      subtitle: "Real native mobile screens powered by one pure-Rust ViewModel with zero-knowledge Noise E2EE.",
      cards: [
        {
          img: "/images/orbit-mobile-models.webp",
          alt: "Real Orbit iPhone 18 Pro screenshot showing multi-agent and thinking effort picker",
          title: "Switch Agents & Effort Live",
          sub: "Claude Code · Codex · Antigravity on the go",
        },
        {
          img: "/images/orbit-mobile-stage.webp",
          alt: "Real Orbit iPhone 18 Pro screenshot showing native dark workspace launchpad",
          title: "Native Fleet Workspace",
          sub: "LAN Direct · live diff badges · /plan & review",
          center: true,
        },
        {
          img: "/images/orbit-mobile-pty.webp",
          alt: "Real Orbit iPhone 18 Pro screenshot showing live raw binary PTY terminal",
          title: "Live Raw Binary PTY",
          sub: "Full remote shell with native ESC / TAB / CTRL bar",
        },
      ],
      platformStrip: "Shared native experience across iOS (SwiftUI) · Android (Jetpack Compose) · HarmonyOS NEXT (ArkTS V2)",
      alsoLink: "Also native on macOS · Windows · Linux (Tauri v2) and zero-install Web WASM PWA",
    },
    bento: {
      title: "Agent-first, end to end.",
      subtitle: "Zero fluff. The hard systems primitives engineers rely on every day — compiled into one static Rust binary.",
      items: [
        {
          id: "shim",
          span: "span-4",
          eyebrow: "ZERO-DOWNTIME",
          title: "Upgrade Orbit without killing a running agent",
          desc: "Every stateful agent CLI and interactive PTY runs inside a detached setsid shim over Unix sockets. Restart or upgrade orbit daemon live with zero dropped turns.",
          hrefSlug: "/reference/cli-and-architecture/",
        },
        {
          id: "im",
          span: "span-2",
          eyebrow: "ZERO-WEBHOOK IM",
          title: "Approve from Slack & Discord behind any NAT",
          desc: "Pure-Rust outbound Socket Mode and Gateway connections with interactive approval cards — zero public IP or ngrok.",
          hrefSlug: "/guides/personal-agent/",
        },
        {
          id: "worktree",
          span: "span-2",
          eyebrow: "PARALLEL ISOLATION",
          title: "Isolated Git worktrees & instant rewind",
          desc: "Every worker runs in its own branch and worktree with 3-way conflict resolution and one-click turn rollback.",
          hrefSlug: "/guides/code-and-review/",
        },
        {
          id: "sandbox",
          span: "span-4",
          eyebrow: "KERNEL SECURITY",
          title: "Kernel-level sandbox with verifiable audit trail",
          desc: "macOS Seatbelt and Linux bubblewrap block ~/.ssh reads and pin outbound domains, sealing privileged actions into a SHA-256 + Ed25519 tamper-evident chain.",
          hrefSlug: "/guides/security-sandbox/",
        },
        {
          id: "patrol",
          span: "span-3",
          eyebrow: "CODE & REVIEW",
          title: "Adversarial PR review & inline code completion",
          desc: "Orbit Patrol audits immutable Git blobs with 1-click Fix with Agent, paired with CodeMirror 6, full LSP diagnostics, and IME-safe ghost text.",
          hrefSlug: "/guides/code-and-review/",
        },
        {
          id: "e2ee",
          span: "span-3",
          eyebrow: "ZERO-KNOWLEDGE MESH",
          title: "Encrypted end to end, even through a relay",
          desc: "Clients race direct LAN, SSH, and Tailscale first, falling back to your <6MB self-hosted blind relay over mutual Noise_NKpsk0 — zero plaintext ever leaves your hosts.",
          hrefSlug: "/guides/multi-host-fleet/",
        },
      ],
    },
    testimonials: {
      title: "Loved by developers",
      subtitle: "Built for multi-host workflows across Mac Studios, Linux build boxes, and cloud GPUs.",
      items: [
        {
          quote:
            "\"Stopped juggling four SSH terminals. One goal goes to Personal Agent — Codex builds and tests on Linux, Claude Code reviews, and I tap Approve in Slack on the commute.\"",
          role: "Infrastructure Engineer",
          setup: "Mac Studio + 3 Linux runners · Rust / K8s",
          tag: "Slack",
          avatarBg: "#1b2838",
          avatarAccent: "#67c3ff",
          avatarShape: 0,
          row: 1,
        },
        {
          quote:
            "\"Detached shims are rock solid. Upgraded orbit daemon mid-sprint while three agents were running a 20-minute integration suite — zero dropped turns, zero lost PTY output.\"",
          role: "Staff Systems Engineer",
          setup: "Apple Silicon + self-hosted CI fleet",
          tag: "Shim",
          avatarBg: "#162b24",
          avatarAccent: "#34d399",
          avatarShape: 1,
          row: 1,
        },
        {
          quote:
            "\"Our corporate firewall blocks every inbound webhook. Orbit's pure-Rust Slack Socket Mode wired up in three minutes with zero public IP or ngrok tunnels.\"",
          role: "Full-Stack Tech Lead",
          setup: "Corporate VPN · Zero inbound ports",
          tag: "NAT",
          avatarBg: "#2c2238",
          avatarAccent: "#c084fc",
          avatarShape: 2,
          row: 1,
        },
        {
          quote:
            "\"iOS, Android foldables, and HarmonyOS all driven by the exact same Rust ViewModel. Switching from Wi-Fi to 5G feels instant and genuinely native.\"",
          role: "Mobile Systems Architect",
          setup: "iOS + Android Foldable + HarmonyOS NEXT",
          tag: "Mobile",
          avatarBg: "#2e2418",
          avatarAccent: "#fbbf24",
          avatarShape: 3,
          row: 1,
        },
        {
          quote:
            "\"Orbit Patrol audited immutable Git blobs on our PR and caught a subtle deep-link CSRF before merge. Clicking Fix with Agent patched it and added a regression test.\"",
          role: "Security Engineering Lead",
          setup: "Multi-repo PR audits · Strict trust boundary",
          tag: "Patrol",
          avatarBg: "#311c24",
          avatarAccent: "#fb7185",
          avatarShape: 1,
          row: 2,
        },
        {
          quote:
            "\"One static Rust binary — no Node, no Python. Drop it onto a fresh H100 box, run orbit pair, and dispatch jobs straight from Discord or the native Mac app.\"",
          role: "AI Infra Engineer",
          setup: "Bare-metal Linux H100 cluster · Training",
          tag: "GPU",
          avatarBg: "#192930",
          avatarAccent: "#38bdf8",
          avatarShape: 2,
          row: 2,
        },
        {
          quote:
            "\"Shared .orbit/memory/*.md finally fixed agent amnesia across tools. Document a tricky build pitfall once and both Claude Code and Codex respect it automatically.\"",
          role: "Distributed Storage Maintainer",
          setup: "macOS + Linux + Windows cross-builds",
          tag: "Memory",
          avatarBg: "#1c2b20",
          avatarAccent: "#4ade80",
          avatarShape: 0,
          row: 2,
        },
        {
          quote:
            "\"Self-hosted blind relay uses under 6MB of RAM on a $5 VPS with mutual Noise_NKpsk0 E2EE. Security signed off after reading the architecture docs.\"",
          role: "Principal Platform Architect",
          setup: "Self-hosted relay · Zero-knowledge mesh",
          tag: "E2EE",
          avatarBg: "#252036",
          avatarAccent: "#a78bfa",
          avatarShape: 3,
          row: 2,
        },
      ],
    },
    compare: {
      title: "Built for multi-host fleets, not retrofitted",
      subtitle:
        "Editors were built for one laptop. Wrappers stop at a terminal. Orbit is the full multi-host control plane: Personal Agent, shims, sandboxes, and five native apps.",
      colCap: "Capability",
      colOrbit: "Orbit",
      colOthers: "IDEs and wrappers",
      rows: [
        { capability: "One Personal Agent chaining multi-agent handoffs (build → review → verify)", orbit: "3-stage chain", others: "—" },
        { capability: "Zero-public-IP Slack, Discord & Lark/Feishu long connections", orbit: "✓", others: "—" },
        { capability: "Detached setsid orbit shim survives daemon hot upgrades", orbit: "✓", others: "—" },
        { capability: "Five native apps (Desktop, iOS, Android, HarmonyOS, Web WASM)", orbit: "5 native apps", others: "Desktop only" },
        { capability: "Single static Rust multi-host control binary (zero Node / Python runtime)", orbit: "✓", others: "—" },
        { capability: "End-to-end Noise_NKpsk0 encryption with self-hosted blind relay", orbit: "<6MB relay", others: "Plaintext cloud" },
        { capability: "OS kernel sandbox (Seatbelt / bwrap) + Ed25519 audit chain", orbit: "✓", others: "—" },
        { capability: "Parallel agents in auto-isolated Git worktrees + 3-way merge", orbit: "✓", others: "Varies" },
        { capability: "CodeMirror 6 + LSP + inline FIM + Orbit Patrol PR review", orbit: "✓", others: "Partial" },
        { capability: "Cross-provider Markdown memory + zero-idle-token Wasm plugins", orbit: "✓", others: "Vendor-locked" },
        { capability: "34+ native & ACP agents, dual-licensed MIT / Apache-2.0", orbit: "34+ agents", others: "1–2" },
      ],
    },
    faq: {
      title: "Frequently Asked Questions",
      subtitle: "Everything about multi-host orchestration, zero-knowledge security, and bringing your own subscriptions.",
      items: [
        {
          q: "How is Orbit different from single-machine wrappers or SSH + tmux?",
          a: "Single-machine wrappers assume one laptop and one foreground terminal. Orbit connects your Mac, Linux build boxes, and cloud GPUs into one encrypted workspace: you talk only to your Personal Agent, which chains specialist agents end to end (such as Codex implementing & testing, Claude Code reviewing, and Antigravity verifying) before returning one clean result.",
        },
        {
          q: "Does Orbit route my code or prompts through third-party cloud servers?",
          a: "No. orbit daemon binds to loopback 127.0.0.1:6868 by default. Remote clients race direct LAN/SSH/Tailscale paths first and fall back to your own <6MB blind relay using mutual Noise_NKpsk0_25519_ChaChaPoly_SHA256 E2EE — the relay never sees plaintext.",
        },
        {
          q: "How do Slack, Discord, and Lark/Feishu work without a public IP or webhook?",
          a: "Orbit maintains pure-Rust outbound WebSocket connections (Slack Socket Mode, Discord Gateway v10, and Lark/Feishu WS) directly from behind home or corporate NATs — no public domain, no inbound ports, and strict default-deny allowlists.",
        },
        {
          q: "Can I bring my existing Claude Code, Codex, or Antigravity subscriptions?",
          a: "Yes. Orbit wraps the coding agent CLIs and subscriptions you already pay for — including Claude Code, Codex, Antigravity, Gemini CLI, OpenCode, and 34+ ACP agents — with one-click config migration from 5 mainstream tools.",
        },
        {
          q: "What happens to running agents when I restart or upgrade orbit daemon?",
          a: "Nothing breaks. Every stateful agent and interactive PTY runs inside a detached setsid orbit shim communicating over Unix sockets. Upgrading orbit daemon transparently reattaches to every live session with zero dropped turns.",
        },
        {
          q: "Is Orbit free and open source? Which platforms are supported?",
          a: "Yes — Orbit is dual-licensed under MIT OR Apache-2.0. The static host binary and Tauri desktop app support macOS (Apple Silicon), Linux (x64/arm64), and Windows; native mobile & web clients cover iOS, Android, HarmonyOS NEXT, and Web WASM PWA.",
        },
      ],
    },
    cta: {
      title: "Get Orbit",
      subtitle: "Free and open source (MIT / Apache-2.0). Native desktop & mobile apps to connect every machine you own into one workspace.",
      readDocs: "Read the Quickstart",
      alsoFor: "Also on Apple Silicon · Windows · Linux · iOS · Android · HarmonyOS",
    },
    footer: {
      blurb: "The multi-host agent control plane. Free and open source for macOS, Windows, Linux, iOS, Android, and HarmonyOS.",
      cols: [
        {
          title: "Product",
          links: [
            { label: "Download", href: `${prefix}/download/` },
            { label: "Changelog", href: "https://github.com/danielgaoyy/orbit/releases", external: true },
            { label: "Plugins & Memory", href: `${prefix}/guides/memory-and-plugins/` },
            { label: "Security", href: `${prefix}/guides/security-sandbox/` },
            { label: "Docs", href: `${prefix}/getting-started/quickstart/` },
          ],
        },
        {
          title: "Community",
          links: [
            { label: "GitHub", href: "https://github.com/danielgaoyy/orbit", external: true },
            { label: "Discord", href: DISCUSSIONS_URL, external: true },
            { label: "X", href: DISCUSSIONS_URL, external: true },
            { label: "Reddit", href: DISCUSSIONS_URL, external: true },
          ],
        },
        {
          title: "Resources",
          links: [
            { label: "Personal Agent", href: `${prefix}/guides/personal-agent/` },
            { label: "Multi-Host Fleet", href: `${prefix}/guides/multi-host-fleet/` },
            { label: "CLI & Architecture", href: `${prefix}/reference/cli-and-architecture/` },
            { label: "Security Policy", href: "https://github.com/danielgaoyy/orbit/blob/main/SECURITY.md", external: true },
          ],
        },
      ],
      copyright: "© 2026 Orbit Contributors · MIT / Apache-2.0",
      rightNote: "Built in Pure Rust · Zero-Knowledge E2EE",
    },
  };
}
