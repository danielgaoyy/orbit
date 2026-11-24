//! Fast, deterministic E2E pitfall regression test suite covering past production regressions:
//! 1. Claude multi-turn interrupt + queue flush-now: zero off-by-one turn misalignment
//! 2. Codex multi-turn steer + interrupt: zero off-by-one turn misalignment
//! 3. Tail-first stage pagination (`SubscribeTimeline.tail_limit`, `FetchTimelineBefore`, lazy `get_tool_call_diff`)
//! 4. Off-LAN mobile relay propagation (`apply_relay_to_fleet` -> `ConfigureHostRelay`, `OnlineRelay`, Welcome adoption)
//! 5. Mission Workspace parallel-first worktree isolation & historical handle alias resolution
//! 6. Cross-turn late assistant finalization dedup over live WebSocket & fresh catchup
//! 7. Archiving an agent kills ONLY that agent's shim/PGID without broadcast-signaling siblings (`--` before `-<pgid>`)
//! 8. Multi-agent non-interrupting subagent/peer delivery, thinking-stall progress telemetry, queue-preserving interrupt, and human preemption ordering

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use orbit_client_core::app_core::{OrbitAppCore, TimelineItemView};
use orbit_client_core::contract::AgentStageDeltaRequest;
use orbit_client_core::stage::TurnLiveSegmentView;
use orbit_client_core::{HostConnectionState, connect_via_relay};
use orbit_daemon::manager::DaemonBroadcast;
use orbit_daemon::{DaemonRuntime, spawn_embedded_daemon};
use orbit_protocol::{
    AgentMessageDeliveryStatus, AgentSummary, AttentionState, ProviderKind, SessionMode,
    TimelinePayload, ToolCallDetail, ToolCallStatus, WorkspaceSetupStatus, WorkspaceSummary,
};
use orbit_relay::{RelayConfig, run_relay};
use uuid::Uuid;

#[cfg(unix)]
static ORBIT_EXE_INIT: std::sync::Once = std::sync::Once::new();
#[cfg(unix)]
static MOCK_CLAUDE_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
#[cfg(unix)]
static MOCK_CODEX_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
#[cfg(unix)]
static MOCK_ANTIGRAVITY_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

#[cfg(unix)]
fn init_orbit_exe_override() {
    ORBIT_EXE_INIT.call_once(|| {
        orbit_daemon::manager::set_orbit_exe_override(PathBuf::from(env!("CARGO_BIN_EXE_orbit")));
        orbit_shim::set_test_parent_pid_watchdog(Some(std::process::id()));
    });
}

#[cfg(unix)]
const MOCK_CLAUDE_SCRIPT: &str = r#"#!/usr/bin/env python3
import collections
import json
import os
import select
import sys
import time
import urllib.request

if "--version" in sys.argv[1:]:
    sys.stdout.write("2.1.0 (mock-claude-reg)\n")
    sys.stdout.flush()
    sys.exit(0)

def parse_args():
    args = sys.argv[1:]
    mcp_cfg = None
    session_id = None
    resume_id = None
    i = 0
    while i < len(args):
        if args[i] == "--mcp-config" and i + 1 < len(args):
            raw_mcp = args[i + 1]
            if not os.path.isfile(raw_mcp):
                sys.stderr.write(f"expected --mcp-config to be a readable file path, got: {raw_mcp}\n")
                sys.exit(2)
            with open(raw_mcp, "r", encoding="utf-8") as fh:
                mcp_cfg = json.load(fh)
            i += 2
        elif args[i].startswith("--session-id="):
            session_id = args[i].split("=", 1)[1]
            i += 1
        elif args[i] == "--session-id" and i + 1 < len(args):
            session_id = args[i + 1]
            i += 2
        elif args[i].startswith("--resume="):
            resume_id = args[i].split("=", 1)[1]
            i += 1
        elif args[i] == "--resume" and i + 1 < len(args):
            resume_id = args[i + 1]
            i += 2
        else:
            i += 1
    return mcp_cfg, resume_id or session_id or "mock-claude-reg-1"

MCP_CFG, SESSION_ID = parse_args()
STDIN_FD = sys.stdin.fileno()
READ_BUF = bytearray()
PENDING_LINES = collections.deque()

def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

def mcp_call(tool_name, arguments):
    srv = (MCP_CFG or {}).get("mcpServers", {}).get("orbit", {})
    url = srv.get("url") or os.environ.get("ORBIT_MCP_URL")
    headers = dict(srv.get("headers") or {})
    if "Authorization" not in headers and os.environ.get("ORBIT_MCP_TOKEN"):
        headers["Authorization"] = "Bearer " + os.environ["ORBIT_MCP_TOKEN"]
    headers["Content-Type"] = "application/json"
    body = json.dumps({
        "jsonrpc": "2.0",
        "id": int(time.time() * 1000) % 1000000,
        "method": "tools/call",
        "params": {"name": tool_name, "arguments": arguments}
    }).encode("utf-8")
    req = urllib.request.Request(url, data=body, headers=headers, method="POST")
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read().decode("utf-8"))

def read_line(timeout_sec):
    if PENDING_LINES:
        return PENDING_LINES.popleft()
    deadline = None if timeout_sec is None else (time.monotonic() + timeout_sec)
    while True:
        nl = READ_BUF.find(b"\n")
        if nl != -1:
            line = bytes(READ_BUF[:nl + 1]).decode("utf-8", errors="replace")
            del READ_BUF[:nl + 1]
            return line
        wait = None if deadline is None else max(0.0, deadline - time.monotonic())
        if wait is not None and wait <= 0.0:
            return None
        rlist, _, _ = select.select([STDIN_FD], [], [], wait)
        if not rlist:
            return None
        chunk = os.read(STDIN_FD, 4096)
        if not chunk:
            if READ_BUF:
                line = bytes(READ_BUF).decode("utf-8", errors="replace")
                READ_BUF.clear()
                return line
            return ""
        READ_BUF.extend(chunk)

def strip_mock_envelopes(raw):
    import re
    s = re.sub(r"<orbit-workspace(?:\s[^>]*)?>[\s\S]*?</orbit-workspace>", "", raw)
    s = re.sub(r"<orbit-workspace-delta(?:\s[^>]*)?>[\s\S]*?</orbit-workspace-delta>", "", s)
    s = re.sub(r"<WORKSPACE_RULES>[\s\S]*?</WORKSPACE_RULES>", "", s)
    s = re.sub(r"</?orbit-system[^>]*>", "", s)
    return s.strip()

def extract_text(frame):
    content = frame.get("message", {}).get("content", "")
    if isinstance(content, str):
        raw = content
    elif isinstance(content, list):
        raw = "\n".join(
            b.get("text", "") for b in content if isinstance(b, dict) and b.get("type") == "text"
        )
    else:
        raw = ""
    return strip_mock_envelopes(raw)

def parse_kv_tokens(text):
    out = {}
    for tok in text.split():
        if "=" in tok:
            k, v = tok.split("=", 1)
            out[k.strip()] = v.strip()
    return out

def handle_control_request(frame):
    req_id = frame.get("request_id", "ctrl_1")
    sub = frame.get("request", {}).get("subtype", "")
    emit({
        "type": "control_response",
        "response": {"subtype": "success", "request_id": req_id, "response": {"ok": True}}
    })
    if sub == "initialize":
        emit({"type": "system", "subtype": "init", "session_id": SESSION_ID, "model": "mock-claude-reg"})
    return sub

turn_counter = 0
deferred_frames = collections.deque()

while True:
    if deferred_frames:
        line = deferred_frames.popleft()
    else:
        line = read_line(None)
        if line == "":
            break
    try:
        frame = json.loads(line)
    except Exception:
        continue

    ftype = frame.get("type")
    if ftype == "control_request":
        handle_control_request(frame)
        continue
    if ftype != "user":
        continue

    turn_counter += 1
    text = extract_text(frame)
    kv = parse_kv_tokens(text)
    echo_tok = kv.get("ECHO", "")
    reply_parts = []

    sleep_ms_str = kv.get("SLEEP_MS")
    if sleep_ms_str:
        msg_id = f"stream_{turn_counter}"
        emit({
            "type": "stream_event",
            "event": {"type": "message_start", "message": {"id": msg_id}}
        })
        emit({
            "type": "stream_event",
            "event": {"type": "content_block_start", "index": 0, "content_block": {"type": "text"}}
        })
        emit({
            "type": "stream_event",
            "event": {
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": f"STREAMING={echo_tok}"}
            }
        })
        deadline = time.monotonic() + (float(sleep_ms_str) / 1000.0)
        interrupted = False
        while time.monotonic() < deadline:
            remaining = max(0.0, deadline - time.monotonic())
            nxt = read_line(min(0.02, remaining))
            if nxt is None:
                continue
            if nxt == "":
                sys.exit(0)
            try:
                nf = json.loads(nxt)
            except Exception:
                continue
            if nf.get("type") == "control_request":
                sub = handle_control_request(nf)
                if sub == "interrupt":
                    emit({
                        "type": "result",
                        "subtype": "error_during_execution",
                        "is_error": True,
                        "result": "interrupted",
                        "session_id": SESSION_ID
                    })
                    interrupted = True
                    break
            elif nf.get("type") == "user":
                deferred_frames.append(nxt)
        if interrupted:
            continue

    if "[MOCK:FORK_WORKTREE]" in text:
        res = mcp_call("spawn_workspace_peer", {
            "handle": "beta",
            "title": "Worker Session",
            "worktree": True,
            "initial_prompt": "ECHO=BETA_INIT",
            "notify_on_finish": False
        })
        sc = res.get("result", {}).get("structuredContent", {})
        reply_parts.append(f"FORK_OK={sc.get('agent_id', '')}")

    if "[MOCK:CHECK_PEERS]" in text:
        res = mcp_call("workspace_peers", {})
        sc = res.get("result", {}).get("structuredContent", {})
        shared = sc.get("shared_checkout_peers", [])
        peers = sc.get("peers", [])
        beta_isolated = any(
            p.get("handle") == "beta" and bool(p.get("is_worktree_isolated"))
            for p in peers
        )
        reply_parts.append(f"SHARED_COUNT={len(shared)} BETA_ISOLATED={beta_isolated}")

    if "[MOCK:MSG_PEER]" in text:
        target = kv.get("TARGET", "beta")
        msg_text = kv.get("TEXT", "PING")
        res = mcp_call("message_peer", {
            "handle": target,
            "message": msg_text,
            "notify_on_finish": False
        })
        sc = res.get("result", {}).get("structuredContent", {})
        delivered = sc.get("delivered", [])
        resolved_via_alias = any(bool(d.get("resolved_via_alias")) for d in delivered)
        reply_parts.append(f"MSG_OK={bool(sc.get('ok'))} RESOLVED_VIA_ALIAS={resolved_via_alias}")

    if "[MOCK:SEND_AGENT_PROMPT]" in text:
        target_id = kv.get("TARGET_ID", "")
        msg_text = kv.get("MSG", "PING")
        res = mcp_call("send_agent_prompt", {
            "agent_id": target_id,
            "prompt": msg_text,
        })
        result_obj = res.get("result", {})
        ok = "error" not in res and not bool(result_obj.get("isError", False))
        reply_parts.append(f"SEND_OK={ok}")

    if echo_tok and not reply_parts:
        reply_parts.append(f"REPLY={echo_tok}")
    elif not reply_parts:
        reply_parts.append(f"ACK_TURN={turn_counter}")

    final_text = " | ".join(reply_parts)
    emit({
        "type": "assistant",
        "message": {
            "id": f"msg_{turn_counter}",
            "content": [{"type": "text", "text": final_text}]
        }
    })
    emit({
        "type": "result",
        "subtype": "success",
        "is_error": False,
        "result": final_text,
        "session_id": SESSION_ID
    })
