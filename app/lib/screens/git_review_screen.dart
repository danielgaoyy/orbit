import 'package:flutter/material.dart';

import '../core/fleet_controller.dart';
import '../core/models.dart';

/// Zero-input Git Diff Review Drawer / Sheet automatically bound to the active conversation's `cwd`.
class GitReviewScreen extends StatefulWidget {
  const GitReviewScreen({
    super.key,
    required this.controller,
    required this.hostId,
    required this.initialCwd,
    this.embedded = false,
  });

  final FleetController controller;
  final String hostId;
  final String initialCwd;
  final bool embedded;

  @override
  State<GitReviewScreen> createState() => _GitReviewScreenState();
}

class _GitReviewScreenState extends State<GitReviewScreen> {
  String? _selectedPath;

  @override
  void initState() {
    super.initState();
    widget.controller.fetchGitStatus(widget.hostId, widget.initialCwd);
    widget.controller.fetchGitDiff(widget.hostId, widget.initialCwd);
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: widget.controller,
      builder: (context, _) {
        final session = widget.controller.sessionFor(widget.hostId);
        final status = session?.gitStatuses[widget.initialCwd];
        final files = status?.files ?? const <GitFileEntry>[];
        if (_selectedPath == null && files.isNotEmpty) {
          _selectedPath = files.first.path;
          widget.controller.fetchGitDiff(
            widget.hostId,
            widget.initialCwd,
            path: files.first.path,
            staged: files.first.staged,
          );
        }

        final diffKey = '${widget.initialCwd}::${_selectedPath ?? "__all__"}';
        final diffText = session?.gitDiffs[diffKey];

        final content = Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Container(
              padding: const EdgeInsets.fromLTRB(18, 14, 12, 12),
              decoration: BoxDecoration(
                border: Border(
                  bottom:
                      BorderSide(color: Colors.white.withValues(alpha: 0.08)),
                ),
              ),
              child: Row(
                children: [
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            const Text(
                              '代码改动审阅',
                              style: TextStyle(
                                fontSize: 15,
                                fontWeight: FontWeight.w600,
                              ),
                            ),
                            if (status?.branch != null) ...[
                              const SizedBox(width: 10),
                              Container(
                                padding: const EdgeInsets.symmetric(
                                  horizontal: 7,
                                  vertical: 2,
                                ),
                                decoration: BoxDecoration(
                                  color: const Color(0xFF38BDF8)
                                      .withValues(alpha: 0.14),
                                  borderRadius: BorderRadius.circular(6),
                                ),
                                child: Text(
                                  status!.branch!,
                                  style: const TextStyle(
                                    fontSize: 11,
                                    fontFamily: 'monospace',
                                    color: Color(0xFF38BDF8),
                                  ),
                                ),
                              ),
                            ],
                          ],
                        ),
                        const SizedBox(height: 2),
                        Text(
                          widget.initialCwd,
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
                  IconButton(
                    tooltip: '刷新改动',
                    icon: const Icon(Icons.refresh_rounded, size: 18),
                    onPressed: () {
                      widget.controller
                          .fetchGitStatus(widget.hostId, widget.initialCwd);
                      if (_selectedPath != null) {
                        widget.controller.fetchGitDiff(
                          widget.hostId,
                          widget.initialCwd,
                          path: _selectedPath,
                        );
                      }
                    },
                  ),
                  if (widget.embedded)
                    IconButton(
                      tooltip: '关闭',
                      icon: const Icon(Icons.close_rounded, size: 19),
                      onPressed: () => Navigator.of(context).maybePop(),
                    ),
                ],
              ),
            ),
            if (files.isNotEmpty)
              Container(
                height: 44,
                padding: const EdgeInsets.symmetric(horizontal: 14),
                decoration: BoxDecoration(
                  border: Border(
                    bottom:
                        BorderSide(color: Colors.white.withValues(alpha: 0.06)),
                  ),
                ),
                child: ListView.separated(
                  scrollDirection: Axis.horizontal,
                  itemCount: files.length,
                  separatorBuilder: (_, __) => const SizedBox(width: 6),
                  itemBuilder: (_, i) {
                    final f = files[i];
                    final selected = f.path == _selectedPath;
                    return Center(
                      child: ChoiceChip(
                        visualDensity: VisualDensity.compact,
                        label: Text(
                          f.path,
                          style: const TextStyle(
                            fontSize: 11.5,
                            fontFamily: 'monospace',
                          ),
                        ),
                        selected: selected,
                        onSelected: (_) {
                          setState(() => _selectedPath = f.path);
                          widget.controller.fetchGitDiff(
                            widget.hostId,
                            widget.initialCwd,
                            path: f.path,
                            staged: f.staged,
                          );
                        },
                      ),
                    );
                  },
                ),
              ),
            Expanded(
              child: files.isEmpty
                  ? const Center(
                      child: Text(
                        '当前目录没有未提交改动',
                        style: TextStyle(color: Colors.white54, fontSize: 13),
                      ),
                    )
                  : _buildDiffPane(diffText),
            ),
          ],
        );

        if (widget.embedded) return content;

        return Scaffold(
          appBar: AppBar(title: const Text('代码改动审阅')),
          body: content,
        );
      },
    );
  }

  Widget _buildDiffPane(String? diffText) {
    if (diffText == null || diffText.isEmpty) {
      return const Center(
        child: Text(
          '正在加载 Diff…',
          style: TextStyle(color: Colors.white38, fontSize: 12.5),
        ),
      );
    }
    return ListView(
      padding: const EdgeInsets.all(14),
      children: [
        for (final line in diffText.split('\n'))
          Container(
            color: line.startsWith('+') && !line.startsWith('+++')
                ? const Color(0xFF34D399).withValues(alpha: 0.14)
                : line.startsWith('-') && !line.startsWith('---')
                    ? const Color(0xFFFB7185).withValues(alpha: 0.14)
                    : Colors.transparent,
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 2),
            child: SelectableText(
              line,
              style: TextStyle(
                fontFamily: 'monospace',
                fontSize: 12,
                color: line.startsWith('@@')
                    ? const Color(0xFF38BDF8)
                    : Colors.white.withValues(alpha: 0.88),
              ),
            ),
          ),
      ],
    );
  }
}
