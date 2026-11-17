import 'dart:async';
import 'dart:convert';
import 'dart:io';

const String kOrbitAppVersion = '0.1.4';
const String kOrbitGithubRepo = 'danielgaoyy/orbit';

class ReleaseUpdateInfo {
  ReleaseUpdateInfo({
    required this.tagName,
    required this.version,
    required this.htmlUrl,
    required this.notes,
    required this.hasNewerVersion,
    this.macosDmgUrl,
    this.linuxArm64TarUrl,
    this.linuxX64TarUrl,
    this.androidApkUrl,
    this.iosIpaUrl,
  });

  final String tagName;
  final String version;
  final String htmlUrl;
  final String notes;
  final bool hasNewerVersion;
  final String? macosDmgUrl;
  final String? linuxArm64TarUrl;
  final String? linuxX64TarUrl;
  final String? androidApkUrl;
  final String? iosIpaUrl;

  factory ReleaseUpdateInfo.fromGithubJson(
    Map<String, dynamic> json, {
    String currentVersion = kOrbitAppVersion,
  }) {
    final tag = (json['tag_name'] as String? ?? 'v0.0.0').trim();
    final cleanVer = tag.startsWith('v') ? tag.substring(1) : tag;
    final htmlUrl = (json['html_url'] as String?) ??
        'https://github.com/$kOrbitGithubRepo/releases/latest';
    final notes = (json['body'] as String? ?? '').trim();

    String? dmgUrl;
    String? linuxArmUrl;
    String? linuxX64Url;
    String? apkUrl;
    String? ipaUrl;
    final assets = json['assets'] as List<dynamic>? ?? const [];
    for (final item in assets) {
      if (item is! Map) continue;
      final name = item['name']?.toString() ?? '';
      final url = item['browser_download_url']?.toString();
      if (url == null || url.isEmpty) continue;
      if (name.endsWith('.dmg')) dmgUrl = url;
      if (name == 'Orbit-Linux-arm64.tar.gz') linuxArmUrl = url;
      if (name == 'Orbit-Linux-x64.tar.gz') linuxX64Url = url;
      if (name.endsWith('.apk')) apkUrl = url;
      if (name.endsWith('.ipa')) ipaUrl = url;
    }

    return ReleaseUpdateInfo(
      tagName: tag,
      version: cleanVer,
      htmlUrl: htmlUrl,
      notes: notes,
      hasNewerVersion: AppUpdater.compareSemver(cleanVer, currentVersion) > 0,
      macosDmgUrl: dmgUrl ??
          'https://github.com/$kOrbitGithubRepo/releases/download/$tag/Orbit-macOS-AppleSilicon.dmg',
      linuxArm64TarUrl: linuxArmUrl ??
          'https://github.com/$kOrbitGithubRepo/releases/download/$tag/Orbit-Linux-arm64.tar.gz',
      linuxX64TarUrl: linuxX64Url ??
          'https://github.com/$kOrbitGithubRepo/releases/download/$tag/Orbit-Linux-x64.tar.gz',
      androidApkUrl: apkUrl ??
          'https://github.com/$kOrbitGithubRepo/releases/download/$tag/Orbit-Android-arm64.apk',
      iosIpaUrl: ipaUrl,
    );
  }
}

class AppUpdater {
  /// Compare two semantic version strings (`a` vs `b`). Returns `> 0` if `a > b`, `0` if equal, `< 0` if `a < b`.
  static int compareSemver(String a, String b) {
    List<int> parseParts(String v) {
      var s = v.trim();
      if (s.startsWith('v') || s.startsWith('V')) {
        s = s.substring(1);
      }
      s = s.split('+').first.split('-').first;
      final parts = s.split('.');
      return [
        parts.isNotEmpty ? (int.tryParse(parts[0]) ?? 0) : 0,
        parts.length > 1 ? (int.tryParse(parts[1]) ?? 0) : 0,
        parts.length > 2 ? (int.tryParse(parts[2]) ?? 0) : 0,
      ];
    }

    final pa = parseParts(a);
    final pb = parseParts(b);
    for (var i = 0; i < 3; i++) {
      if (pa[i] != pb[i]) return pa[i].compareTo(pb[i]);
    }
    return 0;
  }

  /// Fetch latest GitHub release metadata for `danielgaoyy/orbit`.
  static Future<ReleaseUpdateInfo> checkLatestRelease({
    String currentVersion = kOrbitAppVersion,
  }) async {
    final client = HttpClient()..connectionTimeout = const Duration(seconds: 8);
    try {
      final uri = Uri.parse(
        'https://api.github.com/repos/$kOrbitGithubRepo/releases/latest',
      );
      final req = await client.getUrl(uri);
      req.headers.set('Accept', 'application/vnd.github+json');
      req.headers.set('User-Agent', 'Orbit-App/$currentVersion');
      final resp = await req.close().timeout(const Duration(seconds: 10));
      final body = await resp.transform(utf8.decoder).join();
      if (resp.statusCode < 200 || resp.statusCode >= 300) {
        throw Exception('GitHub API HTTP ${resp.statusCode}');
      }
      final decoded = jsonDecode(body) as Map<String, dynamic>;
      return ReleaseUpdateInfo.fromGithubJson(
        decoded,
        currentVersion: currentVersion,
      );
    } finally {
      client.close(force: true);
    }
  }

