import 'dart:convert';
import 'dart:ffi';
import 'dart:io';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'models.dart';

typedef _FreeStrNative = Void Function(Pointer<Utf8>);
typedef _FreeStrDart = void Function(Pointer<Utf8>);

typedef _UnaryStrNative = Pointer<Utf8> Function(Pointer<Utf8>);
typedef _UnaryStrDart = Pointer<Utf8> Function(Pointer<Utf8>);

typedef _FinalizeNative = Pointer<Utf8> Function(Uint64, Pointer<Utf8>);
typedef _FinalizeDart = Pointer<Utf8> Function(int, Pointer<Utf8>);

typedef _CloseNative = Void Function(Uint64);
typedef _CloseDart = void Function(int);

/// Native `dart:ffi` binding to Rust `liborbit_ffi` (`Noise_NK_25519_ChaChaPoly_SHA256` + Zstd).
class OrbitFfiBridge {
  OrbitFfiBridge._(DynamicLibrary? lib) : _lib = lib {
    if (lib != null) {
      _freeString = lib
          .lookup<NativeFunction<_FreeStrNative>>('orbit_ffi_free_string')
          .asFunction<_FreeStrDart>();
      _parsePairing = lib
          .lookup<NativeFunction<_UnaryStrNative>>('orbit_ffi_parse_pairing_uri')
          .asFunction<_UnaryStrDart>();
      _noiseInit = lib
          .lookup<NativeFunction<_UnaryStrNative>>('orbit_ffi_noise_client_init')
          .asFunction<_UnaryStrDart>();
      _noiseFinalize = lib
          .lookup<NativeFunction<_FinalizeNative>>(
              'orbit_ffi_noise_client_finalize')
          .asFunction<_FinalizeDart>();
      _noiseSeal = lib
          .lookup<NativeFunction<_FinalizeNative>>('orbit_ffi_noise_seal')
          .asFunction<_FinalizeDart>();
      _noiseOpen = lib
          .lookup<NativeFunction<_FinalizeNative>>('orbit_ffi_noise_open')
          .asFunction<_FinalizeDart>();
      _noiseClose = lib
          .lookup<NativeFunction<_CloseNative>>('orbit_ffi_noise_close')
          .asFunction<_CloseDart>();
      _zstdCompress = lib
          .lookup<NativeFunction<_UnaryStrNative>>(
              'orbit_ffi_zstd_compress_wire')
          .asFunction<_UnaryStrDart>();
      _zstdDecompress = lib
          .lookup<NativeFunction<_UnaryStrNative>>(
              'orbit_ffi_zstd_decompress_wire')
          .asFunction<_UnaryStrDart>();
    }
  }

  static OrbitFfiBridge? _instance;

  static OrbitFfiBridge get instance {
    return _instance ??= OrbitFfiBridge._(_tryLoadLibrary());
  }

  final DynamicLibrary? _lib;
  late final _FreeStrDart _freeString;
  late final _UnaryStrDart _parsePairing;
  late final _UnaryStrDart _noiseInit;
  late final _FinalizeDart _noiseFinalize;
  late final _FinalizeDart _noiseSeal;
  late final _FinalizeDart _noiseOpen;
  late final _CloseDart _noiseClose;
  late final _UnaryStrDart _zstdCompress;
  late final _UnaryStrDart _zstdDecompress;

  bool get isNativeLoaded => _lib != null;

  static DynamicLibrary? _tryLoadLibrary() {
    try {
      if (Platform.isIOS || Platform.isMacOS) {
        return DynamicLibrary.process();
      }
      if (Platform.isAndroid) {
        return DynamicLibrary.open('liborbit_ffi.so');
      }
      final overridePath = Platform.environment['ORBIT_FFI_PATH'];
      if (overridePath != null && overridePath.isNotEmpty) {
        return DynamicLibrary.open(overridePath);
      }
      final exeDir = File(Platform.resolvedExecutable).parent.path;
      for (final candidate in [
        '$exeDir/lib/liborbit_ffi.so',
        '$exeDir/liborbit_ffi.so',
        '../target/release/liborbit_ffi.so',
        '../target/debug/liborbit_ffi.so',
        'liborbit_ffi.so',
      ]) {
        if (File(candidate).existsSync()) {
          return DynamicLibrary.open(candidate);
        }
      }
    } catch (_) {
      return null;
    }
    return null;
  }

