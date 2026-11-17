import 'package:flutter/material.dart';

import '../core/fleet_controller.dart';

class TerminalScreen extends StatefulWidget {
  const TerminalScreen({
    super.key,
    required this.controller,
    required this.hostId,
    required this.cwd,
  });

  final FleetController controller;
  final String hostId;
  final String cwd;

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

        return Scaffold(
          backgroundColor: Colors.black,
          appBar: AppBar(
            backgroundColor: Colors.black,
            title: Text(
              'PTY · ${widget.cwd}',
              style: const TextStyle(fontSize: 14, fontFamily: 'monospace'),
            ),
          ),
          body: Column(
            children: [
              Expanded(
                child: SingleChildScrollView(
                  reverse: true,
                  padding: const EdgeInsets.all(12),
                  child: SizedBox(
                    width: double.infinity,
                    child: SelectableText(
                      text,
                      style: const TextStyle(
                        fontFamily: 'monospace',
                        fontSize: 12.5,
                        color: Color(0xFFE2E8F0),
                        height: 1.35,
                      ),
                    ),
                  ),
                ),
              ),
              // Mobile tactile terminal modifier bar
              Container(
                color: const Color(0xFF131822),
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
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
                  color: const Color(0xFF0B0E14),
                  padding: const EdgeInsets.fromLTRB(10, 6, 10, 10),
                  child: Row(
                    children: [
                      Expanded(
                        child: TextField(
                          controller: _cmdCtrl,
                          style: const TextStyle(
                              fontFamily: 'monospace', fontSize: 13),
                          decoration: const InputDecoration(
                            isDense: true,
                            hintText: 'Enter shell command…',
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
