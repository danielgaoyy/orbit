#!/usr/bin/env node
/**
 * Reproducible Light & Dark 2x Retina Desktop Showcase Screenshot Pipeline.
 *
 * Uses zero external npm dependencies (built-in Node HTTP server + Chrome CDP,
 * matching `apps/desktop/scripts/smoke-render.mjs`) to render a realistic
 * multi-host Personal Agent + Subagent Crew + Plan Shelf + Review Diff scene
 * in both Dark ("Obsidian Graphite") and Light ("Alabaster Paper") themes.
 *
 * Outputs (1440x900 @ 2x DPR = 2880x1800 PNG):
 *   - docs/assets/orbit-desktop-dark.png
 *   - docs/assets/orbit-desktop-light.png
 *   - apps/docs/public/images/orbit-desktop-dark.png
 *   - apps/docs/public/images/orbit-desktop-light.png
 */
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const DESKTOP_DIR = path.resolve(__dirname, "..");
const REPO_ROOT = path.resolve(DESKTOP_DIR, "..", "..");
const DIST_DIR = path.join(DESKTOP_DIR, "dist");
const DOCS_ASSETS_DIR = path.join(REPO_ROOT, "docs", "assets");
const DOCS_SITE_IMAGES_DIR = path.join(REPO_ROOT, "apps", "docs", "public", "images");

const VIEWPORT_WIDTH = 1440;
const VIEWPORT_HEIGHT = 900;
const DEVICE_SCALE_FACTOR = 2;

const MIME_TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "application/javascript; charset=utf-8",
  ".mjs": "application/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".webp": "image/webp",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
  ".json": "application/json; charset=utf-8",
  ".wasm": "application/wasm",
};

const FATAL_CONSOLE_RE =
  /Orbit UI boundary caught error|Minified React error|Maximum update depth exceeded|The result of getSnapshot should be cached|useOrbitSelector selector returned a non-stable reference|Uncaught\b|\b(?:TypeError|ReferenceError|RangeError|SyntaxError):/;

function span(text, opts = {}) {
  return {
    text,
    bold: Boolean(opts.bold),
    italic: Boolean(opts.italic),
    strikethrough: false,
    inline_code: Boolean(opts.code),
    link_url: opts.link ?? null,
  };
}

function tok(text, darkHex, lightHex, opts = {}) {
  return {
    text,
    fg_hex: darkHex,
    light_fg_hex: lightHex,
    bold: Boolean(opts.bold),
    italic: Boolean(opts.italic),
  };
}

// Syntax palette tuned to Orbit's Obsidian Graphite (dark) & Alabaster Paper (light)
const C = {
  kw: ["#C792EA", "#7C3AED"],
  fn: ["#82AAFF", "#1D4ED8"],
  ty: ["#FFCB6B", "#B45309"],
  str: ["#C3E88D", "#0F7B55"],
  num: ["#F78C6C", "#C2410C"],
  cm: ["#697098", "#6E7487"],
  fg: ["#E2E6F3", "#1F2330"],
  op: ["#89DDFF", "#0369A1"],
};

