import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:orbit_mobile/core/app_updater.dart';
import 'package:orbit_mobile/core/fleet_controller.dart';
import 'package:orbit_mobile/core/models.dart';
import 'package:orbit_mobile/core/orbit_ffi_bridge.dart';
import 'package:orbit_mobile/core/ssh_config.dart';
import 'package:orbit_mobile/main.dart';
import 'package:orbit_mobile/screens/agent_chat_screen.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('OrbitFfiBridge binds native Rust liborbit_ffi (Noise NK + Zstd + QR)',
      () {
    final offerJson = jsonEncode({
      'host_id': 'host_macmini_01',
      'hostname': 'mac-mini.local',
      'daemon_pubkey_b64': 'AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE',
      'direct_endpoints': ['ws://192.168.1.88:6868/ws'],
      'relay_url': 'wss://aws-devbox.example.com:7878',
      'pairing_secret': 'sec_abc',
    });
    final b64 = base64UrlEncode(utf8.encode(offerJson)).replaceAll('=', '');
    final uri = 'orbit://pair/$b64';

    final ffi = OrbitFfiBridge.instance;
    if (Platform.environment.containsKey('ORBIT_FFI_PATH')) {
      expect(ffi.isNativeLoaded, isTrue,
          reason: 'Native Rust liborbit_ffi.so must load cleanly via dart:ffi');
    }

    final profile = ffi.parsePairingUri(uri);
    expect(profile.hostId, 'host_macmini_01');
    expect(profile.hostname, 'mac-mini.local');
    expect(profile.relayUrl, 'wss://aws-devbox.example.com:7878');

    if (ffi.isNativeLoaded) {
      // 1. Zstd roundtrip in native Rust
      final compressed = ffi.zstdCompressWire('{"op":"ping","ts_ms":12345}');
      expect(compressed, isNotNull);
      expect(compressed![0], 0x00);
      final decompressed = ffi.zstdDecompressWire(compressed.sublist(1));
      expect(decompressed, '{"op":"ping","ts_ms":12345}');

      // 2. Noise_NK_25519_ChaChaPoly_SHA256 client handshake msg1 generation in native Rust
      final noiseInit =
          ffi.noiseClientInit('AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE');
      expect(noiseInit.handshakeHandle, greaterThan(0));
      expect(noiseInit.msg1.length, greaterThan(32));
    }
  });

  test('TerminalBinaryFrame encodes and decodes input & resize frames accurately',
      () {
    final inputBytes = TerminalBinaryFrame.encodeInput(1, 'ls -la\n');
    final decoded = TerminalBinaryFrame.decode(inputBytes);
    expect(decoded, isNotNull);
    expect(decoded!.opcode, 0x02);
    expect(decoded.slot, 1);
    expect(utf8.decode(decoded.payload), 'ls -la\n');

    final resizeBytes = TerminalBinaryFrame.encodeResize(2, 120, 42);
    expect(resizeBytes.length, 6);
    expect(resizeBytes[0], 0x03);
    expect(resizeBytes[1], 2);
  });

  testWidgets('FleetHomeScreen renders onboarding and Pair Machine modal',
      (tester) async {
    SharedPreferences.setMockInitialValues({});
    final controller = FleetController();
    await tester.pumpWidget(OrbitMobileApp(controller: controller));
    await tester.pumpAndSettle();

    expect(find.text('Orbit Fleet'), findsOneWidget);
    expect(find.text('Connect Your Multi-Host Fleet'), findsOneWidget);
    expect(find.text('Pair Machine'), findsOneWidget);

    await tester.tap(find.text('Pair Machine'));
    await tester.pumpAndSettle();
    expect(find.text('Pair Orbit Host'), findsOneWidget);
    expect(find.text('Connect & Save Host'), findsOneWidget);
  });

  testWidgets('AgentChatScreen renders timeline & composer cleanly',
      (tester) async {
    SharedPreferences.setMockInitialValues({});
    final controller = FleetController();
    await tester.pumpWidget(
      MaterialApp(
        home: AgentChatScreen(
          controller: controller,
          hostId: 'host_test',
          agentId: 'ag_test',
        ),
      ),
    );
    await tester.pump();
    expect(find.text('Waiting for timeline stream…'), findsOneWidget);
    expect(find.text('Message or steer active turn…'), findsOneWidget);
  });

  test('SshConfigService parses ~/.ssh/config hosts and skips git forges', () {
    const sample = '''
Host *
  ServerAliveInterval 30

Host github.com
  HostName github.com
  User git

Host devbox
  HostName 203.0.113.55
  User ec2-user

Host mini
  HostName=198.51.100.8
  User=admin
  Port 2222
''';
    final entries = SshConfigService.parseConfigString(sample);
    expect(entries.length, 2);
    expect(entries[0].alias, 'devbox');
    expect(entries[0].hostName, '203.0.113.55');
    expect(entries[0].displayTarget, 'ec2-user@203.0.113.55');
    expect(entries[0].relayWsUrl(7878), 'ws://203.0.113.55:7878');
    expect(entries[1].alias, 'mini');
    expect(entries[1].displayTarget, 'admin@198.51.100.8:2222');
  });

  test('AppUpdater compares semver and parses GitHub release metadata', () {
    expect(AppUpdater.compareSemver('0.1.3', '0.1.2'), greaterThan(0));
    expect(AppUpdater.compareSemver('v0.1.2', '0.1.2+3'), equals(0));
    expect(AppUpdater.compareSemver('0.1.1', '0.1.2'), lessThan(0));

    final info = ReleaseUpdateInfo.fromGithubJson(
      {
        'tag_name': 'v0.2.0',
        'html_url': 'https://github.com/danielgaoyy/orbit/releases/tag/v0.2.0',
        'body': 'One-click SSH relay + self-updater',
        'assets': [
          {
            'name': 'Orbit-macOS-AppleSilicon.dmg',
            'browser_download_url':
                'https://example.com/Orbit-macOS-AppleSilicon.dmg',
          },
        ],
      },
      currentVersion: '0.1.2',
    );
    expect(info.hasNewerVersion, isTrue);
    expect(info.macosDmgUrl, 'https://example.com/Orbit-macOS-AppleSilicon.dmg');
  });

  testWidgets(
      'Desktop split-pane workbench renders cleanly without overflow at 1360x840 and 880x600',
      (tester) async {
    SharedPreferences.setMockInitialValues({
      'orbit_saved_hosts': [
        jsonEncode({
          'host_id': 'host_workstation_1',
          'hostname': 'workstation-pro',
          'daemon_pubkey_b64': 'AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE',
          'direct_endpoints': ['ws://127.0.0.1:1/ws'],
          'relay_url': null,
        }),
      ],
    });

    for (final scale in [1.0, 1.5]) {
      tester.platformDispatcher.textScaleFactorTestValue = scale;
      for (final size in [
        const Size(1360, 840),
        const Size(880, 600),
        const Size(390, 844),
      ]) {
        tester.view.physicalSize = size;
        tester.view.devicePixelRatio = 1.0;

        final controller = FleetController();
        await controller.loadSavedHosts(autoDiscoverLocal: false);
        await tester.pumpWidget(OrbitMobileApp(controller: controller));
        await tester.pump(const Duration(seconds: 2));

        expect(find.textContaining('workstation-pro'), findsWidgets);

        if (size.width >= 880) {
          expect(find.text('机器舰队'), findsOneWidget);
          expect(find.text('编程会话'), findsOneWidget);
          expect(find.text('Set Relay (SSH)'), findsOneWidget);

          await tester.tap(find.text('Set Relay (SSH)'));
          await tester.pumpAndSettle();
          expect(find.textContaining('配置 E2EE 中继'), findsOneWidget);

          Navigator.of(tester.element(find.textContaining('配置 E2EE 中继')))
              .pop();
          await tester.pumpAndSettle();
        }
      }
    }

    tester.platformDispatcher.clearTextScaleFactorTestValue();
    tester.view.resetPhysicalSize();
    tester.view.resetDevicePixelRatio();
  });
}
