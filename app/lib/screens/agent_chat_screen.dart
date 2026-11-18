import 'package:flutter/material.dart';

import '../core/fleet_controller.dart';
import '../core/models.dart';
import '../widgets/orbit_chat_composer.dart';

class AgentChatScreen extends StatefulWidget {
  const AgentChatScreen({
    super.key,
    required this.controller,
    required this.hostId,
    required this.agentId,
    this.embedded = false,
  });

  final FleetController controller;
  final String hostId;
  final String agentId;
  final bool embedded;

  @override
  State<AgentChatScreen> createState() => _AgentChatScreenState();
}

sealed class _RenderEntry {}

class _SingleEventEntry extends _RenderEntry {
  _SingleEventEntry(this.event);
  final TimelineEvent event;
}

class _ToolGroupEntry extends _RenderEntry {
  _ToolGroupEntry(this.events);
  final List<TimelineEvent> events;
}

class _AgentChatScreenState extends State<AgentChatScreen> {
  final _scrollCtrl = ScrollController();
  int _lastEventCount = 0;

  @override
  void dispose() {
    _scrollCtrl.dispose();
    super.dispose();
  }

  List<_RenderEntry> _coalesceDisplayItems(List<TimelineEvent> raw) {
    final result = <_RenderEntry>[];
    var pendingTools = <TimelineEvent>[];

    void flushTools() {
      if (pendingTools.isNotEmpty) {
        result.add(_ToolGroupEntry(List.of(pendingTools)));
        pendingTools = <TimelineEvent>[];
      }
    }

    for (final ev in raw) {
      if (ev.type == 'tool_call') {
        pendingTools.add(ev);
        continue;
      }
      if (ev.type == 'status_changed' ||
          ev.type == 'permission_resolved' ||
          ev.type == 'question_resolved') {
        continue;
      }
      flushTools();
      result.add(_SingleEventEntry(ev));
    }
    flushTools();
    return result;
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: widget.controller,
      builder: (context, _) {
        final session = widget.controller.sessionFor(widget.hostId);
        final agent = session?.agents[widget.agentId];
        final events = session?.timelines[widget.agentId] ?? const [];

        if (events.length != _lastEventCount) {
          _lastEventCount = events.length;
          WidgetsBinding.instance.addPostFrameCallback((_) {
            if (_scrollCtrl.hasClients) {
              _scrollCtrl.animateTo(
                _scrollCtrl.position.maxScrollExtent,
                duration: const Duration(milliseconds: 180),
                curve: Curves.easeOut,
              );
            }
          });
        }

        final resolvedPermIds = <String>{
          for (final e in events)
            if (e.type == 'permission_resolved')
              e.payload['request_id'] as String? ?? '',
        };
        final resolvedQuestionMap = <String, List<dynamic>>{
          for (final e in events)
            if (e.type == 'question_resolved')
              (e.payload['request_id'] as String? ?? ''):
                  (e.payload['answers'] as List<dynamic>? ?? const []),
        };

        final displayItems = _coalesceDisplayItems(events);

        final bodyContent = Column(
          children: [
            Expanded(
              child: displayItems.isEmpty
                  ? Center(
                      child: Text(
                        agent?.isRunning == true ? '正在思考与准备上下文…' : '等待对话消息…',
                        style: const TextStyle(
                          fontSize: 13,
                          color: Colors.white38,
                        ),
                      ),
                    )
                  : ListView.builder(
                      controller: _scrollCtrl,
                      padding: const EdgeInsets.fromLTRB(20, 16, 20, 20),
                      itemCount: displayItems.length,
                      itemBuilder: (context, i) => Center(
                        child: ConstrainedBox(
                          constraints: const BoxConstraints(maxWidth: 740),
                          child: _buildDisplayEntry(
                            context,
                            displayItems[i],
                            resolvedPermIds,
                            resolvedQuestionMap,
                          ),
                        ),
                      ),
                    ),
            ),
            SafeArea(
              top: false,
              child: Center(
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 760),
                  child: Padding(
                    padding: const EdgeInsets.fromLTRB(16, 4, 16, 14),
                    child: OrbitChatComposer(
                      controller: widget.controller,
                      isNewChat: false,
                      isRunning: agent?.isRunning ?? false,
                      activeHostId: widget.hostId,
                      provider:
                          agent?.provider ?? widget.controller.draftProvider,
                      mode: agent?.mode ?? widget.controller.draftMode,
                      onSelectProvider: (p) => widget.controller
                          .switchAgentProvider(widget.hostId, widget.agentId, p),
                      onSelectMode: (m) => widget.controller
                          .setSessionMode(widget.hostId, widget.agentId, m),
                      onSend: (txt) => widget.controller
                          .sendPrompt(widget.hostId, widget.agentId, txt),
                      onStop: () => widget.controller
                          .interruptAgent(widget.hostId, widget.agentId),
                    ),
                  ),
                ),
              ),
            ),
          ],
        );

