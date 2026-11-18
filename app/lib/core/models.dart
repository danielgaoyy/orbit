import 'dart:convert';
import 'dart:typed_data';

enum ProviderKind {
  antigravity(
    'antigravity',
    'Antigravity',
    'Antigravity',
    '深度推理与多步编排',
  ),
  codex(
    'codex',
    'Codex',
    'Codex',
    '极速代码修改与测试闭环',
  ),
  claudeCode(
    'claude_code',
    'Claude Code',
    'Claude',
    '复杂架构重构与细致审查',
  );

  const ProviderKind(this.wire, this.label, this.shortLabel, this.subtitle);
  final String wire;
  final String label;
  final String shortLabel;
  final String subtitle;

  static ProviderKind fromWire(String? raw) {
    for (final p in ProviderKind.values) {
      if (p.wire == raw) return p;
    }
    return ProviderKind.antigravity;
  }
}

enum SessionMode {
  acceptEdits(
    'accept_edits',
    '自动改码',
    '自动',
    '默认推荐：自动读写项目代码，仅在外部或高危命令前请你点一下确认',
  ),
  plan(
    'plan',
    '先出方案',
    '方案',
    '只读分析代码并给出方案选择题，拍板前绝不改动任何文件',
  ),
  fullAccess(
    'full_access',
    '全自动推进',
    '全自动',
    '全程自主改码、跑测试并修复报错，适合挂在后台独立跑完长任务',
  ),
  defaultMode(
    'default',
    '谨慎确认',
    '谨慎',
    '每次文件修改与命令执行前均请求确认',
  );

  const SessionMode(this.wire, this.label, this.shortLabel, this.description);
  final String wire;
  final String label;
  final String shortLabel;
  final String description;

  /// Top 3 clean human choices shown in the inline composer menu.
  static const List<SessionMode> primaryChoices = [
    SessionMode.acceptEdits,
    SessionMode.plan,
    SessionMode.fullAccess,
  ];

  static SessionMode fromWire(dynamic raw) {
    if (raw is String) {
      for (final m in SessionMode.values) {
        if (m.wire == raw) return m;
      }
    }
    return SessionMode.acceptEdits;
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
    this.sshAlias,
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
  String? sshAlias;
  String? lastConnectionError;
  HostRouteState routeState;
  List<ProviderKind> availableProviders;

  bool get isOnline =>
      routeState == HostRouteState.onlineDirect ||
      routeState == HostRouteState.onlineRelay;

  factory HostProfile.fromPairingJson(Map<String, dynamic> json) {
    return HostProfile(
      hostId: json['host_id'] as String,
      hostname: (json['hostname'] as String?) ?? 'orbit-host',
      daemonPubkeyB64: json['daemon_pubkey_b64'] as String,
      directEndpoints: (json['direct_endpoints'] as List<dynamic>? ?? [])
          .map((e) => e.toString())
          .toList(),
      relayUrl: json['relay_url'] as String?,
      sshAlias: json['ssh_alias'] as String?,
    );
  }

  Map<String, dynamic> toJson() => {
        'host_id': hostId,
        'hostname': hostname,
        'daemon_pubkey_b64': daemonPubkeyB64,
        'direct_endpoints': directEndpoints,
        'relay_url': relayUrl,
        if (sshAlias != null) 'ssh_alias': sshAlias,
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

class DiscoveredProjectItem {
  DiscoveredProjectItem({
    required this.path,
    required this.name,
    this.gitBranch,
    this.isGitRepo = false,
    this.lastModifiedUnix = 0,
  });

  final String path;
  final String name;
  final String? gitBranch;
  final bool isGitRepo;
  final int lastModifiedUnix;

  factory DiscoveredProjectItem.fromJson(Map<String, dynamic> json) {
    return DiscoveredProjectItem(
      path: json['path'] as String,
      name: (json['name'] as String?) ?? 'workspace',
      gitBranch: json['git_branch'] as String?,
      isGitRepo: (json['is_git_repo'] as bool?) ?? false,
      lastModifiedUnix: (json['last_modified_unix'] as num?)?.toInt() ?? 0,
    );
  }
}

/// Cross-host selectable project option for the unified chat composer.
class ProjectChoice {
  const ProjectChoice({
    required this.hostId,
    required this.hostName,
    required this.name,
    required this.path,
    this.gitBranch,
    this.lastModifiedUnix = 0,
  });

  final String hostId;
  final String hostName;
  final String name;
  final String path;
  final String? gitBranch;
  final int lastModifiedUnix;

  String get key => '$hostId::$path';

  String get displayPath {
    final idx = path.indexOf('/workspace/');
    if (idx > 0) return '~${path.substring(idx)}';
    final parts = path.split('/');
    if (parts.length > 3 && parts[1] == 'Users') {
      return '~/${parts.sublist(3).join('/')}';
    }
    if (parts.length > 3 && parts[1] == 'home') {
      return '~/${parts.sublist(3).join('/')}';
    }
    return path;
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
    this.updatedAtIso,
  });

  final String agentId;
  final String workspaceId;
  final String cwd;
  final String title;
  ProviderKind provider;
  SessionMode mode;
  String statusTag;
  String? errorMessage;
  int latestSeq;
  final String? updatedAtIso;

  bool get isRunning =>
      statusTag == 'running' ||
      statusTag == 'autonomous_wakeup' ||
      statusTag == 'initializing';
  bool get isAwaitingPermission => statusTag == 'awaiting_permission';

  String get projectName {
    final clean = cwd.replaceAll(RegExp(r'/+$'), '');
    final idx = clean.lastIndexOf('/');
    if (idx >= 0 && idx + 1 < clean.length) {
      return clean.substring(idx + 1);
    }
    return clean.isEmpty ? 'workspace' : clean;
  }

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
      updatedAtIso: json['updated_at'] as String?,
    );
  }
}

class AgentWithHost {
  const AgentWithHost({
    required this.hostId,
    required this.hostName,
    required this.agent,
    this.gitBranch,
    this.isWorktree = false,
  });

  final String hostId;
  final String hostName;
  final AgentSummary agent;
  final String? gitBranch;
  final bool isWorktree;
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

  double get remainingPercent => (100.0 - usedPercent).clamp(0.0, 100.0);

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

  /// Lowest remaining percentage across active rolling windows (null if unavailable).
  int? get minRemainingPercent {
    if (windows.isEmpty) return null;
    var minRem = 100.0;
    for (final w in windows) {
      if (w.remainingPercent < minRem) {
        minRem = w.remainingPercent;
      }
    }
    return minRem.round();
  }

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
