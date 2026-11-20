const lightTokenCache = new Map<string, string>();

/**
 * Adapt dark-theme `syntect` token hex colors for crisp readability on light backgrounds
 * while preserving hue identity. Cached in a bounded module map.
 */
export function adaptTokenColor(fgHex: string, isDark: boolean): string {
  if (isDark) return fgHex;
  const cached = lightTokenCache.get(fgHex);
  if (cached) return cached;

  const clean = fgHex.replace("#", "");
  if (clean.length !== 6) return fgHex;
  const r = parseInt(clean.slice(0, 2), 16) / 255;
  const g = parseInt(clean.slice(2, 4), 16) / 255;
  const b = parseInt(clean.slice(4, 6), 16) / 255;

  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  const d = max - min;

  let h = 0;
  let s = 0;
  if (d > 0.001) {
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    switch (max) {
      case r:
        h = ((g - b) / d + (g < b ? 6 : 0)) / 6;
        break;
      case g:
        h = ((b - r) / d + 2) / 6;
        break;
      default:
        h = ((r - g) / d + 4) / 6;
        break;
    }
  }

  let out: string;
  if (s < 0.14) {
    out = l > 0.58 ? "#24292F" : "#6E7781";
  } else {
    const newL = Math.max(0.24, Math.min(0.42, 0.88 - l));
    const newS = Math.min(0.88, Math.max(0.55, s * 1.08));
    out = `hsl(${Math.round(h * 360)} ${Math.round(newS * 100)}% ${Math.round(newL * 100)}%)`;
  }

  if (lightTokenCache.size > 512) {
    lightTokenCache.clear();
  }
  lightTokenCache.set(fgHex, out);
  return out;
}
