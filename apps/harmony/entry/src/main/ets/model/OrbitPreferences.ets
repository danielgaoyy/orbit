import { common } from '@kit.AbilityKit';
import { preferences } from '@kit.ArkData';
import { fileIo } from '@kit.CoreFileKit';
import { cryptoFramework } from '@kit.CryptoArchitectureKit';
import { orbitLogWarn } from './OrbitLogger';
import {
  ORBIT_SIDE_PANEL_STORAGE_KEY,
  OrbitSidePanelStorage
} from './stores/OrbitSidePanelStore';
import { OrbitShellWidthStorage } from './stores/OrbitShellStore';
import { PushPreferencesDto } from './OrbitTypes';

const PREFS_NAME: string = 'orbit_harmony_prefs';
const KEY_THEME_MODE: string = 'orbit.harmony.v2.theme_mode';
const KEY_PINNED_THREADS: string = 'orbit.harmony.v2.pinned_threads_json';
const KEY_MIGRATED_THREAD_PREFS_V1: string = 'orbit.harmony.v2.migrated_thread_prefs';
const KEY_DRAFT_PREFS: string = 'orbit.harmony.v2.draft_prefs';
const KEY_DEVICE_ID: string = 'orbit.harmony.v2.device_id';
const KEY_PRIMARY_ROUTER_HOST: string = 'orbit.harmony.v2.primary_router_host_id';
const KEY_SIDEBAR_WIDTH_VP: string = 'orbit.harmony.v2.sidebar_width_vp';
const KEY_SIDE_PANEL_WIDTH_VP: string = 'orbit.harmony.v2.panel_width_vp';
const KEY_PUSH_PREFS_BY_HOST: string = 'orbit.harmony.v2.push_prefs_by_host_json';
const KEY_PA_LAST_VISIT_MAP: string = 'orbit.harmony.v2.pa_last_visit_map_json';

export interface OrbitDraftPrefs {
  provider: string;
  model: string;
  effort: string;
  permissionMode: string;
  sidebarProjectSort: string;
  sidebarThreadFilter: string;
}

function generateAnonymousDeviceId(): string {
  const bytes = new Uint8Array(12);
  let filled = false;
  try {
    if (canIUse('SystemCapability.Security.CryptoFramework.Rand')) {
      const blob = cryptoFramework.createRandom().generateRandomSync(12);
      if (blob.data.length === 12) {
        for (let i = 0; i < 12; i++) {
          bytes[i] = blob.data[i];
        }
        filled = true;
      }
    }
  } catch (_err) {
    filled = false;
  }
  if (!filled) {
    for (let i = 0; i < 12; i++) {
      bytes[i] = Math.floor(Math.random() * 256) & 0xFF;
    }
  }
  let hex = '';
  for (let i = 0; i < bytes.length; i++) {
    hex += bytes[i].toString(16).padStart(2, '0');
  }
  return `ohos-${hex}`;
}

export class OrbitPreferencesStore implements OrbitSidePanelStorage, OrbitShellWidthStorage {
  private prefs: preferences.Preferences | null = null;

  public init(context: common.Context): void {
    if (this.prefs !== null) {
      return;
    }
    try {
      this.prefs = preferences.getPreferencesSync(context, { name: PREFS_NAME });
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to initialize preferences', err as Object);
    }
  }

  public getOrCreateDeviceId(): string {
    if (this.prefs !== null) {
      try {
        const stored = this.prefs.getSync(KEY_DEVICE_ID, '') as string;
        if (typeof stored === 'string' && stored.trim().length > 0) {
          return stored.trim();
        }
        const fresh = generateAnonymousDeviceId();
        this.prefs.putSync(KEY_DEVICE_ID, fresh);
        this.prefs.flush().catch((err: Object) => {
          orbitLogWarn('OrbitPrefs', 'Failed to flush device id', err);
        });
        return fresh;
      } catch (err) {
        orbitLogWarn('OrbitPrefs', 'Failed to read/write device id', err as Object);
      }
    }
    return generateAnonymousDeviceId();
  }

  public getPrimaryRouterHostId(): string {
    if (this.prefs === null) {
      return '';
    }
    try {
      const stored = this.prefs.getSync(KEY_PRIMARY_ROUTER_HOST, '') as string;
      return typeof stored === 'string' ? stored.trim() : '';
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to read primary router host', err as Object);
      return '';
    }
  }