"#;

#[cfg(unix)]
const MOCK_CODEX_SCRIPT: &str = r#"#!/usr/bin/env python3
import collections
import json
import os
import select
import sys
import time

if "--version" in sys.argv[1:]:
    sys.stdout.write("0.1.0 (mock-codex-reg)\n")
    sys.stdout.flush()
    sys.exit(0)

STDIN_FD = sys.stdin.fileno()
READ_BUF = bytearray()

def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

def read_line(timeout_sec):
    deadline = None if timeout_sec is None else (time.monotonic() + timeout_sec)
    while True:
        nl = READ_BUF.find(b"\n")
        if nl != -1:
            line = bytes(READ_BUF[:nl + 1]).decode("utf-8", errors="replace")
            del READ_BUF[:nl + 1]
            return line
        wait = None if deadline is None else max(0.0, deadline - time.monotonic())
        if wait is not None and wait <= 0.0:
            return None
        rlist, _, _ = select.select([STDIN_FD], [], [], wait)
        if not rlist:
            return None
        chunk = os.read(STDIN_FD, 4096)
        if not chunk:
            if READ_BUF:
                line = bytes(READ_BUF).decode("utf-8", errors="replace")
                READ_BUF.clear()
                return line
            return ""
        READ_BUF.extend(chunk)

def extract_input_text(params):
    items = params.get("input", [])
    parts = []
    for item in items:
        if isinstance(item, dict) and item.get("type") == "text":
            parts.append(item.get("text", ""))
    return "\n".join(parts)

def parse_kv_tokens(text):
    out = {}
    for tok in text.split():
        if "=" in tok:
            k, v = tok.split("=", 1)
            out[k.strip()] = v.strip()
    return out

turn_seq = 0

while True:
    line = read_line(None)
    if line == "":
        break
    try:
        msg = json.loads(line)
    except Exception:
        continue

    rpc_id = msg.get("id")
    method = msg.get("method")
    params = msg.get("params", {})

    if method == "initialize":
        emit({"id": rpc_id, "result": {}})
    elif method == "initialized":
        continue
    elif method == "thread/start":
        emit({
            "id": rpc_id,
            "result": {
                "thread": {"id": "thr_mock_codex_reg"},
                "model": "mock-codex-reg"
            }
        })
    elif method == "turn/start":
        turn_seq += 1
        nt = f"nt_{turn_seq}"
        item_id = f"msg_{turn_seq}"
        text = extract_input_text(params)
        kv = parse_kv_tokens(text)
        echo_tok = kv.get("ECHO", f"TURN_{turn_seq}")
        sleep_ms_str = kv.get("SLEEP_MS")

        emit({"id": rpc_id, "result": {"turn": {"id": nt}}})
        emit({"method": "turn/started", "params": {"turn": {"id": nt}}})
        emit({
            "method": "item/agentMessage/delta",
            "params": {"itemId": item_id, "delta": f"STREAMING={echo_tok}"}
        })

        interrupted = False
        reply_tokens = [echo_tok]
        if sleep_ms_str:
            deadline = time.monotonic() + (float(sleep_ms_str) / 1000.0)
            while time.monotonic() < deadline:
                remaining = max(0.0, deadline - time.monotonic())
                nxt = read_line(min(0.02, remaining))
                if nxt is None:
                    continue
                if nxt == "":
                    sys.exit(0)
                try:
                    nmsg = json.loads(nxt)
                except Exception:
                    continue
                nmethod = nmsg.get("method")
                nrid = nmsg.get("id")
                nparams = nmsg.get("params", {})
                if nmethod == "turn/interrupt":
                    if nrid is not None:
                        emit({"id": nrid, "result": {}})
                    emit({
                        "method": "turn/completed",
                        "params": {"turn": {"id": nt, "status": "interrupted"}}
                    })
                    interrupted = True
                    break
                elif nmethod == "turn/steer":
                    if nrid is not None:
                        emit({"id": nrid, "result": {"turnId": nt}})
                    skv = parse_kv_tokens(extract_input_text(nparams))
                    stok = skv.get("ECHO", "")
                    if stok:
                        reply_tokens.append(stok)

        if interrupted:
            continue

        joined = "+".join(reply_tokens)
        emit({
            "method": "item/completed",
            "params": {
                "item": {
                    "id": item_id,
                    "type": "agentMessage",
                    "text": f"REPLY={joined}"
                }
            }
        })
        emit({
            "method": "turn/completed",
            "params": {"turn": {"id": nt, "status": "completed"}}
        })
"#;

#[cfg(unix)]
const MOCK_ANTIGRAVITY_SCRIPT: &str = r#"#!/usr/bin/env python3
import collections
import json
import os
import re
import select
import sys
import time
import urllib.request

if "--version" in sys.argv[1:]:
    sys.stdout.write("1.0.0 (mock-agy)\n")
    sys.stdout.flush()
    sys.exit(0)

STDIN_FD = sys.stdin.fileno()
READ_BUF = bytearray()
PENDING_LINES = collections.deque()
SESSION_ID = "mock-agy-sess-1"
CWD = os.getcwd()
TRACE_PATH = os.path.join(CWD, ".mock_acp_trace.jsonl")
MCP_URL = os.environ.get("ORBIT_MCP_URL")
MCP_HEADERS = {}
if os.environ.get("ORBIT_MCP_TOKEN"):
    MCP_HEADERS["Authorization"] = "Bearer " + os.environ["ORBIT_MCP_TOKEN"]

def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

def log_trace(entry):
    with open(TRACE_PATH, "a", encoding="utf-8") as fh:
        fh.write(json.dumps(entry) + "\n")
        fh.flush()

def read_line(timeout_sec):
    if PENDING_LINES:
        return PENDING_LINES.popleft()
    deadline = None if timeout_sec is None else (time.monotonic() + timeout_sec)
    while True:
        nl = READ_BUF.find(b"\n")
        if nl != -1:
            line = bytes(READ_BUF[:nl + 1]).decode("utf-8", errors="replace")
            del READ_BUF[:nl + 1]
            return line
        wait = None if deadline is None else max(0.0, deadline - time.monotonic())
        if wait is not None and wait <= 0.0:
            return None
        rlist, _, _ = select.select([STDIN_FD], [], [], wait)
        if not rlist:
            return None
        chunk = os.read(STDIN_FD, 4096)
        if not chunk:
            if READ_BUF:
                line = bytes(READ_BUF).decode("utf-8", errors="replace")
                READ_BUF.clear()
                return line
            return ""
        READ_BUF.extend(chunk)

def configure_mcp(mcp_servers):
    global MCP_URL, MCP_HEADERS
    if not isinstance(mcp_servers, list):
        return
    for srv in mcp_servers:
        if isinstance(srv, dict) and srv.get("type") == "http" and srv.get("url"):
            MCP_URL = srv.get("url")
            hdrs = {}
            raw_hdrs = srv.get("headers")
            if isinstance(raw_hdrs, list):
                for h in raw_hdrs:
                    if isinstance(h, dict) and h.get("name"):
                        hdrs[h["name"]] = h.get("value", "")
            elif isinstance(raw_hdrs, dict):
                hdrs.update(raw_hdrs)
            if hdrs:
                MCP_HEADERS = hdrs
            return

def mcp_call(tool_name, arguments):
    headers = dict(MCP_HEADERS)
    if "Authorization" not in headers and os.environ.get("ORBIT_MCP_TOKEN"):
        headers["Authorization"] = "Bearer " + os.environ["ORBIT_MCP_TOKEN"]
    url = MCP_URL or os.environ.get("ORBIT_MCP_URL")
    headers["Content-Type"] = "application/json"
    body = json.dumps({
        "jsonrpc": "2.0",
        "id": int(time.time() * 1000) % 1000000,
        "method": "tools/call",
        "params": {"name": tool_name, "arguments": arguments},
    }).encode("utf-8")
    req = urllib.request.Request(url, data=body, headers=headers, method="POST")
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read().decode("utf-8"))

def extract_prompt_text(params):
    parts = []
    for block in params.get("prompt", []) or []:
        if isinstance(block, dict) and block.get("type") == "text":
            parts.append(block.get("text", ""))
    return "\n".join(parts)

def strip_envelopes(raw):
    s = re.sub(r"<orbit-workspace(?:\s[^>]*)?>[\s\S]*?</orbit-workspace>", "", raw)
    s = re.sub(r"<orbit-workspace-delta(?:\s[^>]*)?>[\s\S]*?</orbit-workspace-delta>", "", s)
    s = re.sub(r"<WORKSPACE_RULES>[\s\S]*?</WORKSPACE_RULES>", "", s)
    return s

def parse_kv(text):
    out = {}
    for tok in text.split():
        if "=" in tok:
            k, v = tok.split("=", 1)
            out[k.strip()] = v.strip()
    return out

while True:
    line = read_line(None)
    if line == "":
        break
    try:
        msg = json.loads(line)
    except Exception:
        continue

    rid = msg.get("id")
    method = msg.get("method")
    params = msg.get("params", {}) or {}

    if method == "initialize":
        emit({
            "jsonrpc": "2.0",
            "id": rid,
            "result": {"protocolVersion": 1, "agentCapabilities": {}},
        })
        continue

    if method in ("session/new", "session/load", "session/fork"):
        if params.get("cwd"):
            CWD = params["cwd"]
            TRACE_PATH = os.path.join(CWD, ".mock_acp_trace.jsonl")
        configure_mcp(params.get("mcpServers"))
        emit({"jsonrpc": "2.0", "id": rid, "result": {"sessionId": SESSION_ID}})
        continue

    if method == "session/cancel":
        log_trace({"method": "session/cancel"})
        if rid is not None:
            emit({"jsonrpc": "2.0", "id": rid, "result": {}})
        continue

    if method != "session/prompt":
        if rid is not None and method is not None:
            emit({"jsonrpc": "2.0", "id": rid, "result": {}})
        continue

    raw_text = extract_prompt_text(params)
    text = strip_envelopes(raw_text)
    kv = parse_kv(text)
    log_trace({"method": "session/prompt", "text": text})

    if kv.get("SPAWN_CODEX_PROMPT"):
        mcp_call("create_agent", {
            "provider": "codex",
            "cwd": CWD,
            "initial_prompt": kv["SPAWN_CODEX_PROMPT"],
            "notify_on_finish": True,
            "mode": "accept_edits",
        })

    wait_file = kv.get("WAIT_FILE")
    think_loop = kv.get("THINK_LOOP") == "1"
    cancelled = False
    if wait_file:
        next_think_at = time.monotonic() + 0.15
        think_step = 0
        while not os.path.exists(wait_file):
            now_m = time.monotonic()
            if think_loop and now_m >= next_think_at:
                think_step += 1
                emit({
                    "jsonrpc": "2.0",
                    "method": "session/update",
                    "params": {
                        "sessionId": SESSION_ID,
                        "update": {
                            "sessionUpdate": "agent_thought_chunk",
                            "content": {"type": "text", "text": f"thinking step {think_step}\n"},
                        },
                    },
                })
                next_think_at = now_m + 0.15
            slice_sec = 0.025
            if think_loop:
                slice_sec = min(slice_sec, max(0.005, next_think_at - time.monotonic()))
            nxt = read_line(slice_sec)
            if nxt == "":
                sys.exit(0)
            if nxt is None:
                continue
            try:
                nmsg = json.loads(nxt)
            except Exception:
                continue
            nmethod = nmsg.get("method")
            nrid = nmsg.get("id")
            if nmethod == "session/cancel":
                log_trace({"method": "session/cancel"})
                if nrid is not None:
                    emit({"jsonrpc": "2.0", "id": nrid, "result": {}})
                emit({"jsonrpc": "2.0", "id": rid, "result": {"stopReason": "cancelled"}})
                cancelled = True
                break
            elif nrid is not None and nmethod is not None:
                emit({"jsonrpc": "2.0", "id": nrid, "result": {}})

    if cancelled:
        continue

    tag_match = re.search(r"NOTIFY_TAG=([A-Za-z0-9_]+)", text)
    if tag_match:
        reply = f"REPLY={tag_match.group(1)}"
    elif "CODEX_NOTIFY" in text:
        reply = "REPLY=CODEX_NOTIFY"
    elif kv.get("ECHO"):
        reply = f"REPLY={kv['ECHO']}"
    else:
        reply = "REPLY=ACK_TURN"

    emit({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": SESSION_ID,
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": reply},
            },
        },
    })
    emit({"jsonrpc": "2.0", "id": rid, "result": {"stopReason": "end_turn"}})
