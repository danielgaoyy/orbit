import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:uuid/uuid.dart';
import 'package:web_socket_channel/web_socket_channel.dart';

import 'models.dart';
import 'orbit_ffi_bridge.dart';

const _uuid = Uuid();

class HostRuntimeSession {
  HostRuntimeSession({
    required this.profile,
    required this.channel,
    this.noiseChannelHandle,
    required this.supportsZstd,
  });

  final HostProfile profile;
  final WebSocketChannel channel;
  final int? noiseChannelHandle;
  final bool supportsZstd;
  StreamSubscription<dynamic>? subscription;

  final List<WorkspaceSummary> workspaces = [];
  final Map<String, AgentSummary> agents = {};
  final Map<String, List<TimelineEvent>> timelines = {};
  final List<ProviderQuotaSnapshot> quotas = [];
  final Map<String, GitStatusSnapshot> gitStatuses = {};
  final Map<String, String> gitDiffs = {};
  final StringBuffer terminalBuffer = StringBuffer();
  String? activeTerminalId;
  int activeTerminalSlot = 1;

  bool get isEncryptedRelay => noiseChannelHandle != null;

  void sendWire(Map<String, dynamic> wireJson) {
    final ffi = OrbitFfiBridge.instance;
    final jsonText = jsonEncode(wireJson);

    if (noiseChannelHandle != null) {
      final compressedWithTag = ffi.zstdCompressWire(jsonText);
      if (compressedWithTag != null) {
        // compressedWithTag already starts with `0x00` (which matches E2EE_FRAME_WIRE = 0x00!)
        final ct = ffi.noiseSeal(noiseChannelHandle!, compressedWithTag);
        channel.sink.add(ct);
      }
      return;
    }

    if (supportsZstd && ffi.isNativeLoaded) {
      final compressed = ffi.zstdCompressWire(jsonText);
      if (compressed != null) {
        channel.sink.add(compressed);
        return;
      }
    }
    channel.sink.add(jsonText);
  }

  void sendTerminalBytes(Uint8List rawTerminalFrame) {
    final ffi = OrbitFfiBridge.instance;
    if (noiseChannelHandle != null) {
      final plain = Uint8List(1 + rawTerminalFrame.length);
      plain[0] = 0x01; // E2EE_FRAME_TERMINAL
      plain.setRange(1, plain.length, rawTerminalFrame);
      final ct = ffi.noiseSeal(noiseChannelHandle!, plain);
      channel.sink.add(ct);
      return;
    }
    channel.sink.add(rawTerminalFrame);
  }

  void dispose() {
    subscription?.cancel();
    if (noiseChannelHandle != null) {
      OrbitFfiBridge.instance.noiseClose(noiseChannelHandle!);
    }
    channel.sink.close();
  }
}

/// Multi-Host Fleet state manager driving the 120Hz Flutter UI across iOS & Android.
class FleetController extends ChangeNotifier {
  final Map<String, HostProfile> _hosts = {};
  final Map<String, HostRuntimeSession> _sessions = {};

  List<HostProfile> get hosts => _hosts.values.toList();

  HostRuntimeSession? sessionFor(String hostId) => _sessions[hostId];

  /// Count of agents across ALL connected hosts currently awaiting user approval or answer.
  int get globalPendingApprovalsCount {
    var total = 0;
    for (final s in _sessions.values) {
      for (final a in s.agents.values) {
        if (a.isAwaitingPermission) total++;
      }
    }
    return total;
  }

  Future<void> loadSavedHosts() async {
    final prefs = await SharedPreferences.getInstance();
    final savedList = prefs.getStringList('orbit_saved_hosts') ?? const [];
    for (final item in savedList) {
      try {
        final profile =
            HostProfile.fromPairingJson(jsonDecode(item) as Map<String, dynamic>);
        _hosts[profile.hostId] = profile;
      } catch (_) {}
    }
    notifyListeners();
    for (final hostId in _hosts.keys) {
      unawaited(connectHost(hostId));
    }
  }

  Future<HostProfile> pairFromQrUri(String uri) async {
    final profile = OrbitFfiBridge.instance.parsePairingUri(uri);
    _hosts[profile.hostId] = profile;
    await _persistHosts();
    notifyListeners();
    await connectHost(profile.hostId);
    return profile;
  }

