import 'package:flutter/material.dart';

import '../core/fleet_controller.dart';
import '../core/models.dart';

class AgentChatScreen extends StatefulWidget {
  const AgentChatScreen({
    super.key,
    required this.controller,
    required this.hostId,
    required this.agentId,
  });

  final FleetController controller;
  final String hostId;
  final String agentId;

  @override
  State<AgentChatScreen> createState() => _AgentChatScreenState();
}

class _AgentChatScreenState extends State<AgentChatScreen> {
  final _inputCtrl = TextEditingController();
  final _scrollCtrl = ScrollController();

  @override
  void dispose() {
    _inputCtrl.dispose();
    _scrollCtrl.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: widget.controller,
      builder: (context, _) {
        final session = widget.controller.sessionFor(widget.hostId);
        final agent = session?.agents[widget.agentId];
        final events = session?.timelines[widget.agentId] ?? const [];

        final resolvedPermIds = <String>{
          for (final e in events)
            if (e.type == 'permission_resolved')
              e.payload['request_id'] as String? ?? '',
        };
        final resolvedQuestionIds = <String>{
          for (final e in events)
            if (e.type == 'question_resolved')
              e.payload['request_id'] as String? ?? '',
        };

        return Scaffold(
          appBar: AppBar(
            title: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  agent?.title ?? widget.agentId,
                  style: const TextStyle(fontSize: 15),
                ),
                Text(
                  '${agent?.provider.label ?? ""} · ${agent?.statusTag ?? "idle"}',
                  style: const TextStyle(fontSize: 11, color: Colors.white60),
                ),
              ],
            ),
            actions: [
              if (agent != null)
                PopupMenuButton<SessionMode>(
                  tooltip: 'Switch Permission Mode',
                  icon: const Icon(Icons.shield_outlined),
                  onSelected: (m) => widget.controller
                      .setSessionMode(widget.hostId, widget.agentId, m),
                  itemBuilder: (_) => [
                    for (final m in SessionMode.values)
                      PopupMenuItem(
                        value: m,
                        child: Text(
                          m == agent.mode ? '✓ ${m.label}' : m.label,
                        ),
                      ),
                  ],
                ),
              if (agent != null && agent.isRunning)
                IconButton(
                  tooltip: 'Interrupt Active Turn',
                  icon: const Icon(Icons.stop_circle_rounded,
                      color: Colors.redAccent),
                  onPressed: () => widget.controller
                      .interruptAgent(widget.hostId, widget.agentId),
                ),
            ],
          ),
          body: Column(
            children: [
              Expanded(
                child: events.isEmpty
                    ? const Center(
                        child: Text(
                          'Waiting for timeline stream…',
                          style: TextStyle(color: Colors.white54),
                        ),
                      )
                    : ListView.builder(
                        controller: _scrollCtrl,
                        padding: const EdgeInsets.all(14),
                        itemCount: events.length,
                        itemBuilder: (context, i) => _buildEventItem(
                          context,
                          events[i],
                          resolvedPermIds,
                          resolvedQuestionIds,
                        ),
                      ),
              ),
              _buildComposer(),
            ],
          ),
        );
      },
    );
  }

  Widget _buildEventItem(
    BuildContext context,
    TimelineEvent ev,
    Set<String> resolvedPerms,
    Set<String> resolvedQuestions,
  ) {
    final p = ev.payload;
    switch (ev.type) {
      case 'user_prompt':
        return Align(
          alignment: Alignment.centerRight,
          child: Container(
            margin: const EdgeInsets.symmetric(vertical: 6),
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
            constraints: const BoxConstraints(maxWidth: 320),
            decoration: BoxDecoration(
              color: const Color(0xFF1E3A5F),
              borderRadius: BorderRadius.circular(14),
            ),
            child: Text(
              (p['text'] as String?) ?? '',
              style: const TextStyle(fontSize: 14),
            ),
          ),
        );

      case 'assistant_thinking':
        return Card(
          margin: const EdgeInsets.symmetric(vertical: 4),
          child: ExpansionTile(
            dense: true,
            leading: const Icon(Icons.psychology_rounded,
                size: 18, color: Colors.white54),
            title: const Text(
              'Reasoning Trace',
              style: TextStyle(fontSize: 12, color: Colors.white70),
            ),
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
                child: Text(
                  (p['text'] as String?) ?? '',
                  style: const TextStyle(
                    fontSize: 12,
                    color: Colors.white60,
                    fontFamily: 'monospace',
                  ),
                ),
              ),
            ],
          ),
        );

      case 'assistant_message':
        final streaming = (p['streaming'] as bool?) ?? false;
        return Container(
          margin: const EdgeInsets.symmetric(vertical: 6),
          padding: const EdgeInsets.all(14),
          decoration: BoxDecoration(
            color: const Color(0xFF131822),
            borderRadius: BorderRadius.circular(14),
            border: Border.all(
              color: streaming
                  ? const Color(0xFF38BDF8).withValues(alpha: 0.5)
                  : const Color(0xFF1E293B),
            ),
          ),
          child: SelectableText(
            (p['text'] as String?) ?? '',
            style: const TextStyle(fontSize: 14, height: 1.45),
          ),
        );

      case 'tool_call':
        final status = (p['status'] as String?) ?? 'running';
        final detail =
            Map<String, dynamic>.from(p['detail'] as Map? ?? const {});
        final kind = (detail['kind'] as String?) ?? 'generic';
        final summary = switch (kind) {
          'shell' => '\$ ${detail["command"] ?? ""}',
          'file_edit' => 'Edit ${detail["path"] ?? ""}',
          'file_read' => 'Read ${detail["path"] ?? ""}',
          'search' => 'Search `${detail["query"] ?? ""}`',
          'subagent' => 'Subagent (${detail["role"] ?? "worker"})',
          _ => (detail['tool_name'] as String?) ?? 'tool',
        };
        final subOutput =
            (detail['output_preview'] ?? detail['unified_diff']) as String?;

        return Card(
          margin: const EdgeInsets.symmetric(vertical: 4),
          child: Padding(
            padding: const EdgeInsets.all(12),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Icon(
                      status == 'completed'
                          ? Icons.check_circle_outline
                          : status == 'failed'
                              ? Icons.error_outline
                              : Icons.autorenew_rounded,
                      size: 16,
                      color: status == 'completed'
                          ? Colors.greenAccent
                          : status == 'failed'
                              ? Colors.redAccent
                              : const Color(0xFF38BDF8),
                    ),
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(
                        summary,
                        style: const TextStyle(
                          fontSize: 12,
                          fontFamily: 'monospace',
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                    ),
                  ],
                ),
                if (subOutput != null && subOutput.trim().isNotEmpty) ...[
                  const SizedBox(height: 8),
                  Container(
                    width: double.infinity,
                    padding: const EdgeInsets.all(8),
                    decoration: BoxDecoration(
                      color: Colors.black38,
                      borderRadius: BorderRadius.circular(8),
                    ),
                    child: Text(
                      subOutput,
                      maxLines: 8,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(
                        fontSize: 11,
                        fontFamily: 'monospace',
                        color: Colors.white70,
                      ),
                    ),
                  ),
                ],
              ],
            ),
          ),
        );

      case 'permission_request':
        final reqId = (p['request_id'] as String?) ?? '';
        final resolved = resolvedPerms.contains(reqId);
        return Container(
          margin: const EdgeInsets.symmetric(vertical: 8),
          padding: const EdgeInsets.all(14),
          decoration: BoxDecoration(
            color: const Color(0xFF282012),
            borderRadius: BorderRadius.circular(14),
            border: Border.all(color: Colors.amber.shade600),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  const Icon(Icons.gpp_maybe_rounded,
                      color: Colors.amberAccent, size: 20),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      (p['title'] as String?) ?? 'Permission Required',
                      style: const TextStyle(fontWeight: FontWeight.bold),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 12),
              if (resolved)
                const Text(
                  'Decision submitted.',
                  style: TextStyle(fontSize: 12, color: Colors.white60),
                )
              else
                Wrap(
                  spacing: 8,
                  children: [
                    FilledButton(
                      onPressed: () => widget.controller.resolvePermission(
                        widget.hostId,
                        widget.agentId,
                        reqId,
                        'allow',
                      ),
                      child: const Text('Allow Once'),
                    ),
                    OutlinedButton(
                      onPressed: () => widget.controller.resolvePermission(
                        widget.hostId,
                        widget.agentId,
                        reqId,
                        'allow_always',
                      ),
                      child: const Text('Always Allow'),
                    ),
                    TextButton(
                      onPressed: () => widget.controller.resolvePermission(
                        widget.hostId,
                        widget.agentId,
                        reqId,
                        'deny',
                      ),
                      child: const Text(
                        'Deny',
                        style: TextStyle(color: Colors.redAccent),
                      ),
                    ),
                  ],
                ),
            ],
          ),
        );

      case 'question_request':
        final reqId = (p['request_id'] as String?) ?? '';
        final resolved = resolvedQuestions.contains(reqId);
        final rawQs = (p['questions'] as List<dynamic>? ?? [])
            .map((q) => QuestionItem.fromJson(q as Map<String, dynamic>))
            .toList();
        return _QuestionPromptCard(
          requestId: reqId,
          questions: rawQs,
          resolved: resolved,
          onSubmit: (answers) => widget.controller.answerQuestion(
            widget.hostId,
            widget.agentId,
            reqId,
            answers,
          ),
        );

      default:
        return const SizedBox.shrink();
    }
  }

  Widget _buildComposer() {
    return SafeArea(
      top: false,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(12, 8, 12, 12),
        child: Row(
          children: [
            Expanded(
              child: TextField(
                controller: _inputCtrl,
                minLines: 1,
                maxLines: 4,
                decoration: InputDecoration(
                  hintText: 'Message or steer active turn…',
                  filled: true,
                  fillColor: const Color(0xFF131822),
                  contentPadding:
                      const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
                  border: OutlineInputBorder(
                    borderRadius: BorderRadius.circular(14),
                  ),
                ),
              ),
            ),
            const SizedBox(width: 8),
            IconButton.filled(
              icon: const Icon(Icons.arrow_upward_rounded),
              onPressed: () {
                final txt = _inputCtrl.text.trim();
                if (txt.isEmpty) return;
                _inputCtrl.clear();
                widget.controller
                    .sendPrompt(widget.hostId, widget.agentId, txt);
              },
            ),
          ],
        ),
      ),
    );
  }
}