"#;

#[cfg(unix)]
fn install_mock_script(
    slot: &std::sync::OnceLock<PathBuf>,
    kind: ProviderKind,
    filename: &str,
    contents: &str,
) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    init_orbit_exe_override();
    slot.get_or_init(|| {
        let dir = base_tmp_dir().join(format!("orb_reg_mocks_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create mock dir");
        let script = dir.join(filename);
        std::fs::write(&script, contents).expect("write mock script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod mock script");
        orbit_providers::set_provider_executable_override(kind, script.clone());
        script
    })
    .clone()
}

fn base_tmp_dir() -> PathBuf {
    if Path::new("/tmp").is_dir() {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    }
}

struct TempTestDir(PathBuf);

impl std::ops::Deref for TempTestDir {
    type Target = Path;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for TempTestDir {
    fn drop(&mut self) {
        orbit_shim::kill_all_shims_in_dir_sync(&self.0.join("shims"));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn new_temp_dir(tag: &str) -> TempTestDir {
    let uid = &Uuid::new_v4().simple().to_string()[..8];
    let dir = base_tmp_dir().join(format!("orb_reg_{tag}_{uid}"));
    std::fs::create_dir_all(&dir).expect("mkdir temp");
    TempTestDir(dir)
}

async fn wait_until<F, Fut>(timeout: Duration, mut pred: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if pred().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(60)).await;
    }
    false
}

async fn open_core(db_path: PathBuf) -> Arc<OrbitAppCore> {
    tokio::task::spawn_blocking(move || OrbitAppCore::open(&db_path, false).expect("open core"))
        .await
        .expect("spawn_blocking open")
}

async fn open_and_connect_core(db_path: PathBuf, uri: String) -> (Arc<OrbitAppCore>, String) {
    let core = open_core(db_path).await;
    let card = core.pair_host(&uri).expect("pair host");
    let host_id = card.host_id.clone();
    core.connect_host_async(host_id.clone());
    let online = wait_until(Duration::from_secs(12), || {
        let c = Arc::clone(&core);
        let hid = host_id.clone();
        async move {
            c.host_detail(&hid)
                .map(|d| d.host.connection_state == HostConnectionState::OnlineDirect)
                .unwrap_or(false)
        }
    })
    .await;
    assert!(online, "OrbitAppCore must connect directly to daemon");
    (core, host_id)
}

async fn drop_core(core: Arc<OrbitAppCore>) {
    let _ = tokio::task::spawn_blocking(move || drop(core)).await;
}

#[cfg(unix)]
fn init_git_repo(dir: &Path) {
    std::fs::create_dir_all(dir).expect("mkdir repo");
    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run(&["init", "-b", "main"]);
    run(&["config", "user.email", "test@orbit.local"]);
    run(&["config", "user.name", "Orbit Test"]);
    std::fs::write(dir.join("README.md"), "# Pitfall Regression Repo\n").expect("write readme");
    run(&["add", "."]);
    run(&["commit", "-m", "initial commit"]);
}

fn turn_user_prompt_text(turn: &orbit_client_core::stage::StageTurnView) -> String {
    match &turn.user_prompt {
        Some(orbit_client_core::app_core::TimelineItemView::UserPrompt { text, .. }) => {
            text.clone()
        }
        _ => String::new(),
    }
}

fn turn_reply_texts(turn: &orbit_client_core::stage::StageTurnView) -> Vec<String> {
    let mut texts = Vec::new();
    for seg in &turn.live_segments {
        if let orbit_client_core::stage::TurnLiveSegmentView::AssistantMessage { item, .. } = seg
            && let orbit_client_core::app_core::TimelineItemView::AssistantMessage { text, .. } =
                item
        {
            texts.push(text.clone());
        }
    }
    if let Some(orbit_client_core::app_core::TimelineItemView::AssistantMessage { text, .. }) =
        &turn.terminal_reply
        && !texts.iter().any(|t| t == text)
    {
        texts.push(text.clone());
    }
    texts
}

fn stage_has_text(core: &OrbitAppCore, host_id: &str, agent_id: &str, needle: &str) -> bool {
    let stage = core.agent_stage(host_id, agent_id);
    stage
        .turns
        .iter()
        .any(|t| turn_reply_texts(t).iter().any(|txt| txt.contains(needle)))
}

fn stage_settled_with_text(
    core: &OrbitAppCore,
    host_id: &str,
    agent_id: &str,
    needle: &str,
) -> bool {
    let settled = core
        .host_detail(host_id)
        .and_then(|d| d.agents.into_iter().find(|a| a.agent_id == agent_id))
        .is_some_and(|a| !a.is_running && !a.is_awaiting_permission);
    settled && stage_has_text(core, host_id, agent_id, needle)
}

fn make_seed_workspace(ws_id: &str, cwd: &str, title: &str) -> WorkspaceSummary {
    let now = Utc::now();
    WorkspaceSummary {
        workspace_id: ws_id.into(),
        title: title.into(),
        cwd: cwd.into(),
        git_branch: None,
        is_worktree: false,
        updated_at: now,
        source_cwd: None,
        base_ref: None,
        parent_workspace_id: None,
        archived_at: None,
        background: false,
        setup_status: WorkspaceSetupStatus::Idle,
        mission_goal: None,
        project_root: None,
        board_revision: 0,
        title_locked: false,
        auto_clustered: false,
        is_curated: false,
    }
}

fn make_seed_agent(agent_id: &str, ws_id: &str, cwd: &str, title: &str) -> AgentSummary {
    let now = Utc::now();
    AgentSummary {
        agent_id: agent_id.into(),
        workspace_id: ws_id.into(),
        cwd: cwd.into(),
        title: title.into(),
        provider: ProviderKind::ClaudeCode,
        model: None,
        effort: None,
        fast_mode: false,
        mode: SessionMode::AcceptEdits,
        status: orbit_protocol::AgentStatus::Idle,
        parent_agent_id: None,
        parent_host_id: None,
        latest_seq: 0,
        timeline_epoch: 0,
        created_at: now,
        updated_at: now,
        archived_at: None,
        background: false,
        auto_archive: false,
        profile_id: None,
        labels: HashMap::new(),
        attention: AttentionState::None,
        last_activity_at: Some(now),
        queued_prompts: Vec::new(),
        read_seq: 0,
        is_pinned: false,
        pinned_order: 0,
        handle: None,
        cluster_suggestion: None,
        task_summary: None,
        forked_from_agent_id: None,
        forked_at_seq: None,
        acp_agent_id: None,
    }
}

// ============================================================================
// PITFALL 1: Claude Multi-Turn Interrupt + Queue Flush (No Off-By-One)
// ============================================================================
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pitfall_01_claude_no_off_by_one_on_multi_turn_interrupt_and_queue_flush() {
    install_mock_script(
        &MOCK_CLAUDE_PATH,
        ProviderKind::ClaudeCode,
        "mock_claude_reg.py",
        MOCK_CLAUDE_SCRIPT,
    );
    let home = new_temp_dir("p1_h");
    let work = new_temp_dir("p1_w");

    let (addr, rt) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("spawn daemon");
    let uri = rt
        .build_pairing_offer(vec![format!("ws://{addr}/ws")], None)
        .expect("offer")
        .to_qr_uri()
        .expect("uri");
    let (core, host_id) = open_and_connect_core(home.join("c1.db"), uri).await;

    core.create_agent(
        &host_id,
        &work.to_string_lossy(),
        ProviderKind::ClaudeCode,
        SessionMode::AcceptEdits,
        "ECHO=ALPHA",
        None,
    );

    assert!(
        wait_until(Duration::from_secs(12), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            async move {
                c.host_detail(&hid).is_some_and(|d| {
                    d.agents.iter().any(|a| {
                        !a.agent_id.starts_with("opt_") && !a.is_running && a.latest_seq > 0
                    })
                })
            }
        })
        .await,
        "Turn 1 (ALPHA) must finish idle"
    );
    let agent_id = core
        .host_detail(&host_id)
        .expect("detail")
        .agents
        .into_iter()
        .find(|a| !a.agent_id.starts_with("opt_"))
        .expect("real agent")
        .agent_id;
    core.open_agent_timeline(&host_id, &agent_id);
    assert!(
        wait_until(Duration::from_secs(8), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let aid = agent_id.clone();
            async move { stage_settled_with_text(&c, &hid, &aid, "REPLY=ALPHA") }
        })
        .await,
        "Turn 1 reply must contain REPLY=ALPHA"
    );

    // Turn 2: normal sequential turn
    core.send_prompt(&host_id, &agent_id, "ECHO=BETA");
    assert!(
        wait_until(Duration::from_secs(10), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let aid = agent_id.clone();
            async move { stage_settled_with_text(&c, &hid, &aid, "REPLY=BETA") }
        })
        .await,
        "Turn 2 must finish with REPLY=BETA"
    );

    // Turn 3: slow active turn -> wait until streaming chunk arrives -> enqueue + flush-now interrupt
    core.send_prompt(&host_id, &agent_id, "ECHO=SLOW_CANCEL SLEEP_MS=4000");
    assert!(
        wait_until(Duration::from_secs(10), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let aid = agent_id.clone();
            async move { stage_has_text(&c, &hid, &aid, "STREAMING=SLOW_CANCEL") }
        })
        .await,
        "Slow turn must begin streaming before interrupt-flush"
    );

    let qid = core.enqueue_prompt(
        &host_id,
        &agent_id,
        "ECHO=FLUSH_WINNER",
        None,
        Vec::new(),
        None,
    );
    core.flush_queued_prompt_now(&host_id, &agent_id, &qid);

    assert!(
        wait_until(Duration::from_secs(12), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let aid = agent_id.clone();
            async move { stage_settled_with_text(&c, &hid, &aid, "REPLY=FLUSH_WINNER") }
        })
        .await,
        "Flushed prompt must complete cleanly with REPLY=FLUSH_WINNER"
    );

    let after_flush = core.agent_stage(&host_id, &agent_id);
    assert!(
        !after_flush.turns.iter().any(|t| turn_reply_texts(t)
            .iter()
            .any(|s| s.contains("REPLY=SLOW_CANCEL"))),
        "Interrupted SLOW_CANCEL turn must not emit REPLY=SLOW_CANCEL"
    );

    // Turn 4: subsequent turn after interrupt-flush must align 1:1 (zero off-by-one shift!)
    core.send_prompt(&host_id, &agent_id, "ECHO=POST_FLUSH_FINAL");
    assert!(
        wait_until(Duration::from_secs(10), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let aid = agent_id.clone();
            async move { stage_settled_with_text(&c, &hid, &aid, "REPLY=POST_FLUSH_FINAL") }
        })
        .await,
        "Final turn must complete with REPLY=POST_FLUSH_FINAL"
    );

    let final_stage = core.agent_stage(&host_id, &agent_id);
    let last_turn = final_stage.turns.last().expect("final turn");
    assert!(
        turn_user_prompt_text(last_turn).contains("ECHO=POST_FLUSH_FINAL"),
        "Final turn user prompt must be ECHO=POST_FLUSH_FINAL, got {:?}",
        turn_user_prompt_text(last_turn)
    );
    let last_replies = turn_reply_texts(last_turn);
    assert!(
        last_replies
            .iter()
            .any(|s| s.contains("REPLY=POST_FLUSH_FINAL")),
        "Final turn reply must contain REPLY=POST_FLUSH_FINAL without 1-turn lag, got {:?}",
        last_replies
    );

    drop_core(core).await;
    drop(rt);
}