  public setPrimaryRouterHostId(hostId: string): void {
    if (this.prefs === null) {
      return;
    }
    try {
      this.prefs.putSync(KEY_PRIMARY_ROUTER_HOST, hostId.trim());
      this.prefs.flush().catch((err: Object) => {
        orbitLogWarn('OrbitPrefs', 'Failed to flush primary router host', err);
      });
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to save primary router host', err as Object);
    }
  }

  public getPushPreferencesMap(): Record<string, PushPreferencesDto> {
    if (this.prefs === null) {
      return {};
    }
    try {
      const raw = this.prefs.getSync(KEY_PUSH_PREFS_BY_HOST, '') as string;
      if (typeof raw !== 'string' || raw.trim().length === 0) {
        return {};
      }
      const parsed = JSON.parse(raw) as Record<string, PushPreferencesDto>;
      return parsed !== null && typeof parsed === 'object' ? parsed : {};
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to read push preferences map', err as Object);
      return {};
    }
  }

  public savePushPreferencesForHost(hostId: string, nextPrefs: PushPreferencesDto): void {
    const scopeKey = hostId.trim().length > 0 ? hostId.trim() : '__default__';
    if (this.prefs === null) {
      return;
    }
    try {
      const map = this.getPushPreferencesMap();
      map[scopeKey] = {
        enabled: nextPrefs.enabled === true,
        notify_permission_required: nextPrefs.notify_permission_required === true,
        notify_turn_completed: nextPrefs.notify_turn_completed === true,
        notify_agent_error: nextPrefs.notify_agent_error === true,
        notify_goal_finished: nextPrefs.notify_goal_finished === true,
        notify_timer_fired: nextPrefs.notify_timer_fired === true,
        include_message_preview: nextPrefs.include_message_preview === true
      };
      this.prefs.putSync(KEY_PUSH_PREFS_BY_HOST, JSON.stringify(map));
      this.prefs.flush().catch((err: Object) => {
        orbitLogWarn('OrbitPrefs', 'Failed to flush push preferences map', err);
      });
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to save push preferences map', err as Object);
    }
  }

  public getPersonalAgentLastVisitMap(): Record<string, number> {
    if (this.prefs === null) {
      return {};
    }
    try {
      const raw = this.prefs.getSync(KEY_PA_LAST_VISIT_MAP, '') as string;
      if (typeof raw !== 'string' || raw.trim().length === 0) {
        return {};
      }
      const parsed = JSON.parse(raw) as Record<string, number>;
      return parsed !== null && typeof parsed === 'object' ? parsed : {};
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to read PA last-visit map', err as Object);
      return {};
    }
  }

  public getPersonalAgentLastVisitMs(hostId: string, agentId: string): number {
    const h = hostId.trim();
    const a = agentId.trim();
    if (h.length === 0 || a.length === 0) {
      return 0;
    }
    const map = this.getPersonalAgentLastVisitMap();
    const val = map[`${h}::${a}`];
    return typeof val === 'number' && Number.isFinite(val) && val > 0 ? val : 0;
  }

  public setPersonalAgentLastVisitMs(hostId: string, agentId: string, tsMs: number): void {
    const h = hostId.trim();
    const a = agentId.trim();
    if (h.length === 0 || a.length === 0 || !Number.isFinite(tsMs) || tsMs <= 0) {
      return;
    }
    if (this.prefs === null) {
      return;
    }
    try {
      const map = this.getPersonalAgentLastVisitMap();
      map[`${h}::${a}`] = Math.floor(tsMs);
      this.prefs.putSync(KEY_PA_LAST_VISIT_MAP, JSON.stringify(map));
      this.prefs.flush().catch((err: Object) => {
        orbitLogWarn('OrbitPrefs', 'Failed to flush PA last-visit map', err);
      });
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to save PA last-visit map', err as Object);
    }
  }

  public getThemeMode(): string {
    if (this.prefs === null) {
      return 'system';
    }
    try {
      const raw = this.prefs.getSync(KEY_THEME_MODE, 'system');
      if (raw === 'light' || raw === 'dark' || raw === 'system') {
        return raw;
      }
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to read theme mode', err as Object);
    }
    return 'system';
  }

