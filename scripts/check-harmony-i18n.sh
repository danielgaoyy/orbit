#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Progressive ArkTS bare-literal enforcement scope (expanded per PR).
# Pass --all to scan all .ets files under apps/harmony/entry/src/main/ets.
ENFORCED_ETS_PATHS=(
  "apps/harmony/entry/src/main/ets/components/shell"
  "apps/harmony/entry/src/main/ets/components/personal-agent"
  "apps/harmony/entry/src/main/ets/components/common/OrbitAgentListItem.ets"
  "apps/harmony/entry/src/main/ets/components/common/OrbitCommonControls.ets"
  "apps/harmony/entry/src/main/ets/components/common/OrbitSubagentAvatar.ets"
  "apps/harmony/entry/src/main/ets/components/composer/ModelPickerSheet.ets"
  "apps/harmony/entry/src/main/ets/components/composer/OmniComposer.ets"
  "apps/harmony/entry/src/main/ets/components/sheets/AutomationsSheet.ets"
  "apps/harmony/entry/src/main/ets/components/sheets/CreateWorkspaceV2Sheet.ets"
  "apps/harmony/entry/src/main/ets/components/sheets/DirectoryBrowserSheet.ets"
  "apps/harmony/entry/src/main/ets/components/sheets/GitDiffSheet.ets"
  "apps/harmony/entry/src/main/ets/components/sheets/HostSettingsSheet.ets"
  "apps/harmony/entry/src/main/ets/components/sheets/OrbitTerminalView.ets"
  "apps/harmony/entry/src/main/ets/components/sheets/RenameWorkspaceSheet.ets"
  "apps/harmony/entry/src/main/ets/components/sheets/WorkspaceBoardSheet.ets"
  "apps/harmony/entry/src/main/ets/components/sheets/WorkspaceScriptsSheet.ets"
  "apps/harmony/entry/src/main/ets/components/sheets/WorktreeHandoffSheet.ets"
  "apps/harmony/entry/src/main/ets/components/panel/OrbitScopedAutomationsTab.ets"
  "apps/harmony/entry/src/main/ets/components/panel/OrbitScopedGitAndFilesTabs.ets"
  "apps/harmony/entry/src/main/ets/components/panel/OrbitScopedTerminalAndBoardTabs.ets"
  "apps/harmony/entry/src/main/ets/components/panel/OrbitScopedToolTabs.ets"
  "apps/harmony/entry/src/main/ets/components/panel/OrbitSidePanelContainer.ets"
  "apps/harmony/entry/src/main/ets/components/panel/OrbitSidePanelLaunchpad.ets"
  "apps/harmony/entry/src/main/ets/components/panel/OrbitSidePanelTabBar.ets"
  "apps/harmony/entry/src/main/ets/components/stage/DiagramBlockView.ets"
  "apps/harmony/entry/src/main/ets/components/stage/PlanReviewCard.ets"
  "apps/harmony/entry/src/main/ets/components/stage/RichMarkdownView.ets"
  "apps/harmony/entry/src/main/ets/components/stage/StageTurnView.ets"
  "apps/harmony/entry/src/main/ets/components/stage/StickyDecisionDock.ets"
  "apps/harmony/entry/src/main/ets/components/stage/StreamStage.ets"
  "apps/harmony/entry/src/main/ets/components/settings"
  "apps/harmony/entry/src/main/ets/model/OrbitCoreBridge.ets"
  "apps/harmony/entry/src/main/ets/model/OrbitDeviceBridge.ets"
  "apps/harmony/entry/src/main/ets/model/OrbitHarmonyStore.ets"
  "apps/harmony/entry/src/main/ets/model/OrbitPreferences.ets"
  "apps/harmony/entry/src/main/ets/model/OrbitTypes.ets"
  "apps/harmony/entry/src/main/ets/model/OrbitWindowTheme.ets"
  "apps/harmony/entry/src/main/ets/model/stores/OrbitFleetStore.ets"
  "apps/harmony/entry/src/main/ets/theme/OrbitTheme.ets"
  "apps/harmony/entry/src/main/ets/theme/OrbitTokens.ets"
)

SCAN_ALL="0"
for arg in "$@"; do
  case "$arg" in
    --all)
      SCAN_ALL="1"
      ;;
    *)
      echo "Usage: bash scripts/check-harmony-i18n.sh [--all]" >&2
      exit 2
      ;;
  esac
done

node - "$ROOT_DIR" "$SCAN_ALL" "${ENFORCED_ETS_PATHS[@]}" <<'NODE'
const fs = require('node:fs');
const path = require('node:path');

const [rootDir, scanAllFlag, ...enforcedPaths] = process.argv.slice(2);
const scanAll = scanAllFlag === '1';

