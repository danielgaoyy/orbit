import 'dart:io';

class SshHostEntry {
  SshHostEntry({
    required this.alias,
    required this.hostName,
    this.user,
    this.port,
  });

  final String alias;
  final String hostName;
  final String? user;
  final int? port;

  String get displayTarget {
    final u = (user != null && user!.isNotEmpty) ? '$user@' : '';
    final p = (port != null && port != 22) ? ':$port' : '';
    return '$u$hostName$p';
  }

  String relayWsUrl(int relayPort) => 'ws://$hostName:$relayPort';
}

class SshConfigService {
  static bool get isDesktopSupported => Platform.isMacOS || Platform.isLinux;

  static String? resolveUserHomeDir() {
    final envHome = Platform.environment['HOME'];
    if (envHome != null && envHome.isNotEmpty) {
      final containerIdx = envHome.indexOf('/Library/Containers/');
      if (containerIdx > 0) {
        return envHome.substring(0, containerIdx);
      }
      return envHome;
    }
    return null;
  }

  /// Auto-discover concrete remote servers from `~/.ssh/config` (and `Include` files).
  static List<SshHostEntry> loadUserSshHosts() {
    if (!isDesktopSupported) return const [];
    final home = resolveUserHomeDir();
    if (home == null) return const [];
    final sshDir = '$home/.ssh';
    final configFile = File('$sshDir/config');
    if (!configFile.existsSync()) return const [];
    return _parseFile(configFile, sshDir: sshDir, home: home, depth: 0);
  }

  /// Pure string parser (used directly by unit tests and file loader).
  static List<SshHostEntry> parseConfigString(
    String raw, {
    String sshDir = '/nonexistent',
    String home = '/nonexistent',
    int depth = 3,
  }) {
    final results = <SshHostEntry>[];
    final currentAliases = <String>[];
    String? curHostName;
    String? curUser;
    int? curPort;

    void flush() {
      for (final alias in currentAliases) {
        final effectiveHost = curHostName ?? alias;
        if (_isIgnoredHost(alias, effectiveHost)) continue;
        if (!results.any((e) => e.alias == alias)) {
          results.add(
            SshHostEntry(
              alias: alias,
              hostName: effectiveHost,
              user: curUser,
              port: curPort,
            ),
          );
        }
      }
      currentAliases.clear();
      curHostName = null;
      curUser = null;
      curPort = null;
    }

    for (final rawLine in raw.split('\n')) {
      final trimmed = _stripInlineComment(rawLine).trim();
      if (trimmed.isEmpty) continue;
      final kv = _splitDirective(trimmed);
      if (kv == null) continue;
      final key = kv.$1.toLowerCase();
      final val = kv.$2.trim().replaceAll('"', '');

      if (key == 'include' && depth < 3) {
        flush();
        for (final pat in val.split(RegExp(r'\s+'))) {
          if (pat.isEmpty) continue;
          for (final incFile in _expandInclude(pat, sshDir: sshDir, home: home)) {
            for (final sub in _parseFile(
              incFile,
              sshDir: sshDir,
              home: home,
              depth: depth + 1,
            )) {
              if (!results.any((e) => e.alias == sub.alias)) {
                results.add(sub);
              }
            }
          }
        }
      } else if (key == 'host') {
        flush();
        for (final tok in val.split(RegExp(r'\s+'))) {
          final clean = tok.replaceAll('"', '').trim();
          if (clean.isNotEmpty &&
              !clean.contains('*') &&
              !clean.contains('?') &&
              !clean.startsWith('!')) {
            currentAliases.add(clean);
          }
        }
      } else if (key == 'match') {
        flush();
      } else if (key == 'hostname' &&
          currentAliases.isNotEmpty &&
          curHostName == null) {
        curHostName = val;
      } else if (key == 'user' &&
          currentAliases.isNotEmpty &&
          curUser == null) {
        curUser = val;
      } else if (key == 'port' &&
          currentAliases.isNotEmpty &&
          curPort == null) {
        curPort = int.tryParse(val);
      }
    }

    flush();
    return results;
  }

  static List<SshHostEntry> _parseFile(
    File file, {
    required String sshDir,
    required String home,
    required int depth,
  }) {
    if (depth > 3) return const [];
    try {
      final raw = file.readAsStringSync();
      return parseConfigString(raw, sshDir: sshDir, home: home, depth: depth);
    } catch (_) {
      return const [];
    }
  }

