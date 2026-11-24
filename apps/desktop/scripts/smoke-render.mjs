import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const DIST_DIR = path.resolve(__dirname, "..", "dist");

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

const MOCK_TAURI_INIT_SCRIPT = `(() => {
  try {
    window.localStorage?.clear?.();
    window.sessionStorage?.clear?.();
  } catch {}

  let nextCallbackId = 1;
  const callbacks = new Map();

  const mockHost = {
    host_id: "host_smoke_1",
    display_name: "Smoke Host",
    hostname: "smoke.local",
    custom_name: null,
    is_local: true,
    ssh_alias: null,
    route_mode: "direct",
    remote_daemon_port: 6868,
    custom_direct_endpoint: null,
    via_ssh_tunnel: false,
    last_ping_ms: 1,
    daemon_version: "0.10.5",
    os_arch: "darwin/aarch64",
    daemon_pubkey_b64: "",
    direct_endpoints: ["127.0.0.1:6868"],
    relay_url: null,
    connection_state: "online_direct",
    last_error: null,
    available_providers: ["codex"],
    workspace_count: 1,
    agent_count: 1,
    pending_approval_count: 0,
    supports_server_queue: true,
  };

  const mockWorkspace = {
    workspace_id: "ws_smoke_1",
    cwd: "/tmp/smoke",
    title: "Smoke Workspace",
    is_worktree: false,
    git_branch: "main",
    archived_at: null,
  };

  const mockAgent = {
    agent_id: "ag_smoke_1",
    workspace_id: "ws_smoke_1",
    cwd: "/tmp/smoke",
    title: "Smoke Session",
    provider: "codex",
    provider_label: "Codex",
    model: null,
    effort: null,
    mode: "auto",
    status_label: "Idle",
    is_running: false,
    has_armed_timers: false,
    is_awaiting_permission: false,
    parent_agent_id: null,
    latest_seq: 0,
    updated_at_rfc3339: "2026-11-23T00:00:00Z",
    is_pinned: false,
    seen_seq: 0,
    is_unread: false,
  };

  const emptyInsights = {
    touched_files: [],
    background_tasks: [],
    subagents: [],
    ancestor_lineage: [],
    detected_local_urls: [],
  };

  const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

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
          return { protocol_version: 2, core_version: "0.10.5", capabilities: [] };
        case "get_app_version":
          return "0.10.5";
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
        case "lsp_goto_definition":
        case "lsp_references":
        case "lsp_completion":
        case "lsp_code_action":
        case "restart_lsp_server":
          return [];
        case "read_mcp_resource":
          return {
            uri: typeof args?.uri === "string" ? args.uri : "ui://smoke/app",
            mime_type: "text/html;profile=mcp-app",
            text: "<div>Smoke MCP App</div>",
            blob_base64: null,
          };
        case "call_artifact_mcp_tool":
          return {
            ok: true,
            is_error: false,
            content: [{ type: "text", text: "ok" }],
            structured_content: null,
            error: null,
          };
        case "lsp_hover":
          return null;
        case "lsp_rename":
        case "lsp_apply_workspace_edit":
          return { applied: false, modified_files: [], total_edits: 0 };
        case "get_cloud_sync_status":
          return null;
        case "preview_rewind_agent":
          return {
            agent_id: typeof args?.agentId === "string" ? args.agentId : "ag_smoke_1",
            to_seq: typeof args?.toSeq === "number" ? args.toSeq : 0,
            checkpoint_kind: "none",
            files: [],
            external_conflicts: [],
          };
        case "send_native_approval_notification":
          return {
            delivered: true,
            interactive: false,
            actions_mode: "open_to_review_only",
            backend: "smoke_mock",
          };
        case "get_fleet_overview":
          return {
            revision: 1,
            total_pending_approvals: 0,
            hosts: [mockHost],
            projects: [],
          };
        case "get_host_detail":
          return {
            revision: 1,
            host: mockHost,
            discovered_projects: [],
            directory_listing: null,
            models_by_provider: {},
            workspaces: [mockWorkspace],
            agents: [mockAgent],
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
          };
        case "get_fleet_sidebar":
          return {
            revision: 1,
            projects: [],
            launchpad_projects: [],
            needs_attention: [],
            pinned_conversations: [],
          };
        case "get_agent_stage":
          await delay(80);
          return {
            revision: 1,
            host_id: "host_smoke_1",
            agent: mockAgent,
            turns: [],
            pending_permissions: [],
            pending_questions: [],
            pending_plan_approval: null,
            session_turn_diffs: [],
            insights: emptyInsights,
            latest_seq: 0,
          };
        case "get_agent_stage_delta":
          await delay(80);
          return {
            revision: 1,
            host_id: "host_smoke_1",
            agent: mockAgent,
            replace_from_turn_index: 0,
            window_start_turn_index: 0,
            changed_turns: [],
            total_turn_count: 0,
            has_older_turns: false,
            has_older_history: false,
            oldest_loaded_seq: 0,
            pending_permissions: [],
            pending_questions: [],
            pending_plan_approval: null,
            session_turn_diffs: [],
            insights: emptyInsights,
            latest_seq: 0,
            reset: true,
            aux_revision: 1,
            aux_unchanged: false,
          };
        case "get_agent_stage_turns_slice":
          return [];
        case "resolve_plan_approval":
          return null;
        case "list_permission_rules":
          return [];
        case "upsert_permission_rule":
          return {
            id: args.rule?.id ?? "pr_smoke_1",
            scope_kind: args.rule?.scope_kind ?? "project",
            project_cwd: args.rule?.project_cwd ?? "/tmp/smoke",
            provider: args.rule?.provider ?? null,
            tool_kind: args.rule?.tool_kind ?? "shell",
            pattern_kind: args.rule?.pattern_kind ?? "command_prefix",
            pattern: args.rule?.pattern ?? "cargo test",
            decision: args.rule?.decision ?? "allow",
            source: args.rule?.source ?? "manual",
            note: args.rule?.note ?? null,
            created_at: "2026-11-24T00:00:00Z",
            last_matched_at: null,
            match_count: 0,
          };
        case "delete_permission_rule":
          return true;
        case "resolve_permissions_batch":
          return { resolved_count: 0, skipped_count: 0, skipped_reasons: [] };
        case "load_older_agent_timeline":
          return true;
        case "get_local_machine_info":
          return {
            hostname: "mock-mac",
            host_id: null,
            os_arch: "macos / aarch64",
          };
        case "get_git_review":
          return {
            revision: 1,
            host_id: args.hostId ?? "host_smoke_1",
            cwd: args.cwd ?? "/tmp/smoke",
            status: null,
            selected_path: args.path ?? null,
            raw_diff: "",
            diff_files: [],
          };
        case "git_stash_list":
          return { entries: [] };
        case "git_stash_push":
        case "git_stash_apply":
        case "git_stash_pop":
        case "git_stash_drop":
          return {
            action: cmd.replace("git_stash_", ""),
            stash_ref: args.stashRef ?? "stash@{0}",
            summary: "ok",
            had_conflicts: false,
          };
        case "git_rebase_status":
          return {
            in_progress: false,
            kind: "none",
            conflicted_files: [],
            merge_in_progress: false,
            cherry_pick_in_progress: false,
          };
        case "git_rebase_start":
        case "git_rebase_continue":
        case "git_rebase_abort":
        case "git_rebase_skip":
          return {
            action: cmd.replace("git_rebase_", ""),
            status: {
              in_progress: false,
              kind: "none",
              conflicted_files: [],
              merge_in_progress: false,
              cherry_pick_in_progress: false,
            },
            summary: "ok",
          };
        case "git_list_conflicts":
          return { files: [] };
        case "git_resolve_conflict":
          return {
            path: args.path ?? "",
            remaining_blocks: 0,
            staged: true,
            summary: "ok",
          };
        case "git_pr_status":
          return {
            availability: "no_pull_request",
            is_draft: false,
            checks: [],
            passing_checks: 0,
            failing_checks: 0,
            pending_checks: 0,
            review_comments: [],
            unresolved_comment_count: 0,
          };
        case "git_generate_commit_message":
          return {
            message: "chore: update workspace",
            subject: "chore: update workspace",
            body: null,
            commit_type: "chore",
            scope: null,
            diff_source: "staged",
            files_analyzed: 1,
            generator: "deterministic",
          };
        case "list_path_rules":
          return {
            host_id: args.hostId ?? "host_smoke_1",
            cwd: args.cwd ?? "/tmp/smoke",
            rules: [],
            total_always_tokens: 0,
            token_budget_warning: false,
          };
        case "search_messages":
          return [];
        case "search_messages_remote":
          return { hits: [], remote_hosts_queried: 0, remote_hosts_failed: 0 };
        case "get_context_occupancy":
          return {
            agent_id: args.agentId ?? "ag_smoke_1",
            model: "default",
            context_window_tokens: 200000,
            used_tokens: 0,
            occupancy_pct: 0,
            segments: [],
          };
        case "get_guardian_config":
        case "update_guardian_config":
          return {
            config: args.config ?? {
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
        case "upsert_lifecycle_hook":
        case "delete_lifecycle_hook":
        case "set_project_hooks_trust":
          return {
            hooks: [],
            recent_executions: [],
            project_hooks_trusted: false,
          };
        case "list_im_channels":
        case "upsert_im_channel":
        case "delete_im_channel":
          return {
            channels: [],
            bindings: [],
            base_webhook_url: "http://127.0.0.1:6868",
          };
        case "list_browser_sessions":
          return {
            host_id: args.hostId ?? "host_smoke_1",
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
        case "open_remote_preview_tunnel":
          return {
            host_id: args.hostId ?? "host_smoke_1",
            target_port: args.targetPort ?? 5173,
            proxy_host: args.proxyHost ?? null,
            local_port: args.targetPort ?? 5173,
            local_url: "http://127.0.0.1:" + (args.targetPort ?? 5173),
            is_tunneled: false,
          };
        case "close_remote_preview_tunnel":
          return true;
        case "get_sandbox_config":
        case "update_sandbox_config":
          return args.sandboxPolicy ?? {
            mode: "workspace_write",
            writable_roots: [],
            denied_read_paths: ["~/.ssh", "~/.aws"],
            allow_network: false,
            allowed_domains: [],
            denied_domains: [],
          };
        case "get_audit_chain_status":
          return {
            verified_entries: 42,
            broken_seq: null,
            signed_checkpoints: 4,
          };
        case "export_audit_chain":
          return { jsonl: "", next_after_seq: null };
        case "export_diagnostic_bundle":
          return { ok: true, host_id: args.hostId ?? "host_smoke_1", redacted: true };
        case "compact_agent_context":
          return null;
        case "inspect_external_ide_migration":
        case "execute_external_ide_migration":
          return [];
        case "list_project_memories":
        case "recall_project_memory":
          return [];
        case "delete_project_memory":
          return true;
        case "list_plugins":
          return [];
        case "set_plugin_enabled":
        case "uninstall_plugin":
          return null;
        case "invoke_plugin_op":
          return { ok: true };
        case "render_plugin_ui":
          return { root: { type: "badge", text: "OK", tone: "success" } };
        case "get_voice_config":
        case "update_voice_config":
        case "download_voice_model":
        case "delete_voice_model":
          return {
            config: args.config ?? {
              enabled: false,
              stt_backend: "local_whisper",
              tts_backend: "system_native",
              language: null,
              vad_threshold: 0.012,
              preroll_ms: 300,
              silence_hangover_ms: 750,
              auto_speak_responses: false,
              voice_id: null,
              openai_endpoint_url: null,
              openai_api_key_env: null,
              stt_model_id: "whisper-base-q5",
              tts_model_id: "kokoro-82m-q8",
            },
            models: [],
            capabilities: {
              local_whisper: false,
              local_sense_voice: false,
              local_kokoro: false,
              system_native_tts: true,
              system_client_fallback: true,
            },
          };
        case "transcribe_voice_audio":
        case "parse_voice_transcript":
          return {
            text: typeof args.text === "string" ? args.text : "",
            language: args.language ?? null,
            duration_ms: 0,
            backend_used: "system_client_fallback",
            parsed_intent: null,
          };
        case "approve_voice_permission":
          return "requires_visual_confirmation";
        case "synthesize_voice_speech":
          return {
            audio_wav_base64: "",
            mime_type: "audio/wav",
            duration_ms: 0,
            backend_used: "system_native",
          };
        default:
          return null;
      }
    },
  };
})();`;

