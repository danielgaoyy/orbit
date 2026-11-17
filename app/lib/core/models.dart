import 'dart:convert';
import 'dart:typed_data';

enum ProviderKind {
  antigravity('antigravity', 'Antigravity'),
  codex('codex', 'Codex'),
  claudeCode('claude_code', 'Claude Code');

  const ProviderKind(this.wire, this.label);
  final String wire;
  final String label;

  static ProviderKind fromWire(String? raw) {
    for (final p in ProviderKind.values) {
      if (p.wire == raw) return p;
    }
    return ProviderKind.antigravity;
  }
}

enum SessionMode {
  defaultMode('default', 'Default'),
  acceptEdits('accept_edits', 'Accept Edits'),
  plan('plan', 'Plan'),
  fullAccess('full_access', 'Full Access');

  const SessionMode(this.wire, this.label);
  final String wire;
  final String label;

  static SessionMode fromWire(dynamic raw) {
    if (raw is String) {
      for (final m in SessionMode.values) {
        if (m.wire == raw) return m;
      }
    }
    return SessionMode.defaultMode;
  }
}

enum HostRouteState {
  disconnected,
  connecting,
  onlineDirect,
  onlineRelay,
}

class HostProfile {
  HostProfile({
    required this.hostId,
    required this.hostname,
    required this.daemonPubkeyB64,
    required this.directEndpoints,
    this.relayUrl,
    this.lastConnectionError,
    this.routeState = HostRouteState.disconnected,
    this.availableProviders = const [
      ProviderKind.antigravity,
      ProviderKind.codex,
      ProviderKind.claudeCode,
    ],
  });

  final String hostId;
  String hostname;
  final String daemonPubkeyB64;
  final List<String> directEndpoints;
  String? relayUrl;
  String? lastConnectionError;
  HostRouteState routeState;
  List<ProviderKind> availableProviders;

  factory HostProfile.fromPairingJson(Map<String, dynamic> json) {
    return HostProfile(
      hostId: json['host_id'] as String,
      hostname: (json['hostname'] as String?) ?? 'orbit-host',
      daemonPubkeyB64: json['daemon_pubkey_b64'] as String,
      directEndpoints: (json['direct_endpoints'] as List<dynamic>? ?? [])
          .map((e) => e.toString())
          .toList(),
      relayUrl: json['relay_url'] as String?,
    );
  }

  Map<String, dynamic> toJson() => {
        'host_id': hostId,
        'hostname': hostname,
        'daemon_pubkey_b64': daemonPubkeyB64,
        'direct_endpoints': directEndpoints,
        'relay_url': relayUrl,
      };
}

class WorkspaceSummary {
  WorkspaceSummary({
    required this.workspaceId,
    required this.title,
    required this.cwd,
    this.gitBranch,
    this.isWorktree = false,
  });

  final String workspaceId;
  final String title;
  final String cwd;
  final String? gitBranch;
  final bool isWorktree;

  factory WorkspaceSummary.fromJson(Map<String, dynamic> json) {
    return WorkspaceSummary(
      workspaceId: json['workspace_id'] as String,
      title: json['title'] as String,
      cwd: json['cwd'] as String,
      gitBranch: json['git_branch'] as String?,
      isWorktree: (json['is_worktree'] as bool?) ?? false,
    );
  }
}

class AgentSummary {
  AgentSummary({
    required this.agentId,
    required this.workspaceId,
    required this.cwd,
    required this.title,
    required this.provider,
    required this.mode,
    required this.statusTag,
    this.errorMessage,
    this.latestSeq = 0,
  });

  final String agentId;
  final String workspaceId;
  final String cwd;
  final String title;
  final ProviderKind provider;
  SessionMode mode;
  String statusTag;
  String? errorMessage;
  int latestSeq;

  bool get isRunning =>
      statusTag == 'running' || statusTag == 'autonomous_wakeup';
  bool get isAwaitingPermission => statusTag == 'awaiting_permission';

  factory AgentSummary.fromJson(Map<String, dynamic> json) {
    final statusObj = json['status'] as Map<String, dynamic>? ?? {};
    return AgentSummary(
      agentId: json['agent_id'] as String,
      workspaceId: json['workspace_id'] as String,
      cwd: json['cwd'] as String,
      title: json['title'] as String,
      provider: ProviderKind.fromWire(json['provider'] as String?),
      mode: SessionMode.fromWire(json['mode']),
      statusTag: (statusObj['state'] as String?) ?? 'idle',
      errorMessage: statusObj['message'] as String?,
      latestSeq: (json['latest_seq'] as num?)?.toInt() ?? 0,
    );
  }
}

class QuestionItem {
  QuestionItem({
    required this.id,
    this.header,
    required this.question,
    required this.options,
    required this.isMultiSelect,
  });

  final String id;
  final String? header;
  final String question;
  final List<String> options;
  final bool isMultiSelect;

