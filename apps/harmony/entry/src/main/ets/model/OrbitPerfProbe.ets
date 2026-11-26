import { hilog, hiTraceMeter } from '@kit.PerformanceAnalysisKit';
import { systemDateTime } from '@kit.BasicServicesKit';
import { ORBIT_LOG_DOMAIN } from './OrbitLogger';

const PERF_LOG_TAG: string = 'OrbitPerf';
const PERF_RING_CAPACITY: number = 512;
const PERF_FLUSH_INTERVAL_MS: number = 10000;
const PERF_MAX_BUCKETS: number = 256;
const PERF_MAX_ACTIVE_STARTS: number = 64;
const COLD_CREATE_START_MARK: string = 'cold.create_start';

class OrbitPerfBucket {
  readonly samplesUs: Float64Array = new Float64Array(PERF_RING_CAPACITY);
  writeIdx: number = 0;
  sampleCount: number = 0;
  totalCalls: number = 0;
  totalBytes: number = 0;
  maxBytes: number = 0;
  activeStartsUs: number[] = [];

  record(micros: number, payloadBytes: number): void {
    const safeMicros = micros >= 0 ? micros : 0;
    const safeBytes = payloadBytes > 0 ? payloadBytes : 0;
    this.samplesUs[this.writeIdx] = safeMicros;
    this.writeIdx = (this.writeIdx + 1) % PERF_RING_CAPACITY;
    if (this.sampleCount < PERF_RING_CAPACITY) {
      this.sampleCount += 1;
    }
    this.totalCalls += 1;
    this.totalBytes += safeBytes;
    if (safeBytes > this.maxBytes) {
      this.maxBytes = safeBytes;
    }
  }

  pushStart(startUs: number): void {
    if (this.activeStartsUs.length >= PERF_MAX_ACTIVE_STARTS) {
      this.activeStartsUs.shift();
    }
    this.activeStartsUs.push(startUs);
  }

  popStart(): number {
    if (this.activeStartsUs.length === 0) {
      return -1;
    }
    const popped = this.activeStartsUs.shift();
    return popped !== undefined ? popped : -1;
  }

  resetWindow(): void {
    this.writeIdx = 0;
    this.sampleCount = 0;
    this.totalCalls = 0;
    this.totalBytes = 0;
    this.maxBytes = 0;
  }
}

function monotonicNowUs(): number {
  try {
    const nanos = systemDateTime.getUptime(systemDateTime.TimeType.STARTUP, true);
    if (nanos > 0) {
      return nanos / 1000;
    }
  } catch (_err) {
    // Fallback to wall-clock ms when uptime API is unavailable (e.g. previewer)
  }
  return Date.now() * 1000;
}

function monotonicNowMs(): number {
  return monotonicNowUs() / 1000;
}

function sortPrefixAscending(buf: Float64Array, count: number): void {
  let gap = 1;
  while (gap < count / 3) {
    gap = gap * 3 + 1;
  }
  while (gap >= 1) {
    for (let i = gap; i < count; i++) {
      const key = buf[i];
      let j = i;
      while (j >= gap && buf[j - gap] > key) {
        buf[j] = buf[j - gap];
        j -= gap;
      }
      buf[j] = key;
    }
    gap = Math.floor(gap / 3);
  }
}

function percentileNearestRank(sorted: Float64Array, count: number, ratio: number): number {
  if (count <= 0) {
    return 0;
  }
  const rank = Math.ceil(count * ratio) - 1;
  const idx = rank < 0 ? 0 : (rank >= count ? count - 1 : rank);
  return sorted[idx];
}

/**
 * Lightweight HarmonyOS performance probe backed by HiTraceMeter and fixed-size
 * 512-sample per-metric ring buffers flushed every 10 seconds to hilog tag "OrbitPerf".
 */
export class OrbitPerfProbe {
  private static readonly buckets: Map<string, OrbitPerfBucket> = new Map<string, OrbitPerfBucket>();
  private static readonly sortScratch: Float64Array = new Float64Array(PERF_RING_CAPACITY);
  private static lastFlushMs: number = 0;
  private static originUs: number = 0;

  private static getOrCreateBucket(name: string): OrbitPerfBucket | null {
    const existing = OrbitPerfProbe.buckets.get(name);
    if (existing !== undefined) {
      return existing;
    }
    if (OrbitPerfProbe.buckets.size >= PERF_MAX_BUCKETS) {
      return null;
    }
    const created = new OrbitPerfBucket();
    OrbitPerfProbe.buckets.set(name, created);
    return created;
  }

