import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../core/app_updater.dart';
import '../core/fleet_controller.dart';
import '../core/models.dart';
import '../core/ssh_config.dart';
import 'host_workspace_screen.dart';

class FleetHomeScreen extends StatefulWidget {
  const FleetHomeScreen({super.key, required this.controller});

  final FleetController controller;

  @override
  State<FleetHomeScreen> createState() => _FleetHomeScreenState();
}

class _FleetHomeScreenState extends State<FleetHomeScreen> {
  ReleaseUpdateInfo? _availableUpdate;

  FleetController get controller => widget.controller;

  @override
  void initState() {
    super.initState();
    _silentCheckUpdate();
  }

  Future<void> _silentCheckUpdate() async {
    try {
      final info = await AppUpdater.checkLatestRelease();
      if (mounted && info.hasNewerVersion) {
        setState(() {
          _availableUpdate = info;
        });
      }
    } catch (_) {
      // Ignore silent startup check failures when offline or in widget tests.
    }
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        final hosts = controller.hosts;
        final pendingApprovals = controller.globalPendingApprovalsCount;

        return Scaffold(
          appBar: AppBar(
            title: Row(
              children: [
                const Icon(Icons.radar_rounded, color: Color(0xFF38BDF8)),
                const SizedBox(width: 10),
                const Text(
                  'Orbit Fleet',
                  style: TextStyle(fontWeight: FontWeight.w700),
                ),
                const SizedBox(width: 8),
                Container(
                  padding:
                      const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                  decoration: BoxDecoration(
                    color: Colors.white10,
                    borderRadius: BorderRadius.circular(6),
                  ),
                  child: const Text(
                    'v$kOrbitAppVersion',
                    style: TextStyle(fontSize: 10.5, color: Colors.white60),
                  ),
                ),
                if (pendingApprovals > 0) ...[
                  const SizedBox(width: 10),
                  Container(
                    padding:
                        const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                    decoration: BoxDecoration(
                      color: Colors.amber.shade700,
                      borderRadius: BorderRadius.circular(999),
                    ),
                    child: Text(
                      '$pendingApprovals Approval',
                      style: const TextStyle(
                        fontSize: 11,
                        fontWeight: FontWeight.bold,
                        color: Colors.black,
                      ),
                    ),
                  ),
                ],
              ],
            ),
            actions: [
              if (_availableUpdate != null)
                Padding(
                  padding: const EdgeInsets.only(right: 4),
                  child: FilledButton.tonalIcon(
                    onPressed: () => _showUpdateSheet(context),
                    icon: const Icon(Icons.rocket_launch_rounded, size: 16),
                    label: Text('一键更新 ${_availableUpdate!.tagName}'),
                  ),
                )
              else
                IconButton(
                  tooltip: '检查并一键更新到最新版本 (当前 v$kOrbitAppVersion)',
                  icon: const Icon(Icons.system_update_alt_rounded),
                  onPressed: () => _showUpdateSheet(context),
                ),
              IconButton(
                tooltip: 'Pair Host (`orbit pair`)',
                icon: const Icon(Icons.qr_code_scanner_rounded),
                onPressed: () => _showPairHostSheet(context),
              ),
            ],
          ),
          body: hosts.isEmpty
              ? _buildEmptyOnboarding(context)
              : ListView.separated(
                  padding: const EdgeInsets.all(16),
                  itemCount: hosts.length,
                  separatorBuilder: (_, __) => const SizedBox(height: 12),
                  itemBuilder: (context, index) =>
                      _buildHostCard(context, hosts[index]),
                ),
          floatingActionButton: FloatingActionButton.extended(
            onPressed: () => _showPairHostSheet(context),
            icon: const Icon(Icons.add_link_rounded),
            label: const Text('Pair Machine'),
          ),
        );
      },
    );
  }

  Widget _buildEmptyOnboarding(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const Icon(
              Icons.hub_outlined,
              size: 64,
              color: Color(0xFF38BDF8),
            ),
            const SizedBox(height: 16),
            const Text(
              'Connect Your Multi-Host Fleet',
              style: TextStyle(fontSize: 20, fontWeight: FontWeight.bold),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: 8),
            const Text(
              'Run `orbit daemon up` (or `orbit use-relay` to auto-pick from ~/.ssh/config) on your Devbox, Linux workstation, or Mac mini, then paste the `orbit://pair/...` link.',
              style: TextStyle(color: Colors.white70, height: 1.4),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: 24),
            FilledButton.icon(
              onPressed: () => _showPairHostSheet(context),
              icon: const Icon(Icons.link_rounded),
              label: const Text('Import Pairing URI'),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildHostCard(BuildContext context, HostProfile host) {
    final session = controller.sessionFor(host.hostId);
    final agentCount = session?.agents.length ?? 0;
    final runningCount =
        session?.agents.values.where((a) => a.isRunning).length ?? 0;
    final awaitingCount =
        session?.agents.values.where((a) => a.isAwaitingPermission).length ?? 0;

    final (badgeText, badgeColor) = switch (host.routeState) {
      HostRouteState.onlineDirect => ('LAN Direct', Colors.greenAccent),
      HostRouteState.onlineRelay =>
        ('Noise E2EE Relay', const Color(0xFF38BDF8)),
      HostRouteState.connecting => ('Connecting…', Colors.orangeAccent),
      HostRouteState.disconnected => ('Offline', Colors.redAccent),
    };

    return Card(
      child: InkWell(
        borderRadius: BorderRadius.circular(14),
        onTap: () {
          Navigator.of(context).push(
            MaterialPageRoute<void>(
              builder: (_) => HostWorkspaceScreen(
                controller: controller,
                hostId: host.hostId,
              ),
            ),
          );
        },
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  const Icon(Icons.dns_rounded, size: 20, color: Colors.white70),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      host.hostname,
                      style: const TextStyle(
                        fontSize: 17,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                  ),
                  Container(
                    padding:
                        const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
                    decoration: BoxDecoration(
                      color: badgeColor.withValues(alpha: 0.14),
                      borderRadius: BorderRadius.circular(999),
                      border:
                          Border.all(color: badgeColor.withValues(alpha: 0.5)),
                    ),
                    child: Text(
                      badgeText,
                      style: TextStyle(
                        fontSize: 11,
                        fontWeight: FontWeight.w600,
                        color: badgeColor,
                      ),
                    ),
                  ),
                  PopupMenuButton<String>(
                    tooltip: 'Host Options',
                    icon: const Icon(Icons.more_vert_rounded, size: 18),
                    onSelected: (action) async {
                      if (action == 'relay') {
                        _showEditRelaySheet(context, host);
                      } else if (action == 'pubkey') {
                        await Clipboard.setData(
                          ClipboardData(text: host.daemonPubkeyB64),
                        );
                        if (context.mounted) {
                          ScaffoldMessenger.of(context).showSnackBar(
                            const SnackBar(
                              content: Text(
                                'Copied host public key for `orbit relay allow`',
                              ),
                            ),
                          );
                        }
                      } else if (action == 'remove') {
                        await controller.removeHost(host.hostId);
                      }
                    },
                    itemBuilder: (_) => const [
                      PopupMenuItem(
                        value: 'relay',
                        child: Text('Set Relay (SSH 一键部署 / 配置)…'),
                      ),
                      PopupMenuItem(
                        value: 'pubkey',
                        child: Text('Copy Host Public Key'),
                      ),
                      PopupMenuItem(
                        value: 'remove',
                        child: Text('Remove Host'),
                      ),
                    ],
                  ),
                ],
              ),
              const SizedBox(height: 6),
              Row(
                children: [
                  Expanded(
                    child: Text(
                      host.relayUrl != null && host.relayUrl!.isNotEmpty
                          ? 'Relay: ${host.relayUrl}'
                          : 'Relay: Not configured (LAN / Tailscale direct only)',
                      style:
                          const TextStyle(fontSize: 11.5, color: Colors.white54),
                    ),
                  ),
                  TextButton.icon(
                    style: TextButton.styleFrom(
                      visualDensity: VisualDensity.compact,
                      padding: const EdgeInsets.symmetric(horizontal: 8),
                    ),
                    onPressed: () => _showEditRelaySheet(context, host),
                    icon: const Icon(Icons.cloud_sync_rounded, size: 15),
                    label: const Text(
                      'Set Relay',
                      style: TextStyle(fontSize: 12),
                    ),
                  ),
                ],
              ),
              if (host.routeState == HostRouteState.disconnected &&
                  host.lastConnectionError != null) ...[
                const SizedBox(height: 6),
                Container(
                  width: double.infinity,
                  padding:
                      const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
                  decoration: BoxDecoration(
                    color: Colors.redAccent.withValues(alpha: 0.10),
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(
                      color: Colors.redAccent.withValues(alpha: 0.35),
                    ),
                  ),
                  child: Row(
                    children: [
                      const Icon(
                        Icons.info_outline_rounded,
                        size: 15,
                        color: Colors.orangeAccent,
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          host.lastConnectionError!,
                          style: const TextStyle(
                            fontSize: 11.5,
                            color: Colors.white70,
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
              ],
              const SizedBox(height: 10),
              Wrap(
                spacing: 8,
                runSpacing: 6,
                children: [
                  for (final p in host.availableProviders)
                    Chip(
                      visualDensity: VisualDensity.compact,
                      label: Text(p.label, style: const TextStyle(fontSize: 11)),
                    ),
                ],
              ),
              const SizedBox(height: 8),
              Row(
                children: [
                  Text(
                    '$agentCount sessions · $runningCount active',
                    style: const TextStyle(fontSize: 12, color: Colors.white60),
                  ),
                  if (awaitingCount > 0) ...[
                    const SizedBox(width: 8),
                    Text(
                      '· $awaitingCount awaiting approval!',
                      style: const TextStyle(
                        fontSize: 12,
                        color: Colors.amberAccent,
                        fontWeight: FontWeight.bold,
                      ),
                    ),
                  ],
                  const Spacer(),
                  IconButton(
                    tooltip: 'Reconnect',
                    visualDensity: VisualDensity.compact,
                    icon: const Icon(Icons.refresh_rounded, size: 18),
                    onPressed: () => controller.connectHost(host.hostId),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  void _showUpdateSheet(BuildContext context) {
    showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      builder: (_) => _OneClickUpdateSheet(
        initialRelease: _availableUpdate,
        onFoundNewer: (info) {
          if (mounted && info.hasNewerVersion) {
            setState(() => _availableUpdate = info);
          }
        },
      ),
    );
  }

  void _showEditRelaySheet(BuildContext context, HostProfile host) {
    showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      builder: (_) => _SshRelaySetupSheet(
        controller: controller,
        host: host,
      ),
    );
  }

  void _showPairHostSheet(BuildContext context) {
    final uriCtrl = TextEditingController();
    showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      builder: (ctx) => Padding(
        padding: EdgeInsets.only(
          left: 20,
          right: 20,
          top: 20,
          bottom: MediaQuery.of(ctx).viewInsets.bottom + 24,
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            const Text(
              'Pair Orbit Host',
              style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
            ),
            const SizedBox(height: 8),
            const Text(
              'Paste the `orbit://pair/...` URI generated by `orbit pair` or `orbit use-relay` on your machine:',
              style: TextStyle(fontSize: 13, color: Colors.white70),
            ),
            const SizedBox(height: 12),
            TextField(
              controller: uriCtrl,
              autofocus: true,
              decoration: const InputDecoration(
                hintText: 'orbit://pair/eyJob3N0X2lkIjoi...',
                border: OutlineInputBorder(),
              ),
            ),
            const SizedBox(height: 16),
            FilledButton(
              onPressed: () async {
                final text = uriCtrl.text.trim();
                if (text.isEmpty) return;
                Navigator.of(ctx).pop();
                try {
                  await controller.pairFromQrUri(text);
                } catch (e) {
                  if (context.mounted) {
                    ScaffoldMessenger.of(context).showSnackBar(
                      SnackBar(content: Text('Invalid pairing URI: $e')),
                    );
                  }
                }
              },
              child: const Text('Connect & Save Host'),
            ),
          ],
        ),
      ),
    );
  }
}

/// Modal sheet that auto-reads `~/.ssh/config` and provides 1-Click SSH Relay Deployment & Attachment!
class _SshRelaySetupSheet extends StatefulWidget {
  const _SshRelaySetupSheet({
    required this.controller,
    required this.host,
  });

  final FleetController controller;
  final HostProfile host;

  @override
  State<_SshRelaySetupSheet> createState() => _SshRelaySetupSheetState();
}

class _SshRelaySetupSheetState extends State<_SshRelaySetupSheet> {
  late final TextEditingController _manualUrlCtrl;
  late final TextEditingController _portCtrl;
  List<SshHostEntry> _sshHosts = const [];
  String? _deployingAlias;
  String? _statusMessage;
  bool _statusIsError = false;
  String? _deployedRelayUrl;

  @override
  void initState() {
    super.initState();
    _manualUrlCtrl = TextEditingController(text: widget.host.relayUrl ?? '');
    _portCtrl = TextEditingController(text: '7878');
    _sshHosts = SshConfigService.loadUserSshHosts();
  }

  @override
  void dispose() {
    _manualUrlCtrl.dispose();
    _portCtrl.dispose();
    super.dispose();
  }

  int get _selectedPort => int.tryParse(_portCtrl.text.trim()) ?? 7878;

  Future<void> _handleOneClickSshDeploy(SshHostEntry entry) async {
    final port = _selectedPort;
    setState(() {
      _deployingAlias = entry.alias;
      _statusIsError = false;
      _deployedRelayUrl = null;
      _statusMessage = '正在连接 SSH 主机 ${entry.alias} (${entry.displayTarget})…';
    });

    try {
      final allPubkeys = widget.controller.hosts
          .map((h) => h.daemonPubkeyB64)
          .toSet()
          .toList();
      if (!allPubkeys.contains(widget.host.daemonPubkeyB64)) {
        allPubkeys.add(widget.host.daemonPubkeyB64);
      }

      final relayUrl = await SshConfigService.provisionRelayOverSsh(
        entry: entry,
        relayPort: port,
        allowedPubkeys: allPubkeys,
        onStep: (step) {
          if (mounted) {
            setState(() => _statusMessage = step);
          }
        },
      );

      _manualUrlCtrl.text = relayUrl;
      if (mounted) {
        setState(() {
          _statusMessage = '已部署 $relayUrl，正在建立 Noise E2EE 加密连接…';
        });
      }

      await widget.controller.updateHostRelay(widget.host.hostId, relayUrl);

      if (!mounted) return;
      final connected =
          widget.host.routeState == HostRouteState.onlineRelay ||
          widget.host.routeState == HostRouteState.onlineDirect;

      setState(() {
        _deployingAlias = null;
        _deployedRelayUrl = relayUrl;
        if (connected) {
          _statusIsError = false;
          _statusMessage =
              '已成功通过 ${entry.alias} ($relayUrl) 连上 ${widget.host.hostname}！';
        } else {
          final err = widget.host.lastConnectionError ?? '';
          if (err.contains('502')) {
            _statusIsError = false;
            _statusMessage =
                '中继已在 ${entry.alias} ($relayUrl) 启动并完成公钥授权！只需在 ${widget.host.hostname} 执行下方一行命令连入中继即可打通：';
          } else {
            _statusIsError = true;
            _statusMessage =
                '中继已在 ${entry.alias} 后台启动，但本机暂无法直连 $relayUrl —— 请确认云厂商安全组（如 AWS Security Group）已放行入站 TCP $port 端口。';
          }
        }
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _deployingAlias = null;
        _statusIsError = true;
        _statusMessage = 'SSH 自动部署失败：$e';
      });
    }
  }

  Future<void> _handleQuickSelectAddress(SshHostEntry entry) async {
    final url = entry.relayWsUrl(_selectedPort);
    _manualUrlCtrl.text = url;
    Navigator.of(context).pop();
    await widget.controller.updateHostRelay(widget.host.hostId, url);
  }

  @override
  Widget build(BuildContext context) {
    final maxHeight = MediaQuery.of(context).size.height * 0.85;
    return ConstrainedBox(
      constraints: BoxConstraints(maxHeight: maxHeight),
      child: Padding(
        padding: EdgeInsets.only(
          left: 20,
          right: 20,
          top: 20,
          bottom: MediaQuery.of(context).viewInsets.bottom + 24,
        ),
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(
                children: [
                  Expanded(
                    child: Text(
                      '配置中继 · ${widget.host.hostname}',
                      style: const TextStyle(
                        fontSize: 18,
                        fontWeight: FontWeight.bold,
                      ),
                    ),
                  ),
                  SizedBox(
                    width: 96,
                    child: TextField(
                      controller: _portCtrl,
                      keyboardType: TextInputType.number,
                      onChanged: (_) => setState(() {}),
                      decoration: const InputDecoration(
                        labelText: '端口',
                        isDense: true,
                        border: OutlineInputBorder(),
                      ),
                    ),
                  ),
                ],
              ),
              if (SshConfigService.isDesktopSupported) ...[
                const SizedBox(height: 14),
                Row(
                  children: [
                    const Icon(
                      Icons.terminal_rounded,
                      size: 16,
                      color: Color(0xFF38BDF8),
                    ),
                    const SizedBox(width: 6),
                    Text(
                      _sshHosts.isEmpty
                          ? '未在 ~/.ssh/config 检测到远程服务器'
                          : '已自动识别 ~/.ssh/config (${_sshHosts.length} 台服务器)',
                      style: const TextStyle(
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                        color: Color(0xFF38BDF8),
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                for (final entry in _sshHosts)
                  Container(
                    margin: const EdgeInsets.only(bottom: 8),
                    padding: const EdgeInsets.all(12),
                    decoration: BoxDecoration(
                      color: Colors.white.withValues(alpha: 0.04),
                      borderRadius: BorderRadius.circular(10),
                      border: Border.all(color: Colors.white12),
                    ),
                    child: Row(
                      children: [
                        Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(
                                entry.alias,
                                style: const TextStyle(
                                  fontWeight: FontWeight.w700,
                                  fontSize: 14.5,
                                ),
                              ),
                              const SizedBox(height: 2),
                              Text(
                                '${entry.displayTarget}  →  ${entry.relayWsUrl(_selectedPort)}',
                                style: const TextStyle(
                                  fontSize: 11.5,
                                  color: Colors.white60,
                                ),
                              ),
                            ],
                          ),
                        ),
                        const SizedBox(width: 8),
                        OutlinedButton(
                          onPressed: _deployingAlias != null
                              ? null
                              : () => _handleQuickSelectAddress(entry),
                          child: const Text('直接选用'),
                        ),
                        const SizedBox(width: 8),
                        FilledButton.icon(
                          onPressed: _deployingAlias != null
                              ? null
                              : () => _handleOneClickSshDeploy(entry),
                          icon: _deployingAlias == entry.alias
                              ? const SizedBox(
                                  width: 14,
                                  height: 14,
                                  child: CircularProgressIndicator(
                                    strokeWidth: 2,
                                  ),
                                )
                              : const Icon(Icons.bolt_rounded, size: 16),
                          label: const Text('一键部署并挂载'),
                        ),
                      ],
                    ),
                  ),
              ],
              if (_statusMessage != null) ...[
                const SizedBox(height: 8),
                Container(
                  padding: const EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: (_statusIsError
                            ? Colors.orangeAccent
                            : Colors.greenAccent)
                        .withValues(alpha: 0.12),
                    borderRadius: BorderRadius.circular(10),
                    border: Border.all(
                      color: (_statusIsError
                              ? Colors.orangeAccent
                              : Colors.greenAccent)
                          .withValues(alpha: 0.4),
                    ),
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        _statusMessage!,
                        style: const TextStyle(fontSize: 12.5, height: 1.4),
                      ),
                      if (_deployedRelayUrl != null &&
                          widget.host.routeState !=
                              HostRouteState.onlineRelay) ...[
                        const SizedBox(height: 8),
                        Row(
                          children: [
                            Expanded(
                              child: SelectableText(
                                'orbit use-relay $_deployedRelayUrl',
                                style: const TextStyle(
                                  fontFamily: 'monospace',
                                  fontSize: 12,
                                  color: Color(0xFF38BDF8),
                                ),
                              ),
                            ),
                            IconButton(
                              tooltip: '复制命令',
                              icon: const Icon(Icons.copy_rounded, size: 16),
                              onPressed: () {
                                Clipboard.setData(
                                  ClipboardData(
                                    text: 'orbit use-relay $_deployedRelayUrl',
                                  ),
                                );
                                ScaffoldMessenger.of(context).showSnackBar(
                                  const SnackBar(
                                    content: Text('已复制命令'),
                                  ),
                                );
                              },
                            ),
                          ],
                        ),
                      ],
                    ],
                  ),
                ),
              ],
              const SizedBox(height: 14),
              TextField(
                controller: _manualUrlCtrl,
                decoration: const InputDecoration(
                  labelText: '或手动输入 Relay 地址',
                  hintText: 'ws://203.0.113.10:7878',
                  border: OutlineInputBorder(),
                ),
              ),
              const SizedBox(height: 14),
              Row(
                children: [
                  if (widget.host.relayUrl != null &&
                      widget.host.relayUrl!.isNotEmpty) ...[
                    OutlinedButton(
                      onPressed: () async {
                        Navigator.of(context).pop();
                        await widget.controller
                            .updateHostRelay(widget.host.hostId, null);
                      },
                      child: const Text('清除中继'),
                    ),
                    const SizedBox(width: 12),
                  ],
                  Expanded(
                    child: FilledButton(
                      onPressed: () async {
                        final text = _manualUrlCtrl.text.trim();
                        Navigator.of(context).pop();
                        await widget.controller
                            .updateHostRelay(widget.host.hostId, text);
                      },
                      child: const Text('保存并连接'),
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Modal sheet for 1-Click Self-Update from GitHub Releases (`Orbit-macOS-AppleSilicon.dmg` / APK / CLI).
class _OneClickUpdateSheet extends StatefulWidget {
  const _OneClickUpdateSheet({
    this.initialRelease,
    required this.onFoundNewer,
  });

  final ReleaseUpdateInfo? initialRelease;
  final void Function(ReleaseUpdateInfo info) onFoundNewer;

  @override
  State<_OneClickUpdateSheet> createState() => _OneClickUpdateSheetState();
}

class _OneClickUpdateSheetState extends State<_OneClickUpdateSheet> {
  ReleaseUpdateInfo? _release;
  bool _checking = false;
  bool _updating = false;
  double _progress = 0.0;
  String? _statusText;
  String? _errorText;

  @override
  void initState() {
    super.initState();
    _release = widget.initialRelease;
    _checkNow();
  }

  Future<void> _checkNow() async {
    setState(() {
      _checking = true;
      _errorText = null;
    });
    try {
      final info = await AppUpdater.checkLatestRelease();
      widget.onFoundNewer(info);
      if (mounted) {
        setState(() {
          _release = info;
          _checking = false;
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _checking = false;
          _errorText = '检查更新失败: $e';
        });
      }
    }
  }

  Future<void> _runUpdate() async {
    final rel = _release;
    if (rel == null) return;
    setState(() {
      _updating = true;
      _errorText = null;
      _progress = 0.02;
      _statusText = '准备下载 ${rel.tagName}…';
    });
    try {
      await AppUpdater.performOneClickUpdate(
        release: rel,
        onProgress: (p, msg) {
          if (mounted) {
            setState(() {
              _progress = p;
              _statusText = msg;
            });
          }
        },
      );
      if (mounted) {
        setState(() => _updating = false);
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _updating = false;
          _errorText = '自动更新失败: $e';
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final rel = _release;
    return Padding(
      padding: EdgeInsets.only(
        left: 20,
        right: 20,
        top: 20,
        bottom: MediaQuery.of(context).viewInsets.bottom + 28,
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              const Icon(
                Icons.system_update_alt_rounded,
                color: Color(0xFF38BDF8),
              ),
              const SizedBox(width: 10),
              const Expanded(
                child: Text(
                  'Orbit 一键在线更新',
                  style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
                ),
              ),
              if (!_updating)
                IconButton(
                  tooltip: '重新检查 GitHub Release',
                  icon: const Icon(Icons.refresh_rounded, size: 18),
                  onPressed: _checking ? null : _checkNow,
                ),
            ],
          ),
          const SizedBox(height: 10),
          Text(
            '当前版本: v$kOrbitAppVersion   ·   GitHub 最新版本: ${rel?.tagName ?? (_checking ? "正在检查…" : "未知")}',
            style: const TextStyle(fontSize: 13, color: Colors.white70),
          ),
          if (_checking && rel == null) ...[
            const SizedBox(height: 18),
            const LinearProgressIndicator(),
          ],
          if (_errorText != null) ...[
            const SizedBox(height: 12),
            Text(
              _errorText!,
              style: const TextStyle(color: Colors.redAccent, fontSize: 12.5),
            ),
          ],
          if (_updating) ...[
            const SizedBox(height: 16),
            LinearProgressIndicator(value: _progress),
            const SizedBox(height: 8),
            Text(
              _statusText ?? '正在更新…',
              style: const TextStyle(fontSize: 12.5, color: Colors.white70),
            ),
          ],
          if (rel != null && !_updating) ...[
            const SizedBox(height: 16),
            FilledButton.icon(
              onPressed: _runUpdate,
              icon: Icon(
                rel.hasNewerVersion
                    ? Icons.rocket_launch_rounded
                    : Icons.replay_rounded,
              ),
              label: Text(
                rel.hasNewerVersion
                    ? '一键下载更新并自动重启 (${rel.tagName})'
                    : '当前已是最新 (${rel.tagName}) · 一键重新覆盖安装',
              ),
            ),
          ],
        ],
      ),
    );
  }
}