  /// Perform 1-click download, atomic installation, and automatic relaunch.
  static Future<void> performOneClickUpdate({
    required ReleaseUpdateInfo release,
    required void Function(double progress, String status) onProgress,
  }) async {
    if (Platform.isMacOS) {
      await _updateMacosDmgAndRelaunch(release, onProgress);
      return;
    }
    if (Platform.isLinux) {
      await _updateLinuxDesktopAndRelaunch(release, onProgress);
      return;
    }
    if (Platform.isAndroid) {
      final url = release.androidApkUrl ?? release.htmlUrl;
      onProgress(0.5, '正在拉起系统浏览器下载最新 APK…');
      await Process.run('am', [
        'start',
        '-a',
        'android.intent.action.VIEW',
        '-d',
        url,
      ]);
      onProgress(1.0, '请在系统下载完成后点击安装更新。');
      return;
    }
    throw Exception('iOS 请通过 TestFlight 或侧载工具安装 ${release.tagName}');
  }

  static String _resolveTargetMacosAppBundle() {
    final exe = Platform.resolvedExecutable;
    final appIdx = exe.indexOf('.app/');
    if (appIdx > 0) {
      final candidate = exe.substring(0, appIdx + 4);
      // If user ran Orbit directly from a read-only mounted DMG (`/Volumes/...`), install to `/Applications/Orbit.app`!
      if (!candidate.startsWith('/Volumes/')) {
        return candidate;
      }
    }
    return '/Applications/Orbit.app';
  }

  static Future<void> _updateMacosDmgAndRelaunch(
    ReleaseUpdateInfo release,
    void Function(double progress, String status) onProgress,
  ) async {
    final dmgUrl = release.macosDmgUrl;
    if (dmgUrl == null || dmgUrl.isEmpty) {
      throw Exception('未找到 macOS 安装包下载链接');
    }

    final tmpDir = Directory.systemTemp.path;
    final myPid = pid;
    final dmgPath = '$tmpDir/Orbit-${release.version}-$myPid.dmg';
    final mountDir = '$tmpDir/orbit_mnt_$myPid';

    onProgress(0.02, '正在连接 GitHub Release 下载 ${release.tagName}…');

    final client = HttpClient()
      ..connectionTimeout = const Duration(seconds: 15);
    try {
      final req = await client.getUrl(Uri.parse(dmgUrl));
      req.headers.set('User-Agent', 'Orbit-App/$kOrbitAppVersion');
      final resp = await req.close();
      if (resp.statusCode < 200 || resp.statusCode >= 300) {
        throw Exception('下载失败 (HTTP ${resp.statusCode})');
      }

      final totalBytes = resp.contentLength;
      var receivedBytes = 0;
      final outFile = File(dmgPath);
      final sink = outFile.openWrite();
      try {
        await for (final chunk in resp) {
          sink.add(chunk);
          receivedBytes += chunk.length;
          if (totalBytes > 0) {
            final ratio = (receivedBytes / totalBytes).clamp(0.0, 1.0);
            final mbDone = (receivedBytes / (1024 * 1024)).toStringAsFixed(1);
            final mbTotal = (totalBytes / (1024 * 1024)).toStringAsFixed(1);
            onProgress(
              0.05 + ratio * 0.80,
              '正在下载 ${release.tagName} ($mbDone / $mbTotal MB)…',
            );
          } else {
            final mbDone = (receivedBytes / (1024 * 1024)).toStringAsFixed(1);
            onProgress(0.5, '正在下载 ${release.tagName} ($mbDone MB)…');
          }
        }
      } finally {
        await sink.flush();
        await sink.close();
      }
    } finally {
      client.close(force: true);
    }

    onProgress(0.90, '正在挂载校验安装包…');
    await Directory(mountDir).create(recursive: true);
    final attachRes = await Process.run('hdiutil', [
      'attach',
      '-nobrowse',
      '-readonly',
      '-mountpoint',
      mountDir,
      dmgPath,
    ]);
    if (attachRes.exitCode != 0) {
      throw Exception('挂载 DMG 失败: ${attachRes.stderr}');
    }

    final mountedApp = Directory('$mountDir/Orbit.app');
    if (!mountedApp.existsSync()) {
      await Process.run('hdiutil', ['detach', mountDir, '-force', '-quiet']);
      throw Exception('安装包内未找到 Orbit.app');
    }

    final targetApp = _resolveTargetMacosAppBundle();
    onProgress(0.98, '正在自动替换 $targetApp 并重启…');

    final sqTarget = targetApp.replaceAll("'", r"'\''");
    final sqMount = mountDir.replaceAll("'", r"'\''");
    final sqDmg = dmgPath.replaceAll("'", r"'\''");

    final updaterScript = '''
(
  for i in 1 2 3 4 5 6 7 8 9 10 11 12; do
    kill -0 $myPid 2>/dev/null || break
    sleep 0.25
  done
  DEST='$sqTarget'
  if ! (rm -rf "\$DEST" && ditto '$sqMount/Orbit.app' "\$DEST") 2>/dev/null; then
    mkdir -p "\$HOME/Applications"
    DEST="\$HOME/Applications/Orbit.app"
    rm -rf "\$DEST"
    ditto '$sqMount/Orbit.app' "\$DEST"
  fi
  xattr -cr "\$DEST" 2>/dev/null || true
  hdiutil detach '$sqMount' -quiet 2>/dev/null || hdiutil detach '$sqMount' -force -quiet 2>/dev/null || true
  rm -f '$sqDmg'
  open "\$DEST"
) >/dev/null 2>&1 &
''';

    await Process.start(
      '/bin/sh',
      ['-c', updaterScript],
      mode: ProcessStartMode.detached,
    );

    await Future<void>.delayed(const Duration(milliseconds: 250));
    exit(0);
  }

