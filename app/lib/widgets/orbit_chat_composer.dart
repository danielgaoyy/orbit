import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../core/fleet_controller.dart';
import '../core/models.dart';

/// Unified zero-form Chat Composer shared by both "New Conversation" and "Active Conversation".
///
/// - Zero text forms: Project, Engine (with live remaining quota), Mode, and Worktree isolation
///   live as minimal inline ghost pills along the bottom edge of the input box.
/// - Adaptive density: On compact screens (`< 600px`), Engine + Mode fold into a single pill.
/// - Morphing primary button: `↑` Send when typing, `■` Stop when agent is running with empty input.
class OrbitChatComposer extends StatefulWidget {
  const OrbitChatComposer({
    super.key,
    required this.controller,
    required this.isNewChat,
    required this.isRunning,
    required this.provider,
    required this.mode,
    this.activeHostId,
    this.autofocus = false,
    required this.onSelectProvider,
    required this.onSelectMode,
    required this.onSend,
    required this.onStop,
  });

  final FleetController controller;
  final bool isNewChat;
  final bool isRunning;
  final ProviderKind provider;
  final SessionMode mode;
  final String? activeHostId;
  final bool autofocus;
  final ValueChanged<ProviderKind> onSelectProvider;
  final ValueChanged<SessionMode> onSelectMode;
  final ValueChanged<String> onSend;
  final VoidCallback onStop;

  @override
  State<OrbitChatComposer> createState() => _OrbitChatComposerState();
}

class _OrbitChatComposerState extends State<OrbitChatComposer> {
  final TextEditingController _textCtrl = TextEditingController();
  late final FocusNode _focusNode = FocusNode(onKeyEvent: _handleKeyEvent);
  bool _focused = false;

  @override
  void initState() {
    super.initState();
    _textCtrl.addListener(_onTextChanged);
    _focusNode.addListener(() {
      if (mounted) {
        setState(() => _focused = _focusNode.hasFocus);
      }
    });
  }

  void _onTextChanged() {
    if (mounted) setState(() {});
  }

  @override
  void dispose() {
    _textCtrl.removeListener(_onTextChanged);
    _textCtrl.dispose();
    _focusNode.dispose();
    super.dispose();
  }

  KeyEventResult _handleKeyEvent(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent) return KeyEventResult.ignored;
    final isEnter = event.logicalKey == LogicalKeyboardKey.enter ||
        event.logicalKey == LogicalKeyboardKey.numpadEnter;
    if (!isEnter) return KeyEventResult.ignored;

    // Respect macOS/Linux CJK IME active Pinyin composition!
    final composing = _textCtrl.value.composing;
    if (composing.isValid && !composing.isCollapsed) {
      return KeyEventResult.ignored;
    }

    if (HardwareKeyboard.instance.isShiftPressed) {
      return KeyEventResult.ignored;
    }