// ============================================================================
// PITFALL 2: Codex Multi-Turn Steer & Interrupt (No Off-By-One)
// ============================================================================
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pitfall_02_codex_no_off_by_one_on_multi_turn_steer_and_interrupt() {
    install_mock_script(
        &MOCK_CODEX_PATH,
        ProviderKind::Codex,
        "mock_codex_reg.py",
        MOCK_CODEX_SCRIPT,
    );
    let home = new_temp_dir("p2_h");
    let work = new_temp_dir("p2_w");

    let (addr, rt) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("spawn daemon");
    let uri = rt
        .build_pairing_offer(vec![format!("ws://{addr}/ws")], None)
        .expect("offer")
        .to_qr_uri()
        .expect("uri");
    let (core, host_id) = open_and_connect_core(home.join("c2.db"), uri).await;

    core.create_agent(
        &host_id,
        &work.to_string_lossy(),
        ProviderKind::Codex,
        SessionMode::AcceptEdits,
        "ECHO=CODEX_1",
        None,
    );

    assert!(
        wait_until(Duration::from_secs(12), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            async move {
                c.host_detail(&hid).is_some_and(|d| {
                    d.agents.iter().any(|a| {
                        !a.agent_id.starts_with("opt_") && !a.is_running && a.latest_seq > 0
                    })
                })
            }
        })
        .await,
        "Codex Turn 1 must finish idle"
    );
    let agent_id = core
        .host_detail(&host_id)
        .expect("detail")
        .agents
        .into_iter()
        .find(|a| !a.agent_id.starts_with("opt_"))
        .expect("codex agent")
        .agent_id;
    core.open_agent_timeline(&host_id, &agent_id);

    assert!(
        wait_until(Duration::from_secs(8), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let aid = agent_id.clone();
            async move { stage_settled_with_text(&c, &hid, &aid, "REPLY=CODEX_1") }
        })
        .await,
        "Codex Turn 1 must reply with REPLY=CODEX_1"
    );

    // Slow turn -> steer active turn mid-flight
    core.send_prompt(&host_id, &agent_id, "ECHO=CODEX_STEER_BASE SLEEP_MS=400");
    assert!(
        wait_until(Duration::from_secs(10), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let aid = agent_id.clone();
            async move { stage_has_text(&c, &hid, &aid, "STREAMING=CODEX_STEER_BASE") }
        })
        .await,
        "Codex slow steer base turn must start streaming"
    );
    core.send_prompt_with_behavior(
        &host_id,
        &agent_id,
        "ECHO=CODEX_STEERED",
        None,
        Vec::new(),
        orbit_protocol::ActiveTurnBehavior::Steer,
    );
    assert!(
        wait_until(Duration::from_secs(10), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let aid = agent_id.clone();
            async move {
                stage_settled_with_text(&c, &hid, &aid, "REPLY=CODEX_STEER_BASE+CODEX_STEERED")
            }
        })
        .await,
        "Steered Codex turn must incorporate CODEX_STEERED and settle idle"
    );

    // Slow turn -> interrupt replacement
    core.send_prompt(&host_id, &agent_id, "ECHO=CODEX_SLOW SLEEP_MS=4000");
    assert!(
        wait_until(Duration::from_secs(10), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let aid = agent_id.clone();
            async move { stage_has_text(&c, &hid, &aid, "STREAMING=CODEX_SLOW") }
        })
        .await,
        "Codex slow turn must start streaming before interrupt"
    );
    core.send_prompt_with_behavior(
        &host_id,
        &agent_id,
        "ECHO=CODEX_REPLACE",
        None,
        Vec::new(),
        orbit_protocol::ActiveTurnBehavior::Interrupt,
    );
    assert!(
        wait_until(Duration::from_secs(12), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let aid = agent_id.clone();
            async move { stage_settled_with_text(&c, &hid, &aid, "REPLY=CODEX_REPLACE") }
        })
        .await,
        "Interrupt replacement turn must finish with REPLY=CODEX_REPLACE"
    );
    assert!(
        !stage_has_text(&core, &host_id, &agent_id, "REPLY=CODEX_SLOW"),
        "Interrupted Codex slow turn must not finish with REPLY=CODEX_SLOW"
    );

    // Final turn after steer + interrupt must align 1:1
    core.send_prompt(&host_id, &agent_id, "ECHO=CODEX_AFTER");
    assert!(
        wait_until(Duration::from_secs(10), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let aid = agent_id.clone();
            async move { stage_settled_with_text(&c, &hid, &aid, "REPLY=CODEX_AFTER") }
        })
        .await,
        "Post-interrupt Codex turn must finish with REPLY=CODEX_AFTER"
    );

    let final_stage = core.agent_stage(&host_id, &agent_id);
    let last_turn = final_stage.turns.last().expect("final codex turn");
    assert!(
        turn_user_prompt_text(last_turn).contains("ECHO=CODEX_AFTER"),
        "Final turn prompt must be ECHO=CODEX_AFTER"
    );
    let last_replies = turn_reply_texts(last_turn);
    assert!(
        last_replies.iter().any(|s| s.contains("REPLY=CODEX_AFTER")),
        "Final turn reply must match ECHO=CODEX_AFTER without lag, got {:?}",
        last_replies
    );

    drop_core(core).await;
    drop(rt);
}

// ============================================================================
// PITFALL 3: Tail-First Stage Pagination + FetchTimelineBefore + Lazy Diff
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pitfall_03_tail_first_pagination_fetch_before_and_lazy_tool_diff() {
    let home = new_temp_dir("p3_h");
    let (addr, rt) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("spawn daemon");

    let ws_id = "ws_pag_1";
    let agent_id = "ag_pag_1";
    let cwd = home.to_string_lossy().to_string();
    rt.store
        .upsert_workspace(&make_seed_workspace(ws_id, &cwd, "Pagination Repo"))
        .expect("upsert ws");
    rt.store
        .upsert_agent(&make_seed_agent(agent_id, ws_id, &cwd, "Big Session"), None)
        .expect("upsert agent");

    // Construct a 500-line unified diff on Turn 209
    let mut big_diff = String::from(
        "diff --git a/src/big.rs b/src/big.rs\n--- a/src/big.rs\n+++ b/src/big.rs\n@@ -1,0 +1,500 @@\n",
    );
    for line_no in 1..=500 {
        big_diff.push_str(&format!(
            "+pub fn big_fn_{line_no}() -> usize {{ {line_no} }}\n"
        ));
    }

    let mut diff_seq = 0_u64;
    for turn_no in 1..=210_u32 {
        rt.store
            .append_timeline_event(
                agent_id,
                1,
                Some(&format!("t_{turn_no}")),
                TimelinePayload::UserPrompt {
                    message_id: format!("u_{turn_no}"),
                    text: format!("PROMPT_{turn_no}"),
                    badges: Vec::new(),
                },
            )
            .expect("append prompt");
        if turn_no == 209 {
            let ev = rt
                .store
                .append_timeline_event(
                    agent_id,
                    1,
                    Some("t_209"),
                    TimelinePayload::ToolCall {
                        call_id: "tc_diff_209".into(),
                        status: ToolCallStatus::Completed,
                        detail: ToolCallDetail::FileEdit {
                            path: "src/big.rs".into(),
                            unified_diff: Some(big_diff.clone()),
                        },
                        background_task: None,
                    },
                )
                .expect("append tool diff");
            diff_seq = ev.seq;
        }
        rt.store
            .append_timeline_event(
                agent_id,
                1,
                Some(&format!("t_{turn_no}")),
                TimelinePayload::AssistantMessage {
                    item_id: format!("a_{turn_no}"),
                    text: format!("ANSWER_{turn_no}"),
                    streaming: false,
                },
            )
            .expect("append answer");
    }
    assert!(diff_seq > 0, "diff_seq must be recorded");

    let uri = rt
        .build_pairing_offer(vec![format!("ws://{addr}/ws")], None)
        .expect("offer")
        .to_qr_uri()
        .expect("uri");
    let (core, host_id) = open_and_connect_core(home.join("c3.db"), uri).await;

    core.open_agent_timeline(&host_id, agent_id);
    assert!(
        wait_until(Duration::from_secs(10), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            async move {
                let d = c.agent_stage_delta_by_request(&AgentStageDeltaRequest {
                    host_id: hid,
                    agent_id: agent_id.into(),
                    since_revision: 0,
                    since_aux_revision: None,
                    turn_limit: Some(40),
                    since_window_start_turn_index: None,
                });
                d.latest_seq >= 421 && !d.changed_turns.is_empty()
            }
        })
        .await,
        "Initial tail catchup must load tail turns with latest_seq >= 421"
    );

    let initial_delta = core.agent_stage_delta_by_request(&AgentStageDeltaRequest {
        host_id: host_id.clone(),
        agent_id: agent_id.into(),
        since_revision: 0,
        since_aux_revision: None,
        turn_limit: Some(40),
        since_window_start_turn_index: None,
    });
    assert!(
        initial_delta.changed_turns.len() <= 40,
        "Initial windowed delta must return at most 40 turns, got {}",
        initial_delta.changed_turns.len()
    );
    assert!(
        initial_delta.has_older_turns,
        "has_older_turns must be true"
    );
    assert!(
        initial_delta.has_older_history,
        "has_older_history must be true"
    );
    assert!(
        initial_delta.oldest_loaded_seq > 1,
        "Tail catchup must start after seq 1, got {}",
        initial_delta.oldest_loaded_seq
    );
    let last_turn = initial_delta.changed_turns.last().expect("tail turn");
    assert!(turn_user_prompt_text(last_turn).contains("PROMPT_210"));
    assert!(
        turn_reply_texts(last_turn)
            .iter()
            .any(|s| s.contains("ANSWER_210"))
    );

    let opts_delta = core.agent_stage_delta_with_options(&host_id, agent_id, 0, None, Some(40));
    assert!(
        opts_delta.has_older_history,
        "agent_stage_delta_with_options must propagate has_older_history"
    );

    // Lazy on-demand tool diff must recover the full 500-line FileEdit diff
    let diffs = core.get_tool_call_diff(&host_id, agent_id, diff_seq);
    assert!(
        !diffs.is_empty(),
        "get_tool_call_diff must return parsed FileEdit diff"
    );
    assert!(
        diffs[0].additions >= 500 || diffs.iter().map(|d| d.lines.len()).sum::<usize>() >= 400,
        "Lazy diff must preserve full additions/rendered lines, got {:?}",
        (diffs[0].additions, diffs[0].lines.len())
    );

    // Paginate backward over FetchTimelineBefore until all 210 turns reach seq 1
    let mut prev_oldest = initial_delta.oldest_loaded_seq;
    for _ in 0..10 {
        if prev_oldest <= 1 {
            break;
        }
        let dispatched = core.load_older_agent_timeline(&host_id, agent_id, 150);
        if !dispatched {
            break;
        }
        let decreased = wait_until(Duration::from_secs(8), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            async move {
                let d = c.agent_stage_delta_with_options(&hid, agent_id, 0, None, Some(40));
                d.oldest_loaded_seq > 0 && d.oldest_loaded_seq < prev_oldest
            }
        })
        .await;
        assert!(
            decreased,
            "FetchTimelineBefore must lower oldest_loaded_seq below {prev_oldest}"
        );
        prev_oldest = core
            .agent_stage_delta_with_options(&host_id, agent_id, 0, None, Some(40))
            .oldest_loaded_seq;
    }

    let full_delta = core.agent_stage_delta_with_options(&host_id, agent_id, 0, None, None);
    assert_eq!(full_delta.oldest_loaded_seq, 1);
    assert!(!full_delta.has_older_history);

    let full_stage = core.agent_stage(&host_id, agent_id);
    assert_eq!(
        full_stage.turns.len(),
        210,
        "All 210 turns must be reconstructed after paging"
    );
    for (idx, turn) in full_stage.turns.iter().enumerate() {
        let expected = format!("PROMPT_{}", idx + 1);
        assert_eq!(
            turn_user_prompt_text(turn),
            expected,
            "Missing or out-of-order turn {}",
            idx + 1
        );
        if idx > 0 {
            assert!(
                turn.anchor_seq > full_stage.turns[idx - 1].anchor_seq,
                "anchor_seq must strictly increase across paginated history"
            );
        }
    }

    drop_core(core).await;
    drop(rt);
}