function buildShowcaseFixture(theme) {
  const now = Date.now();
  const isoAgo = (sec) => new Date(now - sec * 1000).toISOString();

  const hostMac = {
    host_id: "host_mac_studio",
    display_name: "Mac Studio",
    hostname: "mac-studio.local",
    custom_name: "Mac Studio",
    is_local: true,
    ssh_alias: null,
    route_mode: "direct",
    remote_daemon_port: 6868,
    custom_direct_endpoint: null,
    via_ssh_tunnel: false,
    last_ping_ms: 1,
    daemon_version: "0.10.4",
    os_arch: "darwin/aarch64",
    daemon_pubkey_b64: "mK8v2xQ4pL9rN7tW1yZ3bC5dE6fG8hJ0kM2nP4qR6sU=",
    direct_endpoints: ["127.0.0.1:6868"],
    relay_url: "wss://relay.orbit.internal:7878",
    connection_state: "online_direct",
    last_error: null,
    available_providers: ["claude_code", "codex", "antigravity", "opencode", "gemini_cli"],
    workspace_count: 2,
    agent_count: 4,
    pending_approval_count: 0,
    supports_server_queue: true,
  };

  const hostLinux = {
    host_id: "host_linux_build",
    display_name: "linux-build-01",
    hostname: "linux-build-01.corp",
    custom_name: "linux-build-01",
    is_local: false,
    ssh_alias: "linux-build-01",
    route_mode: "relay",
    remote_daemon_port: 6868,
    custom_direct_endpoint: null,
    via_ssh_tunnel: false,
    last_ping_ms: 14,
    daemon_version: "0.10.4",
    os_arch: "linux/x86_64",
    daemon_pubkey_b64: "bN4pQ8rS2tU6vW0xY4zA8bC2dE6fG0hI4jK8lM2nO6p=",
    direct_endpoints: ["10.42.0.18:6868"],
    relay_url: "wss://relay.orbit.internal:7878",
    connection_state: "online_relay",
    last_error: null,
    available_providers: ["codex", "claude_code", "opencode"],
    workspace_count: 2,
    agent_count: 3,
    pending_approval_count: 1,
    supports_server_queue: true,
  };

  const hostGpu = {
    host_id: "host_gpu_cloud",
    display_name: "gpu-h100-cloud",
    hostname: "h100-us-west.cloud",
    custom_name: "gpu-h100-cloud",
    is_local: false,
    ssh_alias: "h100-cloud",
    route_mode: "relay",
    remote_daemon_port: 6868,
    custom_direct_endpoint: null,
    via_ssh_tunnel: true,
    last_ping_ms: 28,
    daemon_version: "0.10.4",
    os_arch: "linux/x86_64",
    daemon_pubkey_b64: "qR8sT2uV6wX0yZ4aB8cD2eF6gH0iJ4kL8mN2oP6qR0s=",
    direct_endpoints: [],
    relay_url: "ssh+relay://h100-cloud:7878",
    connection_state: "online_relay",
    last_error: null,
    available_providers: ["antigravity", "gemini_cli", "claude_code"],
    workspace_count: 1,
    agent_count: 2,
    pending_approval_count: 0,
    supports_server_queue: true,
  };

  const wsOrbitMain = {
    workspace_id: "ws_orbit_main",
    cwd: "/Users/jess/workspace/orbit",
    title: "orbit",
    is_worktree: false,
    git_branch: "feat/e2ee-rotation",
    archived_at: null,
  };

  const wsLinuxWorktree = {
    workspace_id: "ws_linux_wt",
    cwd: "/srv/builds/orbit-wt-e2ee",
    title: "orbit-core",
    is_worktree: true,
    git_branch: "feat/e2ee-rotation",
    archived_at: null,
  };

  const wsGpuBench = {
    workspace_id: "ws_gpu_wasm",
    cwd: "/workspace/orbit-web",
    title: "orbit-wasm-pwa",
    is_worktree: false,
    git_branch: "main",
    archived_at: null,
  };

  const agentPersonal = {
    agent_id: "ag_personal_chief",
    workspace_id: "ws_orbit_main",
    cwd: "/Users/jess/workspace/orbit",
    title: "Personal Agent · Fleet Release Coordinator",
    provider: "claude_code",
    provider_label: "Claude Code",
    model: "claude-opus-4-6",
    effort: "high",
    mode: "accept_edits",
    status_label: "Running",
    is_running: true,
    has_background_tasks: true,
    has_armed_timers: true,
    has_running_subagents: true,
    is_awaiting_permission: false,
    parent_agent_id: null,
    labels: {
      "orbit.agent_role": "personal_router",
      "orbit.im_channel": "feishu_longconn",
    },
    latest_seq: 42,
    read_seq: 42,
    handle: "chief-of-staff",
    worktree_branch: "feat/e2ee-rotation",
    task_summary: "Coordinating v0.10.5 across 3 fleet hosts",
    created_at_rfc3339: isoAgo(1800),
    updated_at_rfc3339: isoAgo(12),
    is_pinned: true,
    seen_seq: 42,
    is_unread: false,
  };

  const subSecurityMac = {
    agent_id: "ag_sub_sec_mac",
    workspace_id: "ws_orbit_main",
    cwd: "/Users/jess/workspace/orbit",
    title: "Security Auditor · Noise_NKpsk0",
    provider: "claude_code",
    provider_label: "Claude Code",
    model: "claude-sonnet-4-6",
    effort: "medium",
    mode: "plan",
    status_label: "Idle",
    is_running: false,
    has_background_tasks: false,
    has_armed_timers: false,
    is_awaiting_permission: false,
    parent_agent_id: "ag_personal_chief",
    parent_host_id: "host_mac_studio",
    latest_seq: 18,
    created_at_rfc3339: isoAgo(120),
    updated_at_rfc3339: isoAgo(48),
    is_pinned: false,
    seen_seq: 18,
    is_unread: false,
  };

  const subBuildLinux = {
    agent_id: "ag_sub_build_linux",
    workspace_id: "ws_linux_wt",
    cwd: "/srv/builds/orbit-wt-e2ee",
    title: "Cargo E2E Runner · linux-build-01",
    provider: "codex",
    provider_label: "Codex",
    model: "gpt-5.4",
    effort: "high",
    mode: "accept_edits",
    status_label: "Running",
    is_running: true,
    has_background_tasks: false,
    has_armed_timers: false,
    is_awaiting_permission: false,
    parent_agent_id: "ag_personal_chief",
    parent_host_id: "host_mac_studio",
    worktree_branch: "feat/e2ee-rotation",
    latest_seq: 27,
    created_at_rfc3339: isoAgo(46),
    updated_at_rfc3339: isoAgo(4),
    is_pinned: false,
    seen_seq: 27,
    is_unread: false,
  };

  const subWasmGpu = {
    agent_id: "ag_sub_wasm_gpu",
    workspace_id: "ws_gpu_wasm",
    cwd: "/workspace/orbit-web",
    title: "WASM & Patrol Audit · gpu-h100-cloud",
    provider: "antigravity",
    provider_label: "Antigravity",
    model: "gemini-2.5-pro",
    effort: "medium",
    mode: "accept_edits",
    status_label: "Idle",
    is_running: false,
    has_background_tasks: false,
    has_armed_timers: false,
    is_awaiting_permission: false,
    parent_agent_id: "ag_personal_chief",
    parent_host_id: "host_mac_studio",
    latest_seq: 15,
    created_at_rfc3339: isoAgo(110),
    updated_at_rfc3339: isoAgo(32),
    is_pinned: false,
    seen_seq: 15,
    is_unread: false,
  };

  const agentNeedsApproval = {
    agent_id: "ag_linux_deploy",
    workspace_id: "ws_linux_wt",
    cwd: "/srv/builds/orbit-wt-e2ee",
    title: "Rotate Relay Pubkey & Deploy",
    provider: "codex",
    provider_label: "Codex",
    model: "gpt-5.4",
    effort: "medium",
    mode: "accept_edits",
    status_label: "Awaiting Approval",
    is_running: false,
    has_background_tasks: false,
    has_armed_timers: false,
    is_awaiting_permission: true,
    parent_agent_id: null,
    worktree_branch: "feat/e2ee-rotation",
    latest_seq: 12,
    created_at_rfc3339: isoAgo(900),
    updated_at_rfc3339: isoAgo(120),
    is_pinned: false,
    seen_seq: 10,
    is_unread: true,
  };

  const agentTimerWatcher = {
    agent_id: "ag_linux_ci_watch",
    workspace_id: "ws_linux_wt",
    cwd: "/srv/builds/orbit-wt-e2ee",
    title: "PR #68 CI & Patrol Monitor",
    provider: "claude_code",
    provider_label: "Claude Code",
    model: "claude-sonnet-4-6",
    effort: "low",
    mode: "plan",
    status_label: "Idle",
    is_running: false,
    has_background_tasks: true,
    has_armed_timers: true,
    is_awaiting_permission: false,
    parent_agent_id: null,
    latest_seq: 9,
    created_at_rfc3339: isoAgo(2400),
    updated_at_rfc3339: isoAgo(480),
    is_pinned: false,
    seen_seq: 9,
    is_unread: false,
  };

  const agentMacMemory = {
    agent_id: "ag_mac_memory",
    workspace_id: "ws_orbit_main",
    cwd: "/Users/jess/workspace/orbit",
    title: "Cross-Provider FTS5 + RRF Tuning",
    provider: "antigravity",
    provider_label: "Antigravity",
    model: "gemini-2.5-pro",
    effort: "high",
    mode: "accept_edits",
    status_label: "Idle",
    is_running: false,
    has_background_tasks: false,
    has_armed_timers: false,
    is_awaiting_permission: false,
    parent_agent_id: null,
    latest_seq: 21,
    created_at_rfc3339: isoAgo(3600),
    updated_at_rfc3339: isoAgo(960),
    is_pinned: false,
    seen_seq: 21,
    is_unread: false,
  };

  const agentGpuVoice = {
    agent_id: "ag_gpu_voice",
    workspace_id: "ws_gpu_wasm",
    cwd: "/workspace/orbit-web",
    title: "Offline Whisper VAD & WASM Audio",
    provider: "gemini_cli",
    provider_label: "Gemini CLI",
    model: "gemini-2.5-pro",
    effort: "medium",
    mode: "accept_edits",
    status_label: "Idle",
    is_running: false,
    has_background_tasks: false,
    has_armed_timers: false,
    is_awaiting_permission: false,
    parent_agent_id: null,
    latest_seq: 14,
    created_at_rfc3339: isoAgo(5400),
    updated_at_rfc3339: isoAgo(2100),
    is_pinned: false,
    seen_seq: 14,
    is_unread: false,
  };

  // Concise, zero-clipping syntax-highlighted unified diffs for the right Review panel
  const diffFiles = [
    {
      path: "crates/orbit-crypto/src/noise.rs",
      additions: 14,
      deletions: 3,
      lines: [
        {
          kind: "hunk_header",
          old_line: null,
          new_line: null,
          content: "@@ -142,8 +142,14 @@ impl NoiseRelaySession {",
        },
        {
          kind: "context",
          old_line: 142,
          new_line: 142,
          content: "pub fn rekey(&mut self, pk: &[u8; 32]) -> Result<()> {",
          tokens: [
            tok("pub fn ", C.kw[0], C.kw[1]),
            tok("rekey", C.fn[0], C.fn[1]),
            tok("(&mut self, pk: &[u8; 32]) -> ", C.fg[0], C.fg[1]),
            tok("Result", C.ty[0], C.ty[1]),
            tok("<()> {", C.fg[0], C.fg[1]),
          ],
        },
        {
          kind: "removed",
          old_line: 143,
          new_line: null,
          content: "  let psk = derive_v1(&self.secret);",
          tokens: [
            tok("  ", C.fg[0], C.fg[1]),
            tok("let ", C.kw[0], C.kw[1]),
            tok("psk = ", C.fg[0], C.fg[1]),
            tok("derive_v1", C.fn[0], C.fn[1]),
            tok("(&self.secret);", C.fg[0], C.fg[1]),
          ],
        },
        {
          kind: "added",
          old_line: null,
          new_line: 143,
          content: "  // Reject low-order Curve25519 points",
          tokens: [
            tok("  // Reject low-order Curve25519 points", C.cm[0], C.cm[1], { italic: true }),
          ],
        },
        {
          kind: "added",
          old_line: null,
          new_line: 144,
          content: "  ensure_contributory_x25519(pk)?;",
          tokens: [
            tok("  ", C.fg[0], C.fg[1]),
            tok("ensure_contributory_x25519", C.fn[0], C.fn[1]),
            tok("(pk)?;", C.fg[0], C.fg[1]),
          ],
        },
        {
          kind: "added",
          old_line: null,
          new_line: 145,
          content: "  let psk = hkdf_v2(PROLOGUE_V2, &self.secret);",
          tokens: [
            tok("  ", C.fg[0], C.fg[1]),
            tok("let ", C.kw[0], C.kw[1]),
            tok("psk = ", C.fg[0], C.fg[1]),
            tok("hkdf_v2", C.fn[0], C.fn[1]),
            tok("(PROLOGUE_V2, &self.secret);", C.fg[0], C.fg[1]),
          ],
        },
        {
          kind: "context",
          old_line: 144,
          new_line: 146,
          content: "  self.noise.rekey_nkpsk0(&psk)?;",
          tokens: [
            tok("  self.noise.", C.fg[0], C.fg[1]),
            tok("rekey_nkpsk0", C.fn[0], C.fn[1]),
            tok("(&psk)?;", C.fg[0], C.fg[1]),
          ],
        },
        {
          kind: "added",
          old_line: null,
          new_line: 147,
          content: "  self.audit.append_signed(Event::Rekeyed)?;",
          tokens: [
            tok("  self.audit.", C.fg[0], C.fg[1]),
            tok("append_signed", C.fn[0], C.fn[1]),
            tok("(", C.fg[0], C.fg[1]),
            tok("Event", C.ty[0], C.ty[1]),
            tok("::Rekeyed)?;", C.fg[0], C.fg[1]),
          ],
        },
        {
          kind: "context",
          old_line: 145,
          new_line: 148,
          content: "  Ok(())",
          tokens: [
            tok("  ", C.fg[0], C.fg[1]),
            tok("Ok", C.ty[0], C.ty[1]),
            tok("(())", C.fg[0], C.fg[1]),
          ],
        },
        {
          kind: "context",
          old_line: 146,
          new_line: 149,
          content: "}",
          tokens: [tok("}", C.fg[0], C.fg[1])],
        },
      ],
    },
    {
      path: "crates/orbit-daemon/src/personal_router.rs",
      additions: 11,
      deletions: 2,
      lines: [
        {
          kind: "hunk_header",
          old_line: null,
          new_line: null,
          content: "@@ -318,7 +318,14 @@ impl PersonalRouter {",
        },
        {
          kind: "context",
          old_line: 318,
          new_line: 318,
          content: "pub fn gate(&self, risk: Risk) -> Gate {",
          tokens: [
            tok("pub fn ", C.kw[0], C.kw[1]),
            tok("gate", C.fn[0], C.fn[1]),
            tok("(&self, risk: ", C.fg[0], C.fg[1]),
            tok("Risk", C.ty[0], C.ty[1]),
            tok(") -> ", C.fg[0], C.fg[1]),
            tok("Gate ", C.ty[0], C.ty[1]),
            tok("{", C.fg[0], C.fg[1]),
          ],
        },
        {
          kind: "removed",
          old_line: 319,
          new_line: null,
          content: "  Gate::ForwardToDesktop",
          tokens: [
            tok("  ", C.fg[0], C.fg[1]),
            tok("Gate", C.ty[0], C.ty[1]),
            tok("::ForwardToDesktop", C.fg[0], C.fg[1]),
          ],
        },
        {
          kind: "added",
          old_line: null,
          new_line: 319,
          content: "  match risk {",
          tokens: [
            tok("  ", C.fg[0], C.fg[1]),
            tok("match ", C.kw[0], C.kw[1]),
            tok("risk {", C.fg[0], C.fg[1]),
          ],
        },
        {
          kind: "added",
          old_line: null,
          new_line: 320,
          content: "    Risk::Low => Gate::AutoAllowTier1,",
          tokens: [
            tok("    ", C.fg[0], C.fg[1]),
            tok("Risk", C.ty[0], C.ty[1]),
            tok("::Low => ", C.fg[0], C.fg[1]),
            tok("Gate", C.ty[0], C.ty[1]),
            tok("::AutoAllowTier1,", C.fg[0], C.fg[1]),
          ],
        },
        {
          kind: "added",
          old_line: null,
          new_line: 321,
          content: "    Risk::Medium => Gate::RouterPreReview,",
          tokens: [
            tok("    ", C.fg[0], C.fg[1]),
            tok("Risk", C.ty[0], C.ty[1]),
            tok("::Medium => ", C.fg[0], C.fg[1]),
            tok("Gate", C.ty[0], C.ty[1]),
            tok("::RouterPreReview,", C.fg[0], C.fg[1]),
          ],
        },
        {
          kind: "added",
          old_line: null,
          new_line: 322,
          content: "    Risk::High | Risk::Critical =>",
          tokens: [
            tok("    ", C.fg[0], C.fg[1]),
            tok("Risk", C.ty[0], C.ty[1]),
            tok("::High | ", C.fg[0], C.fg[1]),
            tok("Risk", C.ty[0], C.ty[1]),
            tok("::Critical =>", C.fg[0], C.fg[1]),
          ],
        },
        {
          kind: "added",
          old_line: null,
          new_line: 323,
          content: "      Gate::HumanOnly { im_card: true },",
          tokens: [
            tok("      ", C.fg[0], C.fg[1]),
            tok("Gate", C.ty[0], C.ty[1]),
            tok("::HumanOnly { im_card: ", C.fg[0], C.fg[1]),
            tok("true ", C.num[0], C.num[1]),
            tok("},", C.fg[0], C.fg[1]),
          ],
        },
        {
          kind: "added",
          old_line: null,
          new_line: 324,
          content: "  }",
          tokens: [tok("  }", C.fg[0], C.fg[1])],
        },
        {
          kind: "context",
          old_line: 320,
          new_line: 325,
          content: "}",
          tokens: [tok("}", C.fg[0], C.fg[1])],
        },
      ],
    },
  ];

  // Rich narrative stage turn showing Personal Agent dispatching 3 cross-host subagents
  const toolInspectFleet = {
    item_type: "tool_call",
    seq: 21,
    call_id: "call_inspect_fleet",
    status: "completed",
    summary_title: "orbit_control · inspect_project_context (3 online hosts)",
    detail: {
      kind: "generic",
      tool_name: "orbit_control",
      arguments_json: JSON.stringify({
        action: "inspect_project_context",
        hosts: ["Mac Studio", "linux-build-01", "gpu-h100-cloud"],
      }),
    },
    diff_files: [],
    timestamp_ms: now - 410_000,
  };

  const toolDispatchMac = {
    item_type: "tool_call",
    seq: 24,
    call_id: "call_sub_mac",
    status: "completed",
    summary_title: "Security Auditor · Mac Studio",
    detail: {
      kind: "subagent",
      child_agent_id: "ag_sub_sec_mac",
      role: "Security Auditor · Mac Studio (Claude Code)",
      prompt_preview:
        "Audit Noise_NKpsk0 rekeying, low-order X25519 checks, and active E2EE disconnect on pairing_secret rotation.",
    },
    diff_files: [],
    timestamp_ms: now - 380_000,
  };

  const toolDispatchLinux = {
    item_type: "tool_call",
    seq: 25,
    call_id: "call_sub_linux",
    status: "running",
    summary_title: "Cargo E2E Runner · linux-build-01",
    detail: {
      kind: "subagent",
      child_agent_id: "ag_sub_build_linux",
      role: "Cargo E2E Runner · linux-build-01 (Codex · worktree)",
      prompt_preview:
        "Fork isolated worktree feat/e2ee-rotation on linux-build-01 and run cargo test --workspace + pitfall regressions.",
    },
    diff_files: [],
    timestamp_ms: now - 44_000,
  };

  const toolDispatchGpu = {
    item_type: "tool_call",
    seq: 26,
    call_id: "call_sub_gpu",
    status: "completed",
    summary_title: "WASM & Patrol Audit · gpu-h100-cloud",
    detail: {
      kind: "subagent",
      child_agent_id: "ag_sub_wasm_gpu",
      role: "WASM & Patrol Audit · gpu-h100-cloud (Antigravity)",
      prompt_preview:
        "Verify orbit-client-core WASM bundle handshake and run Two-Stage Orbit Patrol on PR #68.",
    },
    diff_files: [],
    timestamp_ms: now - 375_000,
  };

  const assistantReplyItem = {
    item_type: "assistant_message",
    seq: 34,
    item_id: "msg_chief_summary",
    text: "Dispatched all three fleet tracks in parallel across your connected hosts.",
    streaming: false,
    timestamp_ms: now - 90_000,
    blocks: [
      {
        kind: "paragraph",
        spans: [
          span("Dispatched all three release tracks in parallel across your fleet and armed a conditional wakeup timer on "),
          span("linux-build-01", { code: true }),
          span(". Low-risk tool checks were auto-approved by "),
          span("Guardian Tier-1", { bold: true }),
          span("; only the production relay key rotation is queued for your decision."),
        ],
      },
      {
        kind: "table",
        alignments: ["left", "left", "left", "left"],
        headers: [
          { spans: [span("Host", { bold: true })] },
          { spans: [span("Agent", { bold: true })] },
          { spans: [span("Route", { bold: true })] },
          { spans: [span("Status", { bold: true })] },
        ],
        rows: [
          {
            cells: [
              { spans: [span("Mac Studio", { bold: true })] },
              { spans: [span("Claude Code · Opus 4.6")] },
              { spans: [span("Local · 1ms", { code: true })] },
              { spans: [span("Verified (+14 −3)")] },
            ],
          },
          {
            cells: [
              { spans: [span("linux-build-01", { bold: true })] },
              { spans: [span("Codex · GPT-5.4 (worktree)")] },
              { spans: [span("Noise E2EE · 14ms", { code: true })] },
              { spans: [span("184/184 tests · running E2E")] },
            ],
          },
          {
            cells: [
              { spans: [span("gpu-h100-cloud", { bold: true })] },
              { spans: [span("Antigravity · Gemini 2.5")] },
              { spans: [span("SSH+Relay · 28ms", { code: true })] },
              { spans: [span("Patrol clean · 0 findings")] },
            ],
          },
        ],
      },
      {
        kind: "list_item",
        ordered: false,
        index: 0,
        checked: true,
        depth: 0,
        spans: [
          span("Security audit: ", { bold: true }),
          span("Added contributory X25519 guard and Ed25519 audit-chain entry in "),
          span("crates/orbit-crypto/src/noise.rs", { code: true }),
          span("."),
        ],
      },
      {
        kind: "list_item",
        ordered: false,
        index: 1,
        checked: false,
        depth: 0,
        spans: [
          span("Needs your approval (Tier-3): ", { bold: true }),
          span("Production relay pubkey rotation on "),
          span("linux-build-01", { code: true }),
          span(" — also mirrored to your Feishu interactive card."),
        ],
      },
    ],
  };

  const workCounts = {
    edited_file_count: 2,
    created_file_count: 0,
    command_count: 3,
    read_file_count: 6,
    search_count: 2,
    subagent_count: 3,
    mcp_server_count: 1,
    other_tool_count: 0,
    thought_count: 1,
    narration_count: 0,
    action_steps: 4,
    total_steps: 4,
    failed_count: 0,
    denied_count: 0,
    unrecovered_failure_count: 0,
    additions: 25,
    deletions: 5,
  };

  const stepGroupSegment = {
    kind: "step_group",
    key: "sg_turn_1",
    first_seq: 21,
    items: [toolInspectFleet, toolDispatchMac, toolDispatchLinux, toolDispatchGpu],
    summary: {
      counts: workCounts,
      label: "Dispatched 3 fleet agents · edited 2 files (+25 −5)",
      live_label: "1 fleet agent running on linux-build-01",
      live_detail: "cargo test --workspace (e2e_pitfall_regressions)",
      label_includes_failure: false,
    },
    thought_duration_ms: 2400,
    work_duration_ms: 48200,
    started_at_ms: now - 410_000,
    latest_activity_ms: now - 12_000,
    has_following_content: true,
    has_running_step: true,
  };

  const turn1 = {
    key: "turn-0",
    turn_index: 0,
    anchor_seq: 20,
    user_prompt: {
      item_type: "user_prompt",
      seq: 20,
      message_id: "u_prompt_1",
      text: "Coordinate the v0.10.5 security release across the fleet: audit Noise_NKpsk0 rotation on Mac Studio, run cargo E2E in an isolated worktree on linux-build-01, and verify the WASM client on gpu-h100-cloud. Only ping me on Feishu for Tier-3 approvals.",
      badges: [
        { kind: "slash", label: "/orbit", path: null },
        { kind: "file", label: "noise.rs", path: "crates/orbit-crypto/src/noise.rs" },
      ],
      blocks: [
        {
          kind: "paragraph",
          spans: [
            span(
              "Coordinate the v0.10.5 security release across the fleet: audit Noise_NKpsk0 rotation on Mac Studio, run cargo E2E in an isolated worktree on linux-build-01, and verify the WASM client on gpu-h100-cloud. Only ping me on Feishu for Tier-3 approvals.",
            ),
          ],
        },
      ],
      timestamp_ms: now - 420_000,
    },
    agent_message: null,
    is_compaction_command: false,
    folded_steps: [toolInspectFleet, toolDispatchMac, toolDispatchLinux, toolDispatchGpu],
    work_counts: workCounts,
    work_summary_label: "Dispatched 3 fleet agents · edited 2 files (+25 −5)",
    work_summary_live_label: "1 fleet agent running on linux-build-01",
    work_summary_live_detail: "cargo test --workspace (e2e_pitfall_regressions)",
    work_summary_includes_failure: false,
    thought_duration_ms: 2400,
    work_duration_ms: 48200,
    turn_duration_ms: 52000,
    terminal_reply: assistantReplyItem,
    last_assistant_reply: { ...assistantReplyItem, blocks: [] },
    notices: [],
    compactions: [],
    live_segments: [
      stepGroupSegment,
      {
        kind: "assistant_message",
        key: "seg_msg_chief_summary",
        item: assistantReplyItem,
      },
    ],
    turn_diffs: [
      { path: "crates/orbit-crypto/src/noise.rs", additions: 14, deletions: 3 },
      { path: "crates/orbit-daemon/src/personal_router.rs", additions: 11, deletions: 2 },
    ],
    has_error: false,
    has_unrecovered_error: false,
    has_active_item: true,
    started_at_ms: now - 420_000,
    completed_at_ms: now - 90_000,
    latest_activity_ms: now - 12_000,
    latest_visible_progress_ms: now - 12_000,
  };

  const insights = {
    outline_prompts: [
      { seq: 20, preview: "Coordinate the v0.10.5 security release across the fleet" },
    ],
    plan_steps: [
      {
        id: "plan_1",
        title: "Inspect project context across 3 online fleet hosts",
        status: "completed",
        updated_seq: 22,
      },
      {
        id: "plan_2",
        title: "Audit Noise_NKpsk0 rekeying & audit chain (Mac Studio · Claude Code)",
        status: "completed",
        updated_seq: 28,
      },
      {
        id: "plan_3",
        title: "Run cargo E2E suite in isolated worktree (linux-build-01 · Codex)",
        status: "in_progress",
        active_form: "Running e2e_pitfall_regressions on linux-build-01",
        updated_seq: 36,
      },
      {
        id: "plan_4",
        title: "Approve Tier-3 relay key rotation & tag v0.10.5",
        status: "pending",
        updated_seq: 36,
      },
    ],
    touched_files: [
      {
        path: "crates/orbit-crypto/src/noise.rs",
        display_path: "crates/orbit-crypto/src/noise.rs",
        action: "edited",
        additions: 14,
        deletions: 3,
        last_seq: 28,
      },
      {
        path: "crates/orbit-daemon/src/personal_router.rs",
        display_path: "crates/orbit-daemon/src/personal_router.rs",
        action: "edited",
        additions: 11,
        deletions: 2,
        last_seq: 31,
      },
    ],
    background_tasks: [
      {
        id: "bg_timer_1",
        kind: "wakeup",
        title: "Wakeup on linux-build-01 completion",
        subtitle: "sender:codex-e2ee · armed",
        status: "queued",
        started_at_ms: now - 350_000,
        command: null,
        output_preview: null,
      },
    ],
    subagents: [
      {
        id: "call_sub_mac",
        role: "Security Auditor · Mac Studio",
        prompt_preview: "Audit Noise_NKpsk0 rekeying & Ed25519 audit chain",
        status: "completed",
        child_agent_id: "ag_sub_sec_mac",
        last_seq: 28,
      },
      {
        id: "call_sub_linux",
        role: "Cargo E2E Runner · linux-build-01",
        prompt_preview: "Run cargo test --workspace in isolated worktree",
        status: "running",
        child_agent_id: "ag_sub_build_linux",
        last_seq: 36,
      },
      {
        id: "call_sub_gpu",
        role: "WASM & Patrol Audit · gpu-h100-cloud",
        prompt_preview: "Verify WASM client bundle & Orbit Patrol PR #68",
        status: "completed",
        child_agent_id: "ag_sub_wasm_gpu",
        last_seq: 33,
      },
    ],
    detected_local_urls: [],
    pull_requests: [
      {
        url: "https://github.com/danielgaoyy/orbit/pull/68",
        number: 68,
        repo: "danielgaoyy/orbit",
        title: "feat(security): Noise_NKpsk0 rotation & Tier-3 IM escalation",
        last_seq: 34,
      },
    ],
    detected_links: [],
  };

  const sidebarView = {
    revision: 5,
    needs_attention: [
      {
        host_id: "host_linux_build",
        agent: agentNeedsApproval,
        is_worktree: true,
        is_pinned: false,
        is_unread: true,
        seen_seq: 10,
      },
    ],
    pinned_conversations: [
      {
        host_id: "host_mac_studio",
        agent: agentPersonal,
        is_worktree: false,
        is_pinned: true,
        is_unread: false,
        seen_seq: 42,
      },
    ],
    projects: [
      {
        id: "proj_mac_orbit",
        host_id: "host_mac_studio",
        name: "orbit",
        primary_cwd: "/Users/jess/workspace/orbit",
        directories: ["/Users/jess/workspace/orbit"],
        git_branch: "feat/e2ee-rotation",
        is_git_repo: true,
        pinned_project_id: "proj_mac_orbit",
        is_home_workspace: false,
        sort_weight_ms: now,
        conversations: [
          {
            host_id: "host_mac_studio",
            agent: agentMacMemory,
            is_worktree: false,
            is_pinned: false,
            is_unread: false,
            seen_seq: 21,
          },
        ],
      },
      {
        id: "proj_linux_orbit",
        host_id: "host_linux_build",
        name: "orbit-core",
        primary_cwd: "/srv/builds/orbit-wt-e2ee",
        directories: ["/srv/builds/orbit-wt-e2ee"],
        git_branch: "feat/e2ee-rotation",
        is_git_repo: true,
        pinned_project_id: null,
        is_home_workspace: false,
        sort_weight_ms: now - 10_000,
        conversations: [
          {
            host_id: "host_linux_build",
            agent: agentTimerWatcher,
            is_worktree: true,
            is_pinned: false,
            is_unread: false,
            seen_seq: 9,
          },
        ],
      },
      {
        id: "proj_gpu_wasm",
        host_id: "host_gpu_cloud",
        name: "orbit-wasm-pwa",
        primary_cwd: "/workspace/orbit-web",
        directories: ["/workspace/orbit-web"],
        git_branch: "main",
        is_git_repo: true,
        pinned_project_id: null,
        is_home_workspace: false,
        sort_weight_ms: now - 20_000,
        conversations: [
          {
            host_id: "host_gpu_cloud",
            agent: agentGpuVoice,
            is_worktree: false,
            is_pinned: false,
            is_unread: false,
            seen_seq: 14,
          },
        ],
      },
    ],
    launchpad_projects: [],
  };

  const gitReview = {
    revision: 3,
    host_id: "host_mac_studio",
    cwd: "/Users/jess/workspace/orbit",
    status: {
      cwd: "/Users/jess/workspace/orbit",
      is_git_repo: true,
      branch: "feat/e2ee-rotation",
      upstream: "origin/feat/e2ee-rotation",
      ahead: 2,
      behind: 0,
      files: [
        { path: "crates/orbit-crypto/src/noise.rs", status: "M", staged: true, untracked: false },
        { path: "crates/orbit-daemon/src/personal_router.rs", status: "M", staged: false, untracked: false },
      ],
    },
    selected_path: null,
    raw_diff: "",
    diff_files: diffFiles,
  };

  const hostDetails = {
    host_mac_studio: {
      revision: 3,
      host: hostMac,
      discovered_projects: [],
      directory_listing: null,
      models_by_provider: {},
      workspaces: [wsOrbitMain],
      agents: [agentPersonal, subSecurityMac, agentMacMemory],
      last_created_agent_id: null,
      replaced_optimistic_agents: {},
      quotas: [],
      terminals: [],
      active_terminal_id: null,
      active_terminal_slot: 0,
      artifacts: [],
      goal_runs: [],
      monitor_watches: [],
      workflow_runs: [],
      provider_subagents: {},
    },
    host_linux_build: {
      revision: 3,
      host: hostLinux,
      discovered_projects: [],
      directory_listing: null,
      models_by_provider: {},
      workspaces: [wsLinuxWorktree],
      agents: [subBuildLinux, agentNeedsApproval, agentTimerWatcher],
      last_created_agent_id: null,
      replaced_optimistic_agents: {},
      quotas: [],
      terminals: [],
      active_terminal_id: null,
      active_terminal_slot: 0,
      artifacts: [],
      goal_runs: [],
      monitor_watches: [],
      workflow_runs: [],
      provider_subagents: {},
    },
    host_gpu_cloud: {
      revision: 3,
      host: hostGpu,
      discovered_projects: [],
      directory_listing: null,
      models_by_provider: {},
      workspaces: [wsGpuBench],
      agents: [subWasmGpu, agentGpuVoice],
      last_created_agent_id: null,
      replaced_optimistic_agents: {},
      quotas: [],
      terminals: [],
      active_terminal_id: null,
      active_terminal_slot: 0,
      artifacts: [],
      goal_runs: [],
      monitor_watches: [],
      workflow_runs: [],
      provider_subagents: {},
    },
  };

  const prefs = {
    locale: "en",
    theme,
    sidebarOpen: true,
    sidebarWidth: 256,
    sidePanelWidth: 468,
    overviewDesktopOpenPref: false,
    diffLayout: "unified",
  };

  return `(() => {
    try {
      window.localStorage.clear();
      window.sessionStorage.clear();
      window.localStorage.setItem("orbit.desktop.prefs.v3", ${JSON.stringify(JSON.stringify(prefs))});
      window.localStorage.setItem("orbit.desktop.locale.v1", "en");
      window.localStorage.setItem("orbit.desktop.onboarding.completed.v1", "true");
      window.localStorage.setItem("orbit.desktop.thread_prefs_migrated.v1", "1");
    } catch {}

    // Ensure document theme class matches once DOM element is available
    const syncTheme = () => {
      if (!document.documentElement) return;
      if (${JSON.stringify(theme)} === "dark") {
        document.documentElement.classList.add("dark");
        document.documentElement.style.colorScheme = "dark";
      } else {
        document.documentElement.classList.remove("dark");
        document.documentElement.style.colorScheme = "light";
      }
    };
    if (document.documentElement) syncTheme();
    document.addEventListener("DOMContentLoaded", syncTheme, { once: true });

    const hostDetails = ${JSON.stringify(hostDetails)};
    const fleetHosts = ${JSON.stringify([hostMac, hostLinux, hostGpu])};
    const sidebarView = ${JSON.stringify(sidebarView)};
    const mainAgent = ${JSON.stringify(agentPersonal)};
    const mainTurns = ${JSON.stringify([turn1])};
    const mainInsights = ${JSON.stringify(insights)};
    const gitReview = ${JSON.stringify(gitReview)};
    const diffFiles = ${JSON.stringify(diffFiles)};

    let nextCallbackId = 1;
    const callbacks = new Map();

    window.__TAURI_INTERNALS__ = {
      transformCallback(cb, once = false) {
        const id = nextCallbackId++;
        callbacks.set(id, { cb, once });
        return id;
      },
      unregisterCallback(id) {
        callbacks.delete(id);
      },
      convertFileSrc(filePath) {
        return filePath;
      },
      async invoke(cmd, args = {}) {
        switch (cmd) {
          case "subscribe_core_events":
            return null;
          case "get_bridge_info":
            return { protocol_version: 2, core_version: "0.10.4", capabilities: [] };
          case "get_app_version":
            return "0.10.4";
          case "get_local_machine_info":
            return {
              hostname: "mac-studio.local",
              host_id: "host_mac_studio",
              os_arch: "darwin / aarch64",
            };
          case "get_fleet_overview":
            return {
              revision: 3,
              total_pending_approvals: 1,
              hosts: fleetHosts,
              projects: [],
            };
          case "get_host_detail":
            return hostDetails[args.hostId] ?? hostDetails.host_mac_studio;
          case "get_fleet_sidebar":
            return sidebarView;
          case "get_agent_stage":
            return {
              revision: 5,
              host_id: args.hostId ?? "host_mac_studio",
              agent: mainAgent,
              turns: mainTurns,
              pending_permissions: [],
              pending_questions: [],
              pending_plan_approval: null,
              session_turn_diffs: diffFiles,
              insights: mainInsights,
              latest_seq: 42,
            };
          case "get_agent_stage_delta":
            return {
              revision: 5,
              host_id: args.hostId ?? "host_mac_studio",
              agent: mainAgent,
              replace_from_turn_index: 0,
              window_start_turn_index: 0,
              changed_turns: mainTurns,
              total_turn_count: mainTurns.length,
              has_older_turns: false,
              has_older_history: false,
              oldest_loaded_seq: 20,
              pending_permissions: [],
              pending_questions: [],
              pending_plan_approval: null,
              session_turn_diffs: diffFiles,
              insights: mainInsights,
              latest_seq: 42,
              reset: true,
              aux_revision: 5,
              aux_unchanged: false,
            };
          case "get_git_review":
            return gitReview;
          case "git_pr_status":
            return {
              availability: "available",
              number: 68,
              title: "feat(security): Noise_NKpsk0 rotation & Tier-3 IM escalation",
              url: "https://github.com/danielgaoyy/orbit/pull/68",
              state: "OPEN",
              is_draft: false,
              review_decision: "APPROVED",
              checks: [
                { name: "rust-quality-and-unit", status: "COMPLETED", conclusion: "SUCCESS" },
                { name: "desktop-smoke", status: "COMPLETED", conclusion: "SUCCESS" },
              ],
              passing_checks: 2,
              failing_checks: 0,
              pending_checks: 0,
              review_comments: [],
              unresolved_comment_count: 0,
            };
          case "get_context_occupancy":
            return {
              agent_id: args.agentId ?? "ag_personal_chief",
              model: "claude-opus-4-6",
              context_window_tokens: 200000,
              used_tokens: 46200,
              occupancy_pct: 23.1,
              segments: [],
            };
          case "list_external_editors":
          case "list_ssh_hosts":
          case "get_fallback_provider_catalogs":
          case "get_slash_commands":
          case "list_managed_skills":
          case "list_provider_subagents":
          case "list_conditional_timers":
          case "list_goal_runs":
          case "list_monitor_watches":
          case "list_artifacts":
          case "get_workspace_files":
          case "list_lsp_servers":
          case "lsp_query_symbols":
          case "lsp_get_diagnostics":
          case "list_permission_rules":
          case "search_messages":
          case "get_agent_stage_turns_slice":
            return [];
          case "git_stash_list":
            return { entries: [] };
          case "git_rebase_status":
            return {
              in_progress: false,
              kind: "none",
              conflicted_files: [],
              merge_in_progress: false,
              cherry_pick_in_progress: false,
            };
          case "git_list_conflicts":
            return { files: [] };
          case "list_path_rules":
            return {
              host_id: "host_mac_studio",
              cwd: "/Users/jess/workspace/orbit",
              rules: [],
              total_always_tokens: 0,
              token_budget_warning: false,
            };
          case "list_im_channels":
            return { channels: [], bindings: [], base_webhook_url: "http://127.0.0.1:6868" };
          case "get_guardian_config":
            return {
              config: {
                enabled: true,
                auto_approve_max_risk: "low",
                use_ai_classifier_for_medium: true,
                classifier_timeout_ms: 4000,
                allow_read_only_tools: true,
                allow_workspace_tests: true,
                blocked_command_patterns: [],
                trusted_command_prefixes: [],
                llm_tier2_enabled: true,
                llm_provider: null,
                llm_model: null,
                llm_timeout_ms: 8000,
              },
              recent_audits: [],
            };
          case "list_lifecycle_hooks":
            return { hooks: [], recent_executions: [], project_hooks_trusted: true };
          case "list_browser_sessions":
            return {
              host_id: "host_mac_studio",
              sessions: [],
              policy: {
                enabled: true,
                headless: true,
                allow_localhost: true,
                allowed_domains: [],
                blocked_domains: [],
                block_password_inputs: true,
                max_sessions: 4,
              },
              chrome_binary: null,
            };
          default:
            return null;
        }
      },
    };
  })();`;
}