const LOCALES = [
  { name: 'base', file: path.join(rootDir, 'apps/harmony/entry/src/main/resources/base/element/string.json') },
  { name: 'zh_CN', file: path.join(rootDir, 'apps/harmony/entry/src/main/resources/zh_CN/element/string.json') },
  { name: 'ja_JP', file: path.join(rootDir, 'apps/harmony/entry/src/main/resources/ja_JP/element/string.json') },
];

const FORBIDDEN_TERMS = [
  '个人路由',
  '机群网',
  'Personal Router',
  'Hermes',
  'パーソナルルーター',
];

const BARE_LITERAL_WHITELIST = new Set([
  'Orbit',
  'BOARD.md',
  'MCP',
  'PR',
  'URL',
]);

const errors = [];

// 1. Load all three string.json files and verify key parity.
const maps = new Map();
for (const loc of LOCALES) {
  if (!fs.existsSync(loc.file)) {
    errors.push(`Missing string resource file: ${path.relative(rootDir, loc.file)}`);
    continue;
  }
  const parsed = JSON.parse(fs.readFileSync(loc.file, 'utf8'));
  const entries = Array.isArray(parsed.string) ? parsed.string : [];
  const map = new Map();
  for (const item of entries) {
    if (!item || typeof item.name !== 'string' || typeof item.value !== 'string') {
      errors.push(`Malformed entry in ${loc.name}: ${JSON.stringify(item)}`);
      continue;
    }
    if (map.has(item.name)) {
      errors.push(`Duplicate key "${item.name}" in ${loc.name}`);
    }
    map.set(item.name, item.value);
  }
  maps.set(loc.name, map);
}

/**
 * Validate and extract canonical placeholder argument signature for HarmonyOS printf-style
 * format strings.
 *
 * Enforces per-locale validity BEFORE cross-locale comparison:
 * - Rejects mixing positional (`%1$s`) and sequential (`%s`/`%d`) specifiers.
 * - Rejects zero-indexed positional slots (`%0$s`).
 * - Rejects type conflicts on the same slot (`%1$s %1$d`).
 * - Rejects missing gap slots (e.g. `%2$d` without `%1$...`).
 * - Tracks BOTH slot type AND exact usage count per slot (e.g. `1:s*1,2:d*1`) so extra
 *   repeated references in one locale are caught.
 */
function analyzePlaceholders(str) {
  const slotTypes = new Map();
  const slotCounts = new Map();
  const sequential = [];
  let hasPositional = false;
  let hasSequential = false;

  for (let i = 0; i < str.length; i++) {
    if (str[i] !== '%') continue;
    if (str[i + 1] === '%') {
      i++;
      continue;
    }
    const rest = str.slice(i + 1);
    const anyPosPrefix = /^([0-9]+)\$/.exec(rest);
    if (anyPosPrefix) {
      const slotRaw = anyPosPrefix[1];
      const slot = Number(slotRaw);
      if (slot === 0 || /^0/.test(slotRaw)) {
        return { ok: false, error: `invalid positional slot "%${anyPosPrefix[0]}"` };
      }
      const convChar = rest[anyPosPrefix[0].length] ?? '';
      if (convChar !== 's' && convChar !== 'd') {
        if (convChar.length === 0) {
          return { ok: false, error: `incomplete positional placeholder "%${anyPosPrefix[0]}"` };
        }
        return {
          ok: false,
          error: `unsupported positional placeholder "%${anyPosPrefix[0]}${convChar}" (only s and d are allowed)`,
        };
      }
      hasPositional = true;
      const existing = slotTypes.get(slot);
      if (existing && existing !== convChar) {
        return { ok: false, error: `conflicting types on slot ${slot} (%${slot}$${existing} vs %${slot}$${convChar})` };
      }
      slotTypes.set(slot, convChar);
      slotCounts.set(slot, (slotCounts.get(slot) ?? 0) + 1);
      i += anyPosPrefix[0].length + 1;
      continue;
    }
    const seqMatch = /^([sd])/.exec(rest);
    if (seqMatch) {
      hasSequential = true;
      sequential.push(seqMatch[1]);
      i += 1;
    }
  }

  if (hasPositional && hasSequential) {
    return { ok: false, error: 'mixes positional (%n$s/%n$d) and sequential (%s/%d) specifiers' };
  }

  if (hasPositional) {
    const maxSlot = Math.max(...slotTypes.keys());
    const parts = [];
    for (let s = 1; s <= maxSlot; s++) {
      if (!slotTypes.has(s)) {
        return { ok: false, error: `missing positional slot ${s} (max slot is ${maxSlot})` };
      }
      parts.push(`${s}:${slotTypes.get(s)}*${slotCounts.get(s)}`);
    }
    return { ok: true, signature: parts.join(',') };
  }

  const parts = sequential.map((spec, idx) => `${idx + 1}:${spec}*1`);
  return { ok: true, signature: parts.join(',') };
}