        if (widget.embedded) return bodyContent;

        return Scaffold(
          appBar: AppBar(
            title: Text(
              agent?.title ?? widget.agentId,
              style: const TextStyle(fontSize: 15),
            ),
          ),
          body: bodyContent,
        );
      },
    );
  }

  Widget _buildDisplayEntry(
    BuildContext context,
    _RenderEntry entry,
    Set<String> resolvedPerms,
    Map<String, List<dynamic>> resolvedQuestions,
  ) {
    if (entry is _ToolGroupEntry) {
      return _ToolGroupTraceWidget(events: entry.events);
    }

    final ev = (entry as _SingleEventEntry).event;
    final p = ev.payload;

    switch (ev.type) {
      case 'user_prompt':
        return Align(
          alignment: Alignment.centerRight,
          child: Container(
            margin: const EdgeInsets.symmetric(vertical: 8),
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 11),
            constraints: const BoxConstraints(maxWidth: 560),
            decoration: BoxDecoration(
              color: const Color(0xFF1C2230),
              borderRadius: BorderRadius.circular(18),
            ),
            child: SelectableText(
              (p['text'] as String?) ?? '',
              style: const TextStyle(
                fontSize: 14.5,
                height: 1.48,
                color: Color(0xFFF3F4F6),
              ),
            ),
          ),
        );

      case 'assistant_thinking':
        return _ThinkingTraceWidget(text: (p['text'] as String?) ?? '');

      case 'assistant_message':
        return Padding(
          padding: const EdgeInsets.symmetric(vertical: 8),
          child: SizedBox(
            width: double.infinity,
            child: SelectableText(
              (p['text'] as String?) ?? '',
              style: const TextStyle(
                fontSize: 14.5,
                height: 1.65,
                color: Color(0xFFE6E9EF),
              ),
            ),
          ),
        );

      case 'notice':
        return Padding(
          padding: const EdgeInsets.symmetric(vertical: 6),
          child: Center(
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
              decoration: BoxDecoration(
                color: Colors.white.withValues(alpha: 0.04),
                borderRadius: BorderRadius.circular(999),
              ),
              child: Text(
                (p['message'] as String?) ?? '',
                style: const TextStyle(fontSize: 11.5, color: Colors.white54),
              ),
            ),
          ),
        );

      case 'permission_request':
        final reqId = (p['request_id'] as String?) ?? '';
        final resolved = resolvedPerms.contains(reqId);
        final detail =
            Map<String, dynamic>.from(p['detail'] as Map? ?? const {});
        final cmdPreview = detail['command']?.toString() ??
            detail['path']?.toString() ??
            detail['tool_name']?.toString();

        return Container(
          margin: const EdgeInsets.symmetric(vertical: 10),
          padding: const EdgeInsets.all(16),
          decoration: BoxDecoration(
            color: const Color(0xFF191714),
            borderRadius: BorderRadius.circular(16),
            border: Border.all(
              color: const Color(0xFFFBBF24).withValues(alpha: 0.30),
            ),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  const Icon(
                    Icons.gpp_maybe_rounded,
                    color: Color(0xFFFBBF24),
                    size: 18,
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      (p['title'] as String?) ?? '需要确认操作权限',
                      style: const TextStyle(
                        fontSize: 13.5,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ),
                ],
              ),
              if (cmdPreview != null && cmdPreview.isNotEmpty) ...[
                const SizedBox(height: 10),
                Container(
                  width: double.infinity,
                  padding:
                      const EdgeInsets.symmetric(horizontal: 12, vertical: 9),
                  decoration: BoxDecoration(
                    color: Colors.black.withValues(alpha: 0.38),
                    borderRadius: BorderRadius.circular(10),
                    border: Border.all(
                      color: Colors.white.withValues(alpha: 0.06),
                    ),
                  ),
                  child: SelectableText(
                    cmdPreview,
                    style: const TextStyle(
                      fontSize: 12,
                      fontFamily: 'monospace',
                      color: Color(0xFFFDE68A),
                    ),
                  ),
                ),
              ],
              const SizedBox(height: 12),
              if (resolved)
                const Row(
                  children: [
                    Icon(
                      Icons.check_circle_outline_rounded,
                      size: 15,
                      color: Color(0xFF34D399),
                    ),
                    SizedBox(width: 6),
                    Text(
                      '已提交确认结果',
                      style: TextStyle(fontSize: 12, color: Colors.white54),
                    ),
                  ],
                )
              else
                Wrap(
                  spacing: 8,
                  runSpacing: 8,
                  children: [
                    FilledButton(
                      style: FilledButton.styleFrom(
                        backgroundColor: const Color(0xFFF3F4F6),
                        foregroundColor: const Color(0xFF0B0D11),
                        visualDensity: VisualDensity.compact,
                      ),
                      onPressed: () => widget.controller.resolvePermission(
                        widget.hostId,
                        widget.agentId,
                        reqId,
                        'allow',
                      ),
                      child: const Text(
                        '允许执行',
                        style: TextStyle(fontWeight: FontWeight.w600),
                      ),
                    ),
                    OutlinedButton(
                      style: OutlinedButton.styleFrom(
                        visualDensity: VisualDensity.compact,
                        side: BorderSide(
                          color: Colors.white.withValues(alpha: 0.14),
                        ),
                      ),
                      onPressed: () => widget.controller.resolvePermission(
                        widget.hostId,
                        widget.agentId,
                        reqId,
                        'allow_always',
                      ),
                      child: const Text('本对话不再询问'),
                    ),
                    TextButton(
                      style: TextButton.styleFrom(
                        visualDensity: VisualDensity.compact,
                      ),
                      onPressed: () => widget.controller.resolvePermission(
                        widget.hostId,
                        widget.agentId,
                        reqId,
                        'deny',
                      ),
                      child: const Text(
                        '拒绝',
                        style: TextStyle(color: Color(0xFFFB7185)),
                      ),
                    ),
                  ],
                ),
            ],
          ),
        );

      case 'question_request':
        final reqId = (p['request_id'] as String?) ?? '';
        final resolvedAns = resolvedQuestions[reqId];
        final rawQs = (p['questions'] as List<dynamic>? ?? [])
            .map((q) => QuestionItem.fromJson(q as Map<String, dynamic>))
            .toList();
        return _QuestionChoiceCard(
          requestId: reqId,
          questions: rawQs,
          resolvedAnswers: resolvedAns,
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
}