function findChromeBinary() {
  const envBin = process.env.CHROME_BIN;
  if (envBin) {
    if (fs.existsSync(envBin)) return envBin;
    throw new Error(`CHROME_BIN is set to '${envBin}', but the file does not exist.`);
  }

  const macCandidates = [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
  ];
  for (const candidate of macCandidates) {
    if (fs.existsSync(candidate)) return candidate;
  }

  for (const name of ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser"]) {
    const res = spawnSync("which", [name], { encoding: "utf8" });
    if (res.status === 0) {
      const resolved = res.stdout.trim().split(/\r?\n/)[0];
      if (resolved && fs.existsSync(resolved)) return resolved;
    }
  }

  throw new Error(
    "Could not locate a Chrome/Chromium executable. Set CHROME_BIN or install Chrome/Chromium.",
  );
}

function ensureDesktopDist() {
  const indexPath = path.join(DIST_DIR, "index.html");
  if (fs.existsSync(indexPath)) return;
  console.log("[capture-showcase] apps/desktop/dist missing; running npm run build...");
  const res = spawnSync("npm", ["run", "build"], {
    cwd: DESKTOP_DIR,
    stdio: "inherit",
  });
  if (res.status !== 0 || !fs.existsSync(indexPath)) {
    throw new Error("Failed to build apps/desktop/dist before capturing screenshots.");
  }
}

