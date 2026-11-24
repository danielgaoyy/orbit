import { common } from '@kit.AbilityKit';
import { preferences } from '@kit.ArkData';
import { fileIo } from '@kit.CoreFileKit';
import { cryptoFramework } from '@kit.CryptoArchitectureKit';
import { orbitLogWarn } from './OrbitLogger';

const PREFS_NAME: string = 'orbit_harmony_prefs';
const KEY_THEME_MODE: string = 'theme_mode';
const KEY_PINNED_THREADS: string = 'pinned_threads_json';
const KEY_MIGRATED_THREAD_PREFS_V1: string = 'migrated_thread_prefs_v1';
const KEY_DRAFT_PREFS: string = 'draft_prefs_v1';
const KEY_DEVICE_ID: string = 'orbit_harmony_device_id_v1';
const KEY_PRIMARY_ROUTER_HOST: string = 'primary_router_host_id_v1';

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

export class OrbitPreferencesStore {
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