class _ThinkingTraceWidget extends StatefulWidget {
  const _ThinkingTraceWidget({required this.text});
  final String text;

  @override
  State<_ThinkingTraceWidget> createState() => _ThinkingTraceWidgetState();
}

class _ThinkingTraceWidgetState extends State<_ThinkingTraceWidget> {
  bool _open = false;

  @override
  Widget build(BuildContext context) {
    if (widget.text.trim().isEmpty) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          InkWell(
            borderRadius: BorderRadius.circular(6),
            onTap: () => setState(() => _open = !_open),
            child: Padding(
              padding: const EdgeInsets.symmetric(vertical: 3),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Icon(
                    _open
                        ? Icons.keyboard_arrow_down_rounded
                        : Icons.keyboard_arrow_right_rounded,
                    size: 16,
                    color: Colors.white38,
                  ),
                  const SizedBox(width: 4),
                  const Text(
                    '思考过程',
                    style: TextStyle(fontSize: 12, color: Colors.white38),
                  ),
                ],
              ),
            ),
          ),
          if (_open)
            Container(
              margin: const EdgeInsets.only(left: 8, top: 4),
              padding: const EdgeInsets.only(left: 12),
              decoration: BoxDecoration(
                border: Border(
                  left: BorderSide(color: Colors.white.withValues(alpha: 0.10)),
                ),
              ),
              child: SelectableText(
                widget.text,
                style: const TextStyle(
                  fontSize: 12,
                  height: 1.5,
                  color: Colors.white54,
                ),
              ),
            ),
        ],
      ),
    );
  }
}

