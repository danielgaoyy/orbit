#!/usr/bin/env node
/**
 * HarmonyOS PR-1 stream replay performance report generator (§8.1 / §8.2).
 *
 * Parses:
 *  - `stream-replay.ftrace` (`H:RSMainThread::DoComposition` / `H:ReceiveVsync`) for P1
 *  - `orbit_perf_hilog.txt` (`OrbitPerf` metric/mark lines) for P2, P3, P4, P7
 *
 * Emits `perf-report.json` + Markdown summary table (stdout & `perf-report.md`).
 * Exits with code 1 if any required PR-1/PR-3 threshold fails or required input is missing.
 */

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const HARMONY_DIR = path.resolve(SCRIPT_DIR, '..', '..');
const DEFAULT_OUT_DIR = path.join(HARMONY_DIR, 'build', 'perf_artifacts');

const P2_STAGE_TICK_P95_LIMIT_MS = 4.0;
const P3_AUX_UNCHANGED_P50_LIMIT_BYTES = 8 * 1024;
const P3_AUX_UNCHANGED_P95_LIMIT_BYTES = 16 * 1024;
const P1_MAX_DROP_BURSTS_PER_MIN = 1.0;
const IDLE_GAP_FILTER_MS = 2000.0;

function parseArgs(argv) {
  const opts = {
    dir: DEFAULT_OUT_DIR,
    trace: '',
    hilog: '',
    meta: '',
    out: '',
    mdOut: '',
    refreshHz: 0,
  };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--dir' && i + 1 < argv.length) {
      opts.dir = path.resolve(argv[++i]);
    } else if (arg === '--trace' && i + 1 < argv.length) {
      opts.trace = path.resolve(argv[++i]);
    } else if (arg === '--hilog' && i + 1 < argv.length) {
      opts.hilog = path.resolve(argv[++i]);
    } else if (arg === '--meta' && i + 1 < argv.length) {
      opts.meta = path.resolve(argv[++i]);
    } else if (arg === '--out' && i + 1 < argv.length) {
      opts.out = path.resolve(argv[++i]);
    } else if (arg === '--md-out' && i + 1 < argv.length) {
      opts.mdOut = path.resolve(argv[++i]);
    } else if (arg === '--refresh-hz' && i + 1 < argv.length) {
      opts.refreshHz = Number(argv[++i]) || 0;
    } else if (arg === '--help' || arg === '-h') {
      console.log(
        'Usage: node report.mjs [--dir DIR] [--trace FILE] [--hilog FILE] [--meta FILE] [--out FILE] [--md-out FILE] [--refresh-hz 60|120]'
      );
      process.exit(0);
    } else {
      console.error(`Unknown argument: ${arg}`);
      process.exit(2);
    }
  }
  opts.trace = opts.trace || path.join(opts.dir, 'stream-replay.ftrace');
  opts.hilog = opts.hilog || path.join(opts.dir, 'orbit_perf_hilog.txt');
  opts.meta = opts.meta || path.join(opts.dir, 'run_meta.json');
  opts.out = opts.out || path.join(opts.dir, 'perf-report.json');
  opts.mdOut = opts.mdOut || path.join(opts.dir, 'perf-report.md');
  return opts;
}

function percentileNearestRank(sortedValues, ratio) {
  if (sortedValues.length === 0) {
    return null;
  }
  const rank = Math.ceil(sortedValues.length * ratio) - 1;
  const idx = Math.max(0, Math.min(sortedValues.length - 1, rank));
  return sortedValues[idx];
}

function round2(n) {
  if (n === null || n === undefined || !Number.isFinite(n)) {
    return null;
  }
  return Math.round(n * 100) / 100;
}