  Future<void> removeHost(String hostId) async {
    _sessions.remove(hostId)?.dispose();
    _hosts.remove(hostId);
    await _persistHosts();
    notifyListeners();
  }

  static String? normalizeRelayInput(String? raw) {
    if (raw == null) return null;
    final trimmed = raw.trim().replaceAll(RegExp(r'/+$'), '');
    if (trimmed.isEmpty) return null;

    final hadScheme = trimmed.contains('://');
    var withScheme = trimmed;
    if (trimmed.startsWith('http://')) {
      withScheme = 'ws://${trimmed.substring(7)}';
    } else if (trimmed.startsWith('https://')) {
      withScheme = 'wss://${trimmed.substring(8)}';
    } else if (!trimmed.startsWith('ws://') && !trimmed.startsWith('wss://')) {
      withScheme = 'ws://$trimmed';
    }

    final idx = withScheme.indexOf('://');
    final scheme = withScheme.substring(0, idx);
    var authority = withScheme.substring(idx + 3).split('/').first.trim();
    if (!hadScheme &&
        !authority.contains(':') &&
        (authority == 'localhost' ||
            RegExp(r'^\d{1,3}(\.\d{1,3}){3}$').hasMatch(authority))) {
      authority = '$authority:7878';
    }
    return '$scheme://$authority';
  }

  Future<void> updateHostRelay(String hostId, String? rawRelayUrl) async {
    final profile = _hosts[hostId];
    if (profile == null) return;
    profile.relayUrl = normalizeRelayInput(rawRelayUrl);
    await _persistHosts();
    notifyListeners();
    await connectHost(hostId);
  }

  Future<void> _persistHosts() async {
    final prefs = await SharedPreferences.getInstance();
    final encoded =
        _hosts.values.map((h) => jsonEncode(h.toJson())).toList();
    await prefs.setStringList('orbit_saved_hosts', encoded);
  }

  /// Happy-Eyeballs connection: races LAN/Tailscale direct WebSocket first, falls back to Noise E2EE Relay.
  Future<void> connectHost(String hostId) async {
    final profile = _hosts[hostId];
    if (profile == null) return;

    _sessions.remove(hostId)?.dispose();
    profile.routeState = HostRouteState.connecting;
    profile.lastConnectionError = null;
    notifyListeners();

    final ffi = OrbitFfiBridge.instance;

    // 1. Try direct endpoints (LAN / Tailscale / Devbox public WS)
    for (final ep in profile.directEndpoints) {
      try {
        final ws = WebSocketChannel.connect(Uri.parse(ep));
        await ws.ready.timeout(const Duration(milliseconds: 1200));
        final session = HostRuntimeSession(
          profile: profile,
          channel: ws,
          supportsZstd: ffi.isNativeLoaded,
        );
        _bindSessionStream(hostId, session);
        profile.routeState = HostRouteState.onlineDirect;
        profile.lastConnectionError = null;
        _sessions[hostId] = session;

        session.sendWire({
          'op': 'hello',
          'client_id': _uuid.v4(),
          'client_name': 'orbit-flutter',
          'protocol_version': 1,
          'supports_zstd': ffi.isNativeLoaded,
        });
        refreshAll(hostId);
        notifyListeners();
        return;
      } catch (_) {
        // Direct endpoint unreachable, continue to next or Relay fallback
      }
    }

    // 2. Fallback to Private Self-Hosted Relay with `Noise_NK_25519_ChaChaPoly_SHA256` E2EE
    final relayBase = profile.relayUrl;
    if (relayBase == null || relayBase.isEmpty) {
      profile.lastConnectionError =
          'LAN direct unreachable & no Relay attached (run `orbit use-relay <HOST:PORT>` on host)';
    } else if (!ffi.isNativeLoaded) {
      profile.lastConnectionError =
          'Native Noise E2EE engine (liborbit_ffi) unavailable on this build';
    } else {
      final cleanRelay = relayBase.endsWith('/')
          ? relayBase.substring(0, relayBase.length - 1)
          : relayBase;
      try {
        final url = '$cleanRelay/v1/client/${profile.daemonPubkeyB64}';
        final ws = WebSocketChannel.connect(Uri.parse(url));
        await ws.ready.timeout(const Duration(seconds: 6));

        final bcast = ws.stream.asBroadcastStream();
        final init = ffi.noiseClientInit(profile.daemonPubkeyB64);
        ws.sink.add(init.msg1);

        final firstMsg = await bcast
            .firstWhere((frame) => frame is List<int>)
            .timeout(const Duration(seconds: 8)) as List<int>;
        final chanHandle =
            ffi.noiseClientFinalize(init.handshakeHandle, Uint8List.fromList(firstMsg));

        final session = HostRuntimeSession(
          profile: profile,
          channel: ws,
          noiseChannelHandle: chanHandle,
          supportsZstd: true,
        );
        _bindBroadcastStream(hostId, session, bcast);
        profile.routeState = HostRouteState.onlineRelay;
        profile.lastConnectionError = null;
        _sessions[hostId] = session;

        session.sendWire({
          'op': 'hello',
          'client_id': _uuid.v4(),
          'client_name': 'orbit-flutter-e2ee',
          'protocol_version': 1,
          'supports_zstd': true,
        });
        refreshAll(hostId);
        notifyListeners();
        return;
      } catch (e) {
        final errStr = e.toString();
        if (errStr.contains('502')) {
          profile.lastConnectionError =
              'Relay online ($cleanRelay), but host daemon has not connected to it yet (HTTP 502)';
        } else if (errStr.contains('403')) {
          profile.lastConnectionError =
              'Relay rejected host pubkey (HTTP 403 — run `orbit relay allow` on relay server)';
        } else {
          profile.lastConnectionError =
              'Cannot reach Relay ($cleanRelay) — check port & cloud firewall / Security Group';
        }
      }
    }

    profile.routeState = HostRouteState.disconnected;
    notifyListeners();
  }

