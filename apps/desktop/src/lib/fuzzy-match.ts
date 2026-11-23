export interface FuzzyMatchResult {
  score: number;
  indices: number[];
}

export interface HighlightSegment {
  text: string;
  matched: boolean;
}

export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem?(key: string): void;
}

export const PALETTE_RECENTS_STORAGE_KEY = "orbit:palette_recents_v1";
export const DEFAULT_MAX_PALETTE_RECENTS = 24;

// Integer scoring weights tuned for Raycast/Linear-style precision across Latin & CJK.
const SCORE_MATCH = 32;
const BONUS_EXACT = 200;
const BONUS_PREFIX_RUN = 44;
const BONUS_CONTIGUOUS = 32;
const BONUS_START = 48;
const BONUS_PATH_BOUNDARY = 46;
const BONUS_WORD_BOUNDARY = 40;
const BONUS_CAMEL = 36;
const BONUS_DIGIT_BOUNDARY = 18;
const BONUS_CONSECUTIVE = 44;
const BONUS_CASE_MATCH = 2;
const BONUS_TOKEN_ORDER = 24;
const BONUS_MULTI_SUBSTRING = 40;
const PENALTY_GAP_START = 14;
const PENALTY_GAP_EXT = 3;
const PENALTY_LEADING_MAX = 24;
const PENALTY_UNMATCHED_MAX = 80;
const MAX_DP_TARGET_LEN = 512;

const NEG_INF = -1_000_000_000;

function isPathSeparator(code: number): boolean {
  return code === 47 /* / */ || code === 92 /* \ */;
}

function isWordSeparator(ch: string, code: number): boolean {
  if (
    code === 32 || // space
    code === 9 || // tab
    code === 45 || // -
    code === 95 || // _
    code === 46 || // .
    code === 58 || // :
    code === 40 || // (
    code === 41 || // )
    code === 91 || // [
    code === 93 || // ]
    code === 123 || // {
    code === 125 || // }
    code === 44 || // ,
    code === 59 // ;
  ) {
    return true;
  }
  // Common typographic & CJK separators
  return (
    ch === "·" ||
    ch === "—" ||
    ch === "–" ||
    ch === "：" ||
    ch === "、" ||
    ch === "，" ||
    ch === "。" ||
    ch === "（" ||
    ch === "）" ||
    ch === "「" ||
    ch === "」" ||
    ch === "『" ||
    ch === "』" ||
    ch === "【" ||
    ch === "】" ||
    ch === "・"
  );
}

function isAsciiLower(code: number): boolean {
  return code >= 97 && code <= 122;
}

function isAsciiUpper(code: number): boolean {
  return code >= 65 && code <= 90;
}

function isAsciiDigit(code: number): boolean {
  return code >= 48 && code <= 57;
}

function isAsciiLetter(code: number): boolean {
  return isAsciiLower(code) || isAsciiUpper(code);
}

/**
 * Lowercase a single UTF-16 code unit into a single character so indices remain
 * 1:1 aligned with original string offsets (even for unusual Unicode foldings).
 */
function lowerCharAt(str: string, index: number): string {
  const code = str.charCodeAt(index);
  if (code >= 65 && code <= 90) {
    return String.fromCharCode(code + 32);
  }
  if (code < 128) {
    return str[index];
  }
  const lowered = str[index].toLowerCase();
  return lowered.length === 1 ? lowered : lowered[0] ?? str[index];
}

function computeBoundaryBonus(target: string, j: number): number {
  if (j === 0) return BONUS_START;
  const prevCode = target.charCodeAt(j - 1);
  if (isPathSeparator(prevCode)) return BONUS_PATH_BOUNDARY;
  const prevCh = target[j - 1];
  if (isWordSeparator(prevCh, prevCode)) return BONUS_WORD_BOUNDARY;

  const currCode = target.charCodeAt(j);
  if (isAsciiLower(prevCode) && isAsciiUpper(currCode)) {
    return BONUS_CAMEL;
  }
  if (
    (isAsciiLetter(prevCode) && isAsciiDigit(currCode)) ||
    (isAsciiDigit(prevCode) && isAsciiLetter(currCode))
  ) {
    return BONUS_DIGIT_BOUNDARY;
  }
  return 0;
}

