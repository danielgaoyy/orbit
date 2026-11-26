import type {
  AgentStageView,
  CoreNotification,
  HostCardView,
  HostDetailView,
  OrbitTransportBackend,
} from "./orbit-ipc";
import { WebProfileStore } from "./web-profile-store";

type Args = Record<string, unknown>;
interface Profile {
  host_id: string;
  hostname: string;
  daemon_pubkey_b64: string;
  pairing_secret: string;
  direct_endpoints: string[];
  relay_url?: string;
}
interface Noise {
  write_handshake_e(): Uint8Array;
  read_handshake_ee_es(bytes: Uint8Array): void;
  encrypt_wire_json(json: string): Uint8Array;
  decrypt_wire_json(bytes: Uint8Array): string;
  free(): void;
}
interface HostModel {
  receive(json: string): string | undefined;
  snapshot(): HostDetailView;
  git_review(cwd: string, path?: string): unknown;
  card(): HostCardView;
  stage(id: string): AgentStageView;
  stage_delta(id: string, revision: number, limit?: number): unknown;
  timeline(id: string): unknown;
  events(id: string): unknown[];
  oldest(id: string): number;
  cursor(id: string): number;
  connection(state: string, message?: string): void;
  free(): void;
}
export interface BrowserBindings {
  WasmNoiseSession: { create_initiator(key: string, secret: string): Noise };
  WasmHost: new (profile: Profile, cached: unknown) => HostModel;
  parse_pairing_uri(text: string): Profile;
  project_fleet_sidebar(data: unknown, req: unknown): unknown;
  get_personal_agent_briefing(
    hosts: HostDetailView[],
    primaryRouterHostId?: string | null,
    nowMs?: number | null,
  ): unknown;
  compose_prompt(req: unknown): unknown;
  provider_catalogs(live: unknown): unknown;
  resolve_model_selection(req: unknown, live: unknown): unknown;
  slash_commands(query: string): unknown;
  render_diagram_svg(req: unknown): unknown;
  export_transcript(meta: unknown, events: unknown, opts: unknown): unknown;
  fleet_overview(revision: number, cards: HostCardView[]): unknown;
  bridge_info(): {
    protocol_version: number;
    core_version: string;
    capabilities: string[];
  };
  render_markdown_blocks(text: string, streaming: boolean): unknown;
  protocol_version(): number;
}

type Wire = Args & {
  op: string;
  request_id?: string;
  agent?: { agent_id: string };
};
type Reply = {
  resolve: (msg: Wire) => void;
  reject: (err: Error) => void;
  timer: number;
};
interface Host {
  profile: Profile;
  model: HostModel;
  subscriptions: Set<string>;
  workspaceFiles: Map<string, string[]>;
  pending: Map<string, Reply>;
  socket?: WebSocket;
  noise?: Noise;
  connecting?: Promise<void>;
  saveTimer?: number;
  writes: Promise<void>;
  active: boolean;
}

function routes(p: Profile) {
  const entries = [
    ...p.direct_endpoints.map((raw) => ({ raw, relay: false })),
    { raw: p.relay_url, relay: true },
  ];
  return entries.flatMap(({ raw, relay }) => {
    if (!raw) return [];
    try {
      const url = new URL(raw);
      if (
        !["wss:", "ws:"].includes(url.protocol) ||
        url.username ||
        url.password ||
        url.search ||
        url.hash
      )
        return [];
      const base = url.pathname.replace(/\/+$/, "");
      url.pathname = relay
        ? base + "/v1/client/" + encodeURIComponent(p.daemon_pubkey_b64)
        : base.replace(/\/ws(?:\/e2ee)?$/, "") + "/ws/e2ee";
      return [{ url, relay }];
    } catch {
      return [];
    }
  });
}

export class WasmWebBackend implements OrbitTransportBackend {
  private hosts = new Map<string, Host>();
  private handler?: (ev: CoreNotification) => void;
  private revision = 0;

  private constructor(
    private wasm: BrowserBindings,
    private db: WebProfileStore,
  ) {}