function parseFtraceFile(tracePath, explicitRefreshHz) {
  if (!fs.existsSync(tracePath)) {
    return {
      present: false,
      error: `Trace file not found: ${tracePath}`,
      source: 'none',
      refreshHz: explicitRefreshHz || 60,
      budgetMs: (explicitRefreshHz || 60) >= 90 ? 8.3 : 16.6,
      sampleCount: 0,
      skippedIdleGaps: 0,
      durationSec: 0,
      p50Ms: null,
      p95Ms: null,
      p99Ms: null,
      maxMs: null,
      dropBurstCount: 0,
      dropBurstsPerMin: null,
    };
  }

  const content = fs.readFileSync(tracePath, 'utf8');
  const lines = content.split(/\r?\n/);

  const doCompTs = [];
  const vsyncTs = [];

  // Standard Linux/OpenHarmony ftrace line:
  // <proc>-<pid> (<tgid>) [cpu] flags <timestamp_sec>: tracing_mark_write: <B|E|S|C>|<pid>|<name>
  const lineRe = /(\d+\.\d+):\s+tracing_mark_write:\s+([BESC])\|\d+\|([^\r\n]+)/;

  for (let i = 0; i < lines.length; i++) {
    const m = lineRe.exec(lines[i]);
    if (!m) {
      continue;
    }
    const tsSec = Number.parseFloat(m[1]);
    const phase = m[2];
    const tag = m[3];
    if (!Number.isFinite(tsSec)) {
      continue;
    }
    if (phase === 'B' || phase === 'S') {
      if (tag.includes('H:RSMainThread::DoComposition') || tag.includes('RSMainThread::DoComposition')) {
        doCompTs.push(tsSec);
      } else if (tag.includes('H:ReceiveVsync') || tag.includes('ReceiveVsync')) {
        vsyncTs.push(tsSec);
      }
    }
  }

  const source = doCompTs.length >= 2 ? 'H:RSMainThread::DoComposition' : (vsyncTs.length >= 2 ? 'H:ReceiveVsync' : 'none');
  const activeTs = source === 'H:RSMainThread::DoComposition' ? doCompTs : vsyncTs;
  activeTs.sort((a, b) => a - b);

  // Determine refresh rate (explicit > inferred from vsync > 60 Hz default)
  let refreshHz = explicitRefreshHz;
  if (!refreshHz && vsyncTs.length >= 4) {
    const sortedVsync = vsyncTs.slice().sort((a, b) => a - b);
    const vDeltas = [];
    for (let i = 1; i < sortedVsync.length; i++) {
      const dt = (sortedVsync[i] - sortedVsync[i - 1]) * 1000;
      if (dt > 4 && dt < 40) {
        vDeltas.push(dt);
      }
    }
    if (vDeltas.length > 0) {
      vDeltas.sort((a, b) => a - b);
      const med = percentileNearestRank(vDeltas, 0.5);
      refreshHz = med !== null && med <= 11.5 ? 120 : 60;
    }
  }
  if (!refreshHz) {
    refreshHz = 60;
  }
  const budgetMs = refreshHz >= 90 ? 8.3 : 16.6;

  const intervalsMs = [];
  let skippedIdleGaps = 0;
  for (let i = 1; i < activeTs.length; i++) {
    const dtMs = (activeTs[i] - activeTs[i - 1]) * 1000;
    if (dtMs <= 0.1) {
      continue;
    }
    if (dtMs > IDLE_GAP_FILTER_MS) {
      skippedIdleGaps += 1;
      continue;
    }
    intervalsMs.push(dtMs);
  }

  if (intervalsMs.length === 0) {
    return {
      present: true,
      error: `No valid frame intervals found in ${tracePath}`,
      source,
      refreshHz,
      budgetMs,
      sampleCount: 0,
      skippedIdleGaps,
      durationSec: 0,
      p50Ms: null,
      p95Ms: null,
      p99Ms: null,
      maxMs: null,
      dropBurstCount: 0,
      dropBurstsPerMin: null,
    };
  }

  // Count consecutive dropped-frame bursts (> 2 frames dropped):
  // Either a single frame interval exceeding 3 * budgetMs (more than 2 missed vsyncs),
  // or a run of >= 3 consecutive intervals each exceeding 1.25 * budgetMs.
  let dropBurstCount = 0;
  let slowStreak = 0;
  let totalActiveMs = 0;
  for (let i = 0; i < intervalsMs.length; i++) {
    const dt = intervalsMs[i];
    totalActiveMs += dt;
    if (dt > budgetMs * 3.0) {
      dropBurstCount += 1;
      slowStreak = 0;
    } else if (dt > budgetMs * 1.25) {
      slowStreak += 1;
      if (slowStreak === 3) {
        dropBurstCount += 1;
      }
    } else {
      slowStreak = 0;
    }
  }

  const sorted = intervalsMs.slice().sort((a, b) => a - b);
  const durationSec = totalActiveMs / 1000;
  const durationMin = Math.max(durationSec / 60, 1 / 60);
  const dropBurstsPerMin = dropBurstCount / durationMin;

  return {
    present: true,
    error: null,
    source,
    refreshHz,
    budgetMs,
    sampleCount: sorted.length,
    skippedIdleGaps,
    durationSec: round2(durationSec),
    p50Ms: round2(percentileNearestRank(sorted, 0.5)),
    p95Ms: round2(percentileNearestRank(sorted, 0.95)),
    p99Ms: round2(percentileNearestRank(sorted, 0.99)),
    maxMs: round2(sorted[sorted.length - 1]),
    dropBurstCount,
    dropBurstsPerMin: round2(dropBurstsPerMin),
  };
}

