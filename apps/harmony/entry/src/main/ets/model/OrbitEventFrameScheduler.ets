import { FrameCallback, UIContext } from '@kit.ArkUI';
import { CoreNotificationDto } from './OrbitTypes';
import { orbitLogWarn } from './OrbitLogger';
import { OrbitPerfProbe } from './OrbitPerfProbe';

const FALLBACK_FRAME_MS: number = 16;

class OrbitSchedulerFrameCallback extends FrameCallback {
  private readonly onTrigger: () => void;

  constructor(onTrigger: () => void) {
    super();
    this.onTrigger = onTrigger;
  }

  onFrame(_frameTimeInNano: number): void {
    this.onTrigger();
  }
}

export interface OrbitEventFlushHandler {
  getSelectedHostId(): string;
  isStageScopeVisible(hostId: string, agentId: string): boolean;
  isGitScopeVisible(hostId: string, cwd: string): boolean;
  isTerminalScopeVisible(hostId: string, slot: number): boolean;
  onFleetDirty(): void;
  onBriefingDirty(): void;
  onSelectedHostDirty(hostId: string): void;
  onBackgroundHostDirty(hostId: string): void;
  onVisibleTimelineDirty(hostId: string, agentId: string): void;
  onBackgroundTimelineDirty(hostId: string, agentId: string): void;
  onVisibleGitDirty(hostId: string, cwd: string): void;
  onBackgroundGitDirty(hostId: string, cwd: string): void;
  onVisibleTerminalDirty(hostId: string, slot: number): void;
  onResync(): void;
}

function splitScopeKey(composite: string): string[] {
  const sep = composite.indexOf('::');
  if (sep < 0) {
    return [composite, ''];
  }
  return [composite.substring(0, sep), composite.substring(sep + 2)];
}

/**
 * Single-frame event coalescer & precision dirty-bit router for Core notifications.
 * Uses `UIContext.postFrameCallback` so bursts of core events collapse into at most
 * one dispatch pass per vsync frame, routing strictly by visible scope.
 */
export class OrbitEventFrameScheduler {
  private dirtyFleet: boolean = false;
  private dirtyHosts: Set<string> = new Set<string>();
  private dirtyTimelines: Set<string> = new Set<string>();
  private dirtyGit: Set<string> = new Set<string>();
  private dirtyTerminals: Set<string> = new Set<string>();
  private dirtyBriefing: boolean = false;
  private flushScheduled: boolean = false;
  private resyncPending: boolean = false;

  private readonly handler: OrbitEventFlushHandler;
  private uiContext: UIContext | null = null;
  private fallbackTimerId: number = -1;

  constructor(handler: OrbitEventFlushHandler) {
    this.handler = handler;
  }

  public bindUIContext(ctx: UIContext | null): void {
    this.uiContext = ctx;
  }

  public enqueue(payloadJson: string): void {
    if (payloadJson.length === 0) {
      return;
    }
    try {
      const ev = JSON.parse(payloadJson) as CoreNotificationDto;
      if (ev.kind === 'resync_required') {
        this.requestResync();
        return;
      }
      if (ev.kind === 'fleet_changed') {
        this.dirtyFleet = true;
        this.dirtyBriefing = true;
        this.ensureScheduled();
        return;
      }
      if (ev.kind === 'host_changed') {
        const hostId = ev.host_id !== undefined ? ev.host_id : '';
        if (hostId.length > 0) {
          this.dirtyHosts.add(hostId);
          this.ensureScheduled();
        }
        return;
      }
      if (ev.kind === 'timeline_changed') {
        const hostId = ev.host_id !== undefined ? ev.host_id : '';
        const agentId = ev.agent_id !== undefined ? ev.agent_id : '';
        if (hostId.length > 0 && agentId.length > 0) {
          this.dirtyTimelines.add(`${hostId}::${agentId}`);
          this.ensureScheduled();
        }
        return;
      }
      if (ev.kind === 'git_changed') {
        const hostId = ev.host_id !== undefined ? ev.host_id : '';
        const cwd = ev.cwd !== undefined ? ev.cwd : '';
        if (hostId.length > 0) {
          this.dirtyGit.add(`${hostId}::${cwd}`);
          this.ensureScheduled();
        }
        return;
      }
      if (ev.kind === 'terminal_changed') {
        const hostId = ev.host_id !== undefined ? ev.host_id : '';
        const slot = (ev.slot !== undefined && ev.slot > 0) ? ev.slot : 1;
        if (hostId.length > 0) {
          this.dirtyTerminals.add(`${hostId}::${slot}`);
          this.ensureScheduled();
        }
      }
    } catch (err) {
      orbitLogWarn('OrbitScheduler', 'Malformed core notification payload', err as Object);
    }
  }

  public requestResync(): void {
    this.resyncPending = true;
    this.dirtyFleet = true;
    this.dirtyBriefing = true;
    this.ensureScheduled();
  }