if (maps.size === LOCALES.length) {
  const baseMap = maps.get('base');
  const zhMap = maps.get('zh_CN');
  const jaMap = maps.get('ja_JP');

  const allKeys = new Set([...baseMap.keys(), ...zhMap.keys(), ...jaMap.keys()]);
  for (const key of [...allKeys].sort()) {
    const missing = [];
    if (!baseMap.has(key)) missing.push('base');
    if (!zhMap.has(key)) missing.push('zh_CN');
    if (!jaMap.has(key)) missing.push('ja_JP');
    if (missing.length > 0) {
      errors.push(`Key "${key}" missing in locale(s): ${missing.join(', ')}`);
      continue;
    }

    // 2. Per-locale placeholder validity + cross-locale slot type & count parity.
    const resBase = analyzePlaceholders(baseMap.get(key));
    const resZh = analyzePlaceholders(zhMap.get(key));
    const resJa = analyzePlaceholders(jaMap.get(key));

    let invalidFound = false;
    if (!resBase.ok) {
      errors.push(`Invalid placeholder in base "${key}": ${resBase.error}`);
      invalidFound = true;
    }
    if (!resZh.ok) {
      errors.push(`Invalid placeholder in zh_CN "${key}": ${resZh.error}`);
      invalidFound = true;
    }
    if (!resJa.ok) {
      errors.push(`Invalid placeholder in ja_JP "${key}": ${resJa.error}`);
      invalidFound = true;
    }
    if (!invalidFound && (resBase.signature !== resZh.signature || resBase.signature !== resJa.signature)) {
      errors.push(
        `Placeholder mismatch for "${key}": base=[${resBase.signature}] zh_CN=[${resZh.signature}] ja_JP=[${resJa.signature}]`
      );
    }
  }

  // 3. Forbidden terminology scan across all three locales.
  for (const [locName, locMap] of maps.entries()) {
    for (const [key, val] of locMap.entries()) {
      const lowerVal = val.toLowerCase();
      for (const term of FORBIDDEN_TERMS) {
        if (lowerVal.includes(term.toLowerCase())) {
          errors.push(`Forbidden term "${term}" found in ${locName} string "${key}": ${JSON.stringify(val)}`);
        }
      }
    }
  }
}

// 4. Verify all configured scan paths exist and collect .ets files.
function collectEtsFiles(targetRelPaths) {
  const out = [];
  const walk = (absPath) => {
    const st = fs.statSync(absPath);
    if (st.isFile()) {
      if (absPath.endsWith('.ets')) out.push(absPath);
      return;
    }
    if (st.isDirectory()) {
      for (const entry of fs.readdirSync(absPath).sort()) {
        walk(path.join(absPath, entry));
      }
    }
  };
  for (const rel of targetRelPaths) {
    const absPath = path.join(rootDir, rel);
    if (!fs.existsSync(absPath)) {
      errors.push(`Configured scan path does not exist: ${rel}`);
      continue;
    }
    walk(absPath);
  }
  return out;
}

const scanTargets = scanAll ? ['apps/harmony/entry/src/main/ets'] : enforcedPaths;
const etsFiles = collectEtsFiles(scanTargets);

/**
 * Strip JS/ArkTS line and block comments while keeping string literals intact and preserving
 * exact line breaks and byte offsets so multiline call matches map back to exact line numbers.
 */
function stripCommentsPreservingLayout(source) {
  let out = '';
  let i = 0;
  let state = 'code'; // 'code' | 'sq' | 'dq' | 'bt' | 'line_comment' | 'block_comment'
  while (i < source.length) {
    const ch = source[i];
    const next = source[i + 1];
    if (state === 'code') {
      if (ch === '/' && next === '/') {
        state = 'line_comment';
        out += '  ';
        i += 2;
      } else if (ch === '/' && next === '*') {
        state = 'block_comment';
        out += '  ';
        i += 2;
      } else if (ch === "'") {
        state = 'sq';
        out += ch;
        i += 1;
      } else if (ch === '"') {
        state = 'dq';
        out += ch;
        i += 1;
      } else if (ch === '`') {
        state = 'bt';
        out += ch;
        i += 1;
      } else {
        out += ch;
        i += 1;
      }
    } else if (state === 'line_comment') {
      if (ch === '\n') {
        state = 'code';
        out += '\n';
      } else {
        out += ' ';
      }
      i += 1;
    } else if (state === 'block_comment') {
      if (ch === '*' && next === '/') {
        state = 'code';
        out += '  ';
        i += 2;
      } else {
        out += ch === '\n' ? '\n' : ' ';
        i += 1;
      }
    } else {
      const quote = state === 'sq' ? "'" : (state === 'dq' ? '"' : '`');
      if (ch === '\\' && i + 1 < source.length) {
        out += ch + next;
        i += 2;
      } else if (ch === quote) {
        state = 'code';
        out += ch;
        i += 1;
      } else {
        out += ch;
        i += 1;
      }
    }
  }
  return out;
}