function startStaticServer(distDir) {
  const indexPath = path.join(distDir, "index.html");
  const server = http.createServer((req, res) => {
    const reqPath = (req.url || "/").split("?")[0];
    let decodedPath;
    try {
      decodedPath = decodeURIComponent(reqPath);
    } catch {
      decodedPath = "/";
    }
    const relPath = decodedPath.replace(/^\/+/, "");
    let filePath = path.resolve(distDir, relPath || "index.html");
    const isInsideDist = filePath === distDir || filePath.startsWith(distDir + path.sep);
    if (!isInsideDist || !fs.existsSync(filePath) || fs.statSync(filePath).isDirectory()) {
      filePath = indexPath;
    }
    const ext = path.extname(filePath).toLowerCase();
    res.writeHead(200, { "Content-Type": MIME_TYPES[ext] || "application/octet-stream" });
    res.end(fs.readFileSync(filePath));
  });

  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const addr = server.address();
      if (!addr || typeof addr === "string") {
        reject(new Error("Failed to bind static server."));
        return;
      }
      resolve({ server, url: `http://127.0.0.1:${addr.port}/` });
    });
  });
}

function waitForDevToolsPort(proc, stderrLogs, timeoutMs = 15_000) {
  return new Promise((resolve, reject) => {
    let buffer = "";
    let settled = false;
    const timer = setTimeout(() => {
      cleanup();
      reject(new Error(`Timed out waiting for Chrome DevTools.\nStderr:\n${stderrLogs.join("")}`));
    }, timeoutMs);
    const onData = (chunk) => {
      const text = chunk.toString("utf8");
      stderrLogs.push(text);
      buffer += text;
      const match = buffer.match(/DevTools listening on (ws:\/\/127\.0\.0\.1:(\d+)\/[^\s]+)/);
      if (match && !settled) {
        settled = true;
        cleanup();
        resolve({ browserWsUrl: match[1], port: Number(match[2]) });
      }
    };
    const onExit = (code, signal) => {
      if (!settled) {
        settled = true;
        cleanup();
        reject(new Error(`Chrome exited prematurely (${code}, ${signal}).`));
      }
    };
    const cleanup = () => {
      clearTimeout(timer);
      proc.stderr?.off("data", onData);
      proc.off("exit", onExit);
    };
    proc.stderr?.on("data", onData);
    proc.on("exit", onExit);
  });
}