const MOCK_TAURI_PAGINATED_INIT_SCRIPT = (() => {
  const makeParagraph = (text) => [
    {
      kind: "paragraph",
      spans: [
        {
          text,
          bold: false,
          italic: false,
          strikethrough: false,
          inline_code: false,
          link_url: null,
        },
      ],
    },
  ];
  const zeroCounts = {
    edited_file_count: 0,
    created_file_count: 0,
    command_count: 0,
    read_file_count: 0,
    search_count: 0,
    subagent_count: 0,
    mcp_server_count: 0,
    other_tool_count: 0,
    thought_count: 0,
    narration_count: 0,
    action_steps: 0,
    total_steps: 0,
    failed_count: 0,
    denied_count: 0,
    unrecovered_failure_count: 0,
    additions: 0,
    deletions: 0,
  };
  const turns = [117, 118, 119].map((idx) => {
    const promptSeq = idx * 3;
    const replySeq = idx * 3 + 1;
    const promptText = `Paginated prompt turn ${idx}`;
    const replyText = `Paginated response ${idx}`;
    const replyItem = {
      item_type: "assistant_message",
      seq: replySeq,
      item_id: `m_pag_${idx}`,
      text: replyText,
      streaming: false,
      blocks: makeParagraph(replyText),
      timestamp_ms: 1732360001000 + idx * 1000,
    };
    return {
      key: `turn-${idx}`,
      turn_index: idx,
      anchor_seq: promptSeq,
      user_prompt: {
        item_type: "user_prompt",
        seq: promptSeq,
        message_id: `u_pag_${idx}`,
        text: promptText,
        badges: [],
        blocks: makeParagraph(promptText),
        timestamp_ms: 1732360000000 + idx * 1000,
      },
      agent_message: null,
      is_compaction_command: false,
      folded_steps: [],
      work_counts: zeroCounts,
      work_summary_label: "",
      work_summary_live_label: "",
      work_summary_live_detail: null,
      work_summary_includes_failure: false,
      thought_duration_ms: null,
      work_duration_ms: null,
      turn_duration_ms: null,
      terminal_reply: replyItem,
      last_assistant_reply: {
        ...replyItem,
        blocks: [],
      },
      notices: [],
      compactions: [],
      live_segments: [
        {
          kind: "assistant_message",
          key: `seg_m_${idx}`,
          item: replyItem,
        },
      ],
      turn_diffs: [],
      has_error: false,
      has_unrecovered_error: false,
      has_active_item: false,
      started_at_ms: 1732360000000 + idx * 1000,
      completed_at_ms: 1732360001000 + idx * 1000,
      latest_activity_ms: 1732360001000 + idx * 1000,
      latest_visible_progress_ms: 1732360001000 + idx * 1000,
    };
  });
  if (turns.length > 0) {
    turns[0].agent_message = {
      item_type: "agent_message",
      seq: 350,
      message_id: "am_smoke_queued_1",
      kind: "finish_notification",
      source_agent_id: "ag_smoke_worker",
      source_host_id: "host_smoke_1",
      source_title: "Codex Worker",
      event: "finished",
      text: "Worker finished cleanly while parent turn was running",
      permission_context: null,
      delivery_status: "queued",
      delivery_turn_id: null,
      blocking_turn_id: "turn_parent_1",
      queued_at_ms: 1732360000500,
      dispatched_at_ms: null,
      settled_at_ms: null,
      blocks: [],
      timestamp_ms: 1732360000500,
    };
  }
  const baseScript = MOCK_TAURI_INIT_SCRIPT.slice(0, -5);
  return `${baseScript}
  const origInvoke = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async (cmd, args = {}) => {
    if (cmd === "get_agent_stage_delta") {
      return {
        revision: 4,
        host_id: "host_smoke_1",
        agent: {
          agent_id: "ag_smoke_1",
          workspace_id: "ws_smoke_1",
          cwd: "/tmp/smoke",
          title: "Smoke Session",
          provider: "codex",
          provider_label: "Codex",
          model: null,
          effort: null,
          mode: "auto",
          status_label: "Idle",
          is_running: false,
          is_awaiting_permission: false,
          parent_agent_id: null,
          latest_seq: 358,
          updated_at_rfc3339: "2026-11-23T00:00:00Z",
          is_pinned: false,
          seen_seq: 358,
          is_unread: false,
        },
        replace_from_turn_index: 117,
        window_start_turn_index: 117,
        changed_turns: ${JSON.stringify(turns)},
        total_turn_count: 120,
        has_older_turns: true,
        has_older_history: true,
        oldest_loaded_seq: 350,
        pending_permissions: [],
        pending_questions: [],
        pending_plan_approval: null,
        session_turn_diffs: [],
        insights: {
          touched_files: [],
          background_tasks: [],
          subagents: [],
          ancestor_lineage: [],
          detected_local_urls: [],
        },
        latest_seq: 358,
        reset: true,
        aux_revision: 1,
        aux_unchanged: false,
      };
    }
    if (cmd === "get_agent_stage_turns_slice" || cmd === "load_older_agent_timeline") {
      return cmd === "load_older_agent_timeline" ? true : [];
    }
    return origInvoke(cmd, args);
  };
})();`;
})();