  static Future<void> _updateLinuxDesktopAndRelaunch(
    ReleaseUpdateInfo release,
    void Function(double progress, String status) onProgress,
  ) async {
    final archRes = await Process.run('uname', ['-m']);
    final arch = (archRes.stdout ?? '').toString().trim();
    final tarUrl = (arch == 'aarch64' || arch == 'arm64')
        ? release.linuxArm64TarUrl
        : release.linuxX64TarUrl;
    if (tarUrl == null || tarUrl.isEmpty) {
      throw Exception('未找到 Linux ($arch) 桌面端安装包下载链接');
    }

    final tmpDir = Directory.systemTemp.path;
    final myPid = pid;
    final tarPath = '$tmpDir/Orbit-linux-${release.version}-$myPid.tar.gz';
    final stageDir = '$tmpDir/orbit_linux_stage_$myPid';

    onProgress(0.02, '正在下载 Linux 桌面端 ${release.tagName}…');
    final client = HttpClient()
      ..connectionTimeout = const Duration(seconds: 15);
    try {
      final req = await client.getUrl(Uri.parse(tarUrl));
      req.headers.set('User-Agent', 'Orbit-App/$kOrbitAppVersion');
      final resp = await req.close();
      if (resp.statusCode < 200 || resp.statusCode >= 300) {
        throw Exception('下载失败 (HTTP ${resp.statusCode})');
      }

      final totalBytes = resp.contentLength;
      var receivedBytes = 0;
      final sink = File(tarPath).openWrite();
      try {
        await for (final chunk in resp) {
          sink.add(chunk);
          receivedBytes += chunk.length;
          if (totalBytes > 0) {
            final ratio = (receivedBytes / totalBytes).clamp(0.0, 1.0);
            final mbDone = (receivedBytes / (1024 * 1024)).toStringAsFixed(1);
            final mbTotal = (totalBytes / (1024 * 1024)).toStringAsFixed(1);
            onProgress(
              0.05 + ratio * 0.80,
              '正在下载 ${release.tagName} ($mbDone / $mbTotal MB)…',
            );
          }
        }
      } finally {
        await sink.flush();
        await sink.close();
      }
    } finally {
      client.close(force: true);
    }

    onProgress(0.90, '正在解压校验 Linux 桌面端安装包…');
    await Directory(stageDir).create(recursive: true);
    final untar = await Process.run('tar', ['-xzf', tarPath, '-C', stageDir]);
    if (untar.exitCode != 0 ||
        !File('$stageDir/orbit-desktop').existsSync()) {
      throw Exception('解压安装包失败: ${untar.stderr}');
    }

    final exeFile = File(Platform.resolvedExecutable);
    final home = Platform.environment['HOME'] ?? '';
    final targetDir = exeFile.path.endsWith('/orbit-desktop')
        ? exeFile.parent.path
        : '$home/.local/opt/orbit';

    onProgress(0.98, '正在自动替换并重启 Orbit 桌面端…');
    final sqStage = stageDir.replaceAll("'", r"'\''");
    final sqTarget = targetDir.replaceAll("'", r"'\''");
    final sqTar = tarPath.replaceAll("'", r"'\''");

    final updaterScript = '''
(
  for i in 1 2 3 4 5 6 7 8 9 10 11 12; do
    kill -0 $myPid 2>/dev/null || break
    sleep 0.25
  done
  mkdir -p '$sqTarget'
  rm -rf '$sqTarget'/*
  cp -a '$sqStage'/. '$sqTarget'/
  chmod +x '$sqTarget/orbit-desktop'
  rm -rf '$sqStage' '$sqTar'
  nohup '$sqTarget/orbit-desktop' >/dev/null 2>&1 &
) >/dev/null 2>&1 &
''';

    await Process.start(
      '/bin/sh',
      ['-c', updaterScript],
      mode: ProcessStartMode.detached,
    );

    await Future<void>.delayed(const Duration(milliseconds: 250));
    exit(0);
  }
}