  void _bindSessionStream(String hostId, HostRuntimeSession session) {
    session.subscription = session.channel.stream.listen(
      (frame) => _handleInboundRawFrame(hostId, session, frame),
      onDone: () => _markDisconnected(hostId),
      onError: (_) => _markDisconnected(hostId),
    );
  }

  void _bindBroadcastStream(
    String hostId,
    HostRuntimeSession session,
    Stream<dynamic> bcast,
  ) {
    session.subscription = bcast.listen(
      (frame) => _handleInboundRawFrame(hostId, session, frame),
      onDone: () => _markDisconnected(hostId),
      onError: (_) => _markDisconnected(hostId),
    );
  }

  void _markDisconnected(String hostId) {
    _hosts[hostId]?.routeState = HostRouteState.disconnected;
    _sessions.remove(hostId);
    notifyListeners();
  }

  void _handleInboundRawFrame(
    String hostId,
    HostRuntimeSession session,
    dynamic frame,
  ) {
    final ffi = OrbitFfiBridge.instance;
    try {
      if (session.noiseChannelHandle != null && frame is List<int>) {
        final plain =
            ffi.noiseOpen(session.noiseChannelHandle!, Uint8List.fromList(frame));
        if (plain.isEmpty) return;
        if (plain[0] == 0x00) {
          final jsonStr = ffi.zstdDecompressWire(plain.sublist(1));
          _dispatchWireMessage(session, jsonDecode(jsonStr) as Map<String, dynamic>);
        } else if (plain[0] == 0x01) {
          final term = TerminalBinaryFrame.decode(plain.sublist(1));
          if (term != null) {
            session.terminalBuffer
                .write(utf8.decode(term.payload, allowMalformed: true));
            notifyListeners();
          }
        }
        return;
      }

      if (frame is String) {
        _dispatchWireMessage(session, jsonDecode(frame) as Map<String, dynamic>);
      } else if (frame is List<int> && frame.isNotEmpty) {
        final bytes = Uint8List.fromList(frame);
        if (bytes[0] == 0x00 && ffi.isNativeLoaded) {
          final jsonStr = ffi.zstdDecompressWire(bytes.sublist(1));
          _dispatchWireMessage(session, jsonDecode(jsonStr) as Map<String, dynamic>);
        } else {
          final term = TerminalBinaryFrame.decode(bytes);
          if (term != null) {
            session.terminalBuffer
                .write(utf8.decode(term.payload, allowMalformed: true));
            notifyListeners();
          }
        }
      }
    } catch (_) {}
  }

