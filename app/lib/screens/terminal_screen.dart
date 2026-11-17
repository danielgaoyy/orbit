import 'package:flutter/material.dart';

import '../core/fleet_controller.dart';

class TerminalScreen extends StatefulWidget {
  const TerminalScreen({
    super.key,
    required this.controller,
    required this.hostId,
    required this.cwd,
    this.embedded = false,
  });

  final FleetController controller;
  final String hostId;
  final String cwd;
  final bool embedded;

  @override
  State<TerminalScreen> createState() => _TerminalScreenState();
}

class _TerminalScreenState extends State<TerminalScreen> {
  final _cmdCtrl = TextEditingController();

  @override
  void initState() {
    super.initState();
    widget.controller.startRemoteTerminal(widget.hostId, widget.cwd);
  }

  @override
  void dispose() {
    _cmdCtrl.dispose();
    super.dispose();
  }

  void _sendRaw(String s) {
    widget.controller.sendTerminalKeys(widget.hostId, s);
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: widget.controller,
      builder: (context, _) {
        final session = widget.controller.sessionFor(widget.hostId);
        final text = session?.terminalBuffer.toString() ?? '';

        final content = Container(
          color: const Color(0xFF06080D),
          child: Column(
            children: [
              if (widget.embedded)
                Container(
                  height: 40,
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  decoration: const BoxDecoration(
                    color: Color(0xFF0E131E),
                    border: Border(
                      bottom: BorderSide(color: Color(0xFF1E293B)),
                    ),
                  ),
                  child: Row(
                    children: [
                      const Icon(
                        Icons.terminal_rounded,
                        size: 15,
                        color: Color(0xFF38BDF8),
                      ),
                      const SizedBox(width: 8),
                      Text(
                        'Detached PTY · ${widget.cwd}',
                        style: const TextStyle(
                          fontSize: 12,
                          fontFamily: 'monospace',
                          color: Colors.white70,
                        ),
                      ),
                    ],
                  ),
                ),
              Expanded(
                child: SingleChildScrollView(
                  reverse: true,
                  padding: const EdgeInsets.all(14),
                  child: SizedBox(
                    width: double.infinity,
                    child: SelectableText(
                      text,
                      style: const TextStyle(
                        fontFamily: 'monospace',
                        fontSize: 12.5,
                        color: Color(0xFFE2E8F0),
                        height: 1.4,
                      ),
                    ),
                  ),
                ),
              ),
              Container(
                color: const Color(0xFF111622),
                padding:
                    const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
                child: SingleChildScrollView(
                  scrollDirection: Axis.horizontal,
                  child: Row(
                    children: [
                      _keyChip('ESC', '\x1b'),
                      _keyChip('TAB', '\t'),
                      _keyChip('CTRL+C', '\x03'),
                      _keyChip('CTRL+D', '\x04'),
                      _keyChip('↑', '\x1b[A'),
                      _keyChip('↓', '\x1b[B'),
                      _keyChip('←', '\x1b[D'),
                      _keyChip('→', '\x1b[C'),
                    ],
                  ),
                ),
              ),
              SafeArea(
                top: false,
                child: Container(
                  color: const Color(0xFF0A0D14),
                  padding: const EdgeInsets.fromLTRB(12, 8, 12, 12),
                  child: Row(
                    children: [
                      Expanded(
                        child: TextField(
                          controller: _cmdCtrl,
                          style: const TextStyle(
                            fontFamily: 'monospace',
                            fontSize: 13,
                          ),
                          decoration: const InputDecoration(
                            isDense: true,
                            hintText: '输入终端命令回车执行…',
                            border: OutlineInputBorder(),
                          ),
                          onSubmitted: (v) {
                            _sendRaw('$v\n');
                            _cmdCtrl.clear();
                          },
                        ),
                      ),
                      const SizedBox(width: 8),
                      IconButton.filled(
                        icon: const Icon(Icons.keyboard_return_rounded),
                        onPressed: () {
                          _sendRaw('${_cmdCtrl.text}\n');
                          _cmdCtrl.clear();
                        },
                      ),
                    ],
                  ),
                ),
              ),
            ],
          ),
        );

        if (widget.embedded) return content;

        return Scaffold(
          backgroundColor: const Color(0xFF06080D),
          appBar: AppBar(
            backgroundColor: const Color(0xFF0E131E),
            title: Text(
              'PTY · ${widget.cwd}',
              style: const TextStyle(fontSize: 14, fontFamily: 'monospace'),
            ),
          ),
          body: content,
        );
      },
    );
  }

  Widget _keyChip(String label, String seq) {
    return Padding(
      padding: const EdgeInsets.only(right: 6),
      child: ActionChip(
        visualDensity: VisualDensity.compact,
        label: Text(
          label,
          style: const TextStyle(fontSize: 11, fontFamily: 'monospace'),
        ),
        onPressed: () => _sendRaw(seq),
      ),
    );
  }
}