function findChromeBinary() {
  const envBin = process.env.CHROME_BIN;
  if (envBin) {
    if (fs.existsSync(envBin)) {
      return envBin;
    }
    throw new Error(`CHROME_BIN is set to '${envBin}', but the file does not exist.`);
  }

  const macCandidates = [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
  ];
  for (const candidate of macCandidates) {
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }

  const pathCandidates = [
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
  ];
  for (const name of pathCandidates) {
    const res = spawnSync("which", [name], { encoding: "utf8" });
    if (res.status === 0) {
      const resolved = res.stdout.trim().split(/\r?\n/)[0];
      if (resolved && fs.existsSync(resolved)) {
        return resolved;
      }
    }
  }

  throw new Error(
    "Could not locate a Chrome/Chromium executable. Set CHROME_BIN or install Chrome/Chromium.",
  );
}

function startStaticServer(distDir) {
  const indexPath = path.join(distDir, "index.html");
  if (!fs.existsSync(indexPath)) {
    throw new Error(`Built frontend not found at ${indexPath}. Run 'npm run build' first.`);
  }

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
    const isInsideDist =
      filePath === distDir || filePath.startsWith(distDir + path.sep);

    if (!isInsideDist || !fs.existsSync(filePath) || fs.statSync(filePath).isDirectory()) {
      filePath = indexPath;
    }

    const ext = path.extname(filePath).toLowerCase();
    const contentType = MIME_TYPES[ext] || "application/octet-stream";
    try {
      const body = fs.readFileSync(filePath);
      res.writeHead(200, { "Content-Type": contentType });
      res.end(body);
    } catch (err) {
      res.writeHead(500, { "Content-Type": "text/plain; charset=utf-8" });
      res.end(String(err));
    }
  });

  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const addr = server.address();
      if (!addr || typeof addr === "string") {
        reject(new Error("Failed to bind static server to an ephemeral port."));
        return;
      }
      resolve({
        server,
        url: `http://127.0.0.1:${addr.port}/`,
      });
    });
  });
}