  void _dispatchWireMessage(
    HostRuntimeSession session,
    Map<String, dynamic> msg,
  ) {
    final op = msg['op'] as String?;
    switch (op) {
      case 'welcome':
        final host = msg['host'] as Map<String, dynamic>?;
        if (host != null) {
          session.profile.hostname =
              (host['hostname'] as String?) ?? session.profile.hostname;
          final provs = (host['available_providers'] as List<dynamic>? ?? [])
              .map((p) => ProviderKind.fromWire(p.toString()))
              .toList();
          if (provs.isNotEmpty) {
            session.profile.availableProviders = provs;
          }
        }
      case 'workspaces_snapshot':
        final list = (msg['workspaces'] as List<dynamic>? ?? [])
            .map((w) => WorkspaceSummary.fromJson(w as Map<String, dynamic>))
            .toList();
        session.workspaces
          ..clear()
          ..addAll(list);
      case 'workspace_created':
        final ws = WorkspaceSummary.fromJson(
            msg['workspace'] as Map<String, dynamic>);
        session.workspaces.removeWhere((x) => x.workspaceId == ws.workspaceId);
        session.workspaces.insert(0, ws);
      case 'agents_snapshot':
        final list = (msg['agents'] as List<dynamic>? ?? [])
            .map((a) => AgentSummary.fromJson(a as Map<String, dynamic>));
        session.agents.clear();
        for (final a in list) {
          session.agents[a.agentId] = a;
        }
      case 'agent_created':
      case 'agent_updated':
        final a = AgentSummary.fromJson(msg['agent'] as Map<String, dynamic>);
        session.agents[a.agentId] = a;
      case 'timeline_catchup':
        final agentId = msg['agent_id'] as String;
        final incoming = (msg['events'] as List<dynamic>? ?? [])
            .map((e) => TimelineEvent.fromJson(e as Map<String, dynamic>));
        final list = session.timelines.putIfAbsent(agentId, () => []);
        final existingSeqs = list.map((e) => e.seq).toSet();
        for (final ev in incoming) {
          if (!existingSeqs.contains(ev.seq)) {
            list.add(ev);
          }
        }
        list.sort((a, b) => a.seq.compareTo(b.seq));
      case 'timeline_live':
        final ev =
            TimelineEvent.fromJson(msg['event'] as Map<String, dynamic>);
        final list = session.timelines.putIfAbsent(ev.agentId, () => []);
        // Coalesce streaming message deltas with same item_id for smooth 120Hz scrolling
        if (ev.type == 'assistant_message') {
          final itemId = ev.payload['item_id'];
          final prevIdx = list.lastIndexWhere((x) =>
              x.type == 'assistant_message' && x.payload['item_id'] == itemId);
          if (prevIdx >= 0) {
            list[prevIdx] = ev;
            notifyListeners();
            return;
          }
        }
        list.add(ev);
      case 'git_status_response':
        final snap = GitStatusSnapshot.fromJson(
            msg['status'] as Map<String, dynamic>);
        session.gitStatuses[snap.cwd] = snap;
      case 'git_diff_response':
        final cwd = msg['cwd'] as String;
        final path = (msg['path'] as String?) ?? '__all__';
        session.gitDiffs['$cwd::$path'] =
            (msg['unified_diff'] as String?) ?? '';
      case 'terminal_attached':
        final term = msg['terminal'] as Map<String, dynamic>;
        session.activeTerminalId = term['terminal_id'] as String?;
        session.activeTerminalSlot = (msg['slot'] as num?)?.toInt() ?? 1;
      case 'quota_report':
        final snaps = (msg['snapshots'] as List<dynamic>? ?? [])
            .map((s) =>
                ProviderQuotaSnapshot.fromJson(s as Map<String, dynamic>))
            .toList();
        session.quotas
          ..clear()
          ..addAll(snaps);
    }
    notifyListeners();
  }

  // --- High-level Mobile Actions ---

  void refreshAll(String hostId) {
    final s = _sessions[hostId];
    if (s == null) return;
    s.sendWire({'op': 'list_workspaces', 'request_id': _uuid.v4()});
    s.sendWire({'op': 'list_agents', 'request_id': _uuid.v4(), 'workspace_id': null});
  }