async function waitForDevToolsHttp(port) {
  const deadline = Date.now() + 5_000;
  while (Date.now() < deadline) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/json/version`);
      if (res.ok) return;
    } catch {}
    await new Promise((r) => setTimeout(r, 80));
  }
  throw new Error(`DevTools HTTP endpoint on port ${port} did not become ready.`);
}

async function createPageTarget(port) {
  for (const method of ["PUT", "POST"]) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method });
      if (res.ok) {
        const created = await res.json();
        if (created?.webSocketDebuggerUrl) {
          return { id: created.id, wsUrl: created.webSocketDebuggerUrl };
        }
      }
    } catch {}
  }
  const listRes = await fetch(`http://127.0.0.1:${port}/json/list`);
  if (listRes.ok) {
    const targets = await listRes.json();
    const page = Array.isArray(targets)
      ? targets.find((t) => t?.type === "page" && t?.webSocketDebuggerUrl)
      : null;
    if (page?.webSocketDebuggerUrl) return { id: page.id, wsUrl: page.webSocketDebuggerUrl };
  }
  throw new Error("Failed to create Chrome page target.");
}

async function closePageTarget(port, targetId) {
  if (!targetId) return;
  try {
    await fetch(`http://127.0.0.1:${port}/json/close/${targetId}`);
  } catch {}
}