function waitForDevToolsPort(proc, stderrLogs, timeoutMs = 15_000) {
  return new Promise((resolve, reject) => {
    let buffer = "";
    let settled = false;

    const timer = setTimeout(() => {
      cleanup();
      reject(
        new Error(
          `Timed out waiting ${timeoutMs}ms for Chrome DevTools endpoint.\nStderr:\n${stderrLogs.join("")}`,
        ),
      );
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
        reject(
          new Error(
            `Chrome exited prematurely (code=${code}, signal=${signal}).\nStderr:\n${stderrLogs.join("")}`,
          ),
        );
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
    } catch {
      // Retry until DevTools HTTP endpoint responds
    }
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
    } catch {
      // Try fallback method
    }
  }

  const listRes = await fetch(`http://127.0.0.1:${port}/json/list`);
  if (listRes.ok) {
    const targets = await listRes.json();
    const page = Array.isArray(targets)
      ? targets.find((t) => t?.type === "page" && t?.webSocketDebuggerUrl)
      : null;
    if (page?.webSocketDebuggerUrl) {
      return { id: page.id, wsUrl: page.webSocketDebuggerUrl };
    }
  }

  throw new Error(`Failed to create or locate page target on DevTools port ${port}.`);
}

async function closePageTarget(port, targetId) {
  if (!targetId) return;
  try {
    await fetch(`http://127.0.0.1:${port}/json/close/${targetId}`);
  } catch {
    // Ignore close errors during cleanup
  }
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
          if (msg.error) {
            entry.reject(new Error(msg.error.message || JSON.stringify(msg.error)));
          } else {
            entry.resolve(msg.result);
          }
        }
        return;
      }
      if (msg.method) {
        const handlers = this.eventHandlers.get(msg.method);
        if (handlers) {
          for (const handler of handlers) {
            handler(msg.params);
          }
        }
      }
    });
  }

  async connect(timeoutMs = 10_000) {
    if (this.ws.readyState === WebSocket.OPEN) return;
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new Error("Timed out connecting to Chrome DevTools WebSocket."));
      }, timeoutMs);
      this.ws.addEventListener(
        "open",
        () => {
          clearTimeout(timer);
          resolve();
        },
        { once: true },
      );
      this.ws.addEventListener(
        "error",
        () => {
          clearTimeout(timer);
          reject(new Error("Chrome DevTools WebSocket connection error."));
        },
        { once: true },
      );
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

  send(method, params = {}, timeoutMs = 15_000) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`CDP call '${method}' timed out after ${timeoutMs}ms.`));
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }

  waitForEvent(method, timeoutMs = 15_000) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        const set = this.eventHandlers.get(method);
        if (set) set.delete(handler);
        reject(new Error(`Timed out waiting for CDP event '${method}' after ${timeoutMs}ms.`));
      }, timeoutMs);
      const handler = (params) => {
        clearTimeout(timer);
        const set = this.eventHandlers.get(method);
        if (set) set.delete(handler);
        resolve(params);
      };
      this.on(method, handler);
    });
  }

  close() {
    for (const entry of this.pending.values()) {
      clearTimeout(entry.timer);
    }
    this.pending.clear();
    if (
      this.ws.readyState === WebSocket.OPEN ||
      this.ws.readyState === WebSocket.CONNECTING
    ) {
      this.ws.close();
    }
  }
}

