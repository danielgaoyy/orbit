import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:uuid/uuid.dart';
import 'package:web_socket_channel/web_socket_channel.dart';

import 'models.dart';
import 'orbit_ffi_bridge.dart';
import 'ssh_config.dart';

const _uuid = Uuid();

class HostRuntimeSession {
  HostRuntimeSession({
    required this.profile,
    required this.channel,
    this.noiseChannelHandle,
    this.sshTunnelProcess,
    required this.supportsZstd,
  });

  final HostProfile profile;
  final WebSocketChannel channel;
  final int? noiseChannelHandle;
  final Process? sshTunnelProcess;
  final bool supportsZstd;
  StreamSubscription<dynamic>? subscription;

  final List<DiscoveredProjectItem> discoveredProjects = [];
  final List<WorkspaceSummary> workspaces = [];
  final Map<String, AgentSummary> agents = {};
  final Map<String, List<TimelineEvent>> timelines = {};
  final List<ProviderQuotaSnapshot> quotas = [];
  final Map<String, GitStatusSnapshot> gitStatuses = {};
  final Map<String, String> gitDiffs = {};
  final StringBuffer terminalBuffer = StringBuffer();
  String? activeTerminalId;
  String? activeTerminalCwd;
  int activeTerminalSlot = 1;
  String? lastCreatedAgentId;

  bool get isEncryptedRelay => noiseChannelHandle != null;

