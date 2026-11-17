import 'package:flutter/material.dart';

import '../core/fleet_controller.dart';
import '../core/models.dart';

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
  late final TextEditingController _cwdCtrl =
      TextEditingController(text: widget.initialCwd);
  String? _selectedPath;

  @override
  void initState() {
    super.initState();
    widget.controller.fetchGitStatus(widget.hostId, widget.initialCwd);
  }

  @override
  void dispose() {
    _cwdCtrl.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: widget.controller,
      builder: (context, _) {
        final session = widget.controller.sessionFor(widget.hostId);
        final cwd = _cwdCtrl.text.trim();
        final status = session?.gitStatuses[cwd];
        final diffKey = '$cwd::${_selectedPath ?? "__all__"}';
        final diffText = session?.gitDiffs[diffKey];

        final content = LayoutBuilder(
          builder: (context, constraints) {
            final isWide = constraints.maxWidth >= 720;
            return Column(
              children: [
                Container(
                  padding: const EdgeInsets.fromLTRB(16, 10, 16, 10),
                  decoration: const BoxDecoration(
                    color: Color(0xFF0E131E),
                    border: Border(
                      bottom: BorderSide(color: Color(0xFF1E293B)),
                    ),
                  ),
                  child: Row(
                    children: [
                      if (status?.branch != null) ...[
                        Container(
                          padding: const EdgeInsets.symmetric(
                            horizontal: 10,
                            vertical: 6,
                          ),
                          decoration: BoxDecoration(
                            color:
                                const Color(0xFF38BDF8).withValues(alpha: 0.12),
                            borderRadius: BorderRadius.circular(8),
                          ),
                          child: Row(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              const Icon(
                                Icons.call_split_rounded,
                                size: 14,
                                color: Color(0xFF38BDF8),
                              ),
                              const SizedBox(width: 5),
                              Text(
                                status!.branch!,
                                style: const TextStyle(
                                  fontSize: 12,
                                  fontFamily: 'monospace',
                                  color: Color(0xFF38BDF8),
                                ),
                              ),
                            ],
                          ),
                        ),
                        const SizedBox(width: 10),
                      ],
                      Expanded(
                        child: TextField(
                          controller: _cwdCtrl,
                          onSubmitted: (v) => widget.controller
                              .fetchGitStatus(widget.hostId, v.trim()),
                          style: const TextStyle(
                            fontSize: 13,
                            fontFamily: 'monospace',
                          ),
                          decoration: const InputDecoration(
                            isDense: true,
                            labelText: 'Git 仓库路径 (cwd)',
                            border: OutlineInputBorder(),
                          ),
                        ),
                      ),
                      const SizedBox(width: 10),
                      FilledButton.icon(
                        onPressed: () => widget.controller
                            .fetchGitStatus(widget.hostId, _cwdCtrl.text.trim()),
                        icon: const Icon(Icons.refresh_rounded, size: 16),
                        label: const Text('刷新状态'),
                      ),
                    ],
                  ),
                ),
                Expanded(
                  child: isWide
                      ? Row(
                          children: [
                            SizedBox(
                              width: 280,
                              child: Container(
                                decoration: const BoxDecoration(
                                  border: Border(
                                    right: BorderSide(color: Color(0xFF1E293B)),
                                  ),
                                ),
                                child: _buildFileList(status, cwd),
                              ),
                            ),
                            Expanded(child: _buildDiffPane(diffText)),
                          ],
                        )
                      : Column(
                          children: [
                            SizedBox(
                              height: 170,
                              child: _buildFileList(status, cwd),
                            ),
                            const Divider(height: 1),
                            Expanded(child: _buildDiffPane(diffText)),
                          ],
                        ),
                ),
              ],
            );
          },
        );

        if (widget.embedded) return content;

        return Scaffold(
          appBar: AppBar(
            title: Text(
              status?.branch != null
                  ? 'Git · ${status!.branch}'
                  : 'Git Working Tree',
            ),
          ),
          body: content,
        );
      },
    );
  }

  Widget _buildFileList(GitStatusSnapshot? status, String cwd) {
    if (status == null || status.files.isEmpty) {
      return const Center(
        child: Padding(
          padding: EdgeInsets.all(16),
          child: Text(
            '工作区干净（或请输入有效的 Git 仓库目录）',
            textAlign: TextAlign.center,
            style: TextStyle(color: Colors.white54, fontSize: 12.5),
          ),
        ),
      );
    }
    return ListView.builder(
      itemCount: status.files.length,
      itemBuilder: (context, i) {
        final f = status.files[i];
        return ListTile(
          dense: true,
          selected: _selectedPath == f.path,
          selectedTileColor: const Color(0xFF38BDF8).withValues(alpha: 0.12),
          leading: Text(
            f.status,
            style: TextStyle(
              fontFamily: 'monospace',
              fontWeight: FontWeight.bold,
              color: f.untracked ? Colors.greenAccent : Colors.amberAccent,
            ),
          ),
          title: Text(
            f.path,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: const TextStyle(fontSize: 12.5, fontFamily: 'monospace'),
          ),
          onTap: () {
            setState(() => _selectedPath = f.path);
            widget.controller.fetchGitDiff(
              widget.hostId,
              cwd,
              path: f.path,
              staged: f.staged,
            );
          },
        );
      },
    );
  }

  Widget _buildDiffPane(String? diffText) {
    if (diffText == null || diffText.isEmpty) {
      return const Center(
        child: Text(
          '点击左侧变更文件查看 Unified Diff',
          style: TextStyle(color: Colors.white54, fontSize: 13),
        ),
      );
    }
    return ListView(
      padding: const EdgeInsets.all(14),
      children: [
        for (final line in diffText.split('\n'))
          Container(
            color: line.startsWith('+') && !line.startsWith('+++')
                ? Colors.green.withValues(alpha: 0.16)
                : line.startsWith('-') && !line.startsWith('---')
                    ? Colors.red.withValues(alpha: 0.16)
                    : Colors.transparent,
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 1.5),
            child: SelectableText(
              line,
              style: TextStyle(
                fontFamily: 'monospace',
                fontSize: 12,
                color: line.startsWith('@@')
                    ? const Color(0xFF38BDF8)
                    : Colors.white,
              ),
            ),
          ),
      ],
    );
  }
}