  static async open(wasm: BrowserBindings) {
    const db = await WebProfileStore.open();
    const backend = new WasmWebBackend(wasm, db);
    for (const id of await db.hosts()) {
      const p = await db.read<Profile>("profile/" + id).catch(() => null);
      if (!p || p.host_id !== id) continue;
      const snapshot = await db.read("snapshot/" + id).catch(() => null);
      let host: Host;
      try {
        host = backend.host(p, snapshot);
      } catch {
        host = backend.host(p, null);
      }
      backend.hosts.set(id, host);
      if (navigator.onLine) void backend.connect(host).catch(() => undefined);
    }
    return backend;
  }

  get paired(): boolean {
    return this.hosts.size > 0;
  }
  private host(profile: Profile, cached: unknown): Host {
    return {
      profile,
      model: new this.wasm.WasmHost(profile, cached),
      active: true,
      subscriptions: new Set(),
      workspaceFiles: new Map(),
      pending: new Map(),
      writes: Promise.resolve(),
    };
  }

  async pair(uri: string) {
    // Publish a replacement only after the pinned peer has authenticated and storage committed.
    const p = this.wasm.parse_pairing_uri(uri.trim());
    const h = this.host(p, null);
    try {
      await this.connect(h);
      await this.db.write("profile/" + p.host_id, p);
    } catch (err) {
      this.retire(h);
      throw err;
    }
    const previous = this.hosts.get(p.host_id);
    if (previous) this.retire(previous);
    this.hosts.set(p.host_id, h);
    this.notify(h);
    return h.model.card();
  }

  private retire(h: Host) {
    h.active = false;
    clearTimeout(h.saveTimer);
    h.socket?.close();
    h.model.free();
  }

  async subscribe(handler: (ev: CoreNotification) => void) {
    this.handler = handler;
  }

  private notify(h: Host, agent?: string) {
    if (this.hosts.get(h.profile.host_id) !== h) return;
    const revision = ++this.revision;
    const host_id = h.profile.host_id;
    if (agent) {
      this.handler?.({
        kind: "timeline_changed",
        revision,
        host_id,
        agent_id: agent,
      });
    } else {
      this.handler?.({ kind: "host_changed", revision, host_id });
      this.handler?.({ kind: "fleet_changed", revision });
      if (h.saveTimer === undefined)
        h.saveTimer = window.setTimeout(() => {
          h.saveTimer = undefined;
          h.writes = h.writes
            .then(() =>
              h.active
                ? this.db.write("snapshot/" + host_id, h.model.snapshot())
                : undefined,
            )
            .catch(() => undefined); // Live views still work with an exhausted browser quota.
        }, 400);
    }
  }

  private connect(h: Host): Promise<void> {
    if (h.connecting) return h.connecting;
    if (h.noise && h.socket?.readyState === WebSocket.OPEN)
      return Promise.resolve();
    h.connecting = (async () => {
      let failure = new Error("No browser WebSocket route in this pairing");
      for (const route of routes(h.profile)) {
        if (!h.active) throw new Error("Host removed");
        try {
          await this.dial(h, route.url, route.relay);
          return;
        } catch (err) {
          failure = err instanceof Error ? err : new Error(String(err));
        }
      }
      if (h.active) {
        h.model.connection("disconnected", failure.message);
        this.notify(h);
      }
      throw failure;
    })().finally(() => {
      h.connecting = undefined;
    });
    return h.connecting;
  }