class CdpClient {
  constructor(wsUrl) {
    this.ws = new WebSocket(wsUrl);
    this.nextId = 1;
    this.pending = new Map();
    this.eventHandlers = new Map();
    this.ws.addEventListener("message", (event) => {
      const msg = JSON.parse(String(event.data));
      if (typeof msg.id === "number") {
        const entry = this.pending.get(msg.id);
        if (entry) {
          this.pending.delete(msg.id);
          clearTimeout(entry.timer);
          if (msg.error) entry.reject(new Error(msg.error.message || JSON.stringify(msg.error)));
          else entry.resolve(msg.result);
        }
        return;
      }
      if (msg.method) {
        const handlers = this.eventHandlers.get(msg.method);
        if (handlers) for (const h of handlers) h(msg.params);
      }
    });
  }
  async connect(timeoutMs = 10_000) {
    if (this.ws.readyState === WebSocket.OPEN) return;
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("CDP WebSocket connect timeout")), timeoutMs);
      this.ws.addEventListener("open", () => { clearTimeout(timer); resolve(); }, { once: true });
      this.ws.addEventListener("error", () => { clearTimeout(timer); reject(new Error("CDP WebSocket error")); }, { once: true });
    });
  }
  on(method, handler) {
    let set = this.eventHandlers.get(method);
    if (!set) {
      set = new Set();
      this.eventHandlers.set(method, set);
    }
    set.add(handler);
  }
  send(method, params = {}, timeoutMs = 20_000) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`CDP call '${method}' timed out.`));
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }
  waitForEvent(method, timeoutMs = 15_000) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.eventHandlers.get(method)?.delete(handler);
        reject(new Error(`Timed out waiting for CDP event '${method}'.`));
      }, timeoutMs);
      const handler = (params) => {
        clearTimeout(timer);
        this.eventHandlers.get(method)?.delete(handler);
        resolve(params);
      };
      this.on(method, handler);
    });
  }
  close() {
    for (const entry of this.pending.values()) clearTimeout(entry.timer);
    this.pending.clear();
    if (this.ws.readyState === WebSocket.OPEN || this.ws.readyState === WebSocket.CONNECTING) {
      this.ws.close();
    }
  }
}

