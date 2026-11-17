import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:orbit_mobile/core/fleet_controller.dart';
import 'package:orbit_mobile/core/models.dart';
import 'package:orbit_mobile/core/orbit_ffi_bridge.dart';
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
}