  private dial(h: Host, url: URL, relay: boolean): Promise<void> {
    return new Promise((resolve, reject) => {
      const ws = new WebSocket(url);
      ws.binaryType = "arraybuffer";
      h.socket = ws;
      let phase: "handshake" | "welcome" | "online" = "handshake";
      let noise: Noise | undefined;
      let heartbeat: number | undefined;
      let finished = false;
      const fail = (err: Error) => {
        if (finished) return;
        finished = true;
        clearTimeout(timeout);
        clearInterval(heartbeat);
        if (h.socket === ws) {
          h.socket = undefined;
          h.noise = undefined;
          for (const req of h.pending.values()) {
            clearTimeout(req.timer);
            req.reject(err);
          }
          h.pending.clear();
          if (h.active && phase === "online") {
            h.model.connection("disconnected", err.message);
            this.notify(h);
          }
        }
        noise?.free();
        ws.close();
        reject(err);
      };
      const timeout = window.setTimeout(
        () => fail(new Error("Host handshake timed out")),
        9000,
      );
      ws.onclose = ws.onerror = () => fail(new Error("Host connection closed"));
      ws.onopen = () => {
        if (finished || !h.active) return;
        try {
          noise = this.wasm.WasmNoiseSession.create_initiator(
            h.profile.daemon_pubkey_b64,
            h.profile.pairing_secret,
          );
          ws.send(new Uint8Array(noise.write_handshake_e()));
        } catch (err) {
          fail(new Error(String(err)));
        }
      };
      ws.onmessage = ({ data }) => {
        try {
          if (finished) return;
          if (!h.active || !(data instanceof ArrayBuffer) || !noise)
            throw new Error("Invalid host stream");
          if (phase === "handshake") {
            noise.read_handshake_ee_es(new Uint8Array(data));
            ws.send(
              new Uint8Array(
                noise.encrypt_wire_json(
                  JSON.stringify({
                    op: "hello",
                    client_id: crypto.randomUUID(),
                    client_name: "Orbit Web",
                    protocol_version: this.wasm.protocol_version(),
                    supports_zstd: false,
                  }),
                ),
              ),
            );
            phase = "welcome";
            return;
          }
          const json = noise.decrypt_wire_json(new Uint8Array(data));
          const msg: Wire = JSON.parse(json);
          if (phase === "welcome" && msg.op !== "welcome")
            throw new Error("Expected authenticated host welcome");
          const changed = h.model.receive(json);
          if (phase === "welcome") {
            phase = "online";
            h.noise = noise;
            h.model.connection(relay ? "online_relay" : "online_direct");
            clearTimeout(timeout);
            heartbeat = window.setInterval(() => {
              try {
                this.send(h, { op: "ping", ts_ms: Date.now() });
              } catch {
                fail(new Error("Host disconnected"));
              }
            }, 10000);
            this.refresh(h);
            for (const id of h.subscriptions)
              void this.timeline(h, id).catch(() => undefined);
            resolve();
          }
          if (
            msg.op === "git_status_response" ||
            msg.op === "git_diff_response"
          ) {
            const cwd =
              msg.op === "git_diff_response"
                ? msg.cwd
                : (msg.status as { cwd: string }).cwd;
            this.handler?.({
              kind: "git_changed",
              revision: ++this.revision,
              host_id: h.profile.host_id,
              cwd: String(cwd),
            });
          }
          if (changed != null) this.notify(h, changed);
          const req = msg.request_id && h.pending.get(msg.request_id);
          if (req && msg.request_id) {
            h.pending.delete(msg.request_id);
            clearTimeout(req.timer);
            if (msg.op === "rpc_error") {
              const code = typeof msg.code === "string" ? msg.code : "";
              const message = String(msg.message ?? "Host RPC error");
              const err = new Error(
                code.startsWith("voice_")
                  ? JSON.stringify({ code, message })
                  : code
                    ? `[${code}] ${message}`
                    : message,
              ) as Error & { code?: string };
              if (code) err.code = code;
              req.reject(err);
            } else req.resolve(msg);
          }
        } catch (err) {
          fail(new Error(String(err)));
        }
      };
    });
  }