  void refreshQuotas(String hostId) {
    _sessions[hostId]?.sendWire({
      'op': 'refresh_quotas',
      'request_id': _uuid.v4(),
    });
  }

  void createAgent({
    required String hostId,
    required String cwd,
    required ProviderKind provider,
    required SessionMode mode,
    required String prompt,
    String? worktreeBranch,
  }) {
    final s = _sessions[hostId];
    if (s == null) return;
    if (worktreeBranch != null && worktreeBranch.trim().isNotEmpty) {
      s.sendWire({
        'op': 'create_workspace',
        'request_id': _uuid.v4(),
        'source_cwd': cwd,
        'title': worktreeBranch.trim(),
        'worktree_branch': worktreeBranch.trim(),
        'base_ref': null,
      });
    }
    s.sendWire({
      'op': 'create_agent',
      'request_id': _uuid.v4(),
      'cwd': cwd,
      'title': null,
      'provider': provider.wire,
      'model': null,
      'mode': mode.wire,
      'initial_prompt': prompt,
    });
  }

  void openAgentTimeline(String hostId, String agentId) {
    final s = _sessions[hostId];
    if (s == null) return;
    final cached = s.timelines[agentId] ?? const [];
    final sinceSeq = cached.isEmpty ? 0 : cached.last.seq;
    s.sendWire({
      'op': 'subscribe_timeline',
      'request_id': _uuid.v4(),
      'agent_id': agentId,
      'since_seq': sinceSeq,
    });
  }

  void sendPrompt(String hostId, String agentId, String prompt) {
    _sessions[hostId]?.sendWire({
      'op': 'send_prompt',
      'request_id': _uuid.v4(),
      'agent_id': agentId,
      'prompt': prompt,
    });
  }

  void interruptAgent(String hostId, String agentId) {
    _sessions[hostId]?.sendWire({
      'op': 'interrupt_turn',
      'request_id': _uuid.v4(),
      'agent_id': agentId,
    });
  }

  void setSessionMode(String hostId, String agentId, SessionMode mode) {
    final s = _sessions[hostId];
    if (s == null) return;
    s.agents[agentId]?.mode = mode;
    s.sendWire({
      'op': 'set_session_mode',
      'request_id': _uuid.v4(),
      'agent_id': agentId,
      'mode': mode.wire,
    });
    notifyListeners();
  }

  void resolvePermission(
    String hostId,
    String agentId,
    String permissionRequestId,
    String decision,
  ) {
    _sessions[hostId]?.sendWire({
      'op': 'resolve_permission',
      'request_id': _uuid.v4(),
      'agent_id': agentId,
      'permission_request_id': permissionRequestId,
      'decision': decision,
    });
  }

  void answerQuestion(
    String hostId,
    String agentId,
    String questionRequestId,
    List<List<String>> answers,
  ) {
    _sessions[hostId]?.sendWire({
      'op': 'answer_question',
      'request_id': _uuid.v4(),
      'agent_id': agentId,
      'question_request_id': questionRequestId,
      'answers': answers,
    });
  }

  void fetchGitStatus(String hostId, String cwd) {
    _sessions[hostId]?.sendWire({
      'op': 'get_git_status',
      'request_id': _uuid.v4(),
      'cwd': cwd,
    });
  }

  void fetchGitDiff(String hostId, String cwd, {String? path, bool staged = false}) {
    _sessions[hostId]?.sendWire({
      'op': 'get_git_diff',
      'request_id': _uuid.v4(),
      'cwd': cwd,
      'path': path,
      'staged': staged,
    });
  }

  void startRemoteTerminal(String hostId, String cwd, {int cols = 80, int rows = 28}) {
    final s = _sessions[hostId];
    if (s == null) return;
    s.terminalBuffer.clear();
    s.sendWire({
      'op': 'create_terminal',
      'request_id': _uuid.v4(),
      'cwd': cwd,
      'title': 'Mobile PTY',
      'cols': cols,
      'rows': rows,
    });
  }

  void sendTerminalKeys(String hostId, String keys) {
    final s = _sessions[hostId];
    if (s == null) return;
    s.sendTerminalBytes(TerminalBinaryFrame.encodeInput(s.activeTerminalSlot, keys));
  }
}