  public flushNow(): void {
    if (this.fallbackTimerId !== -1) {
      clearTimeout(this.fallbackTimerId);
      this.fallbackTimerId = -1;
    }
    this.flush();
  }

  public dispose(): void {
    if (this.fallbackTimerId !== -1) {
      clearTimeout(this.fallbackTimerId);
      this.fallbackTimerId = -1;
    }
    this.flushScheduled = false;
    this.resyncPending = false;
    this.dirtyFleet = false;
    this.dirtyBriefing = false;
    this.dirtyHosts.clear();
    this.dirtyTimelines.clear();
    this.dirtyGit.clear();
    this.dirtyTerminals.clear();
  }

  private ensureScheduled(): void {
    if (this.flushScheduled) {
      return;
    }
    this.flushScheduled = true;

    if (this.uiContext !== null) {
      try {
        const cb = new OrbitSchedulerFrameCallback((): void => {
          this.flush();
        });
        this.uiContext.postFrameCallback(cb);
        return;
      } catch (err) {
        orbitLogWarn('OrbitScheduler', 'postFrameCallback failed; falling back to timer', err as Object);
      }
    }

    this.fallbackTimerId = setTimeout((): void => {
      this.fallbackTimerId = -1;
      this.flush();
    }, FALLBACK_FRAME_MS);
  }

  private flush(): void {
    this.flushScheduled = false;

    const doResync = this.resyncPending;
    const doFleet = this.dirtyFleet;
    const doBriefing = this.dirtyBriefing;
    const hostsToFlush: string[] = [];
    this.dirtyHosts.forEach((h: string): void => {
      hostsToFlush.push(h);
    });
    const timelinesToFlush: string[] = [];
    this.dirtyTimelines.forEach((t: string): void => {
      timelinesToFlush.push(t);
    });
    const gitToFlush: string[] = [];
    this.dirtyGit.forEach((g: string): void => {
      gitToFlush.push(g);
    });
    const terminalsToFlush: string[] = [];
    this.dirtyTerminals.forEach((tm: string): void => {
      terminalsToFlush.push(tm);
    });

    this.resyncPending = false;
    this.dirtyFleet = false;
    this.dirtyBriefing = false;
    this.dirtyHosts.clear();
    this.dirtyTimelines.clear();
    this.dirtyGit.clear();
    this.dirtyTerminals.clear();

    if (
      !doResync &&
      !doFleet &&
      !doBriefing &&
      hostsToFlush.length === 0 &&
      timelinesToFlush.length === 0 &&
      gitToFlush.length === 0 &&
      terminalsToFlush.length === 0
    ) {
      return;
    }

    const startUs = OrbitPerfProbe.nowUs();
    try {
      if (doResync) {
        this.handler.onResync();
        return;
      }

      if (doFleet) {
        this.handler.onFleetDirty();
      }
      if (doBriefing) {
        this.handler.onBriefingDirty();
      }

      const selectedHostId = this.handler.getSelectedHostId();
      for (let i = 0; i < hostsToFlush.length; i++) {
        const hostId = hostsToFlush[i];
        if (hostId === selectedHostId && selectedHostId.length > 0) {
          this.handler.onSelectedHostDirty(hostId);
        } else {
          this.handler.onBackgroundHostDirty(hostId);
        }
      }

      for (let i = 0; i < timelinesToFlush.length; i++) {
        const parts = splitScopeKey(timelinesToFlush[i]);
        const hostId = parts[0];
        const agentId = parts[1];
        if (this.handler.isStageScopeVisible(hostId, agentId)) {
          this.handler.onVisibleTimelineDirty(hostId, agentId);
        } else {
          this.handler.onBackgroundTimelineDirty(hostId, agentId);
        }
      }

      for (let i = 0; i < gitToFlush.length; i++) {
        const parts = splitScopeKey(gitToFlush[i]);
        const hostId = parts[0];
        const cwd = parts[1];
        if (this.handler.isGitScopeVisible(hostId, cwd)) {
          this.handler.onVisibleGitDirty(hostId, cwd);
        } else {
          this.handler.onBackgroundGitDirty(hostId, cwd);
        }
      }

      for (let i = 0; i < terminalsToFlush.length; i++) {
        const parts = splitScopeKey(terminalsToFlush[i]);
        const hostId = parts[0];
        const slot = Number.parseInt(parts[1], 10);
        const normalizedSlot = Number.isFinite(slot) && slot > 0 ? slot : 1;
        if (this.handler.isTerminalScopeVisible(hostId, normalizedSlot)) {
          this.handler.onVisibleTerminalDirty(hostId, normalizedSlot);
        }
      }
    } finally {
      OrbitPerfProbe.tick('scheduler.flush', Math.max(0, OrbitPerfProbe.nowUs() - startUs));
    }
  }
}
