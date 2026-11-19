import { hilog } from '@kit.PerformanceAnalysisKit';

export const ORBIT_LOG_DOMAIN: number = 0x0B17;

export function orbitLogInfo(tag: string, message: string): void {
  hilog.info(ORBIT_LOG_DOMAIN, tag, '%{public}s', message);
}

export function orbitLogWarn(tag: string, message: string, err?: Object): void {
  if (err !== undefined) {
    hilog.warn(ORBIT_LOG_DOMAIN, tag, '%{public}s: %{public}s', message, formatOrbitError(err));
  } else {
    hilog.warn(ORBIT_LOG_DOMAIN, tag, '%{public}s', message);
  }
}

export function orbitLogError(tag: string, message: string, err?: Object): void {
  if (err !== undefined) {
    hilog.error(ORBIT_LOG_DOMAIN, tag, '%{public}s: %{public}s', message, formatOrbitError(err));
  } else {
    hilog.error(ORBIT_LOG_DOMAIN, tag, '%{public}s', message);
  }
}

export function formatOrbitError(err: Object): string {
  if (err instanceof Error) {
    return err.message;
  }
  try {
    return JSON.stringify(err);
  } catch (_e) {
    return `${err}`;
  }
}