  public setThemeMode(mode: string): void {
    const normalized = (mode === 'light' || mode === 'dark') ? mode : 'system';
    if (this.prefs === null) {
      return;
    }
    try {
      this.prefs.putSync(KEY_THEME_MODE, normalized);
      this.prefs.flush().catch((err: Object) => {
        orbitLogWarn('OrbitPrefs', 'Failed to flush theme mode', err);
      });
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to save theme mode', err as Object);
    }
  }

  public hasMigratedThreadPrefsV1(): boolean {
    if (this.prefs === null) {
      return false;
    }
    try {
      return this.prefs.getSync(KEY_MIGRATED_THREAD_PREFS_V1, false) === true;
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to read thread prefs migration flag', err as Object);
      return false;
    }
  }

  public markMigratedThreadPrefsV1(): void {
    if (this.prefs === null) {
      return;
    }
    try {
      this.prefs.putSync(KEY_MIGRATED_THREAD_PREFS_V1, true);
      this.prefs.putSync(KEY_PINNED_THREADS, '');
      this.prefs.flush().catch((err: Object) => {
        orbitLogWarn('OrbitPrefs', 'Failed to flush thread prefs migration flag', err);
      });
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to mark thread prefs migrated', err as Object);
    }
  }

  public loadPinnedThreads(legacyFilePath: string): string[] {
    if (this.prefs !== null) {
      try {
        const stored = this.prefs.getSync(KEY_PINNED_THREADS, '');
        if (typeof stored === 'string' && stored.length > 0) {
          return this.parseStringArray(stored);
        }
      } catch (err) {
        orbitLogWarn('OrbitPrefs', 'Failed to read pinned threads from preferences', err as Object);
      }
    }

    if (legacyFilePath.length > 0) {
      try {
        if (fileIo.accessSync(legacyFilePath)) {
          const text = fileIo.readTextSync(legacyFilePath);
          const migrated = this.parseStringArray(text);
          if (migrated.length > 0) {
            this.savePinnedThreads(migrated);
          }
          return migrated;
        }
      } catch (err) {
        orbitLogWarn('OrbitPrefs', 'Failed to migrate legacy pinned threads', err as Object);
      }
    }
    return [];
  }

  public savePinnedThreads(keys: string[]): void {
    if (this.prefs === null) {
      return;
    }
    try {
      this.prefs.putSync(KEY_PINNED_THREADS, JSON.stringify(keys));
      this.prefs.flush().catch((err: Object) => {
        orbitLogWarn('OrbitPrefs', 'Failed to flush pinned threads', err);
      });
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to save pinned threads', err as Object);
    }
  }

  public loadDraftPrefs(legacyFilePath: string): OrbitDraftPrefs | null {
    if (this.prefs !== null) {
      try {
        const stored = this.prefs.getSync(KEY_DRAFT_PREFS, '');
        if (typeof stored === 'string' && stored.length > 0) {
          return this.parseDraftPrefsText(stored);
        }
      } catch (err) {
        orbitLogWarn('OrbitPrefs', 'Failed to read draft prefs from preferences', err as Object);
      }
    }

    if (legacyFilePath.length > 0) {
      try {
        if (fileIo.accessSync(legacyFilePath)) {
          const raw = fileIo.readTextSync(legacyFilePath);
          const migrated = this.parseDraftPrefsText(raw);
          if (migrated !== null) {
            this.saveDraftPrefs(migrated);
          }
          return migrated;
        }
      } catch (err) {
        orbitLogWarn('OrbitPrefs', 'Failed to migrate legacy draft prefs', err as Object);
      }
    }
    return null;
  }

  public saveDraftPrefs(draft: OrbitDraftPrefs): void {
    if (this.prefs === null) {
      return;
    }
    try {
      const payload = [
        `provider=${draft.provider}`,
        `model=${draft.model}`,
        `effort=${draft.effort}`,
        `permissionMode=${draft.permissionMode}`,
        `sidebarProjectSort=${draft.sidebarProjectSort}`,
        `sidebarThreadFilter=${draft.sidebarThreadFilter}`
      ].join('\n');
      this.prefs.putSync(KEY_DRAFT_PREFS, payload);
      this.prefs.flush().catch((err: Object) => {
        orbitLogWarn('OrbitPrefs', 'Failed to flush draft prefs', err);
      });
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to save draft prefs', err as Object);
    }
  }