function parseOrbitPerfHilog(hilogPath) {
  if (!fs.existsSync(hilogPath)) {
    return {
      present: false,
      error: `OrbitPerf hilog file not found: ${hilogPath}`,
      metrics: {},
      marks: {},
      crashLines: [],
    };
  }

  const content = fs.readFileSync(hilogPath, 'utf8');
  const lines = content.split(/\r?\n/);

  const metricRe =
    /metric=([^\s]+)\s+count=(\d+)\s+samples=(\d+)\s+p50_us=(\d+)\s+p95_us=(\d+)\s+bytes=(\d+)\s+max_bytes=(\d+)(?:\s+p50_bytes=(\d+)\s+p95_bytes=(\d+))?/;
  const markRe = /mark=([^\s]+)\s+ts_us=(\d+)\s+since_start_ms=([\d.]+)/;
  const crashRe = /\b(JSCRASH|CPP_CRASH|Fatal)\b/;

  const metrics = {};
  const marks = {};
  const crashLines = [];

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (!line) {
      continue;
    }
    if (crashRe.test(line)) {
      crashLines.push(line.trim());
    }
    const mm = metricRe.exec(line);
    if (mm) {
      const name = mm[1];
      const count = Number.parseInt(mm[2], 10);
      const samples = Number.parseInt(mm[3], 10);
      const p50Us = Number.parseInt(mm[4], 10);
      const p95Us = Number.parseInt(mm[5], 10);
      const bytes = Number.parseInt(mm[6], 10);
      const maxBytes = Number.parseInt(mm[7], 10);
      const avgBytes = count > 0 ? Math.round(bytes / count) : 0;
      const p50Bytes = mm[8] !== undefined ? Number.parseInt(mm[8], 10) : avgBytes;
      const p95Bytes = mm[9] !== undefined ? Number.parseInt(mm[9], 10) : maxBytes;

      if (!metrics[name]) {
        metrics[name] = {
          windows: 0,
          totalCount: 0,
          totalSamples: 0,
          totalBytes: 0,
          maxBytes: 0,
          p50UsMax: 0,
          p95UsMax: 0,
          p50BytesMax: 0,
          p95BytesMax: 0,
          weightedP50UsSum: 0,
          weightedP50BytesSum: 0,
        };
      }
      const agg = metrics[name];
      agg.windows += 1;
      agg.totalCount += count;
      agg.totalSamples += samples;
      agg.totalBytes += bytes;
      agg.maxBytes = Math.max(agg.maxBytes, maxBytes);
      agg.p50UsMax = Math.max(agg.p50UsMax, p50Us);
      agg.p95UsMax = Math.max(agg.p95UsMax, p95Us);
      agg.p50BytesMax = Math.max(agg.p50BytesMax, p50Bytes);
      agg.p95BytesMax = Math.max(agg.p95BytesMax, p95Bytes);
      const weight = Math.max(samples, count, 1);
      agg.weightedP50UsSum += p50Us * weight;
      agg.weightedP50BytesSum += p50Bytes * weight;
      continue;
    }

    const mk = markRe.exec(line);
    if (mk) {
      const name = mk[1];
      marks[name] = {
        tsUs: Number.parseInt(mk[2], 10),
        sinceStartMs: Number.parseFloat(mk[3]),
      };
    }
  }

  const normalizedMetrics = {};
  for (const name of Object.keys(metrics)) {
    const agg = metrics[name];
    const denom = Math.max(agg.totalSamples, agg.totalCount, 1);
    normalizedMetrics[name] = {
      windows: agg.windows,
      count: agg.totalCount,
      samples: agg.totalSamples,
      p50Us: Math.round(agg.weightedP50UsSum / denom),
      p95Us: agg.p95UsMax,
      p50Ms: round2((agg.weightedP50UsSum / denom) / 1000),
      p95Ms: round2(agg.p95UsMax / 1000),
      totalBytes: agg.totalBytes,
      p50Bytes: Math.round(agg.weightedP50BytesSum / denom),
      p95Bytes: agg.p95BytesMax,
      maxBytes: agg.maxBytes,
    };
  }

  return {
    present: true,
    error: null,
    metrics: normalizedMetrics,
    marks,
    crashLines,
  };
}