class _ToolGroupTraceWidget extends StatefulWidget {
  const _ToolGroupTraceWidget({required this.events});
  final List<TimelineEvent> events;

  @override
  State<_ToolGroupTraceWidget> createState() => _ToolGroupTraceWidgetState();
}

class _ToolGroupTraceWidgetState extends State<_ToolGroupTraceWidget> {
  bool _open = false;

  String _summarizeOne(TimelineEvent ev) {
    final p = ev.payload;
    final detail = Map<String, dynamic>.from(p['detail'] as Map? ?? const {});
    final kind = (detail['kind'] as String?) ?? 'generic';
    return switch (kind) {
      'shell' => '\$ ${detail["command"] ?? ""}',
      'file_edit' => '修改 ${detail["path"] ?? ""}',
      'file_read' => '读取 ${detail["path"] ?? ""}',
      'search' => '检索 ${detail["query"] ?? ""}',
      'subagent' => '子代理 (${detail["role"] ?? "worker"})',
      _ => (detail['tool_name'] as String?) ?? '调用工具',
    };
  }

  @override
  Widget build(BuildContext context) {
    final anyRunning =
        widget.events.any((e) => (e.payload['status'] ?? '') == 'running');
    final headerText = widget.events.length == 1
        ? _summarizeOne(widget.events.first)
        : anyRunning
            ? '正在执行第 ${widget.events.length} 步工具操作…'
            : '已查阅或修改 ${widget.events.length} 处代码与命令';

    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          InkWell(
            borderRadius: BorderRadius.circular(6),
            onTap: () => setState(() => _open = !_open),
            child: Padding(
              padding: const EdgeInsets.symmetric(vertical: 3),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  if (anyRunning)
                    const Padding(
                      padding: EdgeInsets.only(right: 6),
                      child: SizedBox(
                        width: 12,
                        height: 12,
                        child: CircularProgressIndicator(
                          strokeWidth: 1.6,
                          color: Color(0xFF38BDF8),
                        ),
                      ),
                    )
                  else
                    Icon(
                      _open
                          ? Icons.keyboard_arrow_down_rounded
                          : Icons.keyboard_arrow_right_rounded,
                      size: 16,
                      color: Colors.white38,
                    ),
                  const SizedBox(width: 4),
                  Flexible(
                    child: Text(
                      headerText,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(
                        fontSize: 12,
                        color: Colors.white54,
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
          if (_open)
            Container(
              margin: const EdgeInsets.only(left: 8, top: 4),
              padding: const EdgeInsets.only(left: 12),
              decoration: BoxDecoration(
                border: Border(
                  left: BorderSide(color: Colors.white.withValues(alpha: 0.10)),
                ),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  for (final ev in widget.events)
                    Padding(
                      padding: const EdgeInsets.symmetric(vertical: 2.5),
                      child: Text(
                        _summarizeOne(ev),
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(
                          fontSize: 11.5,
                          fontFamily: 'monospace',
                          color: Colors.white54,
                        ),
                      ),
                    ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}

/// Pure Multiple-Choice Question Card:
/// - For single-question single-select: 1 tap on any option submits immediately!
/// - Custom thoughts can simply be typed in the main chat box below (zero nested form boxes!).
class _QuestionChoiceCard extends StatefulWidget {
  const _QuestionChoiceCard({
    required this.requestId,
    required this.questions,
    required this.resolvedAnswers,
    required this.onSubmit,
  });

  final String requestId;
  final List<QuestionItem> questions;
  final List<dynamic>? resolvedAnswers;
  final ValueChanged<List<List<String>>> onSubmit;

  @override
  State<_QuestionChoiceCard> createState() => _QuestionChoiceCardState();
}

class _QuestionChoiceCardState extends State<_QuestionChoiceCard> {
  late final List<Set<String>> _selected = [
    for (final q in widget.questions)
      if (q.options.isNotEmpty) {q.options.first} else <String>{},
  ];

  bool get _isInstantSingleChoice =>
      widget.questions.length == 1 && !widget.questions.first.isMultiSelect;

  @override
  Widget build(BuildContext context) {
    final isResolved = widget.resolvedAnswers != null;

    return Container(
      margin: const EdgeInsets.symmetric(vertical: 10),
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        color: const Color(0xFF141923),
        borderRadius: BorderRadius.circular(16),
        border: Border.all(
          color: const Color(0xFF38BDF8).withValues(alpha: 0.28),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          for (var i = 0; i < widget.questions.length; i++) ...[
            Text(
              widget.questions[i].question,
              style: const TextStyle(
                fontSize: 14,
                fontWeight: FontWeight.w600,
              ),
            ),
            const SizedBox(height: 10),
            if (!isResolved)
              for (final opt in widget.questions[i].options)
                Padding(
                  padding: const EdgeInsets.only(bottom: 7),
                  child: InkWell(
                    borderRadius: BorderRadius.circular(12),
                    onTap: () {
                      if (_isInstantSingleChoice) {
                        widget.onSubmit([
                          [opt],
                        ]);
                        return;
                      }
                      setState(() {
                        if (widget.questions[i].isMultiSelect) {
                          if (_selected[i].contains(opt)) {
                            _selected[i].remove(opt);
                          } else {
                            _selected[i].add(opt);
                          }
                        } else {
                          _selected[i]
                            ..clear()
                            ..add(opt);
                        }
                      });
                    },
                    child: Container(
                      width: double.infinity,
                      padding: const EdgeInsets.symmetric(
                        horizontal: 13,
                        vertical: 11,
                      ),
                      decoration: BoxDecoration(
                        color: (!_isInstantSingleChoice &&
                                _selected[i].contains(opt))
                            ? const Color(0xFF38BDF8).withValues(alpha: 0.16)
                            : Colors.white.withValues(alpha: 0.035),
                        borderRadius: BorderRadius.circular(12),
                        border: Border.all(
                          color: (!_isInstantSingleChoice &&
                                  _selected[i].contains(opt))
                              ? const Color(0xFF38BDF8).withValues(alpha: 0.5)
                              : Colors.white.withValues(alpha: 0.08),
                        ),
                      ),
                      child: Row(
                        children: [
                          Expanded(
                            child: Text(
                              opt,
                              style: const TextStyle(
                                fontSize: 13,
                                height: 1.35,
                              ),
                            ),
                          ),
                          if (!_isInstantSingleChoice &&
                              _selected[i].contains(opt))
                            const Icon(
                              Icons.check_rounded,
                              size: 17,
                              color: Color(0xFF38BDF8),
                            ),
                        ],
                      ),
                    ),
                  ),
                ),
            if (i < widget.questions.length - 1) const SizedBox(height: 8),
          ],
          if (isResolved)
            Row(
              children: [
                const Icon(
                  Icons.check_circle_outline_rounded,
                  size: 15,
                  color: Color(0xFF38BDF8),
                ),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                    '已选择：${widget.resolvedAnswers!.expand((e) => e is List ? e : [e]).join(" · ")}',
                    style: const TextStyle(
                      fontSize: 12.5,
                      color: Color(0xFF7DD3FC),
                    ),
                  ),
                ),
              ],
            )
          else ...[
            if (!_isInstantSingleChoice) ...[
              const SizedBox(height: 6),
              FilledButton(
                style: FilledButton.styleFrom(
                  backgroundColor: const Color(0xFFF3F4F6),
                  foregroundColor: const Color(0xFF0B0D11),
                  visualDensity: VisualDensity.compact,
                ),
                onPressed: () => widget.onSubmit(
                  _selected.map((s) => s.toList()).toList(),
                ),
                child: const Text(
                  '确认选择',
                  style: TextStyle(fontWeight: FontWeight.w600),
                ),
              ),
            ],
            const SizedBox(height: 4),
            const Text(
              '点选任一选项即刻生效；若有其他想法，直接在下方对话框输入即可。',
              style: TextStyle(fontSize: 11, color: Colors.white38),
            ),
          ],
        ],
      ),
    );
  }
}