  public getSidePanelStateJson(): string {
    if (this.prefs === null) {
      return '';
    }
    try {
      const stored = this.prefs.getSync(ORBIT_SIDE_PANEL_STORAGE_KEY, '');
      return typeof stored === 'string' ? stored : '';
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to read sidePanel state', err as Object);
      return '';
    }
  }

  public setSidePanelStateJson(payloadJson: string, onAsyncFailure?: () => void): boolean {
    if (this.prefs === null) {
      return false;
    }
    try {
      this.prefs.putSync(ORBIT_SIDE_PANEL_STORAGE_KEY, payloadJson);
      this.prefs.flush().catch((err: Object) => {
        orbitLogWarn('OrbitPrefs', 'Failed to flush sidePanel state', err);
        if (onAsyncFailure !== undefined) {
          onAsyncFailure();
        }
      });
      return true;
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to save sidePanel state', err as Object);
      return false;
    }
  }

  public getSidebarWidthVp(): number {
    if (this.prefs === null) {
      return 0;
    }
    try {
      const stored = this.prefs.getSync(KEY_SIDEBAR_WIDTH_VP, 0);
      return typeof stored === 'number' && Number.isFinite(stored) ? stored : 0;
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to read sidebar width', err as Object);
      return 0;
    }
  }

  public setSidebarWidthVp(widthVp: number): void {
    if (this.prefs === null || !Number.isFinite(widthVp) || widthVp <= 0) {
      return;
    }
    try {
      this.prefs.putSync(KEY_SIDEBAR_WIDTH_VP, Math.round(widthVp));
      this.prefs.flush().catch((err: Object) => {
        orbitLogWarn('OrbitPrefs', 'Failed to flush sidebar width', err);
      });
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to save sidebar width', err as Object);
    }
  }

  public getSidePanelWidthVp(): number {
    if (this.prefs === null) {
      return 0;
    }
    try {
      const stored = this.prefs.getSync(KEY_SIDE_PANEL_WIDTH_VP, 0);
      return typeof stored === 'number' && Number.isFinite(stored) ? stored : 0;
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to read side panel width', err as Object);
      return 0;
    }
  }

  public setSidePanelWidthVp(widthVp: number): void {
    if (this.prefs === null || !Number.isFinite(widthVp) || widthVp <= 0) {
      return;
    }
    try {
      this.prefs.putSync(KEY_SIDE_PANEL_WIDTH_VP, Math.round(widthVp));
      this.prefs.flush().catch((err: Object) => {
        orbitLogWarn('OrbitPrefs', 'Failed to flush side panel width', err);
      });
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to save side panel width', err as Object);
    }
  }

  private parseStringArray(json: string): string[] {
    try {
      const parsed = JSON.parse(json) as Object;
      if (!Array.isArray(parsed)) {
        return [];
      }
      const out: string[] = [];
      for (let i = 0; i < parsed.length; i++) {
        const item = parsed[i] as Object;
        if (typeof item === 'string' && item.length > 0) {
          out.push(item);
        }
      }
      return out;
    } catch (err) {
      orbitLogWarn('OrbitPrefs', 'Failed to parse string array JSON', err as Object);
      return [];
    }
  }

  private parseDraftPrefsText(raw: string): OrbitDraftPrefs | null {
    const out: OrbitDraftPrefs = {
      provider: '',
      model: '',
      effort: '',
      permissionMode: '',
      sidebarProjectSort: '',
      sidebarThreadFilter: ''
    };
    let hasAny = false;
    const lines = raw.split('\n');
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i].trim();
      const eq = line.indexOf('=');
      if (eq <= 0) {
        continue;
      }
      const key = line.substring(0, eq).trim();
      const val = line.substring(eq + 1).trim();
      if (key === 'provider') {
        out.provider = val;
        hasAny = true;
      } else if (key === 'model') {
        out.model = val;
        hasAny = true;
      } else if (key === 'effort') {
        out.effort = val;
        hasAny = true;
      } else if (key === 'permissionMode') {
        out.permissionMode = val;
        hasAny = true;
      } else if (key === 'sidebarProjectSort') {
        out.sidebarProjectSort = val;
        hasAny = true;
      } else if (key === 'sidebarThreadFilter') {
        out.sidebarThreadFilter = val;
        hasAny = true;
      }
    }
    return hasAny ? out : null;
  }
}