    _submit();
    return KeyEventResult.handled;
  }

  void _submit() {
    final trimmed = _textCtrl.text.trim();
    if (trimmed.isEmpty) {
      if (widget.isRunning) {
        widget.onStop();
      }
      return;
    }
    _textCtrl.clear();
    widget.onSend(trimmed);
    _focusNode.requestFocus();
  }

  IconData _modeIcon(SessionMode m) {
    return switch (m) {
      SessionMode.acceptEdits => Icons.bolt_rounded,
      SessionMode.plan => Icons.explore_outlined,
      SessionMode.fullAccess => Icons.rocket_launch_outlined,
      SessionMode.defaultMode => Icons.shield_outlined,
    };
  }

  Color _modeColor(SessionMode m) {
    return switch (m) {
      SessionMode.acceptEdits => const Color(0xFF38BDF8),
      SessionMode.plan => const Color(0xFFFBBF24),
      SessionMode.fullAccess => const Color(0xFF34D399),
      SessionMode.defaultMode => Colors.white70,
    };
  }

  void _presentChoiceSurface(BuildContext context, Widget child) {
    final isWide = MediaQuery.of(context).size.width >= 680;
    if (isWide) {
      showDialog<void>(
        context: context,
        builder: (_) => Dialog(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 420),
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

  void _openProjectPicker(BuildContext context) {
    _presentChoiceSurface(
      context,
      ListenableBuilder(
        listenable: widget.controller,
        builder: (ctx, _) {
          final choices = widget.controller.allProjectChoices;
          final current = widget.controller.activeDraftProject;
          final useWorktree = widget.controller.draftUseWorktree;

          return Padding(
            padding: const EdgeInsets.fromLTRB(16, 18, 16, 16),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                const Padding(
                  padding: EdgeInsets.symmetric(horizontal: 6),
                  child: Text(
                    '选择代码目录',
                    style: TextStyle(
                      fontSize: 15,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ),
                const SizedBox(height: 4),
                const Padding(
                  padding: EdgeInsets.symmetric(horizontal: 6),
                  child: Text(
                    '已自动发现已连机器上的活跃仓库与目录，点选即切',
                    style: TextStyle(fontSize: 12, color: Colors.white54),
                  ),
                ),
                const SizedBox(height: 12),
                ConstrainedBox(
                  constraints: const BoxConstraints(maxHeight: 300),
                  child: ListView.separated(
                    shrinkWrap: true,
                    itemCount: choices.length,
                    separatorBuilder: (_, __) => const SizedBox(height: 4),
                    itemBuilder: (_, i) {
                      final c = choices[i];
                      final selected = current?.key == c.key;
                      return InkWell(
                        borderRadius: BorderRadius.circular(12),
                        onTap: () {
                          widget.controller.selectDraftProject(c);
                          Navigator.of(ctx).pop();
                          _focusNode.requestFocus();
                        },
                        child: Container(
                          padding: const EdgeInsets.symmetric(
                            horizontal: 12,
                            vertical: 10,
                          ),
                          decoration: BoxDecoration(
                            color: selected
                                ? const Color(0xFF38BDF8)
                                    .withValues(alpha: 0.14)
                                : Colors.white.withValues(alpha: 0.025),
                            borderRadius: BorderRadius.circular(12),
                          ),
                          child: Row(
                            children: [
                              Icon(
                                Icons.folder_outlined,
                                size: 18,
                                color: selected
                                    ? const Color(0xFF38BDF8)
                                    : Colors.white54,
                              ),
                              const SizedBox(width: 10),
                              Expanded(
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Row(
                                      children: [
                                        Flexible(
                                          child: Text(
                                            c.name,
                                            maxLines: 1,
                                            overflow: TextOverflow.ellipsis,
                                            style: const TextStyle(
                                              fontSize: 13.5,
                                              fontWeight: FontWeight.w600,
                                            ),
                                          ),
                                        ),
                                        const SizedBox(width: 8),
                                        Container(
                                          padding: const EdgeInsets.symmetric(
                                            horizontal: 6,
                                            vertical: 1.5,
                                          ),
                                          decoration: BoxDecoration(
                                            color: Colors.white
                                                .withValues(alpha: 0.07),
                                            borderRadius:
                                                BorderRadius.circular(5),
                                          ),
                                          child: Text(
                                            c.hostName,
                                            style: const TextStyle(
                                              fontSize: 10.5,
                                              color: Colors.white70,
                                            ),
                                          ),
                                        ),
                                      ],
                                    ),
                                    const SizedBox(height: 2),
                                    Text(
                                      c.gitBranch != null
                                          ? '${c.displayPath} · ${c.gitBranch}'
                                          : c.displayPath,
                                      maxLines: 1,
                                      overflow: TextOverflow.ellipsis,
                                      style: const TextStyle(
                                        fontSize: 11.5,
                                        fontFamily: 'monospace',
                                        color: Colors.white38,
                                      ),
                                    ),
                                  ],
                                ),
                              ),
                              if (selected)
                                const Icon(
                                  Icons.check_rounded,
                                  size: 18,
                                  color: Color(0xFF38BDF8),
                                ),
                            ],
                          ),
                        ),
                      );
                    },
                  ),
                ),
                const SizedBox(height: 10),
                Divider(
                  height: 1,
                  color: Colors.white.withValues(alpha: 0.08),
                ),
                const SizedBox(height: 8),
                InkWell(
                  borderRadius: BorderRadius.circular(12),
                  onTap: () =>
                      widget.controller.setDraftWorktree(!useWorktree),
                  child: Padding(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 8,
                      vertical: 6,
                    ),
                    child: Row(
                      children: [
                        Icon(
                          Icons.alt_route_rounded,
                          size: 17,
                          color: useWorktree
                              ? const Color(0xFF34D399)
                              : Colors.white54,
                        ),
                        const SizedBox(width: 10),
                        const Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(
                                '独立分支隔离 (Worktree)',
                                style: TextStyle(
                                  fontSize: 13,
                                  fontWeight: FontWeight.w500,
                                ),
                              ),
                              Text(
                                '开启后按首句自动生成隔离分支，不改动主目录',
                                style: TextStyle(
                                  fontSize: 11,
                                  color: Colors.white38,
                                ),
                              ),
                            ],
                          ),
                        ),
                        Switch.adaptive(
                          value: useWorktree,
                          activeColor: const Color(0xFF34D399),
                          onChanged: widget.controller.setDraftWorktree,
                        ),
                      ],
                    ),
                  ),
                ),
              ],
            ),
          );
        },
      ),
    );
  }

  void _openProviderPicker(BuildContext context) {
    final hostId =
        widget.activeHostId ?? widget.controller.activeDraftProject?.hostId;
    if (hostId != null) {
      widget.controller.refreshQuotas(hostId);
    }

    _presentChoiceSurface(
      context,
      ListenableBuilder(
        listenable: widget.controller,
        builder: (ctx, _) => Padding(
          padding: const EdgeInsets.fromLTRB(16, 18, 16, 18),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              const Padding(
                padding: EdgeInsets.symmetric(horizontal: 6),
                child: Text(
                  '选择编程引擎',
                  style: TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
                ),
              ),
              const SizedBox(height: 4),
              const Padding(
                padding: EdgeInsets.symmetric(horizontal: 6),
                child: Text(
                  '对话中途也可随时切换引擎接力推进',
                  style: TextStyle(fontSize: 12, color: Colors.white54),
                ),
              ),
              const SizedBox(height: 12),
              for (final p in ProviderKind.values) ...[
                _buildProviderOptionTile(ctx, p, hostId),
                const SizedBox(height: 4),
              ],
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildProviderOptionTile(
    BuildContext ctx,
    ProviderKind p,
    String? hostId,
  ) {
    final selected = p == widget.provider;
    final quota = widget.controller.quotaFor(hostId, p);
    final rem = quota?.minRemainingPercent;
    final low = rem != null && rem <= 25;

    return InkWell(
      borderRadius: BorderRadius.circular(12),
      onTap: () {
        widget.onSelectProvider(p);
        Navigator.of(ctx).pop();
        _focusNode.requestFocus();
      },
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 11),
        decoration: BoxDecoration(
          color: selected
              ? const Color(0xFF38BDF8).withValues(alpha: 0.14)
              : Colors.white.withValues(alpha: 0.025),
          borderRadius: BorderRadius.circular(12),
        ),
        child: Row(
          children: [
            Icon(
              Icons.auto_awesome_rounded,
              size: 17,
              color: selected ? const Color(0xFF38BDF8) : Colors.white54,
            ),
            const SizedBox(width: 10),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Text(
                        p.label,
                        style: const TextStyle(
                          fontSize: 13.5,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                      const Spacer(),
                      if (rem != null)
                        Container(
                          padding: const EdgeInsets.symmetric(
                            horizontal: 7,
                            vertical: 2,
                          ),
                          decoration: BoxDecoration(
                            color: (low
                                    ? const Color(0xFFFBBF24)
                                    : const Color(0xFF34D399))
                                .withValues(alpha: 0.14),
                            borderRadius: BorderRadius.circular(6),
                          ),
                          child: Text(
                            '余 $rem%',
                            style: TextStyle(
                              fontSize: 11,
                              fontWeight: FontWeight.w600,
                              color: low
                                  ? const Color(0xFFFBBF24)
                                  : const Color(0xFF34D399),
                            ),
                          ),
                        ),
                    ],
                  ),
                  const SizedBox(height: 2),
                  Text(
                    p.subtitle,
                    style: const TextStyle(fontSize: 11.5, color: Colors.white54),
                  ),
                ],
              ),
            ),
            if (selected) ...[
              const SizedBox(width: 8),
              const Icon(
                Icons.check_rounded,
                size: 18,
                color: Color(0xFF38BDF8),
              ),
            ],
          ],
        ),
      ),
    );
  }

  void _openModePicker(BuildContext context) {
    _presentChoiceSurface(
      context,
      Builder(
        builder: (ctx) => Padding(
          padding: const EdgeInsets.fromLTRB(16, 18, 16, 18),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              const Padding(
                padding: EdgeInsets.symmetric(horizontal: 6),
                child: Text(
                  '协作方式',
                  style: TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
                ),
              ),
              const SizedBox(height: 4),
              const Padding(
                padding: EdgeInsets.symmetric(horizontal: 6),
                child: Text(
                  '聊到一半也可随时从「先出方案」切到「自动改码」',
                  style: TextStyle(fontSize: 12, color: Colors.white54),
                ),
              ),
              const SizedBox(height: 12),
              for (final m in SessionMode.primaryChoices) ...[
                InkWell(
                  borderRadius: BorderRadius.circular(12),
                  onTap: () {
                    widget.onSelectMode(m);
                    Navigator.of(ctx).pop();
                    _focusNode.requestFocus();
                  },
                  child: Container(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 12,
                      vertical: 11,
                    ),
                    decoration: BoxDecoration(
                      color: m == widget.mode
                          ? const Color(0xFF38BDF8).withValues(alpha: 0.14)
                          : Colors.white.withValues(alpha: 0.025),
                      borderRadius: BorderRadius.circular(12),
                    ),
                    child: Row(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Padding(
                          padding: const EdgeInsets.only(top: 2),
                          child: Icon(_modeIcon(m), size: 17, color: _modeColor(m)),
                        ),
                        const SizedBox(width: 10),
                        Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(
                                m.label,
                                style: const TextStyle(
                                  fontSize: 13.5,
                                  fontWeight: FontWeight.w600,
                                ),
                              ),
                              const SizedBox(height: 2),
                              Text(
                                m.description,
                                style: const TextStyle(
                                  fontSize: 11.5,
                                  color: Colors.white54,
                                  height: 1.35,
                                ),
                              ),
                            ],
                          ),
                        ),
                        if (m == widget.mode)
                          const Icon(
                            Icons.check_rounded,
                            size: 18,
                            color: Color(0xFF38BDF8),
                          ),
                      ],
                    ),
                  ),
                ),
                const SizedBox(height: 4),
              ],
            ],
          ),
        ),
      ),
    );
  }

  void _openCombinedCompactSheet(BuildContext context) {
    final hostId =
        widget.activeHostId ?? widget.controller.activeDraftProject?.hostId;
    if (hostId != null) {
      widget.controller.refreshQuotas(hostId);
    }

    showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      builder: (ctx) => SafeArea(
        child: ListenableBuilder(
          listenable: widget.controller,
          builder: (_, __) => Padding(
            padding: const EdgeInsets.fromLTRB(18, 18, 18, 20),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                const Text(
                  '引擎与协作方式',
                  style: TextStyle(fontSize: 15.5, fontWeight: FontWeight.w600),
                ),
                const SizedBox(height: 12),
                Row(
                  children: [
                    for (final p in ProviderKind.values)
                      Expanded(
                        child: Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 3),
                          child: InkWell(
                            borderRadius: BorderRadius.circular(12),
                            onTap: () => widget.onSelectProvider(p),
                            child: Container(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 10,
                                vertical: 10,
                              ),
                              decoration: BoxDecoration(
                                color: p == widget.provider
                                    ? const Color(0xFF38BDF8)
                                        .withValues(alpha: 0.16)
                                    : Colors.white.withValues(alpha: 0.04),
                                borderRadius: BorderRadius.circular(12),
                                border: Border.all(
                                  color: p == widget.provider
                                      ? const Color(0xFF38BDF8)
                                          .withValues(alpha: 0.5)
                                      : Colors.white.withValues(alpha: 0.07),
                                ),
                              ),
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  Text(
                                    p.shortLabel,
                                    maxLines: 1,
                                    overflow: TextOverflow.ellipsis,
                                    style: const TextStyle(
                                      fontSize: 12.5,
                                      fontWeight: FontWeight.w600,
                                    ),
                                  ),
                                  const SizedBox(height: 2),
                                  Text(
                                    widget.controller
                                                .quotaFor(hostId, p)
                                                ?.minRemainingPercent !=
                                            null
                                        ? '余 ${widget.controller.quotaFor(hostId, p)!.minRemainingPercent}%'
                                        : '就绪',
                                    style: const TextStyle(
                                      fontSize: 10.5,
                                      color: Colors.white54,
                                    ),
                                  ),
                                ],
                              ),
                            ),
                          ),
                        ),
                      ),
                  ],
                ),
                const SizedBox(height: 14),
                for (final m in SessionMode.primaryChoices) ...[
                  InkWell(
                    borderRadius: BorderRadius.circular(12),
                    onTap: () {
                      widget.onSelectMode(m);
                      Navigator.of(ctx).pop();
                    },
                    child: Container(
                      padding: const EdgeInsets.symmetric(
                        horizontal: 12,
                        vertical: 10,
                      ),
                      decoration: BoxDecoration(
                        color: m == widget.mode
                            ? const Color(0xFF38BDF8).withValues(alpha: 0.14)
                            : Colors.white.withValues(alpha: 0.03),
                        borderRadius: BorderRadius.circular(12),
                      ),
                      child: Row(
                        children: [
                          Icon(_modeIcon(m), size: 17, color: _modeColor(m)),
                          const SizedBox(width: 10),
                          Expanded(
                            child: Text(
                              m.label,
                              style: const TextStyle(
                                fontSize: 13.5,
                                fontWeight: FontWeight.w500,
                              ),
                            ),
                          ),
                          if (m == widget.mode)
                            const Icon(
                              Icons.check_rounded,
                              size: 18,
                              color: Color(0xFF38BDF8),
                            ),
                        ],
                      ),
                    ),
                  ),
                  const SizedBox(height: 6),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final compact = constraints.maxWidth < 520;
        final draftProj = widget.controller.activeDraftProject;
        final useWorktree = widget.controller.draftUseWorktree;
        final hasText = _textCtrl.text.trim().isNotEmpty;

        final placeholder = widget.isNewChat
            ? (draftProj != null
                ? '在 ${draftProj.name} 中直接输入你想做的事…'
                : '直接输入你想推进的任务…')
            : (widget.isRunning
                ? '正在执行中，随时输入可插话引导方向…'
                : '继续对话…');

        return AnimatedContainer(
          duration: const Duration(milliseconds: 140),
          decoration: BoxDecoration(
            color: const Color(0xFF151922),
            borderRadius: BorderRadius.circular(20),
            border: Border.all(
              color: _focused
                  ? Colors.white.withValues(alpha: 0.22)
                  : Colors.white.withValues(alpha: 0.09),
            ),
            boxShadow: [
              BoxShadow(
                color: Colors.black.withValues(alpha: 0.28),
                blurRadius: 18,
                offset: const Offset(0, 6),
              ),
            ],
          ),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              TextField(
                controller: _textCtrl,
                focusNode: _focusNode,
                autofocus: widget.autofocus,
                minLines: widget.isNewChat ? 2 : 1,
                maxLines: 6,
                style: const TextStyle(
                  fontSize: 14.5,
                  height: 1.48,
                  color: Color(0xFFF3F4F6),
                ),
                decoration: InputDecoration(
                  hintText: placeholder,
                  hintStyle: TextStyle(
                    fontSize: 14.5,
                    color: Colors.white.withValues(alpha: 0.34),
                  ),
                  contentPadding: const EdgeInsets.fromLTRB(16, 14, 16, 8),
                  border: InputBorder.none,
                  isDense: true,
                ),
              ),
              Padding(
                padding: const EdgeInsets.fromLTRB(10, 2, 10, 10),
                child: Row(
                  children: [
                    Expanded(
                      child: Wrap(
                        spacing: 4,
                        runSpacing: 4,
                        crossAxisAlignment: WrapCrossAlignment.center,
                        children: [
                          if (widget.isNewChat && draftProj != null)
                            _GhostPillButton(
                              icon: useWorktree
                                  ? Icons.alt_route_rounded
                                  : Icons.folder_outlined,
                              iconColor: useWorktree
                                  ? const Color(0xFF34D399)
                                  : Colors.white60,
                              label: '${draftProj.name} · ${draftProj.hostName}',
                              maxWidth: compact ? 155 : 220,
                              onTap: () => _openProjectPicker(context),
                            ),
                          if (compact)
                            _GhostPillButton(
                              icon: Icons.auto_awesome_rounded,
                              iconColor: const Color(0xFF38BDF8),
                              label:
                                  '${widget.provider.shortLabel} · ${widget.mode.shortLabel}',
                              onTap: () => _openCombinedCompactSheet(context),
                            )
                          else ...[
                            _GhostPillButton(
                              icon: Icons.auto_awesome_rounded,
                              iconColor: const Color(0xFF38BDF8),
                              label: widget.provider.label,
                              onTap: () => _openProviderPicker(context),
                            ),
                            _GhostPillButton(
                              icon: _modeIcon(widget.mode),
                              iconColor: _modeColor(widget.mode),
                              label: widget.mode.label,
                              onTap: () => _openModePicker(context),
                            ),
                          ],
                        ],
                      ),
                    ),
                    const SizedBox(width: 8),
                    if (widget.isRunning && hasText) ...[
                      IconButton(
                        tooltip: '停止当前回合',
                        visualDensity: VisualDensity.compact,
                        onPressed: widget.onStop,
                        icon: const Icon(
                          Icons.stop_rounded,
                          size: 18,
                          color: Colors.white70,
                        ),
                      ),
                      const SizedBox(width: 2),
                    ],
                    _SendOrStopButton(
                      hasText: hasText,
                      isRunning: widget.isRunning,
                      onPressed: _submit,
                    ),
                  ],
                ),
              ),
            ],
          ),
        );
      },
    );
  }
}

