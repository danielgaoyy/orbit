import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../core/app_updater.dart';
import '../core/fleet_controller.dart';
import '../core/models.dart';
import '../core/ssh_config.dart';
import '../widgets/orbit_chat_composer.dart';
import 'agent_chat_screen.dart';
import 'git_review_screen.dart';
import 'terminal_screen.dart';

class FleetHomeScreen extends StatefulWidget {
  const FleetHomeScreen({super.key, required this.controller});

  final FleetController controller;

  @override
  State<FleetHomeScreen> createState() => _FleetHomeScreenState();
}

class _FleetHomeScreenState extends State<FleetHomeScreen> {
  final GlobalKey<ScaffoldState> _scaffoldKey = GlobalKey<ScaffoldState>();
  ReleaseUpdateInfo? _availableUpdate;
  String? _selectedHostId;
  String? _selectedAgentId;
  String? _seenLastCreatedKey;
  bool _terminalOpen = false;
  bool _creatingSession = false;

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
        setState(() => _availableUpdate = info);
      }
    } catch (_) {}
  }

  void _selectNewChat() {
    setState(() {
      _selectedAgentId = null;
      _terminalOpen = false;
      _creatingSession = false;
    });
  }

  void _selectThread(String hostId, String agentId) {
    controller.openAgentTimeline(hostId, agentId);
    setState(() {
      _selectedHostId = hostId;
      _selectedAgentId = agentId;
      _terminalOpen = false;
      _creatingSession = false;
    });
  }

  void _handleStartNewConversation(String prompt) {
    final target = controller.activeDraftProject;
    if (target == null) {
      _showMachinesModal(context);
      return;
    }
    setState(() => _creatingSession = true);
    controller.createAgent(
      hostId: target.hostId,
      cwd: target.path,
      provider: controller.draftProvider,
      mode: controller.draftMode,
      prompt: prompt,
      worktree: controller.draftUseWorktree,
    );
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        // Automatically jump into newly created conversation as soon as Daemon confirms creation
        for (final host in controller.hosts) {
          final s = controller.sessionFor(host.hostId);
          final createdId = s?.lastCreatedAgentId;
          if (createdId != null) {
            final key = '${host.hostId}::$createdId';
            if (key != _seenLastCreatedKey) {
              _seenLastCreatedKey = key;
              _selectedHostId = host.hostId;
              _selectedAgentId = createdId;
              _creatingSession = false;
              controller.openAgentTimeline(host.hostId, createdId);
            }
          }
        }

        // Validate selected thread still exists
        AgentSummary? activeAgent;
        HostRuntimeSession? activeSession;
        if (_selectedHostId != null && _selectedAgentId != null) {
          activeSession = controller.sessionFor(_selectedHostId!);
          activeAgent = activeSession?.agents[_selectedAgentId!];
          if (activeAgent == null) {
            _selectedAgentId = null;
          }
        }

        return LayoutBuilder(
          builder: (context, constraints) {
            final isDesktop = constraints.maxWidth >= 800;

            return Scaffold(
              key: _scaffoldKey,
              drawer: isDesktop
                  ? null
                  : Drawer(
                      width: 280,
                      backgroundColor: const Color(0xFF0F1218),
                      child: SafeArea(
                        child: _buildSidebar(
                          context,
                          closeDrawerOnTap: true,
                        ),
                      ),
                    ),
              body: SafeArea(
                child: Row(
                  children: [
                    if (isDesktop)
                      SizedBox(
                        width: 260,
                        child: _buildSidebar(
                          context,
                          closeDrawerOnTap: false,
                        ),
                      ),
                    Expanded(
                      child: _buildMainStage(
                        context,
                        isDesktop: isDesktop,
                        activeAgent: activeAgent,
                        activeSession: activeSession,
                      ),
                    ),
                  ],
                ),
              ),
            );
          },
        );
      },
    );
  }

  // ===========================================================================
  // CALM UNIFIED SIDEBAR (Threads + Single-Line Machine Status)
  // ===========================================================================

  Widget _buildSidebar(
    BuildContext context, {
    required bool closeDrawerOnTap,
  }) {
    final allThreads = controller.allThreadsSorted;
    final awaiting =
        allThreads.where((t) => t.agent.isAwaitingPermission).toList();
    final recent =
        allThreads.where((t) => !t.agent.isAwaitingPermission).toList();
    final onlineCount = controller.onlineHostsCount;
    final totalHosts = controller.hosts.length;

    return Container(
      decoration: BoxDecoration(
        color: const Color(0xFF0F1218),
        border: Border(
          right: BorderSide(color: Colors.white.withValues(alpha: 0.06)),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // Brand + Quiet Self-Update + "+ 新对话"
          Padding(
            padding: const EdgeInsets.fromLTRB(14, 12, 10, 10),
            child: Row(
              children: [
                const Icon(
                  Icons.radar_rounded,
                  size: 19,
                  color: Color(0xFF38BDF8),
                ),
                const SizedBox(width: 7),
                const Expanded(
                  child: Text(
                    'Orbit',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 15,
                      fontWeight: FontWeight.w700,
                      letterSpacing: -0.2,
                    ),
                  ),
                ),
                IconButton(
                  constraints: const BoxConstraints(),
                  padding: const EdgeInsets.all(6),
                  tooltip: _availableUpdate != null
                      ? '发现新版本 ${_availableUpdate!.tagName} · 点此一键更新'
                      : '检查更新 (v$kOrbitAppVersion)',
                  onPressed: () => _showUpdateModal(context),
                  icon: Badge(
                    isLabelVisible: _availableUpdate != null,
                    backgroundColor: const Color(0xFF38BDF8),
                    child: Icon(
                      Icons.system_update_alt_rounded,
                      size: 17,
                      color: _availableUpdate != null
                          ? const Color(0xFF38BDF8)
                          : Colors.white38,
                    ),
                  ),
                ),
                const SizedBox(width: 2),
                Material(
                  color: _selectedAgentId == null
                      ? Colors.white.withValues(alpha: 0.12)
                      : Colors.white.withValues(alpha: 0.06),
                  borderRadius: BorderRadius.circular(9),
                  child: InkWell(
                    borderRadius: BorderRadius.circular(9),
                    onTap: () {
                      _selectNewChat();
                      if (closeDrawerOnTap) {
                        Navigator.of(context).maybePop();
                      }
                    },
                    child: const Padding(
                      padding: EdgeInsets.symmetric(
                        horizontal: 10,
                        vertical: 6.5,
                      ),
                      child: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Icon(Icons.add_rounded, size: 15),
                          SizedBox(width: 4),
                          Text(
                            '新对话',
                            style: TextStyle(
                              fontSize: 12.5,
                              fontWeight: FontWeight.w600,
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ),

          // Unified Scannable Thread List
          Expanded(
            child: allThreads.isEmpty
                ? Center(
                    child: Padding(
                      padding: const EdgeInsets.all(24),
                      child: Text(
                        totalHosts == 0
                            ? '先在右侧点选接入一台开发机'
                            : '在右侧输入框打字回车即可开启第一段对话',
                        textAlign: TextAlign.center,
                        style: const TextStyle(
                          fontSize: 12,
                          height: 1.5,
                          color: Colors.white38,
                        ),
                      ),
                    ),
                  )
                : ListView(
                    padding:
                        const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
                    children: [
                      if (awaiting.isNotEmpty) ...[
                        Padding(
                          padding: const EdgeInsets.fromLTRB(8, 4, 8, 6),
                          child: Text(
                            '待你确认 (${awaiting.length})',
                            style: const TextStyle(
                              fontSize: 11,
                              fontWeight: FontWeight.w600,
                              color: Color(0xFFFBBF24),
                            ),
                          ),
                        ),
                        for (final item in awaiting)
                          _buildThreadTile(
                            context,
                            item,
                            closeDrawerOnTap: closeDrawerOnTap,
                          ),
                        const SizedBox(height: 12),
                      ],
                      if (recent.isNotEmpty) ...[
                        const Padding(
                          padding: EdgeInsets.fromLTRB(8, 4, 8, 6),
                          child: Text(
                            '最近对话',
                            style: TextStyle(
                              fontSize: 11,
                              fontWeight: FontWeight.w600,
                              color: Colors.white38,
                            ),
                          ),
                        ),
                        for (final item in recent)
                          _buildThreadTile(
                            context,
                            item,
                            closeDrawerOnTap: closeDrawerOnTap,
                          ),
                      ],
                    ],
                  ),
          ),

          // Quiet Single-Line Machine Footer
          Container(
            padding: const EdgeInsets.all(10),
            decoration: BoxDecoration(
              border: Border(
                top: BorderSide(color: Colors.white.withValues(alpha: 0.06)),
              ),
            ),
            child: InkWell(
              borderRadius: BorderRadius.circular(11),
              onTap: () {
                if (closeDrawerOnTap) {
                  Navigator.of(context).maybePop();
                }
                _showMachinesModal(context);
              },
              child: Padding(
                padding:
                    const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
                child: Row(
                  children: [
                    Container(
                      width: 8,
                      height: 8,
                      decoration: BoxDecoration(
                        shape: BoxShape.circle,
                        color: onlineCount > 0
                            ? const Color(0xFF34D399)
                            : totalHosts > 0
                                ? const Color(0xFFFBBF24)
                                : Colors.white24,
                      ),
                    ),
                    const SizedBox(width: 9),
                    Expanded(
                      child: Text(
                        totalHosts == 0
                            ? '连接开发机器'
                            : onlineCount > 0
                                ? '$onlineCount 台机器在线'
                                : '机器重连中…',
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(
                          fontSize: 12.5,
                          color: Colors.white70,
                        ),
                      ),
                    ),
                    const Icon(
                      Icons.dns_outlined,
                      size: 15,
                      color: Colors.white38,
                    ),
                  ],
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildThreadTile(
    BuildContext context,
    AgentWithHost item, {
    required bool closeDrawerOnTap,
  }) {
    final ag = item.agent;
    final selected =
        _selectedHostId == item.hostId && _selectedAgentId == ag.agentId;

    return Padding(
      padding: const EdgeInsets.only(bottom: 2),
      child: InkWell(
        borderRadius: BorderRadius.circular(11),
        onTap: () {
          _selectThread(item.hostId, ag.agentId);
          if (closeDrawerOnTap) {
            Navigator.of(context).maybePop();
          }
        },
        onLongPress: () => _confirmArchiveThread(context, item),
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 9),
          decoration: BoxDecoration(
            color: selected
                ? Colors.white.withValues(alpha: 0.08)
                : Colors.transparent,
            borderRadius: BorderRadius.circular(11),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  if (ag.isAwaitingPermission) ...[
                    Container(
                      width: 7,
                      height: 7,
                      decoration: const BoxDecoration(
                        color: Color(0xFFFBBF24),
                        shape: BoxShape.circle,
                      ),
                    ),
                    const SizedBox(width: 7),
                  ] else if (ag.isRunning) ...[
                    Container(
                      width: 7,
                      height: 7,
                      decoration: const BoxDecoration(
                        color: Color(0xFF38BDF8),
                        shape: BoxShape.circle,
                      ),
                    ),
                    const SizedBox(width: 7),
                  ],
                  Expanded(
                    child: Text(
                      ag.title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontSize: 13,
                        fontWeight:
                            selected ? FontWeight.w600 : FontWeight.w400,
                        color: selected ? Colors.white : Colors.white70,
                      ),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 3),
              Row(
                children: [
                  if (item.isWorktree) ...[
                    const Icon(
                      Icons.alt_route_rounded,
                      size: 11,
                      color: Color(0xFF34D399),
                    ),
                    const SizedBox(width: 4),
                  ],
                  Expanded(
                    child: Text(
                      '${ag.projectName} · ${item.hostName}',
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(
                        fontSize: 11,
                        color: Colors.white38,
                      ),
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

  void _confirmArchiveThread(BuildContext context, AgentWithHost item) {
    showModalBottomSheet<void>(
      context: context,
      builder: (ctx) => SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              ListTile(
                leading: const Icon(
                  Icons.delete_outline_rounded,
                  color: Color(0xFFFB7185),
                ),
                title: const Text('归档移除该对话'),
                subtitle: Text(
                  item.agent.title,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
                onTap: () {
                  Navigator.of(ctx).pop();
                  if (_selectedAgentId == item.agent.agentId) {
                    _selectNewChat();
                  }
                  controller.archiveAgent(item.hostId, item.agent.agentId);
                },
              ),
            ],
          ),
        ),
      ),
    );
  }

  // ===========================================================================
  // MAIN CONVERSATION STAGE (Zero-Form New Chat OR Progressive-Disclosure Chat)
  // ===========================================================================

  Widget _buildMainStage(
    BuildContext context, {
    required bool isDesktop,
    required AgentSummary? activeAgent,
    required HostRuntimeSession? activeSession,
  }) {
    final backgroundAttention = controller.allThreadsSorted
        .where(
          (t) =>
              t.agent.isAwaitingPermission &&
              !(t.hostId == _selectedHostId &&
                  t.agent.agentId == _selectedAgentId),
        )
        .firstOrNull;

    final gitStatus = (activeAgent != null && activeSession != null)
        ? activeSession.gitStatuses[activeAgent.cwd]
        : null;
    final changedCount = gitStatus?.files.length ?? 0;
    final hostName = activeSession?.profile.hostname ?? '';

    return Column(
      children: [
        // Ultra-Minimal 48px Header
        SizedBox(
          height: 48,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 14),
            child: Row(
              children: [
                if (!isDesktop) ...[
                  IconButton(
                    visualDensity: VisualDensity.compact,
                    onPressed: () => _scaffoldKey.currentState?.openDrawer(),
                    icon: Badge(
                      isLabelVisible: backgroundAttention != null,
                      backgroundColor: const Color(0xFFFBBF24),
                      child: const Icon(Icons.menu_rounded, size: 21),
                    ),
                  ),
                  const SizedBox(width: 4),
                ],
                Expanded(
                  child: activeAgent != null
                      ? Row(
                          children: [
                            Flexible(
                              child: Text(
                                activeAgent.title,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: const TextStyle(
                                  fontSize: 13.5,
                                  fontWeight: FontWeight.w600,
                                ),
                              ),
                            ),
                            const SizedBox(width: 8),
                            Flexible(
                              child: Text(
                                '${activeAgent.projectName} · $hostName',
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: const TextStyle(
                                  fontSize: 12,
                                  color: Colors.white38,
                                ),
                              ),
                            ),
                          ],
                        )
                      : const Text(
                          '新对话',
                          style: TextStyle(fontSize: 13, color: Colors.white38),
                        ),
                ),

                // Progressive Disclosure: Diff pill ONLY appears when files changed!
                if (activeAgent != null && changedCount > 0) ...[
                  InkWell(
                    borderRadius: BorderRadius.circular(8),
                    onTap: () => _openDiffDrawer(
                      context,
                      _selectedHostId!,
                      activeAgent.cwd,
                    ),
                    child: Container(
                      padding: const EdgeInsets.symmetric(
                        horizontal: 9,
                        vertical: 4.5,
                      ),
                      decoration: BoxDecoration(
                        color:
                            const Color(0xFF34D399).withValues(alpha: 0.12),
                        borderRadius: BorderRadius.circular(8),
                        border: Border.all(
                          color:
                              const Color(0xFF34D399).withValues(alpha: 0.30),
                        ),
                      ),
                      child: Text(
                        '$changedCount 处改动',
                        style: const TextStyle(
                          fontSize: 11.5,
                          fontWeight: FontWeight.w600,
                          color: Color(0xFF34D399),
                        ),
                      ),
                    ),
                  ),
                  const SizedBox(width: 6),
                ],

                if (activeAgent != null)
                  IconButton(
                    visualDensity: VisualDensity.compact,
                    tooltip: '打开当前目录终端',
                    onPressed: () =>
                        setState(() => _terminalOpen = !_terminalOpen),
                    icon: Icon(
                      Icons.terminal_rounded,
                      size: 18,
                      color: _terminalOpen ? Colors.white : Colors.white38,
                    ),
                  ),

                if (!isDesktop && activeAgent != null)
                  IconButton(
                    visualDensity: VisualDensity.compact,
                    tooltip: '新对话',
                    onPressed: _selectNewChat,
                    icon: const Icon(
                      Icons.add_rounded,
                      size: 20,
                      color: Colors.white60,
                    ),
                  ),
              ],
            ),
          ),
        ),

        // Mobile 1-Tap Attention Banner when a background conversation awaits user choice
        if (!isDesktop && backgroundAttention != null)
          Padding(
            padding: const EdgeInsets.fromLTRB(14, 0, 14, 6),
            child: InkWell(
              borderRadius: BorderRadius.circular(12),
              onTap: () => _selectThread(
                backgroundAttention.hostId,
                backgroundAttention.agent.agentId,
              ),
              child: Container(
                padding:
                    const EdgeInsets.symmetric(horizontal: 12, vertical: 9),
                decoration: BoxDecoration(
                  color: const Color(0xFFFBBF24).withValues(alpha: 0.12),
                  borderRadius: BorderRadius.circular(12),
                  border: Border.all(
                    color: const Color(0xFFFBBF24).withValues(alpha: 0.28),
                  ),
                ),
                child: Row(
                  children: [
                    const Icon(
                      Icons.notifications_active_outlined,
                      size: 15,
                      color: Color(0xFFFBBF24),
                    ),
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(
                        '${backgroundAttention.agent.projectName} 正在等待你点选确认',
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(
                          fontSize: 12,
                          color: Color(0xFFFDE68A),
                        ),
                      ),
                    ),
                    const Text(
                      '去处理 →',
                      style: TextStyle(
                        fontSize: 11.5,
                        fontWeight: FontWeight.w600,
                        color: Color(0xFFFBBF24),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),

        // Main Body
        Expanded(
          child: controller.hosts.isEmpty
              ? _ZeroFormWelcomeView(controller: controller)
              : activeAgent == null
                  ? _buildNewChatCanvas(context)
                  : Column(
                      children: [
                        Expanded(
                          child: AgentChatScreen(
                            key: ValueKey(
                              'chat_${_selectedHostId}_$_selectedAgentId',
                            ),
                            controller: controller,
                            hostId: _selectedHostId!,
                            agentId: _selectedAgentId!,
                            embedded: true,
                          ),
                        ),
                        if (_terminalOpen)
                          SizedBox(
                            height: 210,
                            child: TerminalScreen(
                              key: ValueKey(
                                'term_${_selectedHostId}_${activeAgent.cwd}',
                              ),
                              controller: controller,
                              hostId: _selectedHostId!,
                              cwd: activeAgent.cwd,
                              embedded: true,
                              onClose: () =>
                                  setState(() => _terminalOpen = false),
                            ),
                          ),
                      ],
                    ),
        ),
      ],
    );
  }

  Widget _buildNewChatCanvas(BuildContext context) {
    final onlineCount = controller.onlineHostsCount;

    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.fromLTRB(20, 12, 20, 36),
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 680),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const Text(
                '今天想推进哪块代码？',
                textAlign: TextAlign.center,
                style: TextStyle(
                  fontSize: 24,
                  fontWeight: FontWeight.w600,
                  letterSpacing: -0.4,
                ),
              ),
              const SizedBox(height: 8),
              Text(
                _creatingSession
                    ? '正在为你启动对话…'
                    : onlineCount > 0
                        ? '直接打字回车开聊；目录、引擎与协作方式都在输入框左下角一键点选'
                        : '正在重连开发机，也可点击左下角管理机器…',
                textAlign: TextAlign.center,
                style: const TextStyle(
                  fontSize: 13,
                  color: Colors.white38,
                ),
              ),
              const SizedBox(height: 24),
              OrbitChatComposer(
                controller: controller,
                isNewChat: true,
                isRunning: _creatingSession,
                autofocus: true,
                provider: controller.draftProvider,
                mode: controller.draftMode,
                onSelectProvider: controller.setDraftProvider,
                onSelectMode: controller.setDraftMode,
                onSend: _handleStartNewConversation,
                onStop: () => setState(() => _creatingSession = false),
              ),
            ],
          ),
        ),
      ),
    );
  }

  void _openDiffDrawer(BuildContext context, String hostId, String cwd) {
    final isWide = MediaQuery.of(context).size.width >= 720;
    if (isWide) {
      showGeneralDialog<void>(
        context: context,
        barrierDismissible: true,
        barrierLabel: 'Close Diff',
        barrierColor: Colors.black54,
        pageBuilder: (_, __, ___) => Align(
          alignment: Alignment.centerRight,
          child: Material(
            color: const Color(0xFF11151E),
            child: SizedBox(
              width: 580,
              height: double.infinity,
              child: SafeArea(
                child: GitReviewScreen(
                  controller: controller,
                  hostId: hostId,
                  initialCwd: cwd,
                  embedded: true,
                ),
              ),
            ),
          ),
        ),
      );
    } else {
      showModalBottomSheet<void>(
        context: context,
        isScrollControlled: true,
        builder: (_) => FractionallySizedBox(
          heightFactor: 0.85,
          child: GitReviewScreen(
            controller: controller,
            hostId: hostId,
            initialCwd: cwd,
            embedded: true,
          ),
        ),
      );
    }
  }

  void _presentAdaptiveModal(BuildContext context, Widget child) {
    final isWide = MediaQuery.of(context).size.width >= 680;
    if (isWide) {
      showDialog<void>(
        context: context,
        builder: (_) => Dialog(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 480),
            child: child,
          ),
        ),
      );
    } else {
      showModalBottomSheet<void>(
        context: context,
        isScrollControlled: true,
        builder: (_) => SafeArea(child: child),
      );
    }
  }

  void _showMachinesModal(BuildContext context) {
    _presentAdaptiveModal(
      context,
      _MachinesManageSheet(controller: controller),
    );
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
}

// =============================================================================
// ZERO-FORM FIRST-LAUNCH WELCOME VIEW (1-Click SSH Choices + 1-Tap Clipboard)
// =============================================================================

class _ZeroFormWelcomeView extends StatefulWidget {
  const _ZeroFormWelcomeView({required this.controller});
  final FleetController controller;

  @override
  State<_ZeroFormWelcomeView> createState() => _ZeroFormWelcomeViewState();
}

class _ZeroFormWelcomeViewState extends State<_ZeroFormWelcomeView> {
  late final List<SshHostEntry> _sshHosts = SshConfigService.loadUserSshHosts();
  String? _connectingAlias;
  String? _stepStatus;
  String? _errorText;
  bool _showManualFallback = false;
  final TextEditingController _manualCtrl = TextEditingController();

  @override
  void dispose() {
    _manualCtrl.dispose();
    super.dispose();
  }

  Future<void> _connectSsh(SshHostEntry entry) async {
    setState(() {
      _connectingAlias = entry.alias;
      _errorText = null;
      _stepStatus = '正在连接 ${entry.alias}…';
    });
    try {
      await widget.controller.connectSshHostOneClick(
        entry,
        onStep: (s) {
          if (mounted) setState(() => _stepStatus = s);
        },
      );
    } catch (e) {
      if (mounted) {
        setState(() {
          _connectingAlias = null;
          _errorText = '$e';
        });
      }
    }
  }

  Future<void> _importFromClipboard() async {
    setState(() => _errorText = null);
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    final raw = data?.text?.trim() ?? '';
    if (raw.startsWith('orbit://')) {
      try {
        await widget.controller.importFromAnyCode(raw);
        return;
      } catch (e) {
        setState(() => _errorText = '配对码解析失败: $e');
        return;
      }
    }
    setState(() {
      _showManualFallback = true;
      _errorText = '剪贴板中未检测到 orbit:// 配对码，请复制后再试或粘贴在下方';
    });
  }

  @override
  Widget build(BuildContext context) {
    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.all(24),
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 460),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              const Icon(
                Icons.radar_rounded,
                size: 44,
                color: Color(0xFF38BDF8),
              ),
              const SizedBox(height: 16),
              const Text(
                '选择一台开发机器开始对话',
                textAlign: TextAlign.center,
                style: TextStyle(fontSize: 21, fontWeight: FontWeight.w600),
              ),
              const SizedBox(height: 6),
              Text(
                _sshHosts.isNotEmpty
                    ? '已从你的 ~/.ssh/config 自动识别以下机器，点一下即可自动连入'
                    : '复制开发机 `orbit pair` 或电脑端生成的同步码，点下方按钮一键连入',
                textAlign: TextAlign.center,
                style: const TextStyle(fontSize: 13, color: Colors.white54),
              ),
              const SizedBox(height: 20),
              if (_sshHosts.isNotEmpty) ...[
                for (final entry in _sshHosts.take(6))
                  Padding(
                    padding: const EdgeInsets.only(bottom: 8),
                    child: InkWell(
                      borderRadius: BorderRadius.circular(14),
                      onTap: _connectingAlias != null
                          ? null
                          : () => _connectSsh(entry),
                      child: Container(
                        padding: const EdgeInsets.symmetric(
                          horizontal: 14,
                          vertical: 12,
                        ),
                        decoration: BoxDecoration(
                          color: const Color(0xFF151922),
                          borderRadius: BorderRadius.circular(14),
                          border: Border.all(
                            color: Colors.white.withValues(alpha: 0.08),
                          ),
                        ),
                        child: Row(
                          children: [
                            const Icon(
                              Icons.dns_outlined,
                              size: 18,
                              color: Color(0xFF38BDF8),
                            ),
                            const SizedBox(width: 12),
                            Expanded(
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  Text(
                                    entry.alias,
                                    style: const TextStyle(
                                      fontSize: 14,
                                      fontWeight: FontWeight.w600,
                                    ),
                                  ),
                                  Text(
                                    entry.displayTarget,
                                    style: const TextStyle(
                                      fontSize: 11.5,
                                      fontFamily: 'monospace',
                                      color: Colors.white38,
                                    ),
                                  ),
                                ],
                              ),
                            ),
                            if (_connectingAlias == entry.alias)
                              const SizedBox(
                                width: 16,
                                height: 16,
                                child: CircularProgressIndicator(
                                  strokeWidth: 2,
                                ),
                              )
                            else
                              Container(
                                padding: const EdgeInsets.symmetric(
                                  horizontal: 10,
                                  vertical: 5,
                                ),
                                decoration: BoxDecoration(
                                  color: Colors.white.withValues(alpha: 0.08),
                                  borderRadius: BorderRadius.circular(8),
                                ),
                                child: const Text(
                                  '一键接入',
                                  style: TextStyle(
                                    fontSize: 12,
                                    fontWeight: FontWeight.w500,
                                  ),
                                ),
                              ),
                          ],
                        ),
                      ),
                    ),
                  ),
                const SizedBox(height: 8),
              ],
              if (_stepStatus != null && _connectingAlias != null) ...[
                Padding(
                  padding: const EdgeInsets.only(bottom: 10),
                  child: Text(
                    _stepStatus!,
                    textAlign: TextAlign.center,
                    style: const TextStyle(
                      fontSize: 12,
                      color: Color(0xFF38BDF8),
                    ),
                  ),
                ),
              ],
              OutlinedButton.icon(
                style: OutlinedButton.styleFrom(
                  padding: const EdgeInsets.symmetric(vertical: 12),
                  side: BorderSide(color: Colors.white.withValues(alpha: 0.12)),
                  shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(12),
                  ),
                ),
                onPressed: _importFromClipboard,
                icon: const Icon(Icons.content_paste_go_rounded, size: 17),
                label: const Text('从剪贴板一键读取配对码'),
              ),
              if (_showManualFallback) ...[
                const SizedBox(height: 12),
                Row(
                  children: [
                    Expanded(
                      child: Container(
                        padding: const EdgeInsets.symmetric(horizontal: 12),
                        decoration: BoxDecoration(
                          color: const Color(0xFF151922),
                          borderRadius: BorderRadius.circular(10),
                          border: Border.all(
                            color: Colors.white.withValues(alpha: 0.12),
                          ),
                        ),
                        child: TextField(
                          controller: _manualCtrl,
                          style: const TextStyle(
                            fontSize: 12.5,
                            fontFamily: 'monospace',
                          ),
                          decoration: const InputDecoration(
                            hintText: 'orbit://pair/...',
                            border: InputBorder.none,
                            isDense: true,
                          ),
                        ),
                      ),
                    ),
                    const SizedBox(width: 8),
                    FilledButton(
                      onPressed: () async {
                        final txt = _manualCtrl.text.trim();
                        if (txt.isEmpty) return;
                        try {
                          await widget.controller.importFromAnyCode(txt);
                        } catch (e) {
                          setState(() => _errorText = '$e');
                        }
                      },
                      child: const Text('连接'),
                    ),
                  ],
                ),
              ],
              if (_errorText != null) ...[
                const SizedBox(height: 10),
                Text(
                  _errorText!,
                  textAlign: TextAlign.center,
                  style: const TextStyle(
                    fontSize: 12,
                    color: Color(0xFFFBBF24),
                  ),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }
}

// =============================================================================
// ZERO-TYPING MACHINES MANAGE SHEET (1-Click SSH + Fleet Sync + Optional Relay)
// =============================================================================

class _MachinesManageSheet extends StatefulWidget {
  const _MachinesManageSheet({required this.controller});
  final FleetController controller;

  @override
  State<_MachinesManageSheet> createState() => _MachinesManageSheetState();
}

class _MachinesManageSheetState extends State<_MachinesManageSheet> {
  late final List<SshHostEntry> _allSsh = SshConfigService.loadUserSshHosts();
  String? _busyAlias;
  String? _feedback;

  Future<void> _connectCandidate(SshHostEntry entry) async {
    setState(() {
      _busyAlias = entry.alias;
      _feedback = '正在连接 ${entry.alias}…';
    });
    try {
      await widget.controller.connectSshHostOneClick(
        entry,
        onStep: (s) {
          if (mounted) setState(() => _feedback = s);
        },
      );
      if (mounted) {
        setState(() {
          _busyAlias = null;
          _feedback = '已接入 ${entry.alias}';
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _busyAlias = null;
          _feedback = '连接失败: $e';
        });
      }
    }
  }

  Future<void> _deployWanRelayOneClick(
    HostProfile host,
    SshHostEntry relayServer,
  ) async {
    setState(() => _feedback = '正在 ${relayServer.alias} 部署加密中继…');
    try {
      final pubkeys = widget.controller.hosts
          .map((h) => h.daemonPubkeyB64)
          .toSet()
          .toList();
      final relayUrl = await SshConfigService.provisionRelayOverSsh(
        entry: relayServer,
        relayPort: 7878,
        allowedPubkeys: pubkeys,
        onStep: (s) {
          if (mounted) setState(() => _feedback = s);
        },
      );
      await widget.controller.updateHostRelay(host.hostId, relayUrl);
      if (mounted) {
        setState(() => _feedback = '已为 ${host.hostname} 挂载中继 $relayUrl');
      }
    } catch (e) {
      if (mounted) setState(() => _feedback = '部署中继失败: $e');
    }
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: widget.controller,
      builder: (context, _) {
        final connectedHosts = widget.controller.hosts;
        final existingNames = connectedHosts
            .expand((h) => [h.hostname.toLowerCase(), h.sshAlias?.toLowerCase()])
            .whereType<String>()
            .toSet();
        final candidates = _allSsh
            .where((s) => !existingNames.contains(s.alias.toLowerCase()))
            .toList();

        return ConstrainedBox(
          constraints: BoxConstraints(
            maxHeight: MediaQuery.of(context).size.height * 0.82,
          ),
          child: Padding(
            padding: const EdgeInsets.fromLTRB(20, 20, 20, 20),
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Row(
                    children: [
                      const Expanded(
                        child: Text(
                          '我的开发机器',
                          style: TextStyle(
                            fontSize: 16,
                            fontWeight: FontWeight.w600,
                          ),
                        ),
                      ),
                      IconButton(
                        visualDensity: VisualDensity.compact,
                        onPressed: () => Navigator.of(context).pop(),
                        icon: const Icon(Icons.close_rounded, size: 19),
                      ),
                    ],
                  ),
                  const SizedBox(height: 10),
                  for (final h in connectedHosts)
                    Container(
                      margin: const EdgeInsets.only(bottom: 8),
                      padding: const EdgeInsets.symmetric(
                        horizontal: 13,
                        vertical: 10,
                      ),
                      decoration: BoxDecoration(
                        color: Colors.white.withValues(alpha: 0.035),
                        borderRadius: BorderRadius.circular(13),
                        border: Border.all(
                          color: Colors.white.withValues(alpha: 0.07),
                        ),
                      ),
                      child: Row(
                        children: [
                          Container(
                            width: 8,
                            height: 8,
                            decoration: BoxDecoration(
                              shape: BoxShape.circle,
                              color: h.isOnline
                                  ? const Color(0xFF34D399)
                                  : h.routeState == HostRouteState.connecting
                                      ? const Color(0xFFFBBF24)
                                      : const Color(0xFFFB7185),
                            ),
                          ),
                          const SizedBox(width: 10),
                          Expanded(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text(
                                  h.hostname,
                                  style: const TextStyle(
                                    fontSize: 13.5,
                                    fontWeight: FontWeight.w600,
                                  ),
                                ),
                                Text(
                                  switch (h.routeState) {
                                    HostRouteState.onlineDirect =>
                                      h.sshAlias != null ? 'SSH 加密直达' : '直连已就绪',
                                    HostRouteState.onlineRelay => 'E2EE 中继已就绪',
                                    HostRouteState.connecting => '正在连接…',
                                    HostRouteState.disconnected =>
                                      h.lastConnectionError ?? '离线',
                                  },
                                  maxLines: 1,
                                  overflow: TextOverflow.ellipsis,
                                  style: const TextStyle(
                                    fontSize: 11,
                                    color: Colors.white54,
                                  ),
                                ),
                              ],
                            ),
                          ),
                          PopupMenuButton<String>(
                            icon: const Icon(
                              Icons.more_horiz_rounded,
                              size: 18,
                              color: Colors.white54,
                            ),
                            onSelected: (val) async {
                              if (val == 'reconnect') {
                                await widget.controller.connectHost(h.hostId);
                              } else if (val == 'remove') {
                                await widget.controller.removeHost(h.hostId);
                              } else if (val.startsWith('relay:')) {
                                final alias = val.substring(6);
                                final entry = _allSsh
                                    .where((x) => x.alias == alias)
                                    .firstOrNull;
                                if (entry != null) {
                                  await _deployWanRelayOneClick(h, entry);
                                }
                              }
                            },
                            itemBuilder: (_) => [
                              const PopupMenuItem(
                                value: 'reconnect',
                                child: Text('重新连接'),
                              ),
                              for (final s in _allSsh.take(4))
                                PopupMenuItem(
                                  value: 'relay:${s.alias}',
                                  child: Text('经 ${s.alias} 一键部署手机外网中继'),
                                ),
                              const PopupMenuItem(
                                value: 'remove',
                                child: Text('移除该机器'),
                              ),
                            ],
                          ),
                        ],
                      ),
                    ),
                  if (candidates.isNotEmpty) ...[
                    const SizedBox(height: 8),
                    const Text(
                      '发现的可接入机器 (~/.ssh/config)',
                      style: TextStyle(
                        fontSize: 11.5,
                        fontWeight: FontWeight.w600,
                        color: Colors.white54,
                      ),
                    ),
                    const SizedBox(height: 8),
                    for (final c in candidates.take(6))
                      Container(
                        margin: const EdgeInsets.only(bottom: 7),
                        padding: const EdgeInsets.symmetric(
                          horizontal: 13,
                          vertical: 9,
                        ),
                        decoration: BoxDecoration(
                          color: Colors.white.withValues(alpha: 0.02),
                          borderRadius: BorderRadius.circular(12),
                          border: Border.all(
                            color: Colors.white.withValues(alpha: 0.06),
                          ),
                        ),
                        child: Row(
                          children: [
                            Expanded(
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  Text(
                                    c.alias,
                                    style: const TextStyle(
                                      fontSize: 13,
                                      fontWeight: FontWeight.w600,
                                    ),
                                  ),
                                  Text(
                                    c.displayTarget,
                                    style: const TextStyle(
                                      fontSize: 11,
                                      fontFamily: 'monospace',
                                      color: Colors.white38,
                                    ),
                                  ),
                                ],
                              ),
                            ),
                            TextButton(
                              onPressed: _busyAlias != null
                                  ? null
                                  : () => _connectCandidate(c),
                              child: _busyAlias == c.alias
                                  ? const SizedBox(
                                      width: 14,
                                      height: 14,
                                      child: CircularProgressIndicator(
                                        strokeWidth: 2,
                                      ),
                                    )
                                  : const Text('一键接入'),
                            ),
                          ],
                        ),
                      ),
                  ],
                  if (_feedback != null) ...[
                    const SizedBox(height: 6),
                    Text(
                      _feedback!,
                      style: const TextStyle(
                        fontSize: 12,
                        color: Color(0xFF38BDF8),
                      ),
                    ),
                  ],
                  const SizedBox(height: 12),
                  Row(
                    children: [
                      Expanded(
                        child: OutlinedButton.icon(
                          onPressed: () async {
                            final clip =
                                await Clipboard.getData(Clipboard.kTextPlain);
                            final raw = clip?.text?.trim() ?? '';
                            if (!raw.startsWith('orbit://')) {
                              setState(
                                () => _feedback =
                                    '请先复制 orbit:// 配对码或同步码再点击',
                              );
                              return;
                            }
                            try {
                              final n =
                                  await widget.controller.importFromAnyCode(raw);
                              setState(() => _feedback = '已成功导入 $n 台机器');
                            } catch (e) {
                              setState(() => _feedback = '导入失败: $e');
                            }
                          },
                          icon: const Icon(
                            Icons.content_paste_go_rounded,
                            size: 16,
                          ),
                          label: const Text(
                            '剪贴板一键接入',
                            style: TextStyle(fontSize: 12.5),
                          ),
                        ),
                      ),
                      if (connectedHosts.isNotEmpty) ...[
                        const SizedBox(width: 8),
                        Expanded(
                          child: FilledButton.tonalIcon(
                            onPressed: () async {
                              final syncUri =
                                  widget.controller.exportFleetSyncUri();
                              if (syncUri == null) return;
                              await Clipboard.setData(
                                ClipboardData(text: syncUri),
                              );
                              setState(
                                () => _feedback =
                                    '已复制全舰队同步码，发到手机后点「剪贴板一键接入」即可同步全部机器',
                              );
                            },
                            icon: const Icon(Icons.devices_rounded, size: 16),
                            label: const Text(
                              '复制手机同步码',
                              style: TextStyle(fontSize: 12.5),
                            ),
                          ),
                        ),
                      ],
                    ],
                  ),
                ],
              ),
            ),
          ),
        );
      },
    );
  }
}

// =============================================================================
// 1-CLICK SELF-UPDATE SHEET
// =============================================================================

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
      if (mounted) setState(() => _updating = false);
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
      padding: const EdgeInsets.fromLTRB(22, 20, 22, 24),
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
                  style: TextStyle(fontSize: 16.5, fontWeight: FontWeight.w600),
                ),
              ),
              if (!_updating)
                IconButton(
                  icon: const Icon(Icons.refresh_rounded, size: 18),
                  onPressed: _checking ? null : _checkNow,
                ),
            ],
          ),
          const SizedBox(height: 8),
          Text(
            '当前版本: v$kOrbitAppVersion   ·   最新 Release: ${rel?.tagName ?? (_checking ? "检查中…" : "未知")}',
            style: const TextStyle(fontSize: 12.5, color: Colors.white60),
          ),
          if (_checking && rel == null) ...[
            const SizedBox(height: 16),
            const LinearProgressIndicator(),
          ],
          if (_errorText != null) ...[
            const SizedBox(height: 10),
            Text(
              _errorText!,
              style: const TextStyle(color: Color(0xFFFB7185), fontSize: 12),
            ),
          ],
          if (_updating) ...[
            const SizedBox(height: 14),
            LinearProgressIndicator(value: _progress),
            const SizedBox(height: 8),
            Text(
              _statusText ?? '正在更新…',
              style: const TextStyle(fontSize: 12, color: Colors.white70),
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
                size: 17,
              ),
              label: Text(
                rel.hasNewerVersion
                    ? '一键更新并自动重启 (${rel.tagName})'
                    : '已是最新版本 (${rel.tagName}) · 重新安装',
              ),
            ),
          ],
        ],
      ),
    );
  }
}