function formatRemoteObject(arg) {
  if (!arg || typeof arg !== "object") return String(arg);
  if (arg.value !== undefined) return String(arg.value);
  if (arg.description !== undefined) return String(arg.description);
  if (arg.unserializableValue !== undefined) return String(arg.unserializableValue);
  return `[${arg.type || "object"}]`;
}

async function runPass({ port, serverUrl, name, initScript, assertHealth }) {
  const target = await createPageTarget(port);
  const cdp = new CdpClient(target.wsUrl);
  const exceptions = [];
  const fatalConsoleErrors = [];
  const allConsoleErrors = [];

  try {
    await cdp.connect();

    cdp.on("Runtime.exceptionThrown", (params) => {
      const details = params?.exceptionDetails;
      const text =
        details?.exception?.description ||
        details?.text ||
        JSON.stringify(params);
      exceptions.push(text);
    });

    cdp.on("Runtime.consoleAPICalled", (params) => {
      if (params?.type !== "error") return;
      const formatted = Array.isArray(params.args)
        ? params.args.map(formatRemoteObject).join(" ")
        : "";
      allConsoleErrors.push(formatted);
      if (FATAL_CONSOLE_RE.test(formatted)) {
        fatalConsoleErrors.push(formatted);
      }
    });

    cdp.on("Log.entryAdded", (params) => {
      const entry = params?.entry;
      if (entry?.level === "error" && entry?.source === "javascript") {
        const text = entry.text || JSON.stringify(entry);
        allConsoleErrors.push(text);
        if (FATAL_CONSOLE_RE.test(text)) {
          fatalConsoleErrors.push(text);
        }
      }
    });

    await cdp.send("Page.enable");
    await cdp.send("Runtime.enable");
    await cdp.send("Log.enable");

    if (initScript) {
      await cdp.send("Page.addScriptToEvaluateOnNewDocument", {
        source: initScript,
      });
    }

    const loadedPromise = cdp.waitForEvent("Page.loadEventFired", 15_000);
    await cdp.send("Page.navigate", { url: serverUrl });
    await loadedPromise;

    const evalRes = await cdp.send("Runtime.evaluate", {
      awaitPromise: true,
      returnByValue: true,
      expression: `new Promise((resolve) => {
        setTimeout(() => {
          requestAnimationFrame(() => {
            requestAnimationFrame(() => {
              const root = document.getElementById("root");
              const bodyText = document.body?.innerText || "";
              const errorPre = document.querySelector("pre")?.innerText || "";
              resolve({
                hasErrorBoundary: bodyText.includes("Orbit UI Render Error"),
                errorText: errorPre || bodyText.slice(0, 400),
                bodyText: bodyText.slice(0, 600),
                htmlLen: root?.innerHTML?.length || 0,
                hasMain: Boolean(document.querySelector("main")),
                hasTextarea: Boolean(document.querySelector("textarea")),
              });
            });
          });
        }, 1500);
      })`,
    });

    const health = evalRes?.result?.value;
    if (!health || typeof health !== "object") {
      throw new Error(`[${name}] Failed to evaluate DOM health: ${JSON.stringify(evalRes)}`);
    }

    const failures = [];
    if (exceptions.length > 0) {
      failures.push(
        `Uncaught page exceptions (${exceptions.length}):\n  - ${exceptions.join("\n  - ")}`,
      );
    }
    if (fatalConsoleErrors.length > 0) {
      failures.push(
        `Fatal console errors (${fatalConsoleErrors.length}):\n  - ${fatalConsoleErrors.join("\n  - ")}`,
      );
    }
    if (health.hasErrorBoundary) {
      failures.push(`RootErrorBoundary rendered 'Orbit UI Render Error': ${health.errorText}`);
    }
    if (health.htmlLen < 200) {
      failures.push(`#root innerHTML length too small (${health.htmlLen} < 200).`);
    }
    for (const err of assertHealth(health)) {
      failures.push(err);
    }

    return {
      name,
      ok: failures.length === 0,
      failures,
      health,
      consoleErrorCount: allConsoleErrors.length,
    };
  } finally {
    try {
      cdp.close();
    } catch {
      // ignore
    }
    await closePageTarget(port, target.id);
  }
}