class _GhostPillButton extends StatelessWidget {
  const _GhostPillButton({
    required this.icon,
    required this.iconColor,
    required this.label,
    required this.onTap,
    this.maxWidth = 180,
  });

  final IconData icon;
  final Color iconColor;
  final String label;
  final VoidCallback onTap;
  final double maxWidth;

  @override
  Widget build(BuildContext context) {
    return Material(
      color: Colors.transparent,
      child: InkWell(
        borderRadius: BorderRadius.circular(8),
        onTap: onTap,
        child: ConstrainedBox(
          constraints: BoxConstraints(maxWidth: maxWidth),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 5.5),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(icon, size: 14.5, color: iconColor),
                const SizedBox(width: 5),
                Flexible(
                  child: Text(
                    label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 12,
                      fontWeight: FontWeight.w500,
                      color: Colors.white.withValues(alpha: 0.72),
                    ),
                  ),
                ),
                const SizedBox(width: 2),
                Icon(
                  Icons.keyboard_arrow_down_rounded,
                  size: 14,
                  color: Colors.white.withValues(alpha: 0.38),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _SendOrStopButton extends StatelessWidget {
  const _SendOrStopButton({
    required this.hasText,
    required this.isRunning,
    required this.onPressed,
  });

  final bool hasText;
  final bool isRunning;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    final enabled = hasText || isRunning;
    final showStop = isRunning && !hasText;

    return Material(
      color: showStop
          ? Colors.white.withValues(alpha: 0.14)
          : hasText
              ? const Color(0xFFF3F4F6)
              : Colors.white.withValues(alpha: 0.06),
      shape: const CircleBorder(),
      child: InkWell(
        customBorder: const CircleBorder(),
        onTap: enabled ? onPressed : null,
        child: SizedBox(
          width: 32,
          height: 32,
          child: Icon(
            showStop ? Icons.stop_rounded : Icons.arrow_upward_rounded,
            size: 17,
            color: showStop
                ? Colors.white
                : hasText
                    ? const Color(0xFF0B0D11)
                    : Colors.white.withValues(alpha: 0.24),
          ),
        ),
      ),
    );
  }
}