  void sendWire(Map<String, dynamic> wireJson) {
    final ffi = OrbitFfiBridge.instance;
    final jsonText = jsonEncode(wireJson);

    if (noiseChannelHandle != null) {
      final compressedWithTag = ffi.zstdCompressWire(jsonText);
      if (compressedWithTag != null) {
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
    sshTunnelProcess?.kill();
    channel.sink.close();
  }
}

/// Multi-Host Fleet state manager driving the zero-form chat-first UI across Desktop & Mobile.
class FleetController extends ChangeNotifier {
  final Map<String, HostProfile> _hosts = {};
  final Map<String, HostRuntimeSession> _sessions = {};

  String? draftHostId;
  String? draftCwd;
  ProviderKind draftProvider = ProviderKind.antigravity;
  SessionMode draftMode = SessionMode.acceptEdits;
  bool draftUseWorktree = false;

  List<HostProfile> get hosts => _hosts.values.toList();

  HostRuntimeSession? sessionFor(String hostId) => _sessions[hostId];

  int get onlineHostsCount => _hosts.values.where((h) => h.isOnline).length;

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

  /// Unified thread list across ALL hosts: awaiting human decision first, then sorted by recency.
  List<AgentWithHost> get allThreadsSorted {
    final items = <AgentWithHost>[];
    for (final entry in _hosts.entries) {
      final host = entry.value;
      final s = _sessions[host.hostId];
      if (s == null) continue;
      final wsById = {for (final w in s.workspaces) w.workspaceId: w};
      for (final ag in s.agents.values) {
        final ws = wsById[ag.workspaceId];
        items.add(
          AgentWithHost(
            hostId: host.hostId,
            hostName: host.hostname,
            agent: ag,
            gitBranch: ws?.gitBranch,
            isWorktree: ws?.isWorktree ?? false,
          ),
        );
      }
    }

    items.sort((a, b) {
      if (a.agent.isAwaitingPermission != b.agent.isAwaitingPermission) {
        return a.agent.isAwaitingPermission ? -1 : 1;
      }
      if (a.agent.isRunning != b.agent.isRunning) {
        return a.agent.isRunning ? -1 : 1;
      }
      final ta = a.agent.updatedAtIso ?? '';
      final tb = b.agent.updatedAtIso ?? '';
      final cmp = tb.compareTo(ta);
      if (cmp != 0) return cmp;
      return b.agent.latestSeq.compareTo(a.agent.latestSeq);
    });
    return items;
  }

  /// Unified auto-discovered selectable project choices across ALL connected hosts.
  List<ProjectChoice> get allProjectChoices {
    final byKey = <String, ProjectChoice>{};

    for (final host in _hosts.values) {
      final s = _sessions[host.hostId];
      if (s == null) continue;

      for (final p in s.discoveredProjects) {
        final choice = ProjectChoice(
          hostId: host.hostId,
          hostName: host.hostname,
          name: p.name,
          path: p.path,
          gitBranch: p.gitBranch,
          lastModifiedUnix: p.lastModifiedUnix,
        );
        byKey[choice.key] = choice;
      }

      for (final ws in s.workspaces) {
        if (ws.isWorktree) continue;
        final choice = ProjectChoice(
          hostId: host.hostId,
          hostName: host.hostname,
          name: ws.title,
          path: ws.cwd,
          gitBranch: ws.gitBranch,
          lastModifiedUnix: 1,
        );
        byKey.putIfAbsent(choice.key, () => choice);
      }

      for (final ag in s.agents.values) {
        final choice = ProjectChoice(
          hostId: host.hostId,
          hostName: host.hostname,
          name: ag.projectName,
          path: ag.cwd,
          lastModifiedUnix: 1,
        );
        byKey.putIfAbsent(choice.key, () => choice);
      }

      if (!byKey.keys.any((k) => k.startsWith('${host.hostId}::'))) {
        final fallbackPath = (Platform.isMacOS || Platform.isLinux) &&
                host.directEndpoints.any((e) => e.contains('127.0.0.1'))
            ? (SshConfigService.resolveUserHomeDir() ?? '~')
            : '~';
        final fallback = ProjectChoice(
          hostId: host.hostId,
          hostName: host.hostname,
          name: '主目录',
          path: fallbackPath,
          lastModifiedUnix: 0,
        );
        byKey[fallback.key] = fallback;
      }
    }

    final sorted = byKey.values.toList()
      ..sort((a, b) {
        final t = b.lastModifiedUnix.compareTo(a.lastModifiedUnix);
        if (t != 0) return t;
        return a.name.compareTo(b.name);
      });
    return sorted;
  }

  /// Active selected ProjectChoice for New Chat (remembers last choice, falls back to most recent).
  ProjectChoice? get activeDraftProject {
    final choices = allProjectChoices;
    if (choices.isEmpty) return null;
    if (draftHostId != null && draftCwd != null) {
      final exact = choices
          .where((c) => c.hostId == draftHostId && c.path == draftCwd)
          .firstOrNull;
      if (exact != null) return exact;
    }
    if (draftHostId != null) {
      final sameHost =
          choices.where((c) => c.hostId == draftHostId).firstOrNull;
      if (sameHost != null) return sameHost;
    }
    return choices.first;
  }

  ProviderQuotaSnapshot? quotaFor(String? hostId, ProviderKind provider) {
    if (hostId != null) {
      final q = _sessions[hostId]
          ?.quotas
          .where((s) => s.provider == provider)
          .firstOrNull;
      if (q != null) return q;
    }
    for (final s in _sessions.values) {
      final q = s.quotas.where((x) => x.provider == provider).firstOrNull;
      if (q != null) return q;
    }
    return null;
  }

  void selectDraftProject(ProjectChoice choice) {
    draftHostId = choice.hostId;
    draftCwd = choice.path;
    unawaited(_persistPreferences());
    notifyListeners();
  }

  void setDraftProvider(ProviderKind provider) {
    draftProvider = provider;
    unawaited(_persistPreferences());
    notifyListeners();
  }

  void setDraftMode(SessionMode mode) {
    draftMode = mode;
    unawaited(_persistPreferences());
    notifyListeners();
  }

  void setDraftWorktree(bool enabled) {
    draftUseWorktree = enabled;
    unawaited(_persistPreferences());
    notifyListeners();
  }

  Future<void> _persistPreferences() async {
    final prefs = await SharedPreferences.getInstance();
    if (draftHostId != null) {
      await prefs.setString('orbit_pref_host_id', draftHostId!);
    }
    if (draftCwd != null) {
      await prefs.setString('orbit_pref_cwd', draftCwd!);
    }
    await prefs.setString('orbit_pref_provider', draftProvider.wire);
    await prefs.setString('orbit_pref_mode', draftMode.wire);
    await prefs.setBool('orbit_pref_worktree', draftUseWorktree);
  }

  Future<void> loadSavedHosts({bool autoDiscoverLocal = true}) async {
    final prefs = await SharedPreferences.getInstance();
    draftHostId = prefs.getString('orbit_pref_host_id');
    draftCwd = prefs.getString('orbit_pref_cwd');
    final savedProv = prefs.getString('orbit_pref_provider');
    if (savedProv != null) draftProvider = ProviderKind.fromWire(savedProv);
    final savedMode = prefs.getString('orbit_pref_mode');
    if (savedMode != null) draftMode = SessionMode.fromWire(savedMode);
    draftUseWorktree = prefs.getBool('orbit_pref_worktree') ?? false;

    final savedList = prefs.getStringList('orbit_saved_hosts') ?? const [];
    for (final item in savedList) {
      try {
        final profile = HostProfile.fromPairingJson(
          jsonDecode(item) as Map<String, dynamic>,
        );
        _hosts[profile.hostId] = profile;
      } catch (_) {}
    }

    if (autoDiscoverLocal && (Platform.isLinux || Platform.isMacOS)) {
      await _autoDiscoverLocalMachine();
    }

    notifyListeners();
    for (final hostId in _hosts.keys) {
      unawaited(connectHost(hostId));
    }
  }

  /// Automatically start/attach local `orbit daemon` on macOS & Linux if `orbit` CLI is installed.
  Future<void> _autoDiscoverLocalMachine() async {
    try {
      final home = SshConfigService.resolveUserHomeDir();
      if (home == null || home.isEmpty) return;

      String? orbitBin;
      for (final cand in [
        '$home/.local/bin/orbit',
        '/usr/local/bin/orbit',
        '/opt/homebrew/bin/orbit',
      ]) {
        if (File(cand).existsSync()) {
          orbitBin = cand;
          break;
        }
      }
      if (orbitBin == null) return;

      // Ensure local background daemon is running so 127.0.0.1:6868 works out-of-the-box!
      var localReachable = false;
      try {
        final sock = await Socket.connect(
          InternetAddress.loopbackIPv4,
          6868,
          timeout: const Duration(milliseconds: 250),
        );
        sock.destroy();
        localReachable = true;
      } catch (_) {}

      if (!localReachable) {
        await Process.run(orbitBin, ['daemon', 'up']);
      }

      final res = await Process.run(orbitBin, ['pair']);
      if (res.exitCode != 0) return;
      final uriLine = (res.stdout ?? '')
          .toString()
          .split('\n')
          .map((l) => l.trim())
          .firstWhere((l) => l.startsWith('orbit://pair/'), orElse: () => '');
      if (uriLine.isEmpty) return;

      final localProfile = OrbitFfiBridge.instance.parsePairingUri(uriLine);
      if (!localProfile.directEndpoints.contains('ws://127.0.0.1:6868/ws')) {
        localProfile.directEndpoints.insert(0, 'ws://127.0.0.1:6868/ws');
      }
      if (!_hosts.containsKey(localProfile.hostId)) {
        _hosts[localProfile.hostId] = localProfile;
        await _persistHosts();
      }
    } catch (_) {}
  }

  /// One-click SSH connection for any host in `~/.ssh/config` (zero pairing strings, zero firewall ports!).
  Future<HostProfile> connectSshHostOneClick(
    SshHostEntry entry, {
    void Function(String step)? onStep,
  }) async {
    final uri = await SshConfigService.bootstrapDaemonAndFetchPairUri(
      entry: entry,
      onStep: onStep,
    );
    final profile = OrbitFfiBridge.instance.parsePairingUri(uri);
    profile.sshAlias = entry.alias;
    if (profile.hostname == 'orbit-host' || profile.hostname.isEmpty) {
      profile.hostname = entry.alias;
    }
    _hosts[profile.hostId] = profile;
    await _persistHosts();
    notifyListeners();

    onStep?.call('正在建立安全直达连接…');
    await connectHost(profile.hostId);
    return profile;
  }

  Future<HostProfile> pairFromQrUri(String uri) async {
    final profile = OrbitFfiBridge.instance.parsePairingUri(uri.trim());
    _hosts[profile.hostId] = profile;
    await _persistHosts();
    notifyListeners();
    await connectHost(profile.hostId);
    return profile;
  }

  /// Export a single URI containing ALL paired hosts so scanning/copying once syncs the whole fleet to Mobile!
  String? exportFleetSyncUri() {
    if (_hosts.isEmpty) return null;
    final payload = {
      'v': 1,
      'hosts': _hosts.values.map((h) => h.toJson()).toList(),
    };
    final b64 =
        base64UrlEncode(utf8.encode(jsonEncode(payload))).replaceAll('=', '');
    return 'orbit://fleet/$b64';
  }

  /// Accept either a single `orbit://pair/...` URI or a multi-host `orbit://fleet/...` bundle!
  Future<int> importFromAnyCode(String raw) async {
    final text = raw.trim();
    if (text.startsWith('orbit://fleet/')) {
      var b64 = text.substring('orbit://fleet/'.length).trim();
      while (b64.length % 4 != 0) {
        b64 += '=';
      }
      final decoded =
          jsonDecode(utf8.decode(base64Url.decode(b64))) as Map<String, dynamic>;
      final list = decoded['hosts'] as List<dynamic>? ?? const [];
      var count = 0;
      for (final item in list) {
        if (item is Map<String, dynamic>) {
          final p = HostProfile.fromPairingJson(item);
          _hosts[p.hostId] = p;
          count++;
        }
      }
      if (count > 0) {
        await _persistHosts();
        notifyListeners();
        for (final h in _hosts.keys) {
          unawaited(connectHost(h));
        }
      }
      return count;
    }

    await pairFromQrUri(text);
    return 1;
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
    final encoded = _hosts.values.map((h) => jsonEncode(h.toJson())).toList();
    await prefs.setStringList('orbit_saved_hosts', encoded);
  }

  /// Happy-Eyeballs smart connection:
  /// 1. Direct LAN / Tailscale WebSocket (`1.2s`)
  /// 2. Managed SSH local tunnel (`ssh -L`) on Desktop when `sshAlias` is known
  /// 3. Zero-knowledge `Noise_NK_25519_ChaChaPoly_SHA256` E2EE Relay
  Future<void> connectHost(String hostId) async {
    final profile = _hosts[hostId];
    if (profile == null) return;

    _sessions.remove(hostId)?.dispose();
    profile.routeState = HostRouteState.connecting;
    profile.lastConnectionError = null;
    notifyListeners();

    final ffi = OrbitFfiBridge.instance;

    // 1. Try direct endpoints (LAN / Tailscale / localhost)
    for (final ep in profile.directEndpoints) {
      try {
        final ws = WebSocketChannel.connect(Uri.parse(ep));
        await ws.ready.timeout(const Duration(milliseconds: 1200));
        final session = HostRuntimeSession(
          profile: profile,
          channel: ws,
          supportsZstd: ffi.isNativeLoaded,
        );
        _activateDirectSession(hostId, session);
        return;
      } catch (_) {}
    }

    // 2. Desktop automatic SSH tunnel fallback (`~/.ssh/config`)
    if (profile.sshAlias != null &&
        profile.sshAlias!.isNotEmpty &&
        SshConfigService.isDesktopSupported) {
      try {
        final (proc, localWsUrl) =
            await SshConfigService.startLocalSshTunnel(profile.sshAlias!);
        final ws = WebSocketChannel.connect(Uri.parse(localWsUrl));
        await ws.ready.timeout(const Duration(seconds: 3));
        final session = HostRuntimeSession(
          profile: profile,
          channel: ws,
          sshTunnelProcess: proc,
          supportsZstd: ffi.isNativeLoaded,
        );
        _activateDirectSession(hostId, session);
        return;
      } catch (e) {
        profile.lastConnectionError = 'SSH 隧道连接失败: $e';
      }
    }

    // 3. Fallback to Private Relay with `Noise_NK_25519_ChaChaPoly_SHA256` E2EE
    final relayBase = profile.relayUrl;
    if (relayBase == null || relayBase.isEmpty) {
      profile.lastConnectionError ??= '当前不在同一局域网，且尚未配置外网中继';
    } else if (!ffi.isNativeLoaded) {
      profile.lastConnectionError = '当前环境未加载原生加密引擎 (liborbit_ffi)';
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
        final chanHandle = ffi.noiseClientFinalize(
          init.handshakeHandle,
          Uint8List.fromList(firstMsg),
        );

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
          profile.lastConnectionError = '中继在线，但目标机器守护进程尚未连入 ($cleanRelay)';
        } else if (errStr.contains('403')) {
          profile.lastConnectionError = '中继拒绝了该机器公钥 (HTTP 403)';
        } else {
          profile.lastConnectionError = '无法连接中继 ($cleanRelay)';
        }
      }
    }

    profile.routeState = HostRouteState.disconnected;
    notifyListeners();
  }