  factory QuestionItem.fromJson(Map<String, dynamic> json) {
    return QuestionItem(
      id: (json['id'] as String?) ?? 'q0',
      header: json['header'] as String?,
      question: (json['question'] as String?) ?? '',
      options: (json['options'] as List<dynamic>? ?? [])
          .map((e) => e.toString())
          .toList(),
      isMultiSelect: (json['is_multi_select'] as bool?) ?? false,
    );
  }
}

class TimelineEvent {
  TimelineEvent({
    required this.seq,
    required this.agentId,
    required this.type,
    required this.payload,
  });

  final int seq;
  final String agentId;
  final String type;
  final Map<String, dynamic> payload;

  factory TimelineEvent.fromJson(Map<String, dynamic> json) {
    final payloadMap =
        Map<String, dynamic>.from(json['payload'] as Map? ?? const {});
    return TimelineEvent(
      seq: (json['seq'] as num).toInt(),
      agentId: json['agent_id'] as String,
      type: (payloadMap['type'] as String?) ?? 'unknown',
      payload: payloadMap,
    );
  }
}

class QuotaWindow {
  QuotaWindow({
    required this.label,
    required this.usedPercent,
    this.resetsAt,
  });

  final String label;
  final double usedPercent;
  final String? resetsAt;

  factory QuotaWindow.fromJson(Map<String, dynamic> json) {
    return QuotaWindow(
      label: json['label'] as String,
      usedPercent: (json['used_percent'] as num).toDouble(),
      resetsAt: json['resets_at'] as String?,
    );
  }
}

class ProviderQuotaSnapshot {
  ProviderQuotaSnapshot({
    required this.provider,
    this.accountLabel,
    required this.windows,
    this.creditsRemaining,
    this.error,
  });

  final ProviderKind provider;
  final String? accountLabel;
  final List<QuotaWindow> windows;
  final double? creditsRemaining;
  final String? error;

  factory ProviderQuotaSnapshot.fromJson(Map<String, dynamic> json) {
    return ProviderQuotaSnapshot(
      provider: ProviderKind.fromWire(json['provider'] as String?),
      accountLabel: json['account_label'] as String?,
      windows: (json['windows'] as List<dynamic>? ?? [])
          .map((w) => QuotaWindow.fromJson(w as Map<String, dynamic>))
          .toList(),
      creditsRemaining: (json['credits_remaining'] as num?)?.toDouble(),
      error: json['error'] as String?,
    );
  }
}

class GitFileEntry {
  GitFileEntry({
    required this.path,
    required this.status,
    required this.staged,
    required this.untracked,
  });

  final String path;
  final String status;
  final bool staged;
  final bool untracked;

  factory GitFileEntry.fromJson(Map<String, dynamic> json) {
    return GitFileEntry(
      path: json['path'] as String,
      status: json['status'] as String,
      staged: (json['staged'] as bool?) ?? false,
      untracked: (json['untracked'] as bool?) ?? false,
    );
  }
}

class GitStatusSnapshot {
  GitStatusSnapshot({
    required this.cwd,
    required this.isGitRepo,
    this.branch,
    this.ahead = 0,
    this.behind = 0,
    required this.files,
  });

  final String cwd;
  final bool isGitRepo;
  final String? branch;
  final int ahead;
  final int behind;
  final List<GitFileEntry> files;

  factory GitStatusSnapshot.fromJson(Map<String, dynamic> json) {
    return GitStatusSnapshot(
      cwd: json['cwd'] as String,
      isGitRepo: (json['is_git_repo'] as bool?) ?? false,
      branch: json['branch'] as String?,
      ahead: (json['ahead'] as num?)?.toInt() ?? 0,
      behind: (json['behind'] as num?)?.toInt() ?? 0,
      files: (json['files'] as List<dynamic>? ?? [])
          .map((f) => GitFileEntry.fromJson(f as Map<String, dynamic>))
          .toList(),
    );
  }
}

/// Binary PTY terminal stream frame codec (`0x01` Output, `0x02` Input, `0x03` Resize, `0x04` Snapshot).
class TerminalBinaryFrame {
  TerminalBinaryFrame({
    required this.opcode,
    required this.slot,
    required this.payload,
  });

  final int opcode;
  final int slot;
  final Uint8List payload;

  static Uint8List encodeInput(int slot, String text) {
    final utf8Bytes = utf8.encode(text);
    final out = Uint8List(2 + utf8Bytes.length);
    out[0] = 0x02;
    out[1] = slot;
    out.setRange(2, out.length, utf8Bytes);
    return out;
  }

  static Uint8List encodeResize(int slot, int cols, int rows) {
    final out = Uint8List(6);
    out[0] = 0x03;
    out[1] = slot;
    out[2] = (cols >> 8) & 0xFF;
    out[3] = cols & 0xFF;
    out[4] = (rows >> 8) & 0xFF;
    out[5] = rows & 0xFF;
    return out;
  }

  static TerminalBinaryFrame? decode(Uint8List raw) {
    if (raw.length < 2) return null;
    final op = raw[0];
    if (op < 0x01 || op > 0x04) return null;
    return TerminalBinaryFrame(
      opcode: op,
      slot: raw[1],
      payload: raw.sublist(2),
    );
  }
}
