import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../core/app_updater.dart';
import '../core/fleet_controller.dart';
import '../core/models.dart';
import '../core/ssh_config.dart';
import 'agent_chat_screen.dart';
import 'git_review_screen.dart';
import 'host_workspace_screen.dart';
import 'terminal_screen.dart';

enum _WorkbenchTab { agents, git, terminal, quotas }

class FleetHomeScreen extends StatefulWidget {
  const FleetHomeScreen({super.key, required this.controller});

  final FleetController controller;

  @override
  State<FleetHomeScreen> createState() => _FleetHomeScreenState();
}

class _FleetHomeScreenState extends State<FleetHomeScreen> {
  ReleaseUpdateInfo? _availableUpdate;
  String? _selectedHostId;
  String? _selectedAgentId;
  _WorkbenchTab _activeTab = _WorkbenchTab.agents;
  String? _seenLastCreatedAgentId;

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
    } catch (_) {}
  }

  String _defaultWorkspaceDir(HostRuntimeSession? session) {
    if (session != null && session.workspaces.isNotEmpty) {
      return session.workspaces.first.cwd;
    }
    final home = Platform.environment['HOME'];
    if (home != null && home.isNotEmpty) {
      return home;
    }
    return '~';
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        final hosts = controller.hosts;
        if (_selectedHostId == null ||
            !hosts.any((h) => h.hostId == _selectedHostId)) {
          _selectedHostId = hosts.isNotEmpty ? hosts.first.hostId : null;
          _selectedAgentId = null;
        }

        final activeSession = _selectedHostId != null
            ? controller.sessionFor(_selectedHostId!)
            : null;
        if (activeSession?.lastCreatedAgentId != null &&
            activeSession!.lastCreatedAgentId != _seenLastCreatedAgentId) {
          _seenLastCreatedAgentId = activeSession.lastCreatedAgentId;
          _selectedAgentId = _seenLastCreatedAgentId;
          _activeTab = _WorkbenchTab.agents;
          controller.openAgentTimeline(_selectedHostId!, _selectedAgentId!);
        }

        return LayoutBuilder(
          builder: (context, constraints) {
            if (constraints.maxWidth >= 880) {
              return _buildDesktopWorkbench(context, hosts);
            }
            return _buildMobileLayout(context, hosts);
          },
        );
      },
    );
  }

  // ===========================================================================
  // DESKTOP SPLIT-PANE WORKBENCH (macOS & Linux >= 880px)
  // ===========================================================================

  Widget _buildDesktopWorkbench(BuildContext context, List<HostProfile> hosts) {
    final selectedHost = _selectedHostId != null
        ? hosts.where((h) => h.hostId == _selectedHostId).firstOrNull
        : null;
    final session =
        selectedHost != null ? controller.sessionFor(selectedHost.hostId) : null;
    final agents = session?.agents.values.toList() ?? const [];

    return Scaffold(
      body: Row(
        children: [
          // --- Left Fleet & Sessions Sidebar (276px) ---
          Container(
            width: 276,
            decoration: const BoxDecoration(
              color: Color(0xFF0D111A),
              border: Border(
                right: BorderSide(color: Color(0xFF1E293B)),
              ),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                // Brand + Update + Pair Top Bar
                Container(
                  constraints: const BoxConstraints(minHeight: 54),
                  padding:
                      const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
                  decoration: const BoxDecoration(
                    border: Border(
                      bottom: BorderSide(color: Color(0xFF1E293B)),
                    ),
                  ),
                  child: Row(
                    children: [
                      const Icon(
                        Icons.radar_rounded,
                        color: Color(0xFF38BDF8),
                        size: 19,
                      ),
                      const SizedBox(width: 7),
                      const Flexible(
                        child: Text(
                          'Orbit',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            fontSize: 15.5,
                            fontWeight: FontWeight.w800,
                          ),
                        ),
                      ),
                      const SizedBox(width: 6),
                      Container(
                        padding: const EdgeInsets.symmetric(
                          horizontal: 5,
                          vertical: 1.5,
                        ),
                        decoration: BoxDecoration(
                          color: Colors.white10,
                          borderRadius: BorderRadius.circular(5),
                        ),
                        child: const Text(
                          'v$kOrbitAppVersion',
                          style: TextStyle(fontSize: 10, color: Colors.white60),
                        ),
                      ),
                      const Spacer(),
                      IconButton(
                        constraints: const BoxConstraints(),
                        padding: const EdgeInsets.all(6),
                        tooltip: _availableUpdate != null
                            ? '发现新版本 ${_availableUpdate!.tagName}，点击一键更新'
                            : '检查并一键在线更新 (v$kOrbitAppVersion)',
                        icon: Badge(
                          isLabelVisible: _availableUpdate != null,
                          backgroundColor: const Color(0xFF38BDF8),
                          child: Icon(
                            Icons.system_update_alt_rounded,
                            size: 18,
                            color: _availableUpdate != null
                                ? const Color(0xFF38BDF8)
                                : Colors.white70,
                          ),
                        ),
                        onPressed: () => _showUpdateModal(context),
                      ),
                      const SizedBox(width: 4),
                      IconButton(
                        constraints: const BoxConstraints(),
                        padding: const EdgeInsets.all(6),
                        tooltip: '配对新机器 (`orbit pair`)',
                        icon: const Icon(Icons.add_link_rounded, size: 19),
                        onPressed: () => _showPairHostModal(context),
                      ),
                    ],
                  ),
                ),

                // Machines Section Header
                Padding(
                  padding: const EdgeInsets.fromLTRB(14, 12, 12, 6),
                  child: Row(
                    children: [
                      const Expanded(
                        child: Text(
                          '机器舰队',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            fontSize: 11,
                            fontWeight: FontWeight.w700,
                            color: Colors.white38,
                            letterSpacing: 0.6,
                          ),
                        ),
                      ),
                      Text(
                        '${hosts.length}',
                        style: const TextStyle(
                          fontSize: 11,
                          color: Colors.white38,
                        ),
                      ),
                    ],
                  ),
                ),

                if (hosts.isEmpty)
                  Padding(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 14,
                      vertical: 8,
                    ),
                    child: OutlinedButton.icon(
                      onPressed: () => _showPairHostModal(context),
                      icon: const Icon(Icons.add_rounded, size: 16),
                      label: const Text('导入配对码'),
                    ),
                  )
                else
                  ConstrainedBox(
                    constraints: const BoxConstraints(maxHeight: 210),
                    child: ListView.builder(
                      shrinkWrap: true,
                      padding: const EdgeInsets.symmetric(horizontal: 8),
                      itemCount: hosts.length,
                      itemBuilder: (context, idx) =>
                          _buildSidebarHostItem(context, hosts[idx]),
                    ),
                  ),

                const Padding(
                  padding: EdgeInsets.symmetric(vertical: 8),
                  child: Divider(height: 1, color: Color(0xFF1E293B)),
                ),

                // Sessions Section Header
                Padding(
                  padding: const EdgeInsets.fromLTRB(14, 2, 10, 6),
                  child: Row(
                    children: [
                      const Expanded(
                        child: Text(
                          '编程会话',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            fontSize: 11,
                            fontWeight: FontWeight.w700,
                            color: Colors.white38,
                            letterSpacing: 0.6,
                          ),
                        ),
                      ),
                      if (selectedHost != null)
                        InkWell(
                          borderRadius: BorderRadius.circular(6),
                          onTap: () {
                            setState(() {
                              _activeTab = _WorkbenchTab.agents;
                              _selectedAgentId = null;
                            });
                          },
                          child: const Padding(
                            padding: EdgeInsets.symmetric(
                              horizontal: 6,
                              vertical: 2,
                            ),
                            child: Row(
                              children: [
                                Icon(
                                  Icons.add_rounded,
                                  size: 14,
                                  color: Color(0xFF38BDF8),
                                ),
                                SizedBox(width: 2),
                                Text(
                                  '新建',
                                  style: TextStyle(
                                    fontSize: 11.5,
                                    color: Color(0xFF38BDF8),
                                    fontWeight: FontWeight.w600,
                                  ),
                                ),
                              ],
                            ),
                          ),
                        ),
                    ],
                  ),
                ),

                // Agent Sessions List
                Expanded(
                  child: selectedHost == null
                      ? const Center(
                          child: Text(
                            '请先配对或选择一台机器',
                            style:
                                TextStyle(fontSize: 12, color: Colors.white38),
                          ),
                        )
                      : agents.isEmpty
                          ? Center(
                              child: Padding(
                                padding: const EdgeInsets.all(20),
                                child: Text(
                                  '暂无活跃会话\n在右侧直接选择 CLI 启动即可',
                                  textAlign: TextAlign.center,
                                  style: TextStyle(
                                    fontSize: 12,
                                    height: 1.5,
                                    color: Colors.white.withValues(alpha: 0.35),
                                  ),
                                ),
                              ),
                            )
                          : ListView.builder(
                              padding:
                                  const EdgeInsets.symmetric(horizontal: 8),
                              itemCount: agents.length,
                              itemBuilder: (context, i) =>
                                  _buildSidebarAgentItem(
                                selectedHost.hostId,
                                agents[i],
                              ),
                            ),
                ),
              ],
            ),
          ),

          // --- Main Right Workbench Area ---
          Expanded(
            child: selectedHost == null
                ? _buildEmptyOnboarding(context)
                : _buildHostWorkbenchMain(context, selectedHost, session),
          ),
        ],
      ),
    );
  }

  Widget _buildSidebarHostItem(BuildContext context, HostProfile host) {
    final isSelected = host.hostId == _selectedHostId;
    final (dotColor, routeLabel) = switch (host.routeState) {
      HostRouteState.onlineDirect => (Colors.greenAccent, 'LAN 直连'),
      HostRouteState.onlineRelay => (const Color(0xFF38BDF8), 'E2EE 中继'),
      HostRouteState.connecting => (Colors.orangeAccent, '连接中…'),
      HostRouteState.disconnected => (Colors.redAccent, '离线'),
    };

    return Container(
      margin: const EdgeInsets.symmetric(vertical: 2),
      decoration: BoxDecoration(
        color: isSelected ? const Color(0xFF162033) : Colors.transparent,
        borderRadius: BorderRadius.circular(9),
        border: Border.all(
          color: isSelected
              ? const Color(0xFF38BDF8).withValues(alpha: 0.45)
              : Colors.transparent,
        ),
      ),
      child: InkWell(
        borderRadius: BorderRadius.circular(9),
        onTap: () {
          setState(() {
            _selectedHostId = host.hostId;
            _selectedAgentId = null;
          });
        },
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
          child: Row(
            children: [
              Container(
                width: 8,
                height: 8,
                decoration: BoxDecoration(
                  color: dotColor,
                  shape: BoxShape.circle,
                ),
              ),
              const SizedBox(width: 9),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      host.hostname,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontSize: 13.5,
                        fontWeight:
                            isSelected ? FontWeight.w700 : FontWeight.w500,
                      ),
                    ),
                    const SizedBox(height: 1),
                    Text(
                      routeLabel,
                      style: TextStyle(
                        fontSize: 10.5,
                        color: dotColor.withValues(alpha: 0.85),
                      ),
                    ),
                  ],
                ),
              ),
              IconButton(
                visualDensity: VisualDensity.compact,
                tooltip: 'SSH 一键部署 / 配置中继',
                icon: const Icon(
                  Icons.cloud_sync_rounded,
                  size: 16,
                  color: Colors.white60,
                ),
                onPressed: () => _showEditRelayModal(context, host),
              ),
              PopupMenuButton<String>(
                tooltip: '机器操作',
                icon: const Icon(
                  Icons.more_horiz_rounded,
                  size: 16,
                  color: Colors.white54,
                ),
                onSelected: (action) async {
                  if (action == 'relay') {
                    _showEditRelayModal(context, host);
                  } else if (action == 'reconnect') {
                    await controller.connectHost(host.hostId);
                  } else if (action == 'pubkey') {
                    await Clipboard.setData(
                      ClipboardData(text: host.daemonPubkeyB64),
                    );
                    if (context.mounted) {
                      ScaffoldMessenger.of(context).showSnackBar(
                        const SnackBar(content: Text('已复制机器公钥')),
                      );
                    }
                  } else if (action == 'remove') {
                    await controller.removeHost(host.hostId);
                  }
                },
                itemBuilder: (_) => const [
                  PopupMenuItem(
                    value: 'relay',
                    child: Text('配置中继 (SSH Config)…'),
                  ),
                  PopupMenuItem(
                    value: 'reconnect',
                    child: Text('立即重新连接'),
                  ),
                  PopupMenuItem(
                    value: 'pubkey',
                    child: Text('复制本机身份公钥'),
                  ),
                  PopupMenuItem(
                    value: 'remove',
                    child: Text('移除该机器'),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildSidebarAgentItem(String hostId, AgentSummary agent) {
    final isSelected =
        _activeTab == _WorkbenchTab.agents && _selectedAgentId == agent.agentId;
    final statusColor = switch (agent.statusTag) {
      'running' || 'autonomous_wakeup' => const Color(0xFF38BDF8),
      'awaiting_permission' => Colors.amberAccent,
      'error' => Colors.redAccent,
      _ => Colors.white38,
    };

    return Container(
      margin: const EdgeInsets.symmetric(vertical: 2),
      decoration: BoxDecoration(
        color: isSelected
            ? Colors.white.withValues(alpha: 0.07)
            : Colors.transparent,
        borderRadius: BorderRadius.circular(8),
      ),
      child: InkWell(
        borderRadius: BorderRadius.circular(8),
        onTap: () {
          controller.openAgentTimeline(hostId, agent.agentId);
          setState(() {
            _activeTab = _WorkbenchTab.agents;
            _selectedAgentId = agent.agentId;
          });
        },
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
          child: Row(
            children: [
              Container(
                width: 7,
                height: 7,
                decoration: BoxDecoration(
                  color: statusColor,
                  shape: BoxShape.circle,
                ),
              ),
              const SizedBox(width: 9),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      agent.title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontSize: 12.5,
                        fontWeight:
                            isSelected ? FontWeight.w600 : FontWeight.w400,
                      ),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      agent.provider.label,
                      style:
                          const TextStyle(fontSize: 10.5, color: Colors.white54),
                    ),
                  ],
                ),
              ),
              if (agent.isAwaitingPermission)
                Container(
                  padding:
                      const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                  decoration: BoxDecoration(
                    color: Colors.amberAccent,
                    borderRadius: BorderRadius.circular(999),
                  ),
                  child: const Text(
                    '待审批',
                    style: TextStyle(
                      fontSize: 9.5,
                      color: Colors.black,
                      fontWeight: FontWeight.bold,
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildHostWorkbenchMain(
    BuildContext context,
    HostProfile host,
    HostRuntimeSession? session,
  ) {
    final defaultCwd = _defaultWorkspaceDir(session);
    final (badgeText, badgeColor) = switch (host.routeState) {
      HostRouteState.onlineDirect => ('LAN Direct', Colors.greenAccent),
      HostRouteState.onlineRelay =>
        ('Noise E2EE Relay', const Color(0xFF38BDF8)),
      HostRouteState.connecting => ('Connecting…', Colors.orangeAccent),
      HostRouteState.disconnected => ('Offline', Colors.redAccent),
    };

    return Column(
      children: [
        // Top Workbench Header Bar (elastic Wrap so zero overflow at any width or font scale)
        Container(
          width: double.infinity,
          constraints: const BoxConstraints(minHeight: 54),
          padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 8),
          decoration: const BoxDecoration(
            color: Color(0xFF0D111A),
            border: Border(
              bottom: BorderSide(color: Color(0xFF1E293B)),
            ),
          ),
          child: Wrap(
            alignment: WrapAlignment.spaceBetween,
            crossAxisAlignment: WrapCrossAlignment.center,
            spacing: 12,
            runSpacing: 8,
            children: [
              Wrap(
                crossAxisAlignment: WrapCrossAlignment.center,
                spacing: 8,
                runSpacing: 6,
                children: [
                  const Icon(Icons.dns_rounded, size: 18, color: Colors.white70),
                  Text(
                    host.hostname,
                    style: const TextStyle(
                      fontSize: 15.5,
                      fontWeight: FontWeight.w700,
                    ),
                  ),
                  Container(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 9,
                      vertical: 3.5,
                    ),
                    decoration: BoxDecoration(
                      color: badgeColor.withValues(alpha: 0.14),
                      borderRadius: BorderRadius.circular(999),
                      border:
                          Border.all(color: badgeColor.withValues(alpha: 0.45)),
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
                  OutlinedButton.icon(
                    style: OutlinedButton.styleFrom(
                      visualDensity: VisualDensity.compact,
                      padding: const EdgeInsets.symmetric(horizontal: 10),
                      side: const BorderSide(color: Color(0xFF283548)),
                    ),
                    onPressed: () => _showEditRelayModal(context, host),
                    icon: const Icon(
                      Icons.cloud_sync_rounded,
                      size: 14,
                      color: Color(0xFF38BDF8),
                    ),
                    label: Text(
                      host.relayUrl != null && host.relayUrl!.isNotEmpty
                          ? '中继已配'
                          : 'Set Relay (SSH)',
                      style: const TextStyle(fontSize: 11.5),
                    ),
                  ),
                ],
              ),
              Wrap(
                crossAxisAlignment: WrapCrossAlignment.center,
                spacing: 4,
                runSpacing: 6,
                children: [
                  _workbenchTabPill(
                    tab: _WorkbenchTab.agents,
                    icon: Icons.chat_bubble_outline_rounded,
                    label: '会话',
                  ),
                  _workbenchTabPill(
                    tab: _WorkbenchTab.git,
                    icon: Icons.difference_rounded,
                    label: 'Git',
                  ),
                  _workbenchTabPill(
                    tab: _WorkbenchTab.terminal,
                    icon: Icons.terminal_rounded,
                    label: '终端',
                  ),
                  _workbenchTabPill(
                    tab: _WorkbenchTab.quotas,
                    icon: Icons.donut_large_rounded,
                    label: '额度',
                    onTapExtra: () => controller.refreshQuotas(host.hostId),
                  ),
                  const SizedBox(width: 6),
                  FilledButton.icon(
                    style: FilledButton.styleFrom(
                      visualDensity: VisualDensity.compact,
                      padding: const EdgeInsets.symmetric(horizontal: 12),
                    ),
                    onPressed: () {
                      setState(() {
                        _activeTab = _WorkbenchTab.agents;
                        _selectedAgentId = null;
                      });
                    },
                    icon: const Icon(Icons.add_rounded, size: 16),
                    label: const Text('新建会话', style: TextStyle(fontSize: 12)),
                  ),
                ],
              ),
            ],
          ),
        ),

        // Offline Diagnostic Strip
        if (host.routeState == HostRouteState.disconnected &&
            host.lastConnectionError != null)
          Container(
            width: double.infinity,
            padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 8),
            decoration: BoxDecoration(
              color: Colors.redAccent.withValues(alpha: 0.10),
              border: const Border(
                bottom: BorderSide(color: Color(0xFF3B1D26)),
              ),
            ),
            child: Row(
              children: [
                const Icon(
                  Icons.warning_amber_rounded,
                  size: 16,
                  color: Colors.orangeAccent,
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    host.lastConnectionError!,
                    style:
                        const TextStyle(fontSize: 12, color: Colors.white70),
                  ),
                ),
                TextButton(
                  onPressed: () => _showEditRelayModal(context, host),
                  child: const Text('一键配置 SSH 中继'),
                ),
                TextButton(
                  onPressed: () => controller.connectHost(host.hostId),
                  child: const Text('重试连接'),
                ),
              ],
            ),
          ),

        // Main Workbench Content Pane
        Expanded(
          child: switch (_activeTab) {
            _WorkbenchTab.agents => (_selectedAgentId != null &&
                    session != null &&
                    session.agents.containsKey(_selectedAgentId))
                ? AgentChatScreen(
                    key: ValueKey('chat_${host.hostId}_$_selectedAgentId'),
                    controller: controller,
                    hostId: host.hostId,
                    agentId: _selectedAgentId!,
                    embedded: true,
                  )
                : _DesktopQuickLaunchComposer(
                    controller: controller,
                    host: host,
                    session: session,
                    initialCwd: defaultCwd,
                  ),
            _WorkbenchTab.git => GitReviewScreen(
                key: ValueKey('git_${host.hostId}'),
                controller: controller,
                hostId: host.hostId,
                initialCwd: defaultCwd,
                embedded: true,
              ),
            _WorkbenchTab.terminal => TerminalScreen(
                key: ValueKey('pty_${host.hostId}'),
                controller: controller,
                hostId: host.hostId,
                cwd: defaultCwd,
                embedded: true,
              ),
            _WorkbenchTab.quotas => _DesktopQuotasPane(
                controller: controller,
                hostId: host.hostId,
              ),
          },
        ),
      ],
    );
  }

  Widget _workbenchTabPill({
    required _WorkbenchTab tab,
    required IconData icon,
    required String label,
    VoidCallback? onTapExtra,
  }) {
    final active = _activeTab == tab;
    return InkWell(
      borderRadius: BorderRadius.circular(8),
      onTap: () {
        onTapExtra?.call();
        setState(() => _activeTab = tab);
      },
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
        decoration: BoxDecoration(
          color: active
              ? const Color(0xFF38BDF8).withValues(alpha: 0.16)
              : Colors.transparent,
          borderRadius: BorderRadius.circular(8),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              icon,
              size: 14,
              color: active ? const Color(0xFF38BDF8) : Colors.white60,
            ),
            const SizedBox(width: 5),
            Text(
              label,
              style: TextStyle(
                fontSize: 12,
                fontWeight: active ? FontWeight.w600 : FontWeight.w400,
                color: active ? const Color(0xFF38BDF8) : Colors.white70,
              ),
            ),
          ],
        ),
      ),
    );
  }

  // ===========================================================================
  // MOBILE STACK LAYOUT (iOS & Android < 880px)
  // ===========================================================================

  Widget _buildMobileLayout(BuildContext context, List<HostProfile> hosts) {
    final pendingApprovals = controller.globalPendingApprovalsCount;
    return Scaffold(
      appBar: AppBar(
        title: Row(
          children: [
            const Icon(Icons.radar_rounded, color: Color(0xFF38BDF8)),
            const SizedBox(width: 8),
            const Flexible(
              child: Text(
                'Orbit Fleet',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(fontWeight: FontWeight.w700),
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
          IconButton(
            tooltip: '检查并一键更新 (v$kOrbitAppVersion)',
            icon: Badge(
              isLabelVisible: _availableUpdate != null,
              child: const Icon(Icons.system_update_alt_rounded),
            ),
            onPressed: () => _showUpdateModal(context),
          ),
          IconButton(
            tooltip: 'Pair Host (`orbit pair`)',
            icon: const Icon(Icons.qr_code_scanner_rounded),
            onPressed: () => _showPairHostModal(context),
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
                  _buildMobileHostCard(context, hosts[index]),
            ),
      floatingActionButton: FloatingActionButton.extended(
        onPressed: () => _showPairHostModal(context),
        icon: const Icon(Icons.add_link_rounded),
        label: const Text('Pair Machine'),
      ),
    );
  }

  Widget _buildEmptyOnboarding(BuildContext context) {
    return Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 480),
        child: Padding(
          padding: const EdgeInsets.all(32),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const Icon(
                Icons.hub_outlined,
                size: 60,
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
                '在你的开发机、Linux 工作站或 Mac mini 上执行 `orbit daemon up`（外网执行 `orbit use-relay` 自动识别 ~/.ssh/config），然后导入 `orbit://pair/...` 配对码。',
                style: TextStyle(color: Colors.white70, height: 1.45),
                textAlign: TextAlign.center,
              ),
              const SizedBox(height: 24),
              FilledButton.icon(
                onPressed: () => _showPairHostModal(context),
                icon: const Icon(Icons.link_rounded),
                label: const Text('Import Pairing URI'),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildMobileHostCard(BuildContext context, HostProfile host) {
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
        borderRadius: BorderRadius.circular(12),
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
                ],
              ),
              const SizedBox(height: 6),
              Row(
                children: [
                  Expanded(
                    child: Text(
                      host.relayUrl != null && host.relayUrl!.isNotEmpty
                          ? 'Relay: ${host.relayUrl}'
                          : 'Relay: 未配置 (仅局域网直连)',
                      style:
                          const TextStyle(fontSize: 11.5, color: Colors.white54),
                    ),
                  ),
                  TextButton.icon(
                    style: TextButton.styleFrom(
                      visualDensity: VisualDensity.compact,
                    ),
                    onPressed: () => _showEditRelayModal(context, host),
                    icon: const Icon(Icons.cloud_sync_rounded, size: 15),
                    label: const Text('Set Relay', style: TextStyle(fontSize: 12)),
                  ),
                ],
              ),
              if (host.routeState == HostRouteState.disconnected &&
                  host.lastConnectionError != null) ...[
                const SizedBox(height: 6),
                Text(
                  host.lastConnectionError!,
                  style: const TextStyle(
                    fontSize: 11.5,
                    color: Colors.orangeAccent,
                  ),
                ),
              ],
              const SizedBox(height: 8),
              Row(
                children: [
                  Expanded(
                    child: Text(
                      awaitingCount > 0
                          ? '$agentCount sessions · $runningCount active · $awaitingCount 待审批'
                          : '$agentCount sessions · $runningCount active',
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontSize: 12,
                        color: awaitingCount > 0
                            ? Colors.amberAccent
                            : Colors.white60,
                      ),
                    ),
                  ),
                  IconButton(
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

  // ===========================================================================
  // ADAPTIVE MODALS (Centered Dialog on Desktop >= 720px, BottomSheet on Phone)
  // ===========================================================================

  void _presentAdaptiveModal(BuildContext context, Widget child) {
    final isWide = MediaQuery.of(context).size.width >= 720;
    if (isWide) {
      showDialog<void>(
        context: context,
        builder: (_) => Dialog(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 620),
            child: child,
          ),
        ),
      );
    } else {
      showModalBottomSheet<void>(
        context: context,
        isScrollControlled: true,
        builder: (_) => child,
      );
    }
  }

  void _showUpdateModal(BuildContext context) {
    _presentAdaptiveModal(
      context,
      _OneClickUpdateSheet(
        initialRelease: _availableUpdate,
        onFoundNewer: (info) {
          if (mounted && info.hasNewerVersion) {
            setState(() => _availableUpdate = info);
          }
        },
      ),
    );
  }

  void _showEditRelayModal(BuildContext context, HostProfile host) {
    _presentAdaptiveModal(
      context,
      _SshRelaySetupSheet(controller: controller, host: host),
    );
  }

  void _showPairHostModal(BuildContext context) {
    final uriCtrl = TextEditingController();
    _presentAdaptiveModal(
      context,
      Builder(
        builder: (ctx) => Padding(
          padding: EdgeInsets.only(
            left: 24,
            right: 24,
            top: 22,
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
                '粘贴在目标机器执行 `orbit pair` 或 `orbit use-relay` 生成的 `orbit://pair/...` 配对码：',
                style: TextStyle(fontSize: 13, color: Colors.white70),
              ),
              const SizedBox(height: 14),
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
                    final paired = await controller.pairFromQrUri(text);
                    if (mounted) {
                      setState(() => _selectedHostId = paired.hostId);
                    }
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
      ),
    );
  }
}

/// Inline Desktop Quick-Launch Agent Composer shown in the main workbench area.
class _DesktopQuickLaunchComposer extends StatefulWidget {
  const _DesktopQuickLaunchComposer({
    required this.controller,
    required this.host,
    required this.session,
    required this.initialCwd,
  });

  final FleetController controller;
  final HostProfile host;
  final HostRuntimeSession? session;
  final String initialCwd;

  @override
  State<_DesktopQuickLaunchComposer> createState() =>
      _DesktopQuickLaunchComposerState();
}

class _DesktopQuickLaunchComposerState
    extends State<_DesktopQuickLaunchComposer> {
  ProviderKind _provider = ProviderKind.antigravity;
  SessionMode _mode = SessionMode.acceptEdits;
  late final TextEditingController _cwdCtrl;
  final TextEditingController _branchCtrl = TextEditingController();
  final TextEditingController _promptCtrl = TextEditingController();

  @override
  void initState() {
    super.initState();
    _cwdCtrl = TextEditingController(text: widget.initialCwd);
    if (widget.host.availableProviders.isNotEmpty) {
      _provider = widget.host.availableProviders.first;
    }
  }

  @override
  void dispose() {
    _cwdCtrl.dispose();
    _branchCtrl.dispose();
    _promptCtrl.dispose();
    super.dispose();
  }

  void _launch() {
    final prompt = _promptCtrl.text.trim();
    final cwd = _cwdCtrl.text.trim();
    if (prompt.isEmpty || cwd.isEmpty) return;
    widget.controller.createAgent(
      hostId: widget.host.hostId,
      cwd: cwd,
      provider: _provider,
      mode: _mode,
      prompt: prompt,
      worktreeBranch: _branchCtrl.text.trim().isEmpty
          ? null
          : _branchCtrl.text.trim(),
    );
    _promptCtrl.clear();
  }

  @override
  Widget build(BuildContext context) {
    final isOnline = widget.session != null;
    final workspaces = widget.session?.workspaces ?? const [];

    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.all(28),
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 680),
          child: Container(
            padding: const EdgeInsets.all(26),
            decoration: BoxDecoration(
              color: const Color(0xFF111622),
              borderRadius: BorderRadius.circular(16),
              border: Border.all(color: const Color(0xFF1E293B)),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Row(
                  children: [
                    const Icon(
                      Icons.rocket_launch_rounded,
                      color: Color(0xFF38BDF8),
                      size: 22,
                    ),
                    const SizedBox(width: 10),
                    Expanded(
                      child: Text(
                        '在 ${widget.host.hostname} 启动 AI 编程会话',
                        style: const TextStyle(
                          fontSize: 18,
                          fontWeight: FontWeight.w700,
                        ),
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 6),
                Text(
                  isOnline
                      ? '选择 CLI 引擎与工作目录，会话由独立 `orbit shim` 托管，即使重启客户端也不断流。'
                      : '当前机器离线，请先检查守护进程或点击上方 Set Relay (SSH) 挂载中继。',
                  style: const TextStyle(fontSize: 12.5, color: Colors.white60),
                ),
                const SizedBox(height: 18),

                // Provider Selector
                SegmentedButton<ProviderKind>(
                  segments: const [
                    ButtonSegment(
                      value: ProviderKind.antigravity,
                      icon: Icon(Icons.auto_awesome_rounded, size: 16),
                      label: Text('Antigravity'),
                    ),
                    ButtonSegment(
                      value: ProviderKind.codex,
                      icon: Icon(Icons.code_rounded, size: 16),
                      label: Text('Codex'),
                    ),
                    ButtonSegment(
                      value: ProviderKind.claudeCode,
                      icon: Icon(Icons.psychology_rounded, size: 16),
                      label: Text('Claude Code'),
                    ),
                  ],
                  selected: {_provider},
                  onSelectionChanged: (s) => setState(() => _provider = s.first),
                ),
                const SizedBox(height: 16),

                // CWD & Permission Mode Row
                Row(
                  children: [
                    Expanded(
                      flex: 3,
                      child: TextField(
                        controller: _cwdCtrl,
                        style: const TextStyle(
                          fontSize: 13,
                          fontFamily: 'monospace',
                        ),
                        decoration: const InputDecoration(
                          isDense: true,
                          labelText: '工作目录 (Workspace Directory)',
                          border: OutlineInputBorder(),
                        ),
                      ),
                    ),
                    const SizedBox(width: 12),
                    Expanded(
                      flex: 2,
                      child: DropdownButtonFormField<SessionMode>(
                        isExpanded: true,
                        initialValue: _mode,
                        decoration: const InputDecoration(
                          isDense: true,
                          labelText: '权限模式',
                          border: OutlineInputBorder(),
                        ),
                        items: [
                          for (final m in SessionMode.values)
                            DropdownMenuItem(
                              value: m,
                              child: Text(
                                m.label,
                                style: const TextStyle(fontSize: 13),
                              ),
                            ),
                        ],
                        onChanged: (v) {
                          if (v != null) setState(() => _mode = v);
                        },
                      ),
                    ),
                  ],
                ),

                if (workspaces.isNotEmpty) ...[
                  const SizedBox(height: 8),
                  Wrap(
                    spacing: 6,
                    runSpacing: 6,
                    children: [
                      for (final ws in workspaces.take(5))
                        ActionChip(
                          visualDensity: VisualDensity.compact,
                          label: Text(
                            ws.cwd,
                            style: const TextStyle(
                              fontSize: 11,
                              fontFamily: 'monospace',
                            ),
                          ),
                          onPressed: () => setState(() => _cwdCtrl.text = ws.cwd),
                        ),
                    ],
                  ),
                ],

                const SizedBox(height: 12),
                TextField(
                  controller: _branchCtrl,
                  style: const TextStyle(fontSize: 13, fontFamily: 'monospace'),
                  decoration: const InputDecoration(
                    isDense: true,
                    labelText: '可选：自动创建 Git Worktree 隔离分支',
                    hintText: '例如 feat/refactor-auth（留空则在原目录直接执行）',
                    border: OutlineInputBorder(),
                  ),
                ),

                const SizedBox(height: 12),
                TextField(
                  controller: _promptCtrl,
                  minLines: 3,
                  maxLines: 6,
                  decoration: const InputDecoration(
                    labelText: '任务指令 (Prompt)',
                    hintText: '输入要交给 Agent 完成的代码分析、重构或修复任务…',
                    alignLabelWithHint: true,
                    border: OutlineInputBorder(),
                  ),
                ),

                const SizedBox(height: 18),
                Align(
                  alignment: Alignment.centerRight,
                  child: FilledButton.icon(
                    onPressed: isOnline ? _launch : null,
                    icon: const Icon(Icons.play_arrow_rounded),
                    label: const Padding(
                      padding:
                          EdgeInsets.symmetric(horizontal: 8, vertical: 10),
                      child: Text(
                        '立即启动会话',
                        style: TextStyle(fontWeight: FontWeight.w600),
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// Desktop Real-Time Quotas View (`Antigravity` / `Codex` / `Claude Code`).
class _DesktopQuotasPane extends StatelessWidget {
  const _DesktopQuotasPane({
    required this.controller,
    required this.hostId,
  });

  final FleetController controller;
  final String hostId;

  @override
  Widget build(BuildContext context) {
    final quotas = controller.sessionFor(hostId)?.quotas ?? const [];
    return Padding(
      padding: const EdgeInsets.all(24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Text(
                '三大 CLI 实时配额监控 (5h / Weekly & Credits)',
                style: TextStyle(fontSize: 17, fontWeight: FontWeight.bold),
              ),
              const Spacer(),
              FilledButton.tonalIcon(
                onPressed: () => controller.refreshQuotas(hostId),
                icon: const Icon(Icons.refresh_rounded, size: 16),
                label: const Text('刷新实时额度'),
              ),
            ],
          ),
          const SizedBox(height: 18),
          Expanded(
            child: quotas.isEmpty
                ? const Center(child: CircularProgressIndicator())
                : ListView.separated(
                    itemCount: quotas.length,
                    separatorBuilder: (_, __) => const SizedBox(height: 12),
                    itemBuilder: (context, i) {
                      final q = quotas[i];
                      return Card(
                        child: Padding(
                          padding: const EdgeInsets.all(18),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Row(
                                children: [
                                  Text(
                                    q.provider.label,
                                    style: const TextStyle(
                                      fontSize: 15.5,
                                      fontWeight: FontWeight.w700,
                                    ),
                                  ),
                                  if (q.accountLabel != null) ...[
                                    const SizedBox(width: 8),
                                    Text(
                                      '(${q.accountLabel})',
                                      style: const TextStyle(
                                        fontSize: 12.5,
                                        color: Colors.white54,
                                      ),
                                    ),
                                  ],
                                  const Spacer(),
                                  if (q.creditsRemaining != null)
                                    Text(
                                      '剩余 Credits: ${q.creditsRemaining!.toStringAsFixed(1)}',
                                      style: const TextStyle(
                                        fontSize: 12.5,
                                        color: Colors.greenAccent,
                                        fontWeight: FontWeight.w600,
                                      ),
                                    ),
                                ],
                              ),
                              if (q.error != null) ...[
                                const SizedBox(height: 6),
                                Text(
                                  q.error!,
                                  style: const TextStyle(
                                    fontSize: 12,
                                    color: Colors.white54,
                                  ),
                                ),
                              ],
                              if (q.windows.isNotEmpty)
                                const SizedBox(height: 12),
                              for (final w in q.windows)
                                Padding(
                                  padding:
                                      const EdgeInsets.symmetric(vertical: 6),
                                  child: Column(
                                    crossAxisAlignment:
                                        CrossAxisAlignment.start,
                                    children: [
                                      Row(
                                        mainAxisAlignment:
                                            MainAxisAlignment.spaceBetween,
                                        children: [
                                          Expanded(
                                            child: Text(
                                              w.label,
                                              style: const TextStyle(
                                                fontSize: 12.5,
                                              ),
                                            ),
                                          ),
                                          Text(
                                            '已用 ${w.usedPercent.toStringAsFixed(1)}%',
                                            style: const TextStyle(
                                              fontSize: 12.5,
                                              fontWeight: FontWeight.bold,
                                            ),
                                          ),
                                        ],
                                      ),
                                      const SizedBox(height: 5),
                                      ClipRRect(
                                        borderRadius: BorderRadius.circular(4),
                                        child: LinearProgressIndicator(
                                          minHeight: 7,
                                          value: (w.usedPercent / 100.0)
                                              .clamp(0.0, 1.0),
                                          color: w.usedPercent > 90
                                              ? Colors.redAccent
                                              : w.usedPercent >= 70
                                                  ? Colors.amberAccent
                                                  : const Color(0xFF38BDF8),
                                        ),
                                      ),
                                    ],
                                  ),
                                ),
                            ],
                          ),
                        ),
                      );
                    },
                  ),
          ),
        ],
      ),
    );
  }
}

/// Modal dialog/sheet that auto-reads `~/.ssh/config` and provides 1-Click SSH Relay Deployment & Attachment!
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
      _statusMessage = '正在通过 SSH 连接 ${entry.alias} (${entry.displayTarget})…';
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
          _statusMessage = '已部署 $relayUrl，正在验证加密连通性…';
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
              '已成功在 ${entry.alias} ($relayUrl) 拉起白名单中继并绑定到 ${widget.host.hostname}！';
        } else {
          final err = widget.host.lastConnectionError ?? '';
          if (err.contains('502')) {
            _statusIsError = false;
            _statusMessage =
                '中继已在 ${entry.alias} ($relayUrl) 启动并完成公钥授权！只需在 ${widget.host.hostname} 执行下方命令连入中继即可打通：';
          } else {
            _statusIsError = true;
            _statusMessage =
                '中继已在 ${entry.alias} 后台启动，但本机暂无法直连 $relayUrl —— 请确认云安全组（如 AWS Security Group）已放行入站 TCP $port 端口。';
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
          left: 24,
          right: 24,
          top: 22,
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
                      '配置 E2EE 中继 · ${widget.host.hostname}',
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
                const SizedBox(height: 16),
                Row(
                  children: [
                    const Icon(
                      Icons.terminal_rounded,
                      size: 16,
                      color: Color(0xFF38BDF8),
                    ),
                    const SizedBox(width: 6),
                    Expanded(
                      child: Text(
                        _sshHosts.isEmpty
                            ? '未在 ~/.ssh/config 检测到远程服务器'
                            : '已自动识别本机 ~/.ssh/config (${_sshHosts.length} 台服务器)',
                        style: const TextStyle(
                          fontSize: 13,
                          fontWeight: FontWeight.w600,
                          color: Color(0xFF38BDF8),
                        ),
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 10),
                for (final entry in _sshHosts)
                  Container(
                    margin: const EdgeInsets.only(bottom: 10),
                    padding: const EdgeInsets.all(14),
                    decoration: BoxDecoration(
                      color: Colors.white.withValues(alpha: 0.04),
                      borderRadius: BorderRadius.circular(12),
                      border: Border.all(color: const Color(0xFF1E293B)),
                    ),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            const Icon(
                              Icons.cloud_outlined,
                              size: 17,
                              color: Color(0xFF38BDF8),
                            ),
                            const SizedBox(width: 8),
                            Text(
                              entry.alias,
                              style: const TextStyle(
                                fontWeight: FontWeight.w700,
                                fontSize: 15,
                              ),
                            ),
                            const SizedBox(width: 10),
                            Expanded(
                              child: Text(
                                '${entry.displayTarget}  →  ${entry.relayWsUrl(_selectedPort)}',
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: const TextStyle(
                                  fontSize: 12,
                                  fontFamily: 'monospace',
                                  color: Colors.white60,
                                ),
                              ),
                            ),
                          ],
                        ),
                        const SizedBox(height: 10),
                        Wrap(
                          alignment: WrapAlignment.end,
                          spacing: 10,
                          runSpacing: 8,
                          children: [
                            OutlinedButton(
                              onPressed: _deployingAlias != null
                                  ? null
                                  : () => _handleQuickSelectAddress(entry),
                              child: const Text('直接选用地址'),
                            ),
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
                      ],
                    ),
                  ),
              ],
              if (_statusMessage != null) ...[
                const SizedBox(height: 6),
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
                      if (_deployedRelayUrl != null) ...[
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
                                  const SnackBar(content: Text('已复制命令')),
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
              const SizedBox(height: 16),
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

/// Modal dialog/sheet for 1-Click Self-Update from GitHub Releases.
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
        left: 24,
        right: 24,
        top: 22,
        bottom: MediaQuery.of(context).viewInsets.bottom + 26,
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
                  'Orbit 一键在线自更新',
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
            const SizedBox(height: 18),
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