  static String _stripInlineComment(String line) {
    var inQuotes = false;
    for (var i = 0; i < line.length; i++) {
      final ch = line[i];
      if (ch == '"') {
        inQuotes = !inQuotes;
      } else if (ch == '#' && !inQuotes) {
        return line.substring(0, i);
      }
    }
    return line;
  }

  static (String, String)? _splitDirective(String line) {
    final wsMatch = RegExp(r'\s').firstMatch(line);
    final eqIdx = line.indexOf('=');
    if (eqIdx > 0 && (wsMatch == null || eqIdx < wsMatch.start)) {
      return (line.substring(0, eqIdx).trim(), line.substring(eqIdx + 1).trim());
    }
    if (wsMatch != null) {
      final key = line.substring(0, wsMatch.start).trim();
      var rest = line.substring(wsMatch.start).trimLeft();
      if (rest.startsWith('=')) {
        rest = rest.substring(1).trimLeft();
      }
      return (key, rest);
    }
    return null;
  }

  static bool _isIgnoredHost(String alias, String hostName) {
    final lowerAlias = alias.toLowerCase();
    final lowerHost = hostName.toLowerCase();
    if (lowerHost == 'localhost' ||
        lowerHost == '127.0.0.1' ||
        lowerHost == '::1' ||
        lowerAlias.contains('orbstack')) {
      return true;
    }
    const forges = [
      'github.com',
      'gist.github.com',
      'gitlab.com',
      'bitbucket.org',
      'ssh.dev.azure.com',
      'vs-ssh.visualstudio.com',
    ];
    for (final f in forges) {
      if (lowerHost == f || lowerHost.endsWith('.$f')) {
        return true;
      }
    }
    return false;
  }

  static List<File> _expandInclude(
    String pattern, {
    required String sshDir,
    required String home,
  }) {
    String resolved;
    if (pattern.startsWith('~/')) {
      resolved = '$home/${pattern.substring(2)}';
    } else if (pattern.startsWith('/')) {
      resolved = pattern;
    } else {
      resolved = '$sshDir/$pattern';
    }

    if (!resolved.contains('*') && !resolved.contains('?')) {
      final f = File(resolved);
      return f.existsSync() ? [f] : const [];
    }

    final slashIdx = resolved.lastIndexOf('/');
    if (slashIdx <= 0) return const [];
    final dir = Directory(resolved.substring(0, slashIdx));
    final filePat = resolved.substring(slashIdx + 1);
    if (!dir.existsSync()) return const [];

    try {
      final regexStr =
          '^${RegExp.escape(filePat).replaceAll(r'\*', '.*').replaceAll(r'\?', '.')}\$';
      final regex = RegExp(regexStr);
      final files = dir
          .listSync()
          .whereType<File>()
          .where((f) {
            final name = f.uri.pathSegments.isNotEmpty
                ? f.uri.pathSegments.last
                : '';
            return !name.startsWith('.') && regex.hasMatch(name);
          })
          .toList()
        ..sort((a, b) => a.path.compareTo(b.path));
      return files;
    } catch (_) {
      return const [];
    }
  }

  /// One-click SSH Relay deployment: connects to `entry.alias` using system `ssh` + `~/.ssh/config`,
  /// installs/updates Orbit on the server, starts `orbit relay up` locked to `allowedPubkeys`,
  /// and returns the ready `ws://...` Relay URL.
  static Future<String> provisionRelayOverSsh({
    required SshHostEntry entry,
    required int relayPort,
    required List<String> allowedPubkeys,
    required void Function(String step) onStep,
  }) async {
    onStep('正在通过 SSH 连接 ${entry.alias} (${entry.displayTarget})…');

    final pubkeyFlags = allowedPubkeys
        .map((k) => k.replaceAll("'", '').trim())
        .where((k) => k.isNotEmpty)
        .toSet()
        .map((k) => "--allow-pubkey '$k'")
        .join(' ');

    final remoteScript =
        'set -eu; '
        'export PATH="\$HOME/.local/bin:/usr/local/bin:\$PATH"; '
        'if ! command -v orbit >/dev/null 2>&1 || ! orbit relay --help 2>&1 | grep -q "allow"; then '
        '  curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh; '
        'fi; '
        'orbit relay up --port $relayPort $pubkeyFlags';

    final sshBin = File('/usr/bin/ssh').existsSync() ? '/usr/bin/ssh' : 'ssh';
    final result = await Process.run(
      sshBin,
      [
        '-o',
        'BatchMode=yes',
        '-o',
        'StrictHostKeyChecking=accept-new',
        '-o',
        'ConnectTimeout=12',
        entry.alias,
        remoteScript,
      ],
    );

    final stdoutStr = (result.stdout ?? '').toString();
    final stderrStr = (result.stderr ?? '').toString();

    if (result.exitCode != 0) {
      throw Exception(
        stderrStr.trim().isNotEmpty
            ? stderrStr.trim()
            : 'SSH exited with code ${result.exitCode}',
      );
    }

    onStep('远端 Orbit E2EE 中继已在 ${entry.alias} 启动并锁定公钥白名单！');

    for (final line in stdoutStr.split('\n')) {
      final trimmed = line.trim();
      if (trimmed.startsWith('Relay URL')) {
        final colonIdx = trimmed.indexOf(':');
        if (colonIdx > 0) {
          final url = trimmed.substring(colonIdx + 1).trim();
          if (url.isNotEmpty && !url.contains('<YOUR_SERVER_IP>')) {
            return url;
          }
        }
      }
    }

    return entry.relayWsUrl(relayPort);
  }