  private static ensureClockInitialized(nowUs: number): void {
    if (OrbitPerfProbe.originUs === 0) {
      OrbitPerfProbe.originUs = nowUs;
    }
    if (OrbitPerfProbe.lastFlushMs === 0) {
      OrbitPerfProbe.lastFlushMs = nowUs / 1000;
    }
  }

  public static begin(name: string): void {
    const nowUs = monotonicNowUs();
    OrbitPerfProbe.ensureClockInitialized(nowUs);
    const bucket = OrbitPerfProbe.getOrCreateBucket(name);
    if (bucket !== null) {
      bucket.pushStart(nowUs);
    }
    try {
      hiTraceMeter.startTrace(name, 0);
    } catch (_err) {
      // Ignore when HiTrace is unavailable
    }
  }

  public static end(name: string, payloadBytes: number): void {
    const nowUs = monotonicNowUs();
    OrbitPerfProbe.ensureClockInitialized(nowUs);
    try {
      hiTraceMeter.finishTrace(name, 0);
    } catch (_err) {
      // Ignore when HiTrace is unavailable
    }
    const bucket = OrbitPerfProbe.getOrCreateBucket(name);
    if (bucket !== null) {
      const startUs = bucket.popStart();
      const elapsedUs = startUs >= 0 ? Math.max(0, nowUs - startUs) : 0;
      bucket.record(elapsedUs, payloadBytes);
    }
    OrbitPerfProbe.flushIfDue();
  }

  public static mark(name: string): void {
    const nowUs = monotonicNowUs();
    if (name === COLD_CREATE_START_MARK || OrbitPerfProbe.originUs === 0) {
      OrbitPerfProbe.originUs = nowUs;
    }
    if (OrbitPerfProbe.lastFlushMs === 0) {
      OrbitPerfProbe.lastFlushMs = nowUs / 1000;
    }
    const sinceOriginUs = Math.max(0, nowUs - OrbitPerfProbe.originUs);
    const sinceOriginMs = (sinceOriginUs / 1000).toFixed(2);
    try {
      hiTraceMeter.traceByValue(name, Math.round(sinceOriginUs));
    } catch (_err) {
      // Ignore when HiTrace is unavailable
    }
    hilog.info(
      ORBIT_LOG_DOMAIN,
      PERF_LOG_TAG,
      '%{public}s',
      `mark=${name} ts_us=${Math.round(nowUs)} since_start_ms=${sinceOriginMs}`
    );
  }

  public static tick(name: string, micros: number): void {
    const nowUs = monotonicNowUs();
    OrbitPerfProbe.ensureClockInitialized(nowUs);
    const bucket = OrbitPerfProbe.getOrCreateBucket(name);
    if (bucket !== null) {
      bucket.record(micros, 0);
    }
    OrbitPerfProbe.flushIfDue();
  }

  public static flushIfDue(): void {
    const nowMs = monotonicNowMs();
    if (OrbitPerfProbe.lastFlushMs === 0) {
      OrbitPerfProbe.lastFlushMs = nowMs;
      return;
    }
    if (nowMs - OrbitPerfProbe.lastFlushMs < PERF_FLUSH_INTERVAL_MS) {
      return;
    }
    OrbitPerfProbe.lastFlushMs = nowMs;
    OrbitPerfProbe.buckets.forEach((bucket: OrbitPerfBucket, name: string): void => {
      if (bucket.sampleCount === 0 && bucket.totalCalls === 0) {
        return;
      }
      const n = bucket.sampleCount;
      for (let i = 0; i < n; i++) {
        OrbitPerfProbe.sortScratch[i] = bucket.samplesUs[i];
      }
      sortPrefixAscending(OrbitPerfProbe.sortScratch, n);
      const p50Us = Math.round(percentileNearestRank(OrbitPerfProbe.sortScratch, n, 0.5));
      const p95Us = Math.round(percentileNearestRank(OrbitPerfProbe.sortScratch, n, 0.95));
      const line =
        `metric=${name} count=${bucket.totalCalls} samples=${n} p50_us=${p50Us} p95_us=${p95Us} bytes=${bucket.totalBytes} max_bytes=${bucket.maxBytes}`;
      hilog.info(ORBIT_LOG_DOMAIN, PERF_LOG_TAG, '%{public}s', line);
      bucket.resetWindow();
    });
  }
}