function scoreSingleToken(token: string, target: string): FuzzyMatchResult | null {
  const m = token.length;
  const n = target.length;
  if (m === 0) return { score: 0, indices: [] };
  if (n === 0 || m > n) return null;

  const qLower = new Array<string>(m);
  for (let i = 0; i < m; i += 1) {
    qLower[i] = lowerCharAt(token, i);
  }

  // Fast O(n) subsequence pre-check + capture lowercase characters & greedy fallback indices
  const tLower = new Array<string>(n);
  const firstPassIndices: number[] = [];
  let qi = 0;
  for (let j = 0; j < n; j += 1) {
    const lc = lowerCharAt(target, j);
    tLower[j] = lc;
    if (qi < m && lc === qLower[qi]) {
      firstPassIndices.push(j);
      qi += 1;
    }
  }
  if (qi < m) return null;

  // Bounded greedy fallback for extraordinarily long strings (> 512 chars)
  if (n > MAX_DP_TARGET_LEN) {
    return finalizeSingleTokenScore(token, target, qLower, tLower, firstPassIndices);
  }

  const boundaries = new Int32Array(n);
  for (let j = 0; j < n; j += 1) {
    boundaries[j] = computeBoundaryBonus(target, j);
  }

  // O(m * n) dynamic programming for globally optimal subsequence alignment
  const prevRow = new Int32Array(n);
  const currRow = new Int32Array(n);
  const parent = new Int32Array(m * n);
  parent.fill(-1);

  // Row 0
  const q0 = qLower[0];
  const q0Raw = token[0];
  for (let j = 0; j < n; j += 1) {
    if (tLower[j] === q0) {
      const caseBonus = target[j] === q0Raw ? BONUS_CASE_MATCH : 0;
      prevRow[j] = SCORE_MATCH + boundaries[j] + caseBonus - Math.min(j, PENALTY_LEADING_MAX);
    } else {
      prevRow[j] = NEG_INF;
    }
  }

  // Rows 1..m-1
  for (let i = 1; i < m; i += 1) {
    const qiLower = qLower[i];
    const qiRaw = token[i];
    const rowOffset = i * n;

    // Track best prior non-consecutive candidate `k <= j - 2` with linear gap decay
    let bestGapScore = NEG_INF;
    let bestGapIdx = -1;

    for (let j = 0; j < n; j += 1) {
      // Decay active gap candidate by 1 extra skipped char as j advances
      if (bestGapIdx !== -1) {
        bestGapScore -= PENALTY_GAP_EXT;
      }

      // Introduce `k = j - 2` as a fresh gap-of-1 candidate
      if (j >= 2 && prevRow[j - 2] > NEG_INF / 2) {
        const freshGap = prevRow[j - 2] - PENALTY_GAP_START;
        if (freshGap > bestGapScore) {
          bestGapScore = freshGap;
          bestGapIdx = j - 2;
        }
      }

      if (j < i || tLower[j] !== qiLower) {
        currRow[j] = NEG_INF;
        continue;
      }

      let bestPrev = bestGapScore;
      let bestK = bestGapIdx;

      if (j >= 1 && prevRow[j - 1] > NEG_INF / 2) {
        const consecScore = prevRow[j - 1] + BONUS_CONSECUTIVE;
        if (consecScore >= bestPrev) {
          bestPrev = consecScore;
          bestK = j - 1;
        }
      }

      if (bestK === -1) {
        currRow[j] = NEG_INF;
        continue;
      }

      const caseBonus = target[j] === qiRaw ? BONUS_CASE_MATCH : 0;
      currRow[j] = bestPrev + SCORE_MATCH + boundaries[j] + caseBonus;
      parent[rowOffset + j] = bestK;
    }

    prevRow.set(currRow);
  }

  // Select optimal end position in last row (preferring earlier completion on ties)
  let bestEndJ = -1;
  let bestDpScore = NEG_INF;
  for (let j = m - 1; j < n; j += 1) {
    if (prevRow[j] > bestDpScore) {
      bestDpScore = prevRow[j];
      bestEndJ = j;
    }
  }
  if (bestEndJ === -1) return null;

  const indices = new Array<number>(m);
  let curJ = bestEndJ;
  for (let i = m - 1; i >= 0; i -= 1) {
    indices[i] = curJ;
    curJ = parent[i * n + curJ];
  }

  let totalScore = bestDpScore;
  const span = indices[m - 1] - indices[0] + 1;
  const isContiguous = span === m;

  if (m >= 2 && isContiguous) {
    totalScore += BONUS_CONTIGUOUS;
  }
  if (isContiguous && indices[0] === 0) {
    totalScore += BONUS_PREFIX_RUN;
    if (m === n) {
      totalScore += BONUS_EXACT;
    }
  }

  totalScore -= Math.min(n - m, PENALTY_UNMATCHED_MAX);

  return { score: totalScore, indices };
}