// ============================================================================
// PITFALL 4: Off-LAN Relay Propagation & Fleet ConfigureHostRelay
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pitfall_04_off_lan_relay_propagation_and_fleet_configure_host_relay() {
    let tmp_root = new_temp_dir("p4_r");
    let home_a = tmp_root.join("ha");
    let home_b = tmp_root.join("hb");
    std::fs::create_dir_all(&home_a).expect("mkdir ha");
    std::fs::create_dir_all(&home_b).expect("mkdir hb");

    let pubkey_a = DaemonRuntime::bootstrap(&home_a)
        .await
        .expect("boot a")
        .keypair
        .public_key_b64
        .clone();
    let pubkey_b = DaemonRuntime::bootstrap(&home_b)
        .await
        .expect("boot b")
        .keypair
        .public_key_b64
        .clone();

    let relay_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind relay");
    let relay_addr = relay_listener.local_addr().expect("relay addr");
    drop(relay_listener);
    let relay_url = format!("ws://{relay_addr}");

    let mut allowed = HashSet::new();
    allowed.insert(pubkey_a);
    allowed.insert(pubkey_b);
    let relay_handle = tokio::spawn(async move {
        let _ = run_relay(RelayConfig {
            listen_addr: relay_addr,
            allowed_pubkeys: Arc::new(allowed),
        })
        .await;
    });
    tokio::time::sleep(Duration::from_millis(80)).await;

    let (addr_a, rt_a) = spawn_embedded_daemon(home_a.clone(), None)
        .await
        .expect("spawn daemon a");
    let (addr_b, rt_b) = spawn_embedded_daemon(home_b.clone(), None)
        .await
        .expect("spawn daemon b");

    assert!(rt_a.effective_host_info().await.relay_url.is_none());
    assert!(rt_b.effective_host_info().await.relay_url.is_none());

    // Desktop core #1 pairs with both daemons directly (without relay initially)
    let core1 = open_core(tmp_root.join("c1.db")).await;
    let uri_a = rt_a
        .build_pairing_offer(vec![format!("ws://{addr_a}/ws")], None)
        .expect("offer a")
        .to_qr_uri()
        .expect("uri a");
    let uri_b = rt_b
        .build_pairing_offer(vec![format!("ws://{addr_b}/ws")], None)
        .expect("offer b")
        .to_qr_uri()
        .expect("uri b");
    let hid_a = core1.pair_host(&uri_a).expect("pair a").host_id;
    let hid_b = core1.pair_host(&uri_b).expect("pair b").host_id;
    core1.connect_all_hosts();

    assert!(
        wait_until(Duration::from_secs(10), || {
            let c = Arc::clone(&core1);
            let ha = hid_a.clone();
            let hb = hid_b.clone();
            async move {
                let a_ok = c
                    .host_detail(&ha)
                    .is_some_and(|d| d.host.connection_state == HostConnectionState::OnlineDirect);
                let b_ok = c
                    .host_detail(&hb)
                    .is_some_and(|d| d.host.connection_state == HostConnectionState::OnlineDirect);
                a_ok && b_ok
            }
        })
        .await,
        "Desktop core1 must connect directly to both daemons"
    );

    let updated = core1
        .apply_relay_to_fleet(&relay_url)
        .expect("apply_relay_to_fleet");
    assert_eq!(updated, 2);

    assert!(
        wait_until(Duration::from_secs(10), || {
            let ra = Arc::clone(&rt_a);
            let rb = Arc::clone(&rt_b);
            let rurl = relay_url.clone();
            async move {
                ra.effective_host_info().await.relay_url.as_deref() == Some(rurl.as_str())
                    && rb.effective_host_info().await.relay_url.as_deref() == Some(rurl.as_str())
            }
        })
        .await,
        "Both daemons must receive ConfigureHostRelay and expose effective_host_info().relay_url"
    );

    // Wait briefly until both daemons finish outbound relay registration
    let secret_a = rt_a.pairing_secret.clone();
    let secret_b = rt_b.pairing_secret.clone();
    let pk_a = rt_a.keypair.public_key_b64.clone();
    let pk_b = rt_b.keypair.public_key_b64.clone();
    assert!(
        wait_until(Duration::from_secs(8), || {
            let rurl = relay_url.clone();
            let pka = pk_a.clone();
            let sa = secret_a.clone();
            let pkb = pk_b.clone();
            let sb = secret_b.clone();
            async move {
                let ok_a = connect_via_relay(&rurl, &pka, &sa, None).await.is_ok();
                let ok_b = connect_via_relay(&rurl, &pkb, &sb, None).await.is_ok();
                ok_a && ok_b
            }
        })
        .await,
        "Both daemons must finish outbound registration on relay"
    );

    // Off-LAN Mobile core #2 has unreachable direct LAN endpoints and must connect to BOTH via OnlineRelay
    let core2 = open_core(tmp_root.join("c2.db")).await;
    let mob_uri_a = rt_a
        .build_pairing_offer(vec!["ws://127.0.0.1:1/ws".into()], Some(relay_url.clone()))
        .expect("mob offer a")
        .to_qr_uri()
        .expect("mob uri a");
    let mob_uri_b = rt_b
        .build_pairing_offer(vec!["ws://127.0.0.1:1/ws".into()], Some(relay_url.clone()))
        .expect("mob offer b")
        .to_qr_uri()
        .expect("mob uri b");
    let mob_ha = core2.pair_host(&mob_uri_a).expect("mob pair a").host_id;
    let mob_hb = core2.pair_host(&mob_uri_b).expect("mob pair b").host_id;
    core2.connect_all_hosts();

    assert!(
        wait_until(Duration::from_secs(12), || {
            let c = Arc::clone(&core2);
            let ha = mob_ha.clone();
            let hb = mob_hb.clone();
            async move {
                let a_ok = c
                    .host_detail(&ha)
                    .is_some_and(|d| d.host.connection_state == HostConnectionState::OnlineRelay);
                let b_ok = c
                    .host_detail(&hb)
                    .is_some_and(|d| d.host.connection_state == HostConnectionState::OnlineRelay);
                a_ok && b_ok
            }
        })
        .await,
        "Off-LAN client must reach both fleet hosts over OnlineRelay"
    );

    // Fresh core #3 connecting directly without relay_url must auto-adopt Daemon A's relay_url from Welcome
    let core3 = open_core(tmp_root.join("c3.db")).await;
    let welcome_uri_a = rt_a
        .build_pairing_offer(vec![format!("ws://{addr_a}/ws")], None)
        .expect("welcome offer a")
        .to_qr_uri()
        .expect("welcome uri a");
    let w_ha = core3.pair_host(&welcome_uri_a).expect("pair c3").host_id;
    core3.connect_host_async(w_ha.clone());
    assert!(
        wait_until(Duration::from_secs(10), || {
            let c = Arc::clone(&core3);
            let ha = w_ha.clone();
            let rurl = relay_url.clone();
            async move {
                c.host_detail(&ha).is_some_and(|d| {
                    d.host.connection_state == HostConnectionState::OnlineDirect
                        && d.host.relay_url.as_deref() == Some(rurl.as_str())
                })
            }
        })
        .await,
        "Client pairing directly without relay_url must auto-adopt host's relay_url from Welcome"
    );

    drop_core(core1).await;
    drop_core(core2).await;
    drop_core(core3).await;
    drop(rt_a);
    drop(rt_b);
    relay_handle.abort();
}

// ============================================================================
// PITFALL 5: Mission Workspace Worktree Isolation & Historical Handle Alias
// ============================================================================
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pitfall_05_mission_workspace_parallel_worktree_isolation_and_handle_alias() {
    install_mock_script(
        &MOCK_CLAUDE_PATH,
        ProviderKind::ClaudeCode,
        "mock_claude_reg.py",
        MOCK_CLAUDE_SCRIPT,
    );
    let home = new_temp_dir("p5_h");
    let repo = new_temp_dir("p5_repo");
    init_git_repo(&repo);

    let (addr, rt) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("spawn daemon");
    let uri = rt
        .build_pairing_offer(vec![format!("ws://{addr}/ws")], None)
        .expect("offer")
        .to_qr_uri()
        .expect("uri");
    let (core, host_id) = open_and_connect_core(home.join("c5.db"), uri).await;

    core.create_agent_with_profile(
        orbit_client_core::app_core::CreateAgentRequest {
            host_id: &host_id,
            cwd: &repo.to_string_lossy(),
            extra_dirs: Vec::new(),
            provider: ProviderKind::ClaudeCode,
            acp_agent_id: None,
            model: None,
            effort: None,
            fast_mode: None,
            mode: SessionMode::AcceptEdits,
            initial_prompt: "[MOCK:FORK_WORKTREE]",
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree_branch: None,
            workspace_id: None,
            handle: Some("alpha".into()),
            auto_cluster: Some(false),
        },
        None,
    );

    assert!(
        wait_until(Duration::from_secs(20), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            async move {
                let Some(d) = c.host_detail(&hid) else {
                    return false;
                };
                let alpha = d.agents.iter().find(|a| {
                    !a.agent_id.starts_with("opt_") && a.handle.as_deref() == Some("alpha")
                });
                let beta = d.agents.iter().find(|a| {
                    !a.agent_id.starts_with("opt_") && a.handle.as_deref() == Some("beta")
                });
                match (alpha, beta) {
                    (Some(a), Some(b)) => {
                        !a.is_running
                            && !b.is_running
                            && a.latest_seq > 0
                            && b.latest_seq > 0
                            && a.workspace_id == b.workspace_id
                    }
                    _ => false,
                }
            }
        })
        .await,
        "@alpha and worktree-isolated @beta must both finish in the same Mission Workspace"
    );

    let detail = core.host_detail(&host_id).expect("detail");
    let alpha = detail
        .agents
        .iter()
        .find(|a| a.handle.as_deref() == Some("alpha"))
        .expect("alpha");
    let beta = detail
        .agents
        .iter()
        .find(|a| a.handle.as_deref() == Some("beta"))
        .expect("beta");
    assert_eq!(alpha.workspace_id, beta.workspace_id);
    assert_ne!(
        alpha.cwd, beta.cwd,
        "@beta spawned with worktree=true must run in an isolated checkout directory"
    );
    let alpha_id = alpha.agent_id.clone();
    let beta_id = beta.agent_id.clone();

    core.open_agent_timeline(&host_id, &alpha_id);
    core.open_agent_timeline(&host_id, &beta_id);

    // Check workspace_peers reports 0 shared checkout peers and beta isolated
    core.send_prompt(&host_id, &alpha_id, "[MOCK:CHECK_PEERS]");
    assert!(
        wait_until(Duration::from_secs(12), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let aid = alpha_id.clone();
            async move {
                stage_settled_with_text(&c, &hid, &aid, "SHARED_COUNT=0 BETA_ISOLATED=True")
            }
        })
        .await,
        "workspace_peers must report zero shared_checkout_peers and BETA_ISOLATED=True"
    );

    // Rename @beta -> @beta-renamed and ensure @alpha messaging legacy `TARGET=beta` still resolves
    core.update_agent_with_handle(
        &host_id,
        &beta_id,
        None,
        None,
        None,
        None,
        None,
        Some("beta-renamed".into()),
    );
    assert!(
        wait_until(Duration::from_secs(10), || {
            let store = Arc::clone(&rt.store);
            let bid = beta_id.clone();
            async move {
                store.get_agent(&bid).ok().flatten().is_some_and(|a| {
                    a.handle.as_deref() == Some("beta-renamed")
                        && a.labels
                            .get(orbit_store::LABEL_HANDLE_ALIASES)
                            .is_some_and(|raw| raw.split(',').any(|x| x.trim() == "beta"))
                })
            }
        })
        .await,
        "Updating handle via UpdateAgent must persist `beta-renamed` and preserve old handle `beta` in orbit.handle_aliases"
    );

    core.send_prompt(
        &host_id,
        &alpha_id,
        "[MOCK:MSG_PEER] TARGET=beta TEXT=ECHO=ALIAS_PING",
    );
    let alpha_ok = wait_until(Duration::from_secs(15), || {
        let c = Arc::clone(&core);
        let hid = host_id.clone();
        let aid = alpha_id.clone();
        async move {
            stage_settled_with_text(&c, &hid, &aid, "MSG_OK=True RESOLVED_VIA_ALIAS=True")
        }
    })
    .await;
    assert!(
        alpha_ok,
        "message_peer with legacy handle `@beta` must succeed and report resolved_via_alias=true"
    );
    assert!(
        wait_until(Duration::from_secs(20), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let bid = beta_id.clone();
            async move { stage_settled_with_text(&c, &hid, &bid, "REPLY=ALIAS_PING") }
        })
        .await,
        "@beta-renamed must receive the peer message via historical handle alias and reply with REPLY=ALIAS_PING"
    );

    drop_core(core).await;
    drop(rt);
}