async function captureTheme({ port, serverUrl, theme, outputPaths }) {
  const target = await createPageTarget(port);
  const cdp = new CdpClient(target.wsUrl);
  const fatalErrors = [];

  try {
    await cdp.connect();
    await cdp.send("Page.enable");
    await cdp.send("Runtime.enable");
    await cdp.send("Emulation.setDeviceMetricsOverride", {
      width: VIEWPORT_WIDTH,
      height: VIEWPORT_HEIGHT,
      deviceScaleFactor: DEVICE_SCALE_FACTOR,
      mobile: false,
    });
    await cdp.send("Emulation.setEmulatedMedia", {
      features: [
        { name: "prefers-color-scheme", value: theme },
        { name: "prefers-reduced-motion", value: "reduce" },
      ],
    });

    cdp.on("Runtime.exceptionThrown", (params) => {
      fatalErrors.push(params?.exceptionDetails?.exception?.description || params?.exceptionDetails?.text || "Exception");
    });
    cdp.on("Runtime.consoleAPICalled", (params) => {
      if (params?.type !== "error") return;
      const msg = (params.args || []).map((a) => a?.value ?? a?.description ?? "").join(" ");
      if (FATAL_CONSOLE_RE.test(msg)) fatalErrors.push(msg);
    });

    await cdp.send("Page.addScriptToEvaluateOnNewDocument", {
      source: buildShowcaseFixture(theme),
    });

    const loadedPromise = cdp.waitForEvent("Page.loadEventFired", 15_000);
    await cdp.send("Page.navigate", { url: serverUrl });
    await loadedPromise;

    // Wait for initial fleet hydration, open the Review Diff tab via Cmd/Ctrl+D,
    // and add authentic subtle macOS window traffic lights in the reserved 80px titlebar slot.
    const stateRes = await cdp.send("Runtime.evaluate", {
      awaitPromise: true,
      returnByValue: true,
      expression: `(async () => {
        const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
        await sleep(950);

        // Open Review Diff side panel cleanly using the app's native Cmd/Ctrl+D binding
        const isMac = /Mac/.test(navigator.platform || navigator.userAgent);
        window.dispatchEvent(new KeyboardEvent("keydown", {
          key: "d",
          code: "KeyD",
          metaKey: isMac,
          ctrlKey: !isMac,
          bubbles: true,
          cancelable: true,
        }));

        await sleep(850);

        // Render subtle macOS traffic lights inside the reserved 80px native titlebar inset
        if (isMac && !document.getElementById("orbit-showcase-traffic-lights")) {
          const dots = document.createElement("div");
          dots.id = "orbit-showcase-traffic-lights";
          dots.style.cssText =
            "position:fixed;top:14px;left:16px;z-index:60;display:flex;gap:8px;align-items:center;pointer-events:none;";
          for (const [bg, ring] of [
            ["#FF5F57", "rgba(0,0,0,0.14)"],
            ["#FEBC2E", "rgba(0,0,0,0.14)"],
            ["#28C840", "rgba(0,0,0,0.14)"],
          ]) {
            const dot = document.createElement("span");
            dot.style.cssText =
              "width:12px;height:12px;border-radius:9999px;background:" + bg + ";box-shadow:inset 0 0 0 0.5px " + ring + ";";
            dots.appendChild(dot);
          }
          document.body.appendChild(dots);
        }

        await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
        const bodyText = document.body?.innerText || "";
        return {
          hasErrorBoundary: bodyText.includes("Orbit UI Render Error"),
          isDark: document.documentElement.classList.contains("dark"),
          hasPersonalAgent: bodyText.includes("Personal Agent"),
          hasDiff: bodyText.includes("noise.rs"),
          hasPlan: bodyText.includes("Noise_NKpsk0"),
          len: document.getElementById("root")?.innerHTML?.length || 0,
        };
      })()`,
    });

    const summary = stateRes?.result?.value;
    if (!summary || summary.hasErrorBoundary || summary.len < 500) {
      throw new Error(`[${theme}] Showcase DOM validation failed: ${JSON.stringify(summary)}`);
    }
    if (fatalErrors.length > 0) {
      throw new Error(`[${theme}] Fatal page errors: ${fatalErrors.join(" | ")}`);
    }

    const shot = await cdp.send("Page.captureScreenshot", {
      format: "png",
      fromSurface: true,
    });
    const pngBuffer = Buffer.from(shot.data, "base64");
    if (pngBuffer.length < 20_000) {
      throw new Error(`[${theme}] Screenshot unexpectedly small (${pngBuffer.length} bytes).`);
    }

    for (const outPath of outputPaths) {
      fs.mkdirSync(path.dirname(outPath), { recursive: true });
      fs.writeFileSync(outPath, pngBuffer);
    }

    console.log(
      `[capture-showcase] ${theme.toUpperCase()} captured (${(pngBuffer.length / 1024).toFixed(1)} KiB, ${VIEWPORT_WIDTH * DEVICE_SCALE_FACTOR}x${VIEWPORT_HEIGHT * DEVICE_SCALE_FACTOR}) -> ${outputPaths.map((p) => path.relative(REPO_ROOT, p)).join(", ")} | checks=${JSON.stringify(summary)}`,
    );
  } finally {
    try { cdp.close(); } catch {}
    await closePageTarget(port, target.id);
  }
}