function buildMarkdownReport(report) {
  const lines = [];
  lines.push('# HarmonyOS Stream Replay Performance Report');
  lines.push('');
  lines.push(`- **Generated**: ${report.generatedAtIso}`);
  lines.push(`- **Overall Status**: **${report.passed ? 'PASS' : 'FAIL'}**`);
  lines.push(`- **Display Refresh**: ${report.frame.refreshHz} Hz (budget ≤ ${report.frame.budgetMs} ms)`);
  lines.push('');
  lines.push('| Metric | Target | Measured | Status |');
  lines.push('| --- | --- | --- | --- |');
  for (const check of report.checks) {
    lines.push(`| ${check.id} ${check.name} | ${check.target} | ${check.measured} | ${check.passed ? 'PASS' : 'FAIL'} |`);
  }
  lines.push('');

  if (report.coldStart && Object.keys(report.coldStart).length > 0) {
    lines.push('## Cold Start Markers (P7)');
    lines.push('');
    lines.push('| Marker | Since Start (ms) |');
    lines.push('| --- | --- |');
    for (const key of Object.keys(report.coldStart)) {
      lines.push(`| \`${key}\` | ${report.coldStart[key].sinceStartMs} |`);
    }
    lines.push('');
  }

  if (report.failures.length > 0) {
    lines.push('## Failures');
    lines.push('');
    for (const f of report.failures) {
      lines.push(`- ${f}`);
    }
    lines.push('');
  }

  return lines.join('\n');
}

