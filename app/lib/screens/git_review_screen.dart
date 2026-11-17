import 'package:flutter/material.dart';

import '../core/fleet_controller.dart';

class GitReviewScreen extends StatefulWidget {
  const GitReviewScreen({
    super.key,
    required this.controller,
    required this.hostId,
    required this.initialCwd,
  });

  final FleetController controller;
  final String hostId;
  final String initialCwd;

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

        return Scaffold(
          appBar: AppBar(
            title: Text(
              status?.branch != null
                  ? 'Git · ${status!.branch}'
                  : 'Git Working Tree',
            ),
            actions: [
              IconButton(
                icon: const Icon(Icons.refresh_rounded),
                onPressed: () =>
                    widget.controller.fetchGitStatus(widget.hostId, cwd),
              ),
            ],
          ),
          body: Column(
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(14, 8, 14, 8),
                child: Row(
                  children: [
                    Expanded(
                      child: TextField(
                        controller: _cwdCtrl,
                        decoration: const InputDecoration(
                          isDense: true,
                          labelText: 'Repository Path',
                          border: OutlineInputBorder(),
                        ),
                      ),
                    ),
                    const SizedBox(width: 8),
                    FilledButton(
                      onPressed: () => widget.controller
                          .fetchGitStatus(widget.hostId, _cwdCtrl.text.trim()),
                      child: const Text('Inspect'),
                    ),
                  ],
                ),
              ),
              if (status != null && status.files.isNotEmpty)
                SizedBox(
                  height: 150,
                  child: ListView.builder(
                    itemCount: status.files.length,
                    itemBuilder: (context, i) {
                      final f = status.files[i];
                      return ListTile(
                        dense: true,
                        selected: _selectedPath == f.path,
                        leading: Text(
                          f.status,
                          style: TextStyle(
                            fontFamily: 'monospace',
                            fontWeight: FontWeight.bold,
                            color: f.untracked
                                ? Colors.greenAccent
                                : Colors.amberAccent,
                          ),
                        ),
                        title: Text(
                          f.path,
                          style: const TextStyle(
                              fontSize: 13, fontFamily: 'monospace'),
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
                  ),
                )
              else
                const Padding(
                  padding: EdgeInsets.all(16),
                  child: Text(
                    'Working tree clean (or select a Git directory).',
                    style: TextStyle(color: Colors.white54),
                  ),
                ),
              const Divider(height: 1),
              Expanded(
                child: diffText == null || diffText.isEmpty
                    ? const Center(
                        child: Text(
                          'Tap a changed file above to inspect unified diff.',
                          style: TextStyle(color: Colors.white54),
                        ),
                      )
                    : ListView(
                        padding: const EdgeInsets.all(12),
                        children: [
                          for (final line in diffText.split('\n'))
                            Container(
                              color: line.startsWith('+') &&
                                      !line.startsWith('+++')
                                  ? Colors.green.withValues(alpha: 0.16)
                                  : line.startsWith('-') &&
                                          !line.startsWith('---')
                                      ? Colors.red.withValues(alpha: 0.16)
                                      : Colors.transparent,
                              padding: const EdgeInsets.symmetric(
                                  horizontal: 6, vertical: 1),
                              child: Text(
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
                      ),
              ),
            ],
          ),
        );
      },
    );
  }
}
