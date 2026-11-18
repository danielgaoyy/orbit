import 'package:flutter/material.dart';

import '../core/fleet_controller.dart';

/// Collapsible Detached PTY Drawer automatically attached to the active conversation's `cwd`.
class TerminalScreen extends StatefulWidget {
  const TerminalScreen({
    super.key,
    required this.controller,
    required this.hostId,
    required this.cwd,
    this.embedded = false,
    this.onClose,
  });

  final FleetController controller;
  final String hostId;
  final String cwd;
  final bool embedded;
  final VoidCallback? onClose;

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
          decoration: BoxDecoration(
            color: const Color(0xFF080A0F),
            border: Border(
              top: BorderSide(color: Colors.white.withValues(alpha: 0.10)),
            ),
          ),
          child: Column(
            children: [
              Container(
                height: 34,
                padding: const EdgeInsets.symmetric(horizontal: 14),
                decoration: BoxDecoration(
                  border: Border(
                    bottom:
                        BorderSide(color: Colors.white.withValues(alpha: 0.06)),
                  ),
                ),
                child: Row(
                  children: [
                    const Icon(
                      Icons.terminal_rounded,
                      size: 14,
                      color: Color(0xFF38BDF8),
                    ),
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(
                        widget.cwd,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(
                          fontSize: 11.5,
                          fontFamily: 'monospace',
                          color: Colors.white60,
                        ),
                      ),
                    ),
                    TextButton(
                      style: TextButton.styleFrom(
                        visualDensity: VisualDensity.compact,
                        padding: const EdgeInsets.symmetric(horizontal: 8),
                      ),
                      onPressed: () => _sendRaw('\x03'),
                      child: const Text(
                        'Ctrl+C',
                        style: TextStyle(fontSize: 11, color: Colors.white54),
                      ),
                    ),
                    if (widget.onClose != null)
                      IconButton(
                        visualDensity: VisualDensity.compact,
                        padding: EdgeInsets.zero,
                        constraints: const BoxConstraints(),
                        icon: const Icon(
                          Icons.close_rounded,
                          size: 16,
                          color: Colors.white54,
                        ),
                        onPressed: widget.onClose,
                      ),
                  ],
                ),
              ),
              Expanded(
                child: SingleChildScrollView(
                  reverse: true,
                  padding:
                      const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
                  child: SizedBox(
                    width: double.infinity,
                    child: SelectableText(
                      text,
                      style: const TextStyle(
                        fontFamily: 'monospace',
                        fontSize: 12,
                        color: Color(0xFFE2E8F0),
                        height: 1.4,
                      ),
                    ),
                  ),
                ),
              ),
              Padding(
                padding: const EdgeInsets.fromLTRB(14, 4, 14, 8),
                child: Row(
                  children: [
                    const Text(
                      '\$ ',
                      style: TextStyle(
                        fontFamily: 'monospace',
                        fontSize: 12.5,
                        color: Color(0xFF38BDF8),
                      ),
                    ),
                    Expanded(
                      child: TextField(
                        controller: _cmdCtrl,
                        style: const TextStyle(
                          fontFamily: 'monospace',
                          fontSize: 12.5,
                        ),
                        decoration: const InputDecoration(
                          isDense: true,
                          hintText: '输入命令回车执行…',
                          hintStyle: TextStyle(color: Colors.white30),
                          border: InputBorder.none,
                        ),
                        onSubmitted: (v) {
                          _sendRaw('$v\n');
                          _cmdCtrl.clear();
                        },
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        );

        if (widget.embedded) return content;

        return Scaffold(
          backgroundColor: const Color(0xFF080A0F),
          appBar: AppBar(title: Text(widget.cwd)),
          body: content,
        );
      },
    );
  }
}