async function main() {
  ensureDesktopDist();
  const chromeBin = findChromeBinary();
  const { server, url: serverUrl } = await startStaticServer(DIST_DIR);
  const userDataDir = fs.mkdtempSync(path.join(os.tmpdir(), "orbit-showcase-chrome-"));
  const stderrLogs = [];
  let chromeProc = null;

  try {
    chromeProc = spawn(
      chromeBin,
      [
        "--headless=new",
        "--no-sandbox",
        "--disable-setuid-sandbox",
        "--disable-gpu",
        "--disable-dev-shm-usage",
        "--disable-background-networking",
        "--disable-extensions",
        "--no-first-run",
        "--no-default-browser-check",
        "--hide-scrollbars",
        "--remote-debugging-port=0",
        `--user-data-dir=${userDataDir}`,
        "about:blank",
      ],
      { stdio: ["ignore", "ignore", "pipe"] },
    );

    const { port } = await waitForDevToolsPort(chromeProc, stderrLogs);
    await waitForDevToolsHttp(port);

    await captureTheme({
      port,
      serverUrl,
      theme: "dark",
      outputPaths: [
        path.join(DOCS_ASSETS_DIR, "orbit-desktop-dark.png"),
        path.join(DOCS_SITE_IMAGES_DIR, "orbit-desktop-dark.png"),
      ],
    });

    await captureTheme({
      port,
      serverUrl,
      theme: "light",
      outputPaths: [
        path.join(DOCS_ASSETS_DIR, "orbit-desktop-light.png"),
        path.join(DOCS_SITE_IMAGES_DIR, "orbit-desktop-light.png"),
      ],
    });
  } finally {
    if (chromeProc && chromeProc.exitCode === null) {
      chromeProc.kill("SIGTERM");
      await new Promise((resolve) => {
        const timer = setTimeout(() => {
          if (chromeProc && chromeProc.exitCode === null) chromeProc.kill("SIGKILL");
          resolve();
        }, 1500);
        chromeProc.once("exit", () => {
          clearTimeout(timer);
          resolve();
        });
      });
    }
    await new Promise((resolve) => server.close(() => resolve()));
    try {
      fs.rmSync(userDataDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
    } catch {}
  }
}

main().catch((err) => {
  console.error("[capture-showcase] ERROR:", err);
  process.exit(1);
});