async function main() {
  const chromeBin = findChromeBinary();
  const { server, url: serverUrl } = await startStaticServer(DIST_DIR);
  const userDataDir = fs.mkdtempSync(path.join(os.tmpdir(), "orbit-smoke-chrome-"));
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
        "--remote-debugging-port=0",
        `--user-data-dir=${userDataDir}`,
        "about:blank",
      ],
      { stdio: ["ignore", "ignore", "pipe"] },
    );

    const { port } = await waitForDevToolsPort(chromeProc, stderrLogs);
    await waitForDevToolsHttp(port);

    const passes = [
      await runPass({
        port,
        serverUrl,
        name: "bare-browser-empty-host",
        initScript: null,
        assertHealth: (health) => {
          const errs = [];
          if (!health.hasMain) {
            errs.push("Expected <main> landmark in bare browser render.");
          }
          if (!health.bodyText.includes("Pair or select an Orbit host to begin.")) {
            errs.push(
              `Expected empty-host onboarding prompt in bare browser render, got: ${JSON.stringify(health.bodyText)}`,
            );
          }
          return errs;
        },
      }),
      await runPass({
        port,
        serverUrl,
        name: "mocked-tauri-active-session",
        initScript: MOCK_TAURI_INIT_SCRIPT,
        assertHealth: (health) => {
          const errs = [];
          if (!health.hasMain || !health.hasTextarea) {
            errs.push(
              `Expected active conversation landmarks missing (hasMain=${health.hasMain}, hasTextarea=${health.hasTextarea}).`,
            );
          }
          return errs;
        },
      }),
      await runPass({
        port,
        serverUrl,
        name: "mocked-tauri-paginated-large-session",
        initScript: MOCK_TAURI_PAGINATED_INIT_SCRIPT,
        assertHealth: (health) => {
          const errs = [];
          if (!health.hasMain || !health.hasTextarea) {
            errs.push(
              `Expected paginated conversation landmarks missing (hasMain=${health.hasMain}, hasTextarea=${health.hasTextarea}).`,
            );
          }
          if (!health.bodyText.includes("Paginated response 119")) {
            errs.push(
              `Expected tail paginated assistant response to render in DOM, got: ${JSON.stringify(health.bodyText)}`,
            );
          }
          return errs;
        },
      }),
    ];

    let anyFailed = false;
    for (const pass of passes) {
      if (pass.ok) {
        console.log(
          `[smoke-render] PASS (${pass.name}): htmlLen=${pass.health.htmlLen}, hasMain=${pass.health.hasMain}, hasTextarea=${pass.health.hasTextarea}, hasErrorBoundary=${pass.health.hasErrorBoundary}, consoleErrors=${pass.consoleErrorCount}`,
        );
      } else {
        anyFailed = true;
        console.error(`[smoke-render] FAIL (${pass.name}):`);
        for (const f of pass.failures) {
          console.error(`  ${f}`);
        }
      }
    }

    if (anyFailed) {
      process.exitCode = 1;
    }
  } finally {
    if (chromeProc && chromeProc.exitCode === null) {
      chromeProc.kill("SIGTERM");
      await new Promise((resolve) => {
        const timer = setTimeout(() => {
          if (chromeProc && chromeProc.exitCode === null) {
            chromeProc.kill("SIGKILL");
          }
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
      fs.rmSync(userDataDir, {
        recursive: true,
        force: true,
        maxRetries: 5,
        retryDelay: 100,
      });
    } catch {
      // Best-effort temp profile cleanup; Chrome helper processes may still flush briefly on exit.
    }
  }
}

main().catch((err) => {
  console.error("[smoke-render] ERROR:", err);
  process.exit(1);
});
