import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../core/fleet_controller.dart';
import '../core/models.dart';
import 'host_workspace_screen.dart';

class FleetHomeScreen extends StatelessWidget {
  const FleetHomeScreen({super.key, required this.controller});

  final FleetController controller;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        final hosts = controller.hosts;
        final pendingApprovals = controller.globalPendingApprovalsCount;

        return Scaffold(
          appBar: AppBar(
            title: Row(
              children: [
                const Icon(Icons.radar_rounded, color: Color(0xFF38BDF8)),
                const SizedBox(width: 10),
                const Text(
                  'Orbit Fleet',
                  style: TextStyle(fontWeight: FontWeight.w700),
                ),
                if (pendingApprovals > 0) ...[
                  const SizedBox(width: 10),
                  Container(
                    padding:
                        const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                    decoration: BoxDecoration(
                      color: Colors.amber.shade700,
                      borderRadius: BorderRadius.circular(999),
                    ),
                    child: Text(
                      '$pendingApprovals Approval',
                      style: const TextStyle(
                        fontSize: 11,
                        fontWeight: FontWeight.bold,
                        color: Colors.black,
                      ),
                    ),
                  ),
                ],
              ],
            ),
            actions: [
              IconButton(
                tooltip: 'Pair Host (`orbit pair`)',
                icon: const Icon(Icons.qr_code_scanner_rounded),
                onPressed: () => _showPairHostSheet(context),
              ),
            ],
          ),
          body: hosts.isEmpty
              ? _buildEmptyOnboarding(context)
              : ListView.separated(
                  padding: const EdgeInsets.all(16),
                  itemCount: hosts.length,
                  separatorBuilder: (_, __) => const SizedBox(height: 12),
                  itemBuilder: (context, index) =>
                      _buildHostCard(context, hosts[index]),
                ),
          floatingActionButton: FloatingActionButton.extended(
            onPressed: () => _showPairHostSheet(context),
            icon: const Icon(Icons.add_link_rounded),
            label: const Text('Pair Machine'),
          ),
        );
      },
    );
  }

  Widget _buildEmptyOnboarding(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const Icon(
              Icons.hub_outlined,
              size: 64,
              color: Color(0xFF38BDF8),
            ),
            const SizedBox(height: 16),
            const Text(
              'Connect Your Multi-Host Fleet',
              style: TextStyle(fontSize: 20, fontWeight: FontWeight.bold),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: 8),
            const Text(
              'Run `orbit daemon up` (or `orbit use-relay <HOST:PORT>` for remote access) on your Devbox, Linux workstation, or Mac mini, then paste the `orbit://pair/...` link.',
              style: TextStyle(color: Colors.white70, height: 1.4),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: 24),
            FilledButton.icon(
              onPressed: () => _showPairHostSheet(context),
              icon: const Icon(Icons.link_rounded),
              label: const Text('Import Pairing URI'),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildHostCard(BuildContext context, HostProfile host) {
    final session = controller.sessionFor(host.hostId);
    final agentCount = session?.agents.length ?? 0;
    final runningCount =
        session?.agents.values.where((a) => a.isRunning).length ?? 0;
    final awaitingCount =
        session?.agents.values.where((a) => a.isAwaitingPermission).length ?? 0;

    final (badgeText, badgeColor) = switch (host.routeState) {
      HostRouteState.onlineDirect => ('LAN Direct', Colors.greenAccent),
      HostRouteState.onlineRelay =>
        ('Noise E2EE Relay', const Color(0xFF38BDF8)),
      HostRouteState.connecting => ('Connecting…', Colors.orangeAccent),
      HostRouteState.disconnected => ('Offline', Colors.redAccent),
    };

    return Card(
      child: InkWell(
        borderRadius: BorderRadius.circular(14),
        onTap: () {
          Navigator.of(context).push(
            MaterialPageRoute<void>(
              builder: (_) => HostWorkspaceScreen(
                controller: controller,
                hostId: host.hostId,
              ),
            ),
          );
        },
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  const Icon(Icons.dns_rounded, size: 20, color: Colors.white70),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      host.hostname,
                      style: const TextStyle(
                        fontSize: 17,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                  ),
                  Container(
                    padding:
                        const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
                    decoration: BoxDecoration(
                      color: badgeColor.withValues(alpha: 0.14),
                      borderRadius: BorderRadius.circular(999),
                      border:
                          Border.all(color: badgeColor.withValues(alpha: 0.5)),
                    ),
                    child: Text(
                      badgeText,
                      style: TextStyle(
                        fontSize: 11,
                        fontWeight: FontWeight.w600,
                        color: badgeColor,
                      ),
                    ),
                  ),
                  PopupMenuButton<String>(
                    tooltip: 'Host Options',
                    icon: const Icon(Icons.more_vert_rounded, size: 18),
                    onSelected: (action) async {
                      if (action == 'relay') {
                        _showEditRelaySheet(context, host);
                      } else if (action == 'pubkey') {
                        await Clipboard.setData(
                          ClipboardData(text: host.daemonPubkeyB64),
                        );
                        if (context.mounted) {
                          ScaffoldMessenger.of(context).showSnackBar(
                            const SnackBar(
                              content: Text(
                                'Copied host public key for `orbit relay allow`',
                              ),
                            ),
                          );
                        }
                      } else if (action == 'remove') {
                        await controller.removeHost(host.hostId);
                      }
                    },
                    itemBuilder: (_) => const [
                      PopupMenuItem(
                        value: 'relay',
                        child: Text('Configure Relay URL…'),
                      ),
                      PopupMenuItem(
                        value: 'pubkey',
                        child: Text('Copy Host Public Key'),
                      ),
                      PopupMenuItem(
                        value: 'remove',
                        child: Text('Remove Host'),
                      ),
                    ],
                  ),
                ],
              ),
              const SizedBox(height: 6),
              Text(
                host.relayUrl != null && host.relayUrl!.isNotEmpty
                    ? 'Relay: ${host.relayUrl}'
                    : 'Relay: Not configured (LAN / Tailscale direct only)',
                style: const TextStyle(fontSize: 11.5, color: Colors.white54),
              ),
              if (host.routeState == HostRouteState.disconnected &&
                  host.lastConnectionError != null) ...[
                const SizedBox(height: 8),
                Container(
                  width: double.infinity,
                  padding:
                      const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
                  decoration: BoxDecoration(
                    color: Colors.redAccent.withValues(alpha: 0.10),
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(
                      color: Colors.redAccent.withValues(alpha: 0.35),
                    ),
                  ),
                  child: Row(
                    children: [
                      const Icon(
                        Icons.info_outline_rounded,
                        size: 15,
                        color: Colors.orangeAccent,
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          host.lastConnectionError!,
                          style: const TextStyle(
                            fontSize: 11.5,
                            color: Colors.white70,
                          ),
                        ),
                      ),
                      TextButton(
                        onPressed: () => _showEditRelaySheet(context, host),
                        child: const Text(
                          'Set Relay',
                          style: TextStyle(fontSize: 12),
                        ),
                      ),
                    ],
                  ),
                ),
              ],
              const SizedBox(height: 10),
              Wrap(
                spacing: 8,
                runSpacing: 6,
                children: [
                  for (final p in host.availableProviders)
                    Chip(
                      visualDensity: VisualDensity.compact,
                      label: Text(p.label, style: const TextStyle(fontSize: 11)),
                    ),
                ],
              ),
              const SizedBox(height: 8),
              Row(
                children: [
                  Text(
                    '$agentCount sessions · $runningCount active',
                    style: const TextStyle(fontSize: 12, color: Colors.white60),
                  ),
                  if (awaitingCount > 0) ...[
                    const SizedBox(width: 8),
                    Text(
                      '· $awaitingCount awaiting approval!',
                      style: const TextStyle(
                        fontSize: 12,
                        color: Colors.amberAccent,
                        fontWeight: FontWeight.bold,
                      ),
                    ),
                  ],
                  const Spacer(),
                  IconButton(
                    tooltip: 'Reconnect',
                    visualDensity: VisualDensity.compact,
                    icon: const Icon(Icons.refresh_rounded, size: 18),
                    onPressed: () => controller.connectHost(host.hostId),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  void _showEditRelaySheet(BuildContext context, HostProfile host) {
    final relayCtrl = TextEditingController(text: host.relayUrl ?? '');
    showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      builder: (ctx) => Padding(
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
            Text(
              'Attach E2EE Relay for ${host.hostname}',
              style: const TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
            ),
            const SizedBox(height: 8),
            const Text(
              'Enter your Relay address (e.g. `203.0.113.10:7878` or `ws://203.0.113.10:7878`). Also make sure you ran `orbit use-relay` on the host machine.',
              style: TextStyle(fontSize: 13, color: Colors.white70),
            ),
            const SizedBox(height: 12),
            TextField(
              controller: relayCtrl,
              autofocus: true,
              decoration: const InputDecoration(
                labelText: 'Relay Server Address',
                hintText: 'ws://203.0.113.10:7878',
                border: OutlineInputBorder(),
              ),
            ),
            const SizedBox(height: 16),
            Row(
              children: [
                if (host.relayUrl != null && host.relayUrl!.isNotEmpty) ...[
                  OutlinedButton(
                    onPressed: () async {
                      Navigator.of(ctx).pop();
                      await controller.updateHostRelay(host.hostId, null);
                    },
                    child: const Text('Clear Relay'),
                  ),
                  const SizedBox(width: 12),
                ],
                Expanded(
                  child: FilledButton(
                    onPressed: () async {
                      final text = relayCtrl.text.trim();
                      Navigator.of(ctx).pop();
                      await controller.updateHostRelay(host.hostId, text);
                    },
                    child: const Text('Save & Connect via Noise E2EE'),
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  void _showPairHostSheet(BuildContext context) {
    final uriCtrl = TextEditingController();
    showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      builder: (ctx) => Padding(
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
              'Pair Orbit Host',
              style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
            ),
            const SizedBox(height: 8),
            const Text(
              'Paste the `orbit://pair/...` URI generated by `orbit pair` or `orbit use-relay` on your machine:',
              style: TextStyle(fontSize: 13, color: Colors.white70),
            ),
            const SizedBox(height: 12),
            TextField(
              controller: uriCtrl,
              autofocus: true,
              decoration: const InputDecoration(
                hintText: 'orbit://pair/eyJob3N0X2lkIjoi...',
                border: OutlineInputBorder(),
              ),
            ),
            const SizedBox(height: 16),
            FilledButton(
              onPressed: () async {
                final text = uriCtrl.text.trim();
                if (text.isEmpty) return;
                Navigator.of(ctx).pop();
                try {
                  await controller.pairFromQrUri(text);
                } catch (e) {
                  if (context.mounted) {
                    ScaffoldMessenger.of(context).showSnackBar(
                      SnackBar(content: Text('Invalid pairing URI: $e')),
                    );
                  }
                }
              },
              child: const Text('Connect & Save Host'),
            ),
          ],
        ),
      ),
    );
  }
}