class _QuestionPromptCard extends StatefulWidget {
  const _QuestionPromptCard({
    required this.requestId,
    required this.questions,
    required this.resolved,
    required this.onSubmit,
  });

  final String requestId;
  final List<QuestionItem> questions;
  final bool resolved;
  final ValueChanged<List<List<String>>> onSubmit;

  @override
  State<_QuestionPromptCard> createState() => _QuestionPromptCardState();
}

class _QuestionPromptCardState extends State<_QuestionPromptCard> {
  late final List<Set<String>> _selected = [
    for (final q in widget.questions)
      if (q.options.isNotEmpty) {q.options.first} else <String>{},
  ];

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: const EdgeInsets.symmetric(vertical: 8),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: const Color(0xFF172033),
        borderRadius: BorderRadius.circular(14),
        border: Border.all(color: const Color(0xFF38BDF8)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          for (var i = 0; i < widget.questions.length; i++) ...[
            Text(
              widget.questions[i].question,
              style: const TextStyle(fontWeight: FontWeight.w600),
            ),
            const SizedBox(height: 8),
            Wrap(
              spacing: 8,
              runSpacing: 6,
              children: [
                for (final opt in widget.questions[i].options)
                  FilterChip(
                    label: Text(opt, style: const TextStyle(fontSize: 12)),
                    selected: _selected[i].contains(opt),
                    onSelected: widget.resolved
                        ? null
                        : (val) {
                            setState(() {
                              if (widget.questions[i].isMultiSelect) {
                                if (val) {
                                  _selected[i].add(opt);
                                } else {
                                  _selected[i].remove(opt);
                                }
                              } else {
                                _selected[i]
                                  ..clear()
                                  ..add(opt);
                              }
                            });
                          },
                  ),
              ],
            ),
            const SizedBox(height: 12),
          ],
          if (widget.resolved)
            const Text(
              'Answers submitted.',
              style: TextStyle(fontSize: 12, color: Colors.white60),
            )
          else
            FilledButton.icon(
              onPressed: () =>
                  widget.onSubmit(_selected.map((s) => s.toList()).toList()),
              icon: const Icon(Icons.check_rounded, size: 18),
              label: const Text('Submit Answers'),
            ),
        ],
      ),
    );
  }
}