// ============================================================================
// PITFALL 6: Cross-Turn Late Assistant Finalization Dedup Over WS
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pitfall_06_late_assistant_finalization_cross_turn_dedup_over_ws() {
    let home = new_temp_dir("p6_h");
    let (addr, rt) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("spawn daemon");

    let ws_id = "ws_dedup_1";
    let agent_id = "ag_dedup_1";
    let cwd = home.to_string_lossy().to_string();
    rt.store
        .upsert_workspace(&make_seed_workspace(ws_id, &cwd, "Dedup Repo"))
        .expect("upsert ws");
    rt.store
        .upsert_agent(
            &make_seed_agent(agent_id, ws_id, &cwd, "Dedup Session"),
            None,
        )
        .expect("upsert agent");

    let uri = rt
        .build_pairing_offer(vec![format!("ws://{addr}/ws")], None)
        .expect("offer")
        .to_qr_uri()
        .expect("uri");
    let (core1, host_id) = open_and_connect_core(home.join("c6_1.db"), uri.clone()).await;
    core1.open_agent_timeline(&host_id, agent_id);

    // Turn 1 user prompt + streaming partial reply
    let ev_u1 = rt
        .store
        .append_timeline_event(
            agent_id,
            1,
            Some("t1"),
            TimelinePayload::UserPrompt {
                message_id: "u1".into(),
                text: "Question 1".into(),
                badges: Vec::new(),
            },
        )
        .expect("append u1");
    let _ = rt
        .orchestrator
        .broadcast_tx
        .send(DaemonBroadcast::TimelineLive(ev_u1));

    let ev_m1_partial = rt
        .store
        .append_timeline_event(
            agent_id,
            1,
            Some("t1"),
            TimelinePayload::AssistantMessage {
                item_id: "m1".into(),
                text: "Answer 1 partial".into(),
                streaming: true,
            },
        )
        .expect("append m1 partial");
    let m1_seq = ev_m1_partial.seq;
    let _ = rt
        .orchestrator
        .broadcast_tx
        .send(DaemonBroadcast::TimelineLive(ev_m1_partial));

    assert!(
        wait_until(Duration::from_secs(8), || {
            let c = Arc::clone(&core1);
            let hid = host_id.clone();
            async move { stage_has_text(&c, &hid, agent_id, "Answer 1 partial") }
        })
        .await,
        "core1 must observe live Turn 1 partial streaming message"
    );

    // Turn 2 user prompt arrives BEFORE Turn 1 assistant finalization
    let ev_u2 = rt
        .store
        .append_timeline_event(
            agent_id,
            1,
            Some("t2"),
            TimelinePayload::UserPrompt {
                message_id: "u2".into(),
                text: "Question 2".into(),
                badges: Vec::new(),
            },
        )
        .expect("append u2");
    let _ = rt
        .orchestrator
        .broadcast_tx
        .send(DaemonBroadcast::TimelineLive(ev_u2));

    // Late Turn 1 finalization for item_id="m1" must update pre-boundary seq IN PLACE
    let (ev_m1_final, mutated) = rt
        .store
        .append_timeline_event_deduped(
            agent_id,
            1,
            Some("t1"),
            TimelinePayload::AssistantMessage {
                item_id: "m1".into(),
                text: "Answer 1 final".into(),
                streaming: false,
            },
        )
        .expect("late finalization m1");
    assert!(mutated, "Late finalization must update persisted row");
    assert_eq!(
        ev_m1_final.seq, m1_seq,
        "Late finalization must update pre-boundary seq {} in place instead of jumping after Question 2",
        m1_seq
    );
    let _ = rt
        .orchestrator
        .broadcast_tx
        .send(DaemonBroadcast::TimelineLive(ev_m1_final));

    // Turn 2 assistant finalization
    let ev_m2_final = rt
        .store
        .append_timeline_event(
            agent_id,
            1,
            Some("t2"),
            TimelinePayload::AssistantMessage {
                item_id: "m2".into(),
                text: "Answer 2 final".into(),
                streaming: false,
            },
        )
        .expect("append m2 final");
    let _ = rt
        .orchestrator
        .broadcast_tx
        .send(DaemonBroadcast::TimelineLive(ev_m2_final));

    assert!(
        wait_until(Duration::from_secs(8), || {
            let c = Arc::clone(&core1);
            let hid = host_id.clone();
            async move {
                let st = c.agent_stage(&hid, agent_id);
                st.turns.len() == 2
                    && turn_reply_texts(&st.turns[0])
                        .iter()
                        .any(|s| s == "Answer 1 final")
                    && turn_reply_texts(&st.turns[1])
                        .iter()
                        .any(|s| s == "Answer 2 final")
            }
        })
        .await,
        "Live core1 must keep Answer 1 final inside Turn 1 and Answer 2 final inside Turn 2"
    );

    // Fresh core2 connecting over WS must receive identical non-duplicated 2-turn stage
    let (core2, host_id2) = open_and_connect_core(home.join("c6_2.db"), uri).await;
    core2.open_agent_timeline(&host_id2, agent_id);
    assert!(
        wait_until(Duration::from_secs(8), || {
            let c = Arc::clone(&core2);
            let hid = host_id2.clone();
            async move { stage_has_text(&c, &hid, agent_id, "Answer 2 final") }
        })
        .await,
        "Fresh core2 catchup must load both turns"
    );

    for (label, st) in [
        ("core1", core1.agent_stage(&host_id, agent_id)),
        ("core2", core2.agent_stage(&host_id2, agent_id)),
    ] {
        assert_eq!(st.turns.len(), 2, "{label} must have exactly 2 turns");
        assert_eq!(turn_user_prompt_text(&st.turns[0]), "Question 1");
        assert_eq!(turn_user_prompt_text(&st.turns[1]), "Question 2");
        let t1_replies = turn_reply_texts(&st.turns[0]);
        let t2_replies = turn_reply_texts(&st.turns[1]);
        assert!(
            t1_replies.iter().any(|s| s == "Answer 1 final"),
            "{label} Turn 1 must contain Answer 1 final, got {:?}",
            t1_replies
        );
        assert!(
            t2_replies.iter().any(|s| s == "Answer 2 final")
                && !t2_replies.iter().any(|s| s.contains("Answer 1")),
            "{label} Turn 2 must contain only Answer 2 final without leaking Answer 1, got {:?}",
            t2_replies
        );
        assert_eq!(
            st.turns[0].turn_diffs.len() + st.turns[1].turn_diffs.len(),
            0
        );
    }

    drop_core(core1).await;
    drop_core(core2).await;
    drop(rt);
}

#[cfg(unix)]
fn is_non_zombie_pid_alive(pid: u32) -> bool {
    if pid <= 1 {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return false;
        };
        let Some(rparen) = stat.rfind(')') else {
            return false;
        };
        let state = stat[rparen + 1..].trim_start().chars().next();
        !matches!(state, None | Some('Z') | Some('X'))
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        let Ok(out) = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "state="])
            .output()
        else {
            return false;
        };
        if !out.status.success() {
            return false;
        }
        let state = String::from_utf8_lossy(&out.stdout);
        let trimmed = state.trim();
        !trimmed.is_empty() && !trimmed.starts_with('Z') && !trimmed.starts_with('X')
    }
}

#[cfg(unix)]
fn list_shim_pid_files(shims_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(shims_dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("pid"))
        .collect();
    files.sort();
    files
}