  /// Parse `orbit://pair/<base64url>` into a `HostProfile` (uses Rust FFI when loaded, pure Dart fallback in widget tests).
  HostProfile parsePairingUri(String uri) {
    final trimmed = uri.trim();
    if (isNativeLoaded) {
      final res = _callUnary(_parsePairing, trimmed);
      return HostProfile.fromPairingJson(res as Map<String, dynamic>);
    }
    final b64Part = trimmed.startsWith('orbit://pair/')
        ? trimmed.substring('orbit://pair/'.length)
        : trimmed;
    final normalized = base64Url.normalize(b64Part);
    final decoded = utf8.decode(base64Url.decode(normalized));
    final map = jsonDecode(decoded) as Map<String, dynamic>;
    return HostProfile.fromPairingJson(map);
  }

  ({int handshakeHandle, Uint8List msg1}) noiseClientInit(String pubkeyB64) {
    if (!isNativeLoaded) {
      throw StateError('liborbit_ffi native library is required for Noise E2EE');
    }
    final data = _callUnary(_noiseInit, pubkeyB64) as Map<String, dynamic>;
    return (
      handshakeHandle: (data['handshake_handle'] as num).toInt(),
      msg1: base64Decode(data['msg1_b64'] as String),
    );
  }

  int noiseClientFinalize(int handshakeHandle, Uint8List msg2) {
    final data = _callWithHandle(_noiseFinalize, handshakeHandle, base64Encode(msg2));
    return (data as num).toInt();
  }

  Uint8List noiseSeal(int channelHandle, Uint8List plaintext) {
    final ctB64 =
        _callWithHandle(_noiseSeal, channelHandle, base64Encode(plaintext)) as String;
    return base64Decode(ctB64);
  }

  Uint8List noiseOpen(int channelHandle, Uint8List ciphertext) {
    final ptB64 =
        _callWithHandle(_noiseOpen, channelHandle, base64Encode(ciphertext)) as String;
    return base64Decode(ptB64);
  }

  void noiseClose(int channelHandle) {
    if (isNativeLoaded) {
      _noiseClose(channelHandle);
    }
  }

  Uint8List? zstdCompressWire(String jsonText) {
    if (!isNativeLoaded) return null;
    final b64 = _callUnary(_zstdCompress, jsonText) as String;
    return base64Decode(b64);
  }

  String zstdDecompressWire(Uint8List zstdPayloadWithoutTag) {
    if (!isNativeLoaded) {
      throw StateError('liborbit_ffi required to decompress Zstd wire frame');
    }
    return _callUnary(_zstdDecompress, base64Encode(zstdPayloadWithoutTag)) as String;
  }

  dynamic _callUnary(_UnaryStrDart fn, String arg) {
    final argPtr = arg.toNativeUtf8();
    Pointer<Utf8> outPtr = nullptr;
    try {
      outPtr = fn(argPtr);
      return _unwrapEnvelope(outPtr.toDartString());
    } finally {
      malloc.free(argPtr);
      if (outPtr != nullptr) {
        _freeString(outPtr);
      }
    }
  }

  dynamic _callWithHandle(_FinalizeDart fn, int handle, String arg) {
    final argPtr = arg.toNativeUtf8();
    Pointer<Utf8> outPtr = nullptr;
    try {
      outPtr = fn(handle, argPtr);
      return _unwrapEnvelope(outPtr.toDartString());
    } finally {
      malloc.free(argPtr);
      if (outPtr != nullptr) {
        _freeString(outPtr);
      }
    }
  }

  dynamic _unwrapEnvelope(String rawJson) {
    final env = jsonDecode(rawJson) as Map<String, dynamic>;
    if (env['ok'] == true) {
      return env['data'];
    }
    throw FormatException((env['error'] as String?) ?? 'FFI error');
  }
}