function main() {
  const opts = parseArgs(process.argv.slice(2));

  let meta = {};
  if (fs.existsSync(opts.meta)) {
    try {
      meta = JSON.parse(fs.readFileSync(opts.meta, 'utf8'));
    } catch (_err) {
      meta = {};
    }
  }
  const refreshHz = opts.refreshHz || Number(meta.refreshHz) || 0;

  const frame = parseFtraceFile(opts.trace, refreshHz);
  const hilog = parseOrbitPerfHilog(opts.hilog);

  const stageTick = hilog.metrics['stage.tick'] || null;
  const auxUnchanged = hilog.metrics['stage.delta.aux_unchanged'] || null;
  const sinceZeroUnexpected =
    (hilog.metrics['stage.delta.since_zero.unexpected']?.count || 0) +
    (hilog.metrics['stage.delta.since_zero.fleet']?.count || 0);
  const sinceZeroReload = hilog.metrics['stage.delta.since_zero.reload']?.count || 0;

  const p1Pass =
    frame.present &&
    frame.sampleCount > 0 &&
    frame.p95Ms !== null &&
    frame.p95Ms <= frame.budgetMs &&
    frame.dropBurstsPerMin !== null &&
    frame.dropBurstsPerMin <= P1_MAX_DROP_BURSTS_PER_MIN;

  const p2Pass =
    hilog.present &&
    stageTick !== null &&
    stageTick.p95Ms !== null &&
    stageTick.p95Ms < P2_STAGE_TICK_P95_LIMIT_MS;

  const p3Pass =
    hilog.present &&
    auxUnchanged !== null &&
    auxUnchanged.p50Bytes < P3_AUX_UNCHANGED_P50_LIMIT_BYTES &&
    auxUnchanged.p95Bytes < P3_AUX_UNCHANGED_P95_LIMIT_BYTES;

  const p4Pass = hilog.present && sinceZeroUnexpected === 0 && hilog.crashLines.length === 0;

  const checks = [
    {
      id: 'P1',
      name: 'Stream Frame Interval',
      target: `p95 <= ${frame.budgetMs} ms, drop bursts (>2f) <= ${P1_MAX_DROP_BURSTS_PER_MIN}/min`,
      measured: frame.sampleCount > 0
        ? `p50=${frame.p50Ms}ms, p95=${frame.p95Ms}ms, p99=${frame.p99Ms}ms, max=${frame.maxMs}ms, drops=${frame.dropBurstCount} (${frame.dropBurstsPerMin}/min, n=${frame.sampleCount}, ${frame.source})`
        : (frame.error || 'no trace samples'),
      passed: p1Pass,
    },
    {
      id: 'P2',
      name: 'Stage Tick Main Thread',
      target: `p95 < ${P2_STAGE_TICK_P95_LIMIT_MS} ms`,
      measured: stageTick
        ? `p50=${stageTick.p50Ms}ms, p95=${stageTick.p95Ms}ms (n=${stageTick.count})`
        : 'missing stage.tick metric',
      passed: p2Pass,
    },
    {
      id: 'P3',
      name: 'aux_unchanged Delta Payload',
      target: `p50 < ${P3_AUX_UNCHANGED_P50_LIMIT_BYTES} B, p95 < ${P3_AUX_UNCHANGED_P95_LIMIT_BYTES} B`,
      measured: auxUnchanged
        ? `p50=${auxUnchanged.p50Bytes}B, p95=${auxUnchanged.p95Bytes}B, max=${auxUnchanged.maxBytes}B (n=${auxUnchanged.count})`
        : 'missing stage.delta.aux_unchanged metric',
      passed: p3Pass,
    },
    {
      id: 'P4',
      name: 'fleet_changed Zero since=0 Hotspot',
      target: 'unexpected since_revision===0 count === 0, crashes === 0',
      measured: hilog.present
        ? `unexpected=${sinceZeroUnexpected}, reload/resync=${sinceZeroReload}, crashes=${hilog.crashLines.length}`
        : (hilog.error || 'missing hilog'),
      passed: p4Pass,
    },
  ];

  const failures = [];
  for (const check of checks) {
    if (!check.passed) {
      failures.push(`${check.id} (${check.name}): ${check.measured} [target: ${check.target}]`);
    }
  }

  const coldStart = {};
  for (const k of Object.keys(hilog.marks)) {
    if (k.startsWith('cold.') || k.startsWith('perf.')) {
      coldStart[k] = hilog.marks[k];
    }
  }

  const report = {
    schemaVersion: 1,
    generatedAtIso: new Date().toISOString(),
    passed: failures.length === 0,
    meta,
    inputs: {
      trace: opts.trace,
      hilog: opts.hilog,
      meta: opts.meta,
    },
    frame,
    orbitPerf: {
      stageTick,
      auxUnchanged,
      sinceZeroUnexpected,
      sinceZeroReload,
      allMetrics: hilog.metrics,
      crashCount: hilog.crashLines.length,
    },
    coldStart,
    checks,
    failures,
  };

  fs.mkdirSync(path.dirname(opts.out), { recursive: true });
  fs.writeFileSync(opts.out, `${JSON.stringify(report, null, 2)}\n`, 'utf8');

  const md = buildMarkdownReport(report);
  fs.mkdirSync(path.dirname(opts.mdOut), { recursive: true });
  fs.writeFileSync(opts.mdOut, `${md}\n`, 'utf8');

  process.stdout.write(`${md}\n`);
  console.log(`Saved JSON report: ${opts.out}`);
  console.log(`Saved Markdown report: ${opts.mdOut}`);

  if (!report.passed) {
    process.exit(1);
  }
}

main();