#[cfg(unix)]
fn parse_shim_pid_record(pid_path: &Path) -> Option<(u32, Option<u32>)> {
    let raw = std::fs::read_to_string(pid_path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let shim_pid = u32::try_from(val.get("shim_pid")?.as_u64()?).ok()?;
    let child_pid = val
        .get("child_pid")
        .and_then(|v| v.as_u64())
        .and_then(|n| u32::try_from(n).ok());
    Some((shim_pid, child_pid))
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pitfall_07_archive_agent_terminates_only_target_shim_pgid_without_signaling_siblings() {
    install_mock_script(
        &MOCK_CLAUDE_PATH,
        ProviderKind::ClaudeCode,
        "mock_claude_reg.py",
        MOCK_CLAUDE_SCRIPT,
    );

    let home = new_temp_dir("p7_h");
    let work = home.join("work");
    std::fs::create_dir_all(&work).expect("create work dir");
    let shims_dir = home.join("shims");

    let (addr, rt) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("spawn daemon");
    let uri = rt
        .build_pairing_offer(vec![format!("ws://{addr}/ws")], None)
        .expect("offer")
        .to_qr_uri()
        .expect("uri");
    let (core, host_id) = open_and_connect_core(home.join("c7.db"), uri).await;

    // 1. Spawn Agent A and wait until its shim .pid file and initial turn settle.
    core.create_agent(
        &host_id,
        &work.to_string_lossy(),
        ProviderKind::ClaudeCode,
        SessionMode::AcceptEdits,
        "ECHO=AGENT_A",
        None,
    );
    assert!(
        wait_until(Duration::from_secs(20), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let sd = shims_dir.clone();
            async move {
                let agents_ready = c.host_detail(&hid).is_some_and(|d| {
                    let real: Vec<_> = d
                        .agents
                        .iter()
                        .filter(|a| !a.agent_id.starts_with("opt_"))
                        .collect();
                    real.len() == 1 && real[0].latest_seq > 0 && !real[0].is_running
                });
                agents_ready && list_shim_pid_files(&sd).len() == 1
            }
        })
        .await,
        "Agent A should spawn, write its shim .pid, and finish initial turn"
    );

    let agent_a_id = core
        .host_detail(&host_id)
        .expect("host detail after A")
        .agents
        .into_iter()
        .find(|a| !a.agent_id.starts_with("opt_"))
        .expect("agent A summary")
        .agent_id;
    let a_pid_path = list_shim_pid_files(&shims_dir)
        .into_iter()
        .next()
        .expect("agent A .pid path");
    let a_sock_path = a_pid_path.with_extension("sock");
    let (a_shim_pid, a_child_pid) =
        parse_shim_pid_record(&a_pid_path).expect("parse agent A shim pid record");
    assert!(a_shim_pid > 1);

    // 2. Spawn Agent B and wait until its distinct shim .pid file and initial turn settle.
    core.create_agent(
        &host_id,
        &work.to_string_lossy(),
        ProviderKind::ClaudeCode,
        SessionMode::AcceptEdits,
        "ECHO=AGENT_B",
        None,
    );
    assert!(
        wait_until(Duration::from_secs(20), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let sd = shims_dir.clone();
            async move {
                let agents_ready = c.host_detail(&hid).is_some_and(|d| {
                    let real: Vec<_> = d
                        .agents
                        .iter()
                        .filter(|a| !a.agent_id.starts_with("opt_"))
                        .collect();
                    real.len() == 2 && real.iter().all(|a| a.latest_seq > 0 && !a.is_running)
                });
                agents_ready && list_shim_pid_files(&sd).len() == 2
            }
        })
        .await,
        "Agent B should spawn, write its distinct shim .pid, and finish initial turn"
    );

    let agent_b_id = core
        .host_detail(&host_id)
        .expect("host detail after B")
        .agents
        .into_iter()
        .find(|a| !a.agent_id.starts_with("opt_") && a.agent_id != agent_a_id)
        .expect("agent B summary")
        .agent_id;
    let b_pid_path = list_shim_pid_files(&shims_dir)
        .into_iter()
        .find(|p| p != &a_pid_path)
        .expect("agent B .pid path");
    let b_sock_path = b_pid_path.with_extension("sock");
    let (b_shim_pid, b_child_pid) =
        parse_shim_pid_record(&b_pid_path).expect("parse agent B shim pid record");
    assert!(b_shim_pid > 1);
    assert_ne!(a_shim_pid, b_shim_pid);

    // 3. Spawn an independent sibling canary in its own PGID to guard against kill(-1, SIGTERM).
    let mut canary = tokio::process::Command::new("sleep")
        .arg("30")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .expect("spawn external sibling canary");

    // 4. Archive Agent A and verify ONLY Agent A's shim/PGID terminates.
    core.archive_agent(&host_id, &agent_a_id);

    let store_ref = rt.store.clone();
    let aid_check = agent_a_id.clone();
    let a_pid_check = a_pid_path.clone();
    let a_sock_check = a_sock_path.clone();
    let a_archived_and_dead = wait_until(Duration::from_secs(15), || {
        let store = store_ref.clone();
        let aid = aid_check.clone();
        let pid_p = a_pid_check.clone();
        let sock_p = a_sock_check.clone();
        async move {
            let archived = store
                .get_agent(&aid)
                .ok()
                .flatten()
                .is_some_and(|a| a.archived_at.is_some());
            if !archived || pid_p.exists() {
                return false;
            }
            if orbit_shim::probe_existing_shim(&sock_p).await {
                return false;
            }
            if is_non_zombie_pid_alive(a_shim_pid) {
                return false;
            }
            a_child_pid.is_none_or(|p| p <= 1 || !is_non_zombie_pid_alive(p))
        }
    })
    .await;

    let canary_status = canary.try_wait();

    // Clean up canary unconditionally before any assertions so no sleep process leaks on failure.
    let _ = canary.kill().await;
    let _ = canary.wait().await;

    assert!(
        a_archived_and_dead,
        "Agent A ({agent_a_id}) should be archived with dead shim {a_shim_pid} and removed .pid/.sock"
    );
    assert!(
        matches!(canary_status, Ok(None)),
        "external sibling canary must remain running when Agent A is archived, got {canary_status:?}"
    );

    // 5. Verify Agent B's shim + provider remain intact and functional.
    assert!(b_pid_path.exists(), "Agent B .pid must remain intact");
    assert!(
        orbit_shim::probe_existing_shim(&b_sock_path).await,
        "Agent B shim socket must remain reachable"
    );
    assert!(
        is_non_zombie_pid_alive(b_shim_pid),
        "Agent B shim PID {b_shim_pid} must stay alive"
    );
    if let Some(cpid) = b_child_pid {
        assert!(
            is_non_zombie_pid_alive(cpid),
            "Agent B child PID {cpid} must stay alive"
        );
    }

    core.open_agent_timeline(&host_id, &agent_b_id);
    core.send_prompt(&host_id, &agent_b_id, "ECHO=STILL_ALIVE");
    assert!(
        wait_until(Duration::from_secs(15), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let aid = agent_b_id.clone();
            async move { stage_settled_with_text(&c, &hid, &aid, "REPLY=STILL_ALIVE") }
        })
        .await,
        "Agent B should still process prompts cleanly after Agent A is archived"
    );

    drop_core(core).await;
    drop(rt);
}

// ============================================================================
// PITFALL 8: Multi-Agent Non-Interrupting Delivery, Stall Telemetry & Preemption
// ============================================================================
#[cfg(unix)]
fn collect_stage_agent_messages(
    stage: &orbit_client_core::stage::AgentStageView,
) -> Vec<&TimelineItemView> {
    let mut out = Vec::new();
    for turn in &stage.turns {
        if let Some(item @ TimelineItemView::AgentMessage { .. }) = turn.agent_message.as_ref() {
            out.push(item);
        }
        for seg in &turn.live_segments {
            if let TurnLiveSegmentView::AgentMessage { item, .. } = seg {
                out.push(item);
            }
        }
        for item in &turn.folded_steps {
            if matches!(item, TimelineItemView::AgentMessage { .. }) {
                out.push(item);
            }
        }
    }
    out
}