  private send(h: Host, wire: Wire) {
    if (!h.noise || h.socket?.readyState !== WebSocket.OPEN)
      throw new Error("Host offline");
    const ct = new Uint8Array(h.noise.encrypt_wire_json(JSON.stringify(wire)));
    try {
      h.socket.send(ct);
    } catch (err) {
      h.socket?.close();
      throw err;
    }
  }
  private async rpc(
    h: Host,
    op: string,
    args: Args = {},
    timeoutMs = 30000,
  ): Promise<Wire> {
    await this.connect(h);
    const request_id = crypto.randomUUID();
    return new Promise((resolve, reject) => {
      const timer = window.setTimeout(() => {
        h.pending.delete(request_id);
        reject(new Error("Host request timed out"));
      }, timeoutMs);
      h.pending.set(request_id, { resolve, reject, timer });
      try {
        this.send(h, { ...args, op, request_id });
      } catch (err) {
        clearTimeout(timer);
        h.pending.delete(request_id);
        reject(err);
      }
    });
  }
  // Native refresh commands enqueue work and notify on response. Cached views remain usable offline.
  private requestRefresh(h: Host, op: string, args: Args = {}) {
    if (!navigator.onLine) return;
    void this.rpc(h, op, args).catch(() => undefined);
  }
  private refresh(h: Host) {
    for (const op of [
      "list_agents",
      "list_workspaces",
      "discover_projects",
      "list_models",
      "list_provider_models",
      "list_pending_permissions",
      "list_goal_runs",
    ])
      this.send(h, { op, request_id: crypto.randomUUID() });
  }
  private async timeline(h: Host, id: string) {
    h.subscriptions.add(id);
    let more: boolean;
    do {
      const since_seq = h.model.cursor(id);
      const reply = await this.rpc(h, "subscribe_timeline", {
        agent_id: id,
        since_seq,
        tail_limit: since_seq ? null : 300,
      });
      more = reply.has_more === true && h.model.cursor(id) > since_seq;
    } while (more);
  }

  async invoke<T>(
    command: string,
    args: import("@tauri-apps/api/core").InvokeArgs = {},
  ): Promise<T> {
    if (
      ArrayBuffer.isView(args) ||
      args instanceof ArrayBuffer ||
      Array.isArray(args)
    )
      throw new Error("Binary native IPC is unavailable in Orbit Web");
    return (await this.dispatch(command, args)) as T;
  }