function finalizeSingleTokenScore(
  token: string,
  target: string,
  qLower: string[],
  tLower: string[],
  indices: number[],
): FuzzyMatchResult {
  const m = token.length;
  const n = target.length;
  let score = -Math.min(indices[0], PENALTY_LEADING_MAX);

  for (let i = 0; i < m; i += 1) {
    const j = indices[i];
    score += SCORE_MATCH + computeBoundaryBonus(target, j);
    if (target[j] === token[i]) score += BONUS_CASE_MATCH;
    if (i > 0) {
      const gap = j - indices[i - 1] - 1;
      if (gap === 0) {
        score += BONUS_CONSECUTIVE;
      } else {
        score -= PENALTY_GAP_START + (gap - 1) * PENALTY_GAP_EXT;
      }
    }
  }

  const isContiguous = indices[m - 1] - indices[0] + 1 === m;
  if (m >= 2 && isContiguous) score += BONUS_CONTIGUOUS;
  if (isContiguous && indices[0] === 0) {
    score += BONUS_PREFIX_RUN;
    if (m === n && qLower.every((ch, idx) => ch === tLower[idx])) {
      score += BONUS_EXACT;
    }
  }
  score -= Math.min(n - m, PENALTY_UNMATCHED_MAX);
  return { score, indices };
}

/**
 * Pure zero-dependency fuzzy scorer supporting Latin, camelCase/word/path
 * boundaries, multi-token queries, and CJK characters.
 *
 * Returns `{ score, indices }` on match (with sorted unique UTF-16 character
 * indices into `target`) or `null` when `target` does not satisfy `query`.
 */
export function fuzzyMatch(query: string, target: string): FuzzyMatchResult | null {
  const trimmed = query.trim();
  if (trimmed.length === 0) {
    return { score: 0, indices: [] };
  }
  if (target.length === 0) {
    return null;
  }

  const tokens = trimmed.split(/\s+/).filter(Boolean);
  if (tokens.length === 1) {
    return scoreSingleToken(tokens[0], target);
  }

  // Multi-token AND query (e.g. "open set" or "设置 代理"):
  // Every whitespace-separated token must match; in-order & contiguous hits get bonuses.
  let totalScore = 0;
  const indexSet = new Set<number>();
  let prevLastIdx = -1;
  let allInOrder = true;

  for (const tok of tokens) {
    const res = scoreSingleToken(tok, target);
    if (!res) return null;
    totalScore += res.score;
    if (res.indices.length > 0) {
      if (prevLastIdx >= 0 && res.indices[0] <= prevLastIdx) {
        allInOrder = false;
      }
      prevLastIdx = res.indices[res.indices.length - 1];
      for (const idx of res.indices) {
        indexSet.add(idx);
      }
    }
  }

  if (allInOrder) {
    totalScore += BONUS_TOKEN_ORDER * (tokens.length - 1);
  }

  // Extra reward when the entire raw trimmed query appears literally as a substring
  const rawLower = trimmed.toLowerCase();
  const targetLower = target.toLowerCase();
  const directIdx = targetLower.indexOf(rawLower);
  if (directIdx !== -1 && targetLower.length === target.length && rawLower.length === trimmed.length) {
    totalScore += BONUS_MULTI_SUBSTRING;
    if (directIdx === 0) {
      totalScore += BONUS_PREFIX_RUN;
      if (trimmed.length === target.length) {
        totalScore += BONUS_EXACT;
      }
    }
    const directIndices: number[] = [];
    for (let i = 0; i < trimmed.length; i += 1) {
      if (!/\s/.test(trimmed[i])) {
        directIndices.push(directIdx + i);
      }
    }
    return { score: totalScore, indices: directIndices };
  }

  const mergedIndices = Array.from(indexSet).sort((a, b) => a - b);
  return { score: totalScore, indices: mergedIndices };
}