  /// Zero-form 1-click SSH Host Connection:
  /// Connects to `entry.alias`, installs/upgrades `orbit` if missing, starts `orbit daemon up`,
  /// and returns the `orbit://pair/...` URI automatically without any manual terminal steps!
  static Future<String> bootstrapDaemonAndFetchPairUri({
    required SshHostEntry entry,
    void Function(String step)? onStep,
  }) async {
    onStep?.call('正在通过 SSH 连接 ${entry.alias}…');

    const remoteScript =
        'set -eu; '
        'export PATH="\$HOME/.local/bin:/usr/local/bin:/opt/homebrew/bin:\$PATH"; '
        'if ! command -v orbit >/dev/null 2>&1; then '
        '  curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh >/dev/null 2>&1; '
        'fi; '
        'orbit daemon up >/dev/null 2>&1 || true; '
        'orbit pair';

    final sshBin = File('/usr/bin/ssh').existsSync() ? '/usr/bin/ssh' : 'ssh';
    final result = await Process.run(
      sshBin,
      [
        '-o',
        'BatchMode=yes',
        '-o',
        'StrictHostKeyChecking=accept-new',
        '-o',
        'ConnectTimeout=12',
        entry.alias,
        remoteScript,
      ],
    );

    final stdoutStr = (result.stdout ?? '').toString();
    final stderrStr = (result.stderr ?? '').toString();
    if (result.exitCode != 0) {
      throw Exception(
        stderrStr.trim().isNotEmpty
            ? stderrStr.trim()
            : 'SSH exited with code ${result.exitCode}',
      );
    }

    for (final line in stdoutStr.split('\n')) {
      final trimmed = line.trim();
      if (trimmed.startsWith('orbit://pair/')) {
        return trimmed;
      }
    }
    throw Exception('未在 ${entry.alias} 获取到有效配对信息');
  }

  /// Open a background SSH local port-forward (`127.0.0.1:<freePort> -> 127.0.0.1:6868`)
  /// so Desktop clients connect through existing SSH keys without opening firewall ports or configuring relays!
  static Future<(Process, String)> startLocalSshTunnel(String sshAlias) async {
    final probe = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
    final localPort = probe.port;
    await probe.close();

    final sshBin = File('/usr/bin/ssh').existsSync() ? '/usr/bin/ssh' : 'ssh';
    final proc = await Process.start(
      sshBin,
      [
        '-N',
        '-o',
        'BatchMode=yes',
        '-o',
        'StrictHostKeyChecking=accept-new',
        '-o',
        'ExitOnForwardFailure=yes',
        '-o',
        'ServerAliveInterval=15',
        '-o',
        'ServerAliveCountMax=3',
        '-o',
        'ConnectTimeout=10',
        '-L',
        '127.0.0.1:$localPort:127.0.0.1:6868',
        sshAlias,
      ],
    );

    // Wait up to 4.5s for the forwarded local port to accept TCP connections
    for (var i = 0; i < 30; i++) {
      await Future<void>.delayed(const Duration(milliseconds: 150));
      try {
        final sock = await Socket.connect(
          InternetAddress.loopbackIPv4,
          localPort,
          timeout: const Duration(milliseconds: 200),
        );
        sock.destroy();
        return (proc, 'ws://127.0.0.1:$localPort/ws');
      } catch (_) {}
    }

    proc.kill();
    throw Exception('SSH 隧道建立超时 ($sshAlias)');
  }
}