  private async dispatch(cmd: string, a: Args): Promise<unknown> {
    const hosts = [...this.hosts.values()];
    const req = a.req as Args | undefined;
    const id = String(a.hostId ?? req?.host_id ?? "");
    const agent = String(a.agentId ?? "");
    const target = this.hosts.get(id);
    switch (cmd) {
      case "get_bridge_info":
        return this.wasm.bridge_info();
      case "get_app_version":
        return this.wasm.bridge_info().core_version;
      case "parse_markdown_preview":
        return this.wasm.render_markdown_blocks(
          String(a.markdown ?? ""),
          Boolean(a.streaming),
        );
      case "get_fleet_overview":
        return this.wasm.fleet_overview(
          this.revision,
          hosts.map((h) => h.model.card()),
        );
      case "get_fleet_sidebar":
        return this.wasm.project_fleet_sidebar(
          {
            revision: this.revision,
            hosts: hosts.map((h) => h.model.snapshot()),
          },
          req,
        );
      case "get_personal_agent_briefing":
        return this.wasm.get_personal_agent_briefing(
          hosts.map((h) => h.model.snapshot()),
          a.primaryRouterHostId == null ? null : String(a.primaryRouterHostId),
          Date.now(),
        );
      case "pair_host":
        return this.pair(String(a.pairingUri));
      case "get_host_detail":
        return target?.model.snapshot() ?? null;
      case "connect_all_hosts":
      case "on_network_changed":
      case "on_app_foreground":
        return Promise.allSettled(hosts.map((h) => this.connect(h)));
      case "set_account_default_mode": {
        const mode = a.mode as string | undefined;
        const updatedAtMs = typeof a.updatedAtMs === "number" ? a.updatedAtMs : 0;
        if (!mode || mode === "plan") return;
        await Promise.allSettled(
          hosts.map((host) =>
            this.rpc(host, "set_account_default_mode", {
              mode,
              updated_at_ms: updatedAtMs,
            }).catch(() => {}),
          ),
        );
        return;
      }
      case "on_app_background":
        return;
      case "get_fallback_provider_catalogs":
        return this.wasm.provider_catalogs([]);
      case "resolve_model_selection":
        return this.wasm.resolve_model_selection(
          req,
          this.hosts.get(String(req?.host_id))?.model.snapshot()
            .provider_catalogs ?? [],
        );
      case "get_slash_commands":
        return this.wasm.slash_commands(String(a.query ?? ""));
      case "compose_prompt":
        return this.wasm.compose_prompt(req);
      case "render_diagram":
        return this.wasm.render_diagram_svg(req);
      case "list_external_editors":
      case "list_ssh_hosts":
        return [];
      case "get_local_machine_info":
      case "get_cloud_sync_status":
        return null;
    }
    if (!target)
      throw new Error(
        id ? "Unknown paired host" : "This action is unavailable in Orbit Web",
      );
    const h = target;
    switch (cmd) {
      case "connect_host":
        return this.connect(h);
      case "update_host_config": {
        if (a.reconnect) {
          await this.connect(h);
        }
        return h.model.card();
      }
      case "refresh_host":
        await this.connect(h);
        return this.refresh(h);
      case "remove_host":
        this.retire(h);
        this.hosts.delete(id);
        await h.writes;
        await this.db.remove(id);
        this.handler?.({ kind: "fleet_changed", revision: ++this.revision });
        return;
      case "open_agent_timeline":
        return this.timeline(h, agent);
      case "get_workspace_files": {
        const cwd = String(a.cwd);
        const cached = h.workspaceFiles.get(cwd);
        if ((cached && !a.force) || !navigator.onLine) return cached ?? [];
        try {
          const reply = await this.rpc(h, "list_workspace_files", {cwd});
          const files = reply.files as string[];
          h.workspaceFiles.set(cwd, files);
          return files;
        } catch { return cached ?? []; }
      }
      case "search_workspace_symbols": {
        const reply = await this.rpc(h,cmd,{cwd:a.cwd,query:a.query,limit:a.limit});
        return {host_id:id,cwd:reply.cwd,symbols:reply.symbols,truncated:reply.truncated};
      }
      case "fetch_git_status":
        return this.requestRefresh(h, "get_git_status", { cwd: a.cwd });
      case "fetch_git_diff":
        return this.requestRefresh(h, "get_git_diff", {
          cwd: a.cwd,
          path: a.path,
          staged: a.staged ?? false,
        });
      case "get_git_review":
        return h.model.git_review(
          String(a.cwd),
          a.path == null ? undefined : String(a.path),
        );
      case "get_agent_stage":
        return h.model.stage(agent);
      case "get_agent_stage_turns_slice":
        return h.model
          .stage(agent)
          .turns.slice(
            Number(a.startTurnIndex),
            Number(a.startTurnIndex) + Number(a.count),
          );
      case "get_agent_stage_delta":
        return h.model.stage_delta(
          agent,
          Number(a.sinceRevision),
          a.tailTurnLimit == null ? undefined : Number(a.tailTurnLimit),
        );
      case "get_agent_timeline":
        return h.model.timeline(agent);
      case "load_older_agent_timeline": {
        const before_seq = h.model.oldest(agent);
        if (!before_seq) return false;
        await this.rpc(h, "fetch_timeline_before", {
          agent_id: agent,
          before_seq,
          limit: a.limitEvents,
        });
        return true;
      }
      case "export_agent_transcript": {
        const card = h.model
          .snapshot()
          .agents.find((x) => x.agent_id === agent);
        return this.wasm.export_transcript(
          {
            host_id: id,
            agent_id: agent,
            title: card?.title ?? agent,
            provider: card?.provider,
            cwd: card?.cwd,
          },
          h.model.events(agent),
          a.options ?? {},
        );
      }
      case "refresh_quotas":
        return this.requestRefresh(h, "refresh_quotas");
      case "refresh_provider_models":
        return this.requestRefresh(h, "list_provider_models", {
          force_refresh: a.forceRefresh,
        });
      case "mark_agent_seen":
      case "mark_agent_read":
        return this.rpc(h, "mark_agent_read", {
          agent_id: agent,
          ...(a.seq != null ? { seen_seq: a.seq } : {}),
        });
      case "list_directory":
        return this.rpc(h, cmd, { path: a.path });
      case "mark_agent_unread":
      case "interrupt_agent":
      case "interrupt_agent_and_continue":
      case "set_agent_pinned":
      case "set_session_mode":
      case "set_agent_config":
      case "resolve_permission":
      case "answer_question":
      case "send_prompt":
      case "create_agent":
      case "enqueue_prompt":
      case "remove_queued_prompt":
      case "flush_queued_prompt_now":
      case "reorder_queued_prompts": {
        const fields: Args = {};
        for (const [key, value] of Object.entries(a)) {
          if (key !== "hostId" && value != null)
            fields[key.replace(/[A-Z]/g, (c) => "_" + c.toLowerCase())] = value;
        }
        if (cmd === "create_agent") fields.worktree = !!a.worktreeBranch;
        if (cmd === "enqueue_prompt") fields.queue_id ??= crypto.randomUUID();
        if (cmd === "interrupt_agent_and_continue") fields.preserve_queue = true;
        const wireOp =
          cmd === "interrupt_agent" || cmd === "interrupt_agent_and_continue"
            ? "interrupt_turn"
            : cmd;
        const reply = await this.rpc(h, wireOp, fields);
        if (cmd === "create_agent") return reply.agent?.agent_id;
        if (cmd === "enqueue_prompt") return fields.queue_id;
        return;
      }
      // --- M3 Security, Audit Chain, Diagnostics, Context Compaction & IDE Migration ---
      case "get_sandbox_config": {
        const reply = await this.rpc(h, "get_sandbox_config");
        return reply.sandbox_policy ?? null;
      }
      case "update_sandbox_config": {
        const reply = await this.rpc(h, "update_sandbox_config", {
          sandbox_policy: a.sandboxPolicy ?? null,
        });
        return reply.sandbox_policy ?? null;
      }
      case "get_audit_chain_status": {
        const reply = await this.rpc(h, "get_audit_chain_status");
        return reply.verification;
      }
      case "export_audit_chain": {
        const reply = await this.rpc(h, "export_audit_chain", {
          after_seq: Number(a.afterSeq ?? 0),
        });
        return {
          jsonl: String(reply.jsonl ?? ""),
          next_after_seq: reply.next_after_seq ?? null,
        };
      }
      case "export_diagnostic_bundle": {
        const reply = await this.rpc(h, "export_diagnostic_bundle");
        return reply.bundle ?? {};
      }
      case "compact_agent_context": {
        await this.rpc(h, "compact_agent_context", { agent_id: agent }, 60000);
        return;
      }
      case "inspect_external_ide_migration": {
        const reply = await this.rpc(h, "inspect_external_ide_migration", {
          from: a.source ?? null,
          cwd: a.cwd ?? null,
          conflict: a.conflict ?? "skip",
        });
        return reply.items ?? [];
      }
      case "execute_external_ide_migration": {
        const reply = await this.rpc(
          h,
          "execute_external_ide_migration",
          {
            from: a.source ?? null,
            cwd: a.cwd ?? null,
            conflict: a.conflict ?? "skip",
          },
          45000,
        );
        return reply.items ?? [];
      }
      // --- M4 Project Memory ---
      case "list_project_memories": {
        const reply = await this.rpc(h, "list_project_memories", {
          cwd: String(a.cwd ?? ""),
          status: a.status ?? null,
          category: a.category ?? null,
          query: a.query ?? null,
          limit: a.limit ?? null,
        });
        return reply.items ?? [];
      }
      case "upsert_project_memory": {
        const r = (a.req ?? {}) as Args;
        const reply = await this.rpc(h, "upsert_project_memory", {
          cwd: String(r.cwd ?? ""),
          id: r.id ?? null,
          title: String(r.title ?? ""),
          content: String(r.content ?? ""),
          category: r.category,
          status: r.status ?? null,
          file_paths: r.file_paths ?? [],
          tags: r.tags ?? [],
        });
        return reply.item;
      }
      case "confirm_project_memory": {
        const reply = await this.rpc(h, "confirm_project_memory", {
          cwd: String(a.cwd ?? ""),
          id: String(a.memoryId ?? ""),
        });
        return reply.item;
      }
      case "delete_project_memory": {
        const reply = await this.rpc(h, "delete_project_memory", {
          cwd: String(a.cwd ?? ""),
          id: String(a.memoryId ?? ""),
        });
        return Boolean(reply.deleted);
      }
      case "recall_project_memory": {
        const reply = await this.rpc(h, "recall_project_memories", {
          cwd: String(a.cwd ?? ""),
          query: String(a.query ?? ""),
          limit: a.limit ?? null,
          include_unconfirmed: a.includeUnconfirmed ?? null,
        });
        return reply.hits ?? [];
      }
      // --- M5 WASM Plugins ---
      case "list_plugins": {
        const reply = await this.rpc(h, "list_plugins");
        return reply.plugins ?? [];
      }
      case "install_plugin": {
        const reply = await this.rpc(
          h,
          "install_plugin",
          {
            source: String(a.sourcePath ?? ""),
            enable: Boolean(a.enable),
          },
          120000,
        );
        return reply.plugin;
      }
      case "set_plugin_enabled": {
        await this.rpc(h, "set_plugin_enabled", {
          plugin_id: String(a.pluginId ?? ""),
          enabled: Boolean(a.enabled),
        });
        return;
      }
      case "uninstall_plugin": {
        await this.rpc(h, "uninstall_plugin", {
          plugin_id: String(a.pluginId ?? ""),
        });
        return;
      }
      case "invoke_plugin_op": {
        const reply = await this.rpc(h, "invoke_plugin_op", {
          plugin_id: String(a.pluginId ?? ""),
          operation: String(a.op ?? ""),
          cwd: a.cwd ?? null,
          input: a.argsJson ?? {},
        });
        return reply.result;
      }
      case "render_plugin_ui": {
        const reply = await this.rpc(h, "render_plugin_ui", {
          plugin_id: String(a.pluginId ?? ""),
          slot: a.slot,
          cwd: a.cwd ?? null,
          action_id: a.actionId ?? null,
          payload: a.payload ?? null,
        });
        return reply.spec;
      }
      // --- M6 Voice Control Plane ---
      case "get_voice_config": {
        const reply = await this.rpc(h, "get_voice_config");
        return {
          config: reply.config,
          models: reply.models,
          capabilities: reply.capabilities,
        };
      }
      case "update_voice_config": {
        const reply = await this.rpc(h, "update_voice_config", {
          config: a.config,
        });
        return {
          config: reply.config,
          models: reply.models,
          capabilities: reply.capabilities,
        };
      }
      case "download_voice_model": {
        const reply = await this.rpc(
          h,
          "download_voice_model",
          { model_id: String(a.modelId ?? "") },
          660000,
        );
        return {
          config: reply.config,
          models: reply.models,
          capabilities: reply.capabilities,
        };
      }
      case "delete_voice_model": {
        const reply = await this.rpc(h, "delete_voice_model", {
          model_id: String(a.modelId ?? ""),
        });
        return {
          config: reply.config,
          models: reply.models,
          capabilities: reply.capabilities,
        };
      }
      case "transcribe_voice_audio": {
        const reply = await this.rpc(
          h,
          "transcribe_voice_audio",
          {
            pcm16_le_base64: String(a.pcm16LeBase64 ?? ""),
            sample_rate_hz: Number(a.sampleRateHz ?? 16000),
            language: a.language ?? null,
            agent_id: a.agentId ?? null,
          },
          270000,
        );
        return {
          text: String(reply.text ?? ""),
          language: reply.language ?? null,
          duration_ms: Number(reply.duration_ms ?? 0),
          backend_used: reply.backend_used,
          parsed_intent: reply.parsed_intent ?? null,
          approval_safety: reply.approval_safety ?? null,
        };
      }
      case "parse_voice_transcript": {
        const reply = await this.rpc(h, "parse_voice_transcript", {
          text: String(a.text ?? ""),
          language: a.language ?? null,
          agent_id: a.agentId ?? null,
        });
        return {
          text: String(reply.text ?? ""),
          language: reply.language ?? null,
          duration_ms: Number(reply.duration_ms ?? 0),
          backend_used: reply.backend_used,
          parsed_intent: reply.parsed_intent ?? null,
          approval_safety: reply.approval_safety ?? null,
        };
      }
      case "approve_voice_permission": {
        const reply = await this.rpc(h, "approve_voice_permission", {
          agent_id: agent,
          permission_request_id: String(a.permissionRequestId ?? ""),
          confirmed: Boolean(a.confirmed),
        });
        return reply.safety;
      }
      case "synthesize_voice_speech": {
        const reply = await this.rpc(
          h,
          "synthesize_voice_speech",
          {
            text: String(a.text ?? ""),
            language: a.language ?? null,
            voice_id: a.voiceId ?? null,
          },
          270000,
        );
        return {
          audio_wav_base64: String(reply.audio_wav_base64 ?? ""),
          mime_type: String(reply.mime_type ?? "audio/wav"),
          duration_ms: Number(reply.duration_ms ?? 0),
          backend_used: reply.backend_used,
        };
      }
      case "get_fim_config": {
        const reply = await this.rpc(h, "get_fim_config", {}, 10000);
        return reply.config;
      }
      case "update_fim_config": {
        const reply = await this.rpc(h, "update_fim_config", { config: a.config }, 10000);
        return reply.config;
      }
      case "predict_inline_edit": {
        const input = (a.input ?? {}) as Record<string, unknown>;
        const reply = await this.rpc(
          h,
          "predict_inline_edit",
          {
            cwd: String(input.cwd ?? ""),
            file_path: String(input.file_path ?? ""),
            language: input.language ?? null,
            prefix: String(input.prefix ?? ""),
            suffix: String(input.suffix ?? ""),
            cursor_line: Number(input.cursor_line ?? 1),
            cursor_col: Number(input.cursor_col ?? 0),
          },
          7000,
        );
        return {
          request_id: String(reply.request_id ?? ""),
          completion: String(reply.completion ?? ""),
          latency_ms: Number(reply.latency_ms ?? 0),
          source: String(reply.source ?? "off"),
        };
      }
      case "run_pr_patrol": {
        const reply = await this.rpc(
          h,
          "run_pr_patrol",
          {
            cwd: String(a.cwd ?? ""),
            base_ref: a.baseRef ?? null,
            head_ref: a.headRef ?? null,
            pr_number: a.prNumber ?? null,
            post_github_comments: Boolean(a.postGithubComments),
          },
          130000,
        );
        return reply.report;
      }
      case "start_goal_run": {
        const reply = await this.rpc(h, "start_goal_run", {
          agent_id: agent,
          objective: String(a.objective ?? ""),
          criteria: a.criteria ?? [],
          verify_command: a.verifyCommand ?? null,
          max_iterations: a.maxIterations ?? null,
          max_duration_secs: a.maxDurationSecs ?? null,
          replace_existing: Boolean(a.replaceExisting),
          dispatch_initial_prompt:
            a.dispatchInitialPrompt === undefined ? null : a.dispatchInitialPrompt,
        });
        return reply.goal;
      }
      case "update_goal_run": {
        const reply = await this.rpc(h, "update_goal_run", {
          goal_id: String(a.goalId ?? ""),
          objective: a.objective ?? null,
          criteria: a.criteria ?? null,
          verify_command: a.verifyCommand === undefined ? null : a.verifyCommand,
          max_iterations: a.maxIterations ?? null,
          max_duration_secs: a.maxDurationSecs ?? null,
        });
        return reply.goal;
      }
      case "list_goal_runs": {
        const reply = await this.rpc(h, "list_goal_runs", {
          agent_id: a.agentId ?? null,
        });
        return reply.goals ?? [];
      }
      case "control_goal_run": {
        const reply = await this.rpc(h, "control_goal_run", {
          goal_id: String(a.goalId ?? ""),
          action: a.action,
        });
        return reply.goal;
      }
      case "reset_workspace_board": {
        await this.rpc(h, "reset_workspace_board", {
          workspace_id: String(a.workspaceId ?? ""),
          new_goal: a.newGoal ?? null,
          keep_notes: a.keepNotes ?? null,
        });
        return;
      }
      case "harvest_conversation": {
        await this.rpc(h, "harvest_conversation", {
          agent_id: agent,
          summary_note: a.summaryNote ?? null,
          append_to_workspace_board: Boolean(a.appendToWorkspaceBoard),
        });
        return;
      }
      case "list_personal_agent_activity": {
        const reply = await this.rpc(h, "list_personal_agent_activity", {
          limit: Number(a.limit ?? 50),
          since_ms: a.sinceMs ?? null,
        });
        return reply.entries ?? [];
      }
    }
    throw new Error("This action is unavailable in Orbit Web (" + cmd + ")");
  }
}