/**
 * Splits `text` into contiguous matched / unmatched segments for crisp highlight rendering.
 */
export function splitHighlightSegments(
  text: string,
  indices: readonly number[] | undefined,
): HighlightSegment[] {
  if (!text) return [];
  if (!indices || indices.length === 0) {
    return [{ text, matched: false }];
  }

  const segments: HighlightSegment[] = [];
  let cursor = 0;
  let i = 0;

  while (i < indices.length) {
    const start = indices[i];
    if (start < cursor || start >= text.length) {
      i += 1;
      continue;
    }
    if (start > cursor) {
      segments.push({ text: text.slice(cursor, start), matched: false });
    }
    let end = start + 1;
    i += 1;
    while (i < indices.length && indices[i] === end && end < text.length) {
      end += 1;
      i += 1;
    }
    segments.push({ text: text.slice(start, end), matched: true });
    cursor = end;
  }

  if (cursor < text.length) {
    segments.push({ text: text.slice(cursor), matched: false });
  }
  return segments;
}

function resolveDefaultStorage(): StorageLike | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/**
 * Loads deduplicated most-recent-first palette item IDs from storage.
 */
export function loadPaletteRecents(
  storage: StorageLike | null = resolveDefaultStorage(),
  maxEntries: number = DEFAULT_MAX_PALETTE_RECENTS,
): string[] {
  if (!storage || maxEntries <= 0) return [];
  try {
    const raw = storage.getItem(PALETTE_RECENTS_STORAGE_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    const seen = new Set<string>();
    const out: string[] = [];
    for (const entry of parsed) {
      if (typeof entry !== "string") continue;
      const trimmed = entry.trim();
      if (!trimmed || seen.has(trimmed)) continue;
      seen.add(trimmed);
      out.push(trimmed);
      if (out.length >= maxEntries) break;
    }
    return out;
  } catch {
    return [];
  }
}

/**
 * Records an executed palette item ID at the front of the LRU recents list and persists it.
 */
export function recordPaletteRecent(
  id: string,
  storage: StorageLike | null = resolveDefaultStorage(),
  maxEntries: number = DEFAULT_MAX_PALETTE_RECENTS,
): string[] {
  const cleanId = id.trim();
  const existing = loadPaletteRecents(storage, maxEntries);
  if (!cleanId || maxEntries <= 0) return existing;

  const next = [cleanId, ...existing.filter((entry) => entry !== cleanId)].slice(0, maxEntries);
  if (storage) {
    try {
      storage.setItem(PALETTE_RECENTS_STORAGE_KEY, JSON.stringify(next));
    } catch {
      // Ignore storage quota or private-mode failures gracefully
    }
  }
  return next;
}

/**
 * Subtle decaying recency boost so frequently/recently used items float naturally
 * on empty queries and tie-breaks without overpowering strong fuzzy matches.
 */
export function getPaletteRecencyBonus(
  id: string,
  recents: readonly string[],
  maxBonus: number = 28,
): number {
  if (!id || recents.length === 0 || maxBonus <= 0) return 0;
  const idx = recents.indexOf(id);
  if (idx === -1) return 0;
  if (recents.length === 1) return maxBonus;
  const ratio = 1 - idx / Math.max(recents.length, DEFAULT_MAX_PALETTE_RECENTS);
  return Math.max(2, Math.round(maxBonus * ratio));
}