function lineNumberAtOffset(source, offset) {
  let line = 1;
  for (let i = 0; i < offset; i++) {
    if (source[i] === '\n') line++;
  }
  return line;
}

// Supports both single-line and multiline calls, e.g. `Text(\n  'Hello world'\n)` or with 2nd arg.
const BARE_LITERAL_PATTERNS = [
  { kind: 'Text', re: /\bText\s*\(\s*(['"])((?:\\.|(?!\1)[^\\\n])*)\1\s*[,)]/g },
  { kind: 'accessibilityText', re: /\.accessibilityText\s*\(\s*(['"])((?:\\.|(?!\1)[^\\\n])*)\1\s*[,)]/g },
  { kind: 'Label', re: /\bLabel\s*\(\s*(['"])((?:\\.|(?!\1)[^\\\n])*)\1\s*[,)]/g },
  { kind: 'placeholder', re: /\bplaceholder\s*:\s*(['"])((?:\\.|(?!\1)[^\\\n])*)\1/g },
  { kind: 'showToast', re: /\bshowToast\s*\(\s*(['"])((?:\\.|(?!\1)[^\\\n])*)\1\s*[,)]/g, skipLegacyStore: true },
];

for (const absFile of etsFiles) {
  const relFile = path.relative(rootDir, absFile);
  const rawSource = fs.readFileSync(absFile, 'utf8');
  const rawLines = rawSource.split(/\r?\n/);
  const cleaned = stripCommentsPreservingLayout(rawSource);

  for (const pat of BARE_LITERAL_PATTERNS) {
    if (pat.skipLegacyStore && relFile === 'apps/harmony/entry/src/main/ets/model/OrbitHarmonyStore.ets') {
      // PR-6 clipboard, test-push & ALL `*Scoped` store entrypoints must strictly avoid bare English showToast literals.
      const methodDeclRe = /\n  public (?:async )?([A-Za-z0-9_]+)\s*\(/g;
      const checkedNames = new Set([
        'copyToClipboard',
        'pasteAuthorizedPairingUri',
        'sendTestPushNotification',
      ]);
      const spans = [];
      let dm;
      while ((dm = methodDeclRe.exec(cleaned)) !== null) {
        spans.push({ name: dm[1], start: dm.index });
      }
      for (let si = 0; si < spans.length; si++) {
        const { name, start } = spans[si];
        if (!checkedNames.has(name) && !name.endsWith('Scoped')) continue;
        const end = si + 1 < spans.length ? spans[si + 1].start : cleaned.length;
        const snippet = cleaned.slice(start, end);
        pat.re.lastIndex = 0;
        let sm;
        while ((sm = pat.re.exec(snippet)) !== null) {
          const lit = sm[2].trim();
          if (!/[A-Za-z]{3,}/.test(lit) || BARE_LITERAL_WHITELIST.has(lit)) continue;
          const startLine = lineNumberAtOffset(cleaned, start + sm.index);
          errors.push(
            `Bare literal in ${relFile}:${startLine} (${pat.kind} in ${name}): ${JSON.stringify(sm[2])} (use showToastResource($r('app.string.*')))`
          );
        }
      }
      continue;
    }
    pat.re.lastIndex = 0;
    let match;
    while ((match = pat.re.exec(cleaned)) !== null) {
      const literal = match[2].trim();
      if (!/[A-Za-z]{3,}/.test(literal)) {
        continue;
      }
      if (BARE_LITERAL_WHITELIST.has(literal)) {
        continue;
      }
      const startLine = lineNumberAtOffset(cleaned, match.index);
      const endLine = lineNumberAtOffset(cleaned, match.index + match[0].length);
      let ignored = false;
      for (let ln = startLine; ln <= endLine; ln++) {
        if ((rawLines[ln - 1] ?? '').includes('i18n-ignore')) {
          ignored = true;
          break;
        }
      }
      if (ignored) {
        continue;
      }
      errors.push(
        `Bare literal in ${relFile}:${startLine} (${pat.kind}): ${JSON.stringify(match[2])} (use $r('app.string.*') or add // i18n-ignore)`
      );
    }
  }
}

if (errors.length > 0) {
  for (const err of errors) {
    console.error(`i18n check failed: ${err}`);
  }
  process.exit(1);
}

const keyCount = maps.get('base')?.size ?? 0;
console.log(
  `Verified HarmonyOS i18n: ${keyCount} keys across 3 locales, 0 forbidden terms, ${etsFiles.length} .ets file(s) clean (${scanAll ? 'full' : 'enforced scope'}).`
);
NODE