  void _activateDirectSession(String hostId, HostRuntimeSession session) {
    _bindSessionStream(hostId, session);
    session.profile.routeState = HostRouteState.onlineDirect;
    session.profile.lastConnectionError = null;
    _sessions[hostId] = session;

    session.sendWire({
      'op': 'hello',
      'client_id': _uuid.v4(),
      'client_name': 'orbit-flutter',
      'protocol_version': 1,
      'supports_zstd': OrbitFfiBridge.instance.isNativeLoaded,
    });
    refreshAll(hostId);
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
    _sessions.remove(hostId)?.dispose();
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
        final plain = ffi.noiseOpen(
          session.noiseChannelHandle!,
          Uint8List.fromList(frame),
        );
        if (plain.isEmpty) return;
        if (plain[0] == 0x00) {
          final jsonStr = ffi.zstdDecompressWire(plain.sublist(1));
          _dispatchWireMessage(
            hostId,
            session,
            jsonDecode(jsonStr) as Map<String, dynamic>,
          );
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
        _dispatchWireMessage(
          hostId,
          session,
          jsonDecode(frame) as Map<String, dynamic>,
        );
      } else if (frame is List<int> && frame.isNotEmpty) {
        final bytes = Uint8List.fromList(frame);
        if (bytes[0] == 0x00 && ffi.isNativeLoaded) {
          final jsonStr = ffi.zstdDecompressWire(bytes.sublist(1));
          _dispatchWireMessage(
            hostId,
            session,
            jsonDecode(jsonStr) as Map<String, dynamic>,
          );
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
    String hostId,
    HostRuntimeSession session,
    Map<String, dynamic> msg,
  ) {
    final op = msg['op'] as String?;
    switch (op) {
      case 'welcome':
        final host = msg['host'] as Map<String, dynamic>?;
        if (host != null) {
          if (session.profile.sshAlias == null) {
            session.profile.hostname =
                (host['hostname'] as String?) ?? session.profile.hostname;
          }
          final provs = (host['available_providers'] as List<dynamic>? ?? [])
              .map((p) => ProviderKind.fromWire(p.toString()))
              .toList();
          if (provs.isNotEmpty) {
            session.profile.availableProviders = provs;
          }
        }
      case 'discovered_projects_snapshot':
        final list = (msg['projects'] as List<dynamic>? ?? [])
            .map(
              (p) => DiscoveredProjectItem.fromJson(p as Map<String, dynamic>),
            )
            .toList();
        session.discoveredProjects
          ..clear()
          ..addAll(list);
      case 'workspaces_snapshot':
        final list = (msg['workspaces'] as List<dynamic>? ?? [])
            .map((w) => WorkspaceSummary.fromJson(w as Map<String, dynamic>))
            .toList();
        session.workspaces
          ..clear()
          ..addAll(list);
      case 'workspace_created':
        final ws = WorkspaceSummary.fromJson(
          msg['workspace'] as Map<String, dynamic>,
        );
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
        final a = AgentSummary.fromJson(msg['agent'] as Map<String, dynamic>);
        session.agents[a.agentId] = a;
        session.lastCreatedAgentId = a.agentId;
        fetchGitStatus(hostId, a.cwd);
      case 'agent_updated':
        final a = AgentSummary.fromJson(msg['agent'] as Map<String, dynamic>);
        session.agents[a.agentId] = a;
        if (!a.isRunning) {
          fetchGitStatus(hostId, a.cwd);
        }
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
        if (ev.type == 'assistant_message') {
          final itemId = ev.payload['item_id'];
          final prevIdx = list.lastIndexWhere(
            (x) =>
                x.type == 'assistant_message' && x.payload['item_id'] == itemId,
          );
          if (prevIdx >= 0) {
            list[prevIdx] = ev;
            notifyListeners();
            return;
          }
        }
        list.add(ev);
      case 'git_status_response':
        final snap = GitStatusSnapshot.fromJson(
          msg['status'] as Map<String, dynamic>,
        );
        session.gitStatuses[snap.cwd] = snap;
      case 'git_diff_response':
        final cwd = msg['cwd'] as String;
        final path = (msg['path'] as String?) ?? '__all__';
        session.gitDiffs['$cwd::$path'] =
            (msg['unified_diff'] as String?) ?? '';
      case 'terminal_attached':
        final term = msg['terminal'] as Map<String, dynamic>;
        session.activeTerminalId = term['terminal_id'] as String?;
        session.activeTerminalCwd = term['cwd'] as String?;
        session.activeTerminalSlot = (msg['slot'] as num?)?.toInt() ?? 1;
      case 'quota_report':
        final snaps = (msg['snapshots'] as List<dynamic>? ?? [])
            .map(
              (s) => ProviderQuotaSnapshot.fromJson(s as Map<String, dynamic>),
            )
            .toList();
        session.quotas
          ..clear()
          ..addAll(snaps);
    }
    notifyListeners();
  }

  // --- High-level Client Actions ---

  void refreshAll(String hostId) {
    final s = _sessions[hostId];
    if (s == null) return;
    s.sendWire({'op': 'discover_projects', 'request_id': _uuid.v4()});
    s.sendWire({'op': 'list_workspaces', 'request_id': _uuid.v4()});
    s.sendWire({
      'op': 'list_agents',
      'request_id': _uuid.v4(),
      'workspace_id': null,
    });
    s.sendWire({'op': 'refresh_quotas', 'request_id': _uuid.v4()});
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
    bool worktree = false,
  }) {
    final s = _sessions[hostId];
    if (s == null) return;

    draftHostId = hostId;
    draftCwd = cwd;
    draftProvider = provider;
    draftMode = mode;
    draftUseWorktree = worktree;
    unawaited(_persistPreferences());

    s.sendWire({
      'op': 'create_agent',
      'request_id': _uuid.v4(),
      'cwd': cwd,
      'title': null,
      'provider': provider.wire,
      'model': null,
      'mode': mode.wire,
      'initial_prompt': prompt,
      'worktree': worktree,
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
    final agent = s.agents[agentId];
    if (agent != null) {
      fetchGitStatus(hostId, agent.cwd);
    }
  }

  void sendPrompt(String hostId, String agentId, String prompt) {
    final s = _sessions[hostId];
    if (s == null) return;

    // If there is an unresolved single question card waiting for an answer,
    // allow typing custom thoughts directly in the chat box to answer it!
    final events = s.timelines[agentId] ?? const [];
    final resolvedQIds = <String>{
      for (final e in events)
        if (e.type == 'question_resolved')
          (e.payload['request_id'] as String?) ?? '',
    };
    for (var i = events.length - 1; i >= 0; i--) {
      final ev = events[i];
      if (ev.type == 'question_request') {
        final reqId = (ev.payload['request_id'] as String?) ?? '';
        if (reqId.isNotEmpty && !resolvedQIds.contains(reqId)) {
          final qs = ev.payload['questions'] as List<dynamic>? ?? const [];
          if (qs.isNotEmpty) {
            answerQuestion(
              hostId,
              agentId,
              reqId,
              List.generate(qs.length, (_) => [prompt]),
            );
            return;
          }
        }
        break;
      }
    }

    s.sendWire({
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

  void archiveAgent(String hostId, String agentId) {
    final s = _sessions[hostId];
    if (s == null) return;
    s.agents.remove(agentId);
    s.timelines.remove(agentId);
    s.sendWire({
      'op': 'archive_agent',
      'request_id': _uuid.v4(),
      'agent_id': agentId,
    });
    notifyListeners();
  }

  void setSessionMode(String hostId, String agentId, SessionMode mode) {
    final s = _sessions[hostId];
    if (s == null) return;
    draftMode = mode;
    unawaited(_persistPreferences());
    s.agents[agentId]?.mode = mode;
    s.sendWire({
      'op': 'set_session_mode',
      'request_id': _uuid.v4(),
      'agent_id': agentId,
      'mode': mode.wire,
    });
    notifyListeners();
  }

  void switchAgentProvider(
    String hostId,
    String agentId,
    ProviderKind provider,
  ) {
    final s = _sessions[hostId];
    if (s == null) return;
    draftProvider = provider;
    unawaited(_persistPreferences());
    final ag = s.agents[agentId];
    if (ag == null || ag.provider == provider) return;
    ag.provider = provider;
    s.sendWire({
      'op': 'switch_agent_provider',
      'request_id': _uuid.v4(),
      'agent_id': agentId,
      'provider': provider.wire,
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

  void fetchGitDiff(
    String hostId,
    String cwd, {
    String? path,
    bool staged = false,
  }) {
    _sessions[hostId]?.sendWire({
      'op': 'get_git_diff',
      'request_id': _uuid.v4(),
      'cwd': cwd,
      'path': path,
      'staged': staged,
    });
  }

  void startRemoteTerminal(
    String hostId,
    String cwd, {
    int cols = 88,
    int rows = 24,
  }) {
    final s = _sessions[hostId];
    if (s == null) return;
    if (s.activeTerminalId != null && s.activeTerminalCwd == cwd) {
      return;
    }
    s.terminalBuffer.clear();
    s.activeTerminalCwd = cwd;
    s.sendWire({
      'op': 'create_terminal',
      'request_id': _uuid.v4(),
      'cwd': cwd,
      'title': 'PTY',
      'cols': cols,
      'rows': rows,
    });
  }

  void sendTerminalKeys(String hostId, String keys) {
    final s = _sessions[hostId];
    if (s == null) return;
    s.sendTerminalBytes(
      TerminalBinaryFrame.encodeInput(s.activeTerminalSlot, keys),
    );
  }
}