#[cfg(unix)]
fn read_mock_acp_trace(trace_path: &Path) -> Vec<serde_json::Value> {
    let Ok(raw) = std::fs::read_to_string(trace_path) else {
        return Vec::new();
    };
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .collect()
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pitfall_08_multi_agent_non_interrupting_delivery_and_stall_recovery() {
    install_mock_script(
        &MOCK_ANTIGRAVITY_PATH,
        ProviderKind::Antigravity,
        "mock_agy_reg.py",
        MOCK_ANTIGRAVITY_SCRIPT,
    );
    install_mock_script(
        &MOCK_CODEX_PATH,
        ProviderKind::Codex,
        "mock_codex_reg.py",
        MOCK_CODEX_SCRIPT,
    );
    install_mock_script(
        &MOCK_CLAUDE_PATH,
        ProviderKind::ClaudeCode,
        "mock_claude_reg.py",
        MOCK_CLAUDE_SCRIPT,
    );

    let home = new_temp_dir("p8_h");
    let work = new_temp_dir("p8_w");
    let trace_path = work.join(".mock_acp_trace.jsonl");

    let (addr, rt) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("spawn daemon");
    let uri = rt
        .build_pairing_offer(vec![format!("ws://{addr}/ws")], None)
        .expect("offer")
        .to_qr_uri()
        .expect("uri");
    let (core, host_id) = open_and_connect_core(home.join("c8.db"), uri).await;

    // Spawn Antigravity parent & wait until idle.
    core.create_agent(
        &host_id,
        &work.to_string_lossy(),
        ProviderKind::Antigravity,
        SessionMode::AcceptEdits,
        "ECHO=PARENT_BOOT",
        None,
    );
    assert!(
        wait_until(Duration::from_secs(15), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            async move {
                c.host_detail(&hid).is_some_and(|d| {
                    d.agents.iter().any(|a| {
                        !a.agent_id.starts_with("opt_")
                            && a.provider == ProviderKind::Antigravity
                            && !a.is_running
                            && a.latest_seq > 0
                    })
                })
            }
        })
        .await,
        "Antigravity parent must boot and finish initial turn"
    );
    let parent_id = core
        .host_detail(&host_id)
        .expect("host detail")
        .agents
        .into_iter()
        .find(|a| !a.agent_id.starts_with("opt_") && a.provider == ProviderKind::Antigravity)
        .expect("parent summary")
        .agent_id;
    core.open_agent_timeline(&host_id, &parent_id);
    assert!(
        wait_until(Duration::from_secs(8), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let aid = parent_id.clone();
            async move { stage_settled_with_text(&c, &hid, &aid, "REPLY=PARENT_BOOT") }
        })
        .await,
        "Parent boot turn should render REPLY=PARENT_BOOT"
    );

    // Verify Scenario B baseline invariant on settled turn: latest_visible_progress_ms == latest_activity_ms.
    let boot_stage = core.agent_stage(&host_id, &parent_id);
    let boot_turn = boot_stage.turns.last().expect("boot turn present");
    assert!(boot_turn.latest_activity_ms.is_some());
    assert_eq!(
        boot_turn.latest_visible_progress_ms, boot_turn.latest_activity_ms,
        "settled reply turn must keep latest_visible_progress_ms equal to latest_activity_ms"
    );

    // Spawn Claude peer & wait until idle.
    core.create_agent(
        &host_id,
        &work.to_string_lossy(),
        ProviderKind::ClaudeCode,
        SessionMode::AcceptEdits,
        "ECHO=CLAUDE_BOOT",
        None,
    );
    assert!(
        wait_until(Duration::from_secs(15), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            async move {
                c.host_detail(&hid).is_some_and(|d| {
                    d.agents.iter().any(|a| {
                        !a.agent_id.starts_with("opt_")
                            && a.provider == ProviderKind::ClaudeCode
                            && !a.is_running
                            && a.latest_seq > 0
                    })
                })
            }
        })
        .await,
        "Claude peer must boot and finish initial turn"
    );
    let claude_id = core
        .host_detail(&host_id)
        .expect("host detail after claude")
        .agents
        .into_iter()
        .find(|a| !a.agent_id.starts_with("opt_") && a.provider == ProviderKind::ClaudeCode)
        .expect("claude summary")
        .agent_id;

    // ------------------------------------------------------------------------
    // Scenario A: Non-interrupting multi-provider queueing & FIFO settlement
    // ------------------------------------------------------------------------
    let gate1 = work.join("gate1.flag");
    core.send_prompt(
        &host_id,
        &parent_id,
        &format!(
            "WAIT_FILE={} SPAWN_CODEX_PROMPT=ECHO=CODEX_NOTIFY ECHO=PARENT_T1",
            gate1.display()
        ),
    );

    // Wait for Codex child to finish AND its finish notification to coalesce & enter Queued state.
    assert!(
        wait_until(Duration::from_secs(20), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let pid = parent_id.clone();
            async move {
                let st = c.agent_stage(&hid, &pid);
                collect_stage_agent_messages(&st).iter().any(|it| {
                    matches!(
                        it,
                        TimelineItemView::AgentMessage {
                            delivery_status: Some(AgentMessageDeliveryStatus::Queued),
                            blocking_turn_id: Some(_),
                            ..
                        }
                    )
                })
            }
        })
        .await,
        "Codex finish notification must enqueue as Queued while parent turn is active"
    );

    // Now have Claude peer call MCP `send_agent_prompt` omitting `active_turn_behavior` (defaults to queue).
    core.send_prompt(
        &host_id,
        &claude_id,
        &format!("[MOCK:SEND_AGENT_PROMPT] TARGET_ID={parent_id} MSG=NOTIFY_TAG=CLAUDE_PEER"),
    );

    assert!(
        wait_until(Duration::from_secs(15), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let pid = parent_id.clone();
            async move {
                let st = c.agent_stage(&hid, &pid);
                let queued_count = collect_stage_agent_messages(&st)
                    .iter()
                    .filter(|it| {
                        matches!(
                            it,
                            TimelineItemView::AgentMessage {
                                delivery_status: Some(AgentMessageDeliveryStatus::Queued),
                                blocking_turn_id: Some(_),
                                ..
                            }
                        )
                    })
                    .count();
                queued_count >= 2
            }
        })
        .await,
        "Both Codex finish notification and Claude peer message must queue without interrupting parent"
    );

    let cancel_count_before_release = read_mock_acp_trace(&trace_path)
        .iter()
        .filter(|e| e.get("method").and_then(|v| v.as_str()) == Some("session/cancel"))
        .count();
    assert_eq!(
        cancel_count_before_release, 0,
        "agent notifications must trigger 0 session/cancel calls while parent is busy"
    );

    // Release gate1 -> parent finishes turn 1 and drains both queued items FIFO.
    std::fs::write(&gate1, "go").expect("write gate1");

    assert!(
        wait_until(Duration::from_secs(20), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let pid = parent_id.clone();
            async move {
                let parent_idle = c
                    .host_detail(&hid)
                    .and_then(|d| d.agents.into_iter().find(|a| a.agent_id == pid))
                    .is_some_and(|a| !a.is_running);
                if !parent_idle {
                    return false;
                }
                let st = c.agent_stage(&hid, &pid);
                let msgs = collect_stage_agent_messages(&st);
                msgs.len() >= 2
                    && msgs.iter().take(2).all(|it| {
                        matches!(
                            it,
                            TimelineItemView::AgentMessage {
                                delivery_status: Some(AgentMessageDeliveryStatus::Settled),
                                ..
                            }
                        )
                    })
                    && stage_has_text(&c, &hid, &pid, "REPLY=CODEX_NOTIFY")
                    && stage_has_text(&c, &hid, &pid, "REPLY=CLAUDE_PEER")
            }
        })
        .await,
        "Both queued notifications must drain FIFO and settle cleanly after gate1 release"
    );

    let st_after_a = core.agent_stage(&host_id, &parent_id);
    let msgs_a = collect_stage_agent_messages(&st_after_a);
    let mut dispatch_times_a = Vec::new();
    for msg in msgs_a.iter().take(2) {
        if let TimelineItemView::AgentMessage {
            queued_at_ms,
            dispatched_at_ms,
            settled_at_ms,
            ..
        } = msg
        {
            let q = queued_at_ms.expect("queued_at_ms");
            let d = dispatched_at_ms.expect("dispatched_at_ms");
            let s = settled_at_ms.expect("settled_at_ms");
            assert!(
                q <= d && d <= s,
                "timestamps must be monotonic: q={q} d={d} s={s}"
            );
            dispatch_times_a.push(d);
        }
    }
    assert!(
        dispatch_times_a[0] <= dispatch_times_a[1],
        "queued messages must dispatch in FIFO order: {dispatch_times_a:?}"
    );

    let trace_a = read_mock_acp_trace(&trace_path);
    assert_eq!(
        trace_a
            .iter()
            .filter(|e| e.get("method").and_then(|v| v.as_str()) == Some("session/cancel"))
            .count(),
        0,
        "Scenario A must have zero session/cancel calls"
    );
    let codex_pos = trace_a
        .iter()
        .position(|e| {
            e.get("text")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.contains("CODEX_NOTIFY") && !s.contains("WAIT_FILE="))
        })
        .expect("codex notification prompt recorded");
    let claude_pos = trace_a
        .iter()
        .position(|e| {
            e.get("text")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.contains("CLAUDE_PEER"))
        })
        .expect("claude peer prompt recorded");
    assert!(
        codex_pos < claude_pos,
        "Codex notification ({codex_pos}) must dispatch before Claude peer ({claude_pos})"
    );

    // ------------------------------------------------------------------------
    // Scenario B: Thinking-stall detection + queue-preserving InterruptAndContinue
    // ------------------------------------------------------------------------
    let gate2 = work.join("gate2.flag");
    core.send_prompt(
        &host_id,
        &parent_id,
        &format!(
            "WAIT_FILE={} THINK_LOOP=1 ECHO=THINK_STALL",
            gate2.display()
        ),
    );
    core.send_prompt(
        &host_id,
        &claude_id,
        &format!("[MOCK:SEND_AGENT_PROMPT] TARGET_ID={parent_id} MSG=NOTIFY_TAG=STALL_QUEUED"),
    );

    let reached_stall = wait_until(Duration::from_secs(15), || {
        let c = Arc::clone(&core);
        let hid = host_id.clone();
        let pid = parent_id.clone();
        async move {
            let st = c.agent_stage(&hid, &pid);
            let thinking_stalled = st.turns.last().is_some_and(|t| {
                matches!(
                    (t.latest_activity_ms, t.latest_visible_progress_ms),
                    (Some(act), Some(vis)) if act > vis
                )
            });
            let has_queued = collect_stage_agent_messages(&st).iter().any(|it| {
                matches!(
                    it,
                    TimelineItemView::AgentMessage {
                        text,
                        delivery_status: Some(AgentMessageDeliveryStatus::Queued),
                        ..
                    } if text.contains("STALL_QUEUED")
                )
            });
            thinking_stalled && has_queued
        }
    })
    .await;
    if !reached_stall {
        let st_dbg = core.agent_stage(&host_id, &parent_id);
        let last_dbg = st_dbg.turns.last().map(|t| {
            (
                t.key.clone(),
                t.started_at_ms,
                t.latest_activity_ms,
                t.latest_visible_progress_ms,
                t.folded_steps.len(),
                t.live_segments.len(),
            )
        });
        let msgs_dbg: Vec<_> = collect_stage_agent_messages(&st_dbg)
            .into_iter()
            .filter_map(|it| match it {
                TimelineItemView::AgentMessage {
                    message_id,
                    delivery_status,
                    text,
                    ..
                } => Some((message_id.clone(), *delivery_status, text.clone())),
                _ => None,
            })
            .collect();
        let claude_st_dbg = core.agent_stage(&host_id, &claude_id);
        panic!(
            "Thinking loop must advance latest_activity_ms past latest_visible_progress_ms while STALL_QUEUED stays Queued; last_turn={last_dbg:?}, parent_msgs={msgs_dbg:?}, claude_turns={:?}, trace={:?}",
            claude_st_dbg
                .turns
                .iter()
                .map(turn_reply_texts)
                .collect::<Vec<_>>(),
            read_mock_acp_trace(&trace_path)
        );
    }

    core.interrupt_agent_and_continue(&host_id, &parent_id);

    assert!(
        wait_until(Duration::from_secs(15), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let pid = parent_id.clone();
            async move {
                let parent_idle = c
                    .host_detail(&hid)
                    .and_then(|d| d.agents.into_iter().find(|a| a.agent_id == pid))
                    .is_some_and(|a| !a.is_running);
                if !parent_idle {
                    return false;
                }
                let st = c.agent_stage(&hid, &pid);
                let settled = collect_stage_agent_messages(&st).iter().any(|it| {
                    matches!(
                        it,
                        TimelineItemView::AgentMessage {
                            text,
                            delivery_status: Some(AgentMessageDeliveryStatus::Settled),
                            ..
                        } if text.contains("STALL_QUEUED")
                    )
                });
                settled && stage_has_text(&c, &hid, &pid, "REPLY=STALL_QUEUED")
            }
        })
        .await,
        "interrupt_agent_and_continue must cancel thinking turn and deliver queued STALL_QUEUED message"
    );

    let cancel_after_b = read_mock_acp_trace(&trace_path)
        .iter()
        .filter(|e| e.get("method").and_then(|v| v.as_str()) == Some("session/cancel"))
        .count();
    assert_eq!(
        cancel_after_b, 1,
        "Scenario B must issue exactly 1 session/cancel via interrupt_agent_and_continue"
    );

    // ------------------------------------------------------------------------
    // Scenario C: Issue #52 exact sequence (queued notification + human preemption)
    // ------------------------------------------------------------------------
    let gate3 = work.join("gate3.flag");
    core.send_prompt(
        &host_id,
        &parent_id,
        &format!("WAIT_FILE={} ECHO=PARENT_T3", gate3.display()),
    );
    core.send_prompt(
        &host_id,
        &claude_id,
        &format!("[MOCK:SEND_AGENT_PROMPT] TARGET_ID={parent_id} MSG=NOTIFY_TAG=SUB_AFTER_HUMAN"),
    );

    assert!(
        wait_until(Duration::from_secs(15), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let pid = parent_id.clone();
            async move {
                let st = c.agent_stage(&hid, &pid);
                collect_stage_agent_messages(&st).iter().any(|it| {
                    matches!(
                        it,
                        TimelineItemView::AgentMessage {
                            text,
                            delivery_status: Some(AgentMessageDeliveryStatus::Queued),
                            ..
                        } if text.contains("SUB_AFTER_HUMAN")
                    )
                })
            }
        })
        .await,
        "SUB_AFTER_HUMAN must enter Queued state without interrupting PARENT_T3"
    );
    assert_eq!(
        read_mock_acp_trace(&trace_path)
            .iter()
            .filter(|e| e.get("method").and_then(|v| v.as_str()) == Some("session/cancel"))
            .count(),
        1,
        "queuing SUB_AFTER_HUMAN must not add any session/cancel"
    );

    // Now human sends preemption prompt while PARENT_T3 is still blocked and SUB_AFTER_HUMAN is queued.
    core.send_prompt(&host_id, &parent_id, "ECHO=HUMAN_PREEMPT");

    assert!(
        wait_until(Duration::from_secs(20), || {
            let c = Arc::clone(&core);
            let hid = host_id.clone();
            let pid = parent_id.clone();
            async move {
                let parent_idle = c
                    .host_detail(&hid)
                    .and_then(|d| d.agents.into_iter().find(|a| a.agent_id == pid))
                    .is_some_and(|a| !a.is_running);
                if !parent_idle {
                    return false;
                }
                let st = c.agent_stage(&hid, &pid);
                let sub_settled = collect_stage_agent_messages(&st).iter().any(|it| {
                    matches!(
                        it,
                        TimelineItemView::AgentMessage {
                            text,
                            delivery_status: Some(AgentMessageDeliveryStatus::Settled),
                            ..
                        } if text.contains("SUB_AFTER_HUMAN")
                    )
                });
                sub_settled
                    && stage_has_text(&c, &hid, &pid, "REPLY=HUMAN_PREEMPT")
                    && stage_has_text(&c, &hid, &pid, "REPLY=SUB_AFTER_HUMAN")
            }
        })
        .await,
        "Human preemption must execute immediately and still deliver queued SUB_AFTER_HUMAN afterward"
    );

    let trace_c = read_mock_acp_trace(&trace_path);
    assert_eq!(
        trace_c
            .iter()
            .filter(|e| e.get("method").and_then(|v| v.as_str()) == Some("session/cancel"))
            .count(),
        2,
        "human preemption must issue 1 additional session/cancel (total 2)"
    );
    let human_pos = trace_c
        .iter()
        .position(|e| {
            e.get("text")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.contains("ECHO=HUMAN_PREEMPT"))
        })
        .expect("HUMAN_PREEMPT prompt dispatched");
    let sub_matches: Vec<usize> = trace_c
        .iter()
        .enumerate()
        .filter_map(|(idx, e)| {
            e.get("text")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.contains("SUB_AFTER_HUMAN"))
                .then_some(idx)
        })
        .collect();
    assert_eq!(
        sub_matches.len(),
        1,
        "SUB_AFTER_HUMAN must be dispatched exactly once, got indices {sub_matches:?}"
    );
    assert!(
        human_pos < sub_matches[0],
        "Human preemption ({human_pos}) must run BEFORE queued SUB_AFTER_HUMAN ({})",
        sub_matches[0]
    );

    drop_core(core).await;
    drop(rt);
}
