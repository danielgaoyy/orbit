import 'package:flutter/material.dart';

import '../core/fleet_controller.dart';
import '../core/models.dart';
import 'agent_chat_screen.dart';
import 'git_review_screen.dart';
import 'terminal_screen.dart';

class HostWorkspaceScreen extends StatelessWidget {
  const HostWorkspaceScreen({
    super.key,
    required this.controller,
    required this.hostId,
  });

  final FleetController controller;
  final String hostId;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        final session = controller.sessionFor(hostId);
        final agents = session?.agents.values.toList() ?? const [];
        final defaultCwd = (session != null && session.workspaces.isNotEmpty)
            ? session.workspaces.first.cwd
            : '~';

        return Scaffold(
          appBar: AppBar(
            title: Text(session?.profile.hostname ?? hostId),
            actions: [
              IconButton(
                tooltip: 'Provider Quotas (5h / Weekly)',
                icon: const Icon(Icons.donut_large_rounded),
                onPressed: () {
                  controller.refreshQuotas(hostId);
                  _showQuotasSheet(context);
                },
              ),
              IconButton(
                tooltip: 'Git Working Tree & Diff',
                icon: const Icon(Icons.difference_rounded),
                onPressed: () {
                  Navigator.of(context).push(
                    MaterialPageRoute<void>(
                      builder: (_) => GitReviewScreen(
                        controller: controller,
                        hostId: hostId,
                        initialCwd: defaultCwd,
                      ),
                    ),
                  );
                },
              ),
              IconButton(
                tooltip: 'Detached PTY Terminal',
                icon: const Icon(Icons.terminal_rounded),
                onPressed: () {
                  Navigator.of(context).push(
                    MaterialPageRoute<void>(
                      builder: (_) => TerminalScreen(
                        controller: controller,
                        hostId: hostId,
                        cwd: defaultCwd,
                      ),
                    ),
                  );
                },
              ),
            ],
          ),
          body: session == null
              ? const Center(child: Text('Host currently offline.'))
              : agents.isEmpty
                  ? const Center(
                      child: Text(
                        'No active sessions yet. Tap + New Agent below!',
                        style: TextStyle(color: Colors.white60),
                      ),
                    )
                  : ListView.separated(
                      padding: const EdgeInsets.all(14),
                      itemCount: agents.length,
                      separatorBuilder: (_, __) => const SizedBox(height: 10),
                      itemBuilder: (context, i) =>
                          _buildAgentTile(context, agents[i]),
                    ),
          floatingActionButton: FloatingActionButton.extended(
            onPressed: () => _showNewAgentSheet(context, defaultCwd),
            icon: const Icon(Icons.rocket_launch_rounded),
            label: const Text('New Agent'),
          ),
        );
      },
    );
  }

  Widget _buildAgentTile(BuildContext context, AgentSummary agent) {
    final statusColor = switch (agent.statusTag) {
      'running' || 'autonomous_wakeup' => const Color(0xFF38BDF8),
      'awaiting_permission' => Colors.amberAccent,
      'error' => Colors.redAccent,
      _ => Colors.greenAccent,
    };

    return Card(
      child: ListTile(
        contentPadding:
            const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
        title: Text(
          agent.title,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: const TextStyle(fontWeight: FontWeight.w600),
        ),
        subtitle: Padding(
          padding: const EdgeInsets.only(top: 6),
          child: Text(
            '${agent.provider.label} · ${agent.mode.label} · ${agent.cwd}',
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: const TextStyle(fontSize: 12, color: Colors.white60),
          ),
        ),
        trailing: Container(
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
          decoration: BoxDecoration(
            color: statusColor.withValues(alpha: 0.15),
            borderRadius: BorderRadius.circular(999),
          ),
          child: Text(
            agent.statusTag,
            style: TextStyle(
              color: statusColor,
              fontSize: 11,
              fontWeight: FontWeight.bold,
            ),
          ),
        ),
        onTap: () {
          controller.openAgentTimeline(hostId, agent.agentId);
          Navigator.of(context).push(
            MaterialPageRoute<void>(
              builder: (_) => AgentChatScreen(
                controller: controller,
                hostId: hostId,
                agentId: agent.agentId,
              ),
            ),
          );
        },
      ),
    );
  }

  void _showNewAgentSheet(BuildContext context, String initialCwd) {
    var selectedProvider = ProviderKind.antigravity;
    var selectedMode = SessionMode.acceptEdits;
    final cwdCtrl = TextEditingController(
      text: initialCwd == '~' ? '/tmp' : initialCwd,
    );
    final branchCtrl = TextEditingController();
    final promptCtrl = TextEditingController();

    showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setModalState) => Padding(
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
                'Launch Coding Agent',
                style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
              ),
              const SizedBox(height: 12),
              SegmentedButton<ProviderKind>(
                segments: const [
                  ButtonSegment(
                    value: ProviderKind.antigravity,
                    label: Text('Antigravity'),
                  ),
                  ButtonSegment(
                    value: ProviderKind.codex,
                    label: Text('Codex'),
                  ),
                  ButtonSegment(
                    value: ProviderKind.claudeCode,
                    label: Text('Claude'),
                  ),
                ],
                selected: {selectedProvider},
                onSelectionChanged: (s) =>
                    setModalState(() => selectedProvider = s.first),
              ),
              const SizedBox(height: 12),
              DropdownButtonFormField<SessionMode>(
                isExpanded: true,
                initialValue: selectedMode,
                decoration: const InputDecoration(
                  labelText: 'Permission Mode',
                  border: OutlineInputBorder(),
                ),
                items: [
                  for (final m in SessionMode.values)
                    DropdownMenuItem(value: m, child: Text(m.label)),
                ],
                onChanged: (v) {
                  if (v != null) setModalState(() => selectedMode = v);
                },
              ),
              const SizedBox(height: 10),
              TextField(
                controller: cwdCtrl,
                decoration: const InputDecoration(
                  labelText: 'Workspace Directory (cwd)',
                  border: OutlineInputBorder(),
                ),
              ),
              const SizedBox(height: 10),
              TextField(
                controller: branchCtrl,
                decoration: const InputDecoration(
                  labelText: 'Optional Isolated Git Worktree Branch',
                  hintText: 'feat/mobile-refactor (leave blank for in-place)',
                  border: OutlineInputBorder(),
                ),
              ),
              const SizedBox(height: 10),
              TextField(
                controller: promptCtrl,
                minLines: 2,
                maxLines: 4,
                decoration: const InputDecoration(
                  labelText: 'Initial Task Prompt',
                  border: OutlineInputBorder(),
                ),
              ),
              const SizedBox(height: 16),
              FilledButton.icon(
                onPressed: () {
                  final prompt = promptCtrl.text.trim();
                  if (prompt.isEmpty) return;
                  controller.createAgent(
                    hostId: hostId,
                    cwd: cwdCtrl.text.trim(),
                    provider: selectedProvider,
                    mode: selectedMode,
                    prompt: prompt,
                    worktreeBranch: branchCtrl.text.trim().isEmpty
                        ? null
                        : branchCtrl.text.trim(),
                  );
                  Navigator.of(ctx).pop();
                },
                icon: const Icon(Icons.play_arrow_rounded),
                label: const Text('Start Session'),
              ),
            ],
          ),
        ),
      ),
    );
  }

  void _showQuotasSheet(BuildContext context) {
    showModalBottomSheet<void>(
      context: context,
      builder: (_) => ListenableBuilder(
        listenable: controller,
        builder: (context, _) {
          final quotas = controller.sessionFor(hostId)?.quotas ?? const [];
          return Padding(
            padding: const EdgeInsets.all(20),
            child: quotas.isEmpty
                ? const Center(child: CircularProgressIndicator())
                : ListView(
                    shrinkWrap: true,
                    children: [
                      const Text(
                        'Real-Time Provider Quotas',
                        style: TextStyle(
                            fontSize: 18, fontWeight: FontWeight.bold),
                      ),
                      const SizedBox(height: 12),
                      for (final q in quotas) ...[
                        Text(
                          '${q.provider.label} ${q.accountLabel != null ? "(${q.accountLabel})" : ""}',
                          style: const TextStyle(fontWeight: FontWeight.w700),
                        ),
                        if (q.error != null)
                          Text(
                            q.error!,
                            style: const TextStyle(
                                fontSize: 12, color: Colors.white54),
                          ),
                        for (final w in q.windows)
                          Padding(
                            padding: const EdgeInsets.symmetric(vertical: 6),
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Row(
                                  mainAxisAlignment:
                                      MainAxisAlignment.spaceBetween,
                                  children: [
                                    Expanded(
                                      child: Text(
                                        w.label,
                                        style: const TextStyle(fontSize: 12),
                                      ),
                                    ),
                                    Text(
                                      '${w.usedPercent.toStringAsFixed(1)}%',
                                      style: const TextStyle(
                                        fontSize: 12,
                                        fontWeight: FontWeight.bold,
                                      ),
                                    ),
                                  ],
                                ),
                                const SizedBox(height: 4),
                                LinearProgressIndicator(
                                  value: (w.usedPercent / 100.0).clamp(0.0, 1.0),
                                  color: w.usedPercent > 90
                                      ? Colors.redAccent
                                      : w.usedPercent >= 70
                                          ? Colors.amberAccent
                                          : const Color(0xFF38BDF8),
                                ),
                              ],
                            ),
                          ),
                        if (q.creditsRemaining != null)
                          Text(
                            'Credits Remaining: ${q.creditsRemaining!.toStringAsFixed(1)}',
                            style: const TextStyle(
                                fontSize: 12, color: Colors.greenAccent),
                          ),
                        const Divider(height: 20),
                      ],
                    ],
                  ),
          );
        },
      ),
    );
  }
}
