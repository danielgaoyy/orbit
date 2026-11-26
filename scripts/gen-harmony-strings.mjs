#!/usr/bin/env node
/**
 * Generate HarmonyOS trilingual string.json resources from Desktop locales + Harmony-only appendix.
 * Pure Node.js (zero third-party dependencies).
 *
 * Usage:
 *   node scripts/gen-harmony-strings.mjs          # write generated files
 *   node scripts/gen-harmony-strings.mjs --check  # verify checked-in files match generated output
 */
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const DESKTOP_LOCALES_DIR = join(ROOT, 'apps/desktop/src/lib/i18n/locales');
const WHITELIST_PATH = join(ROOT, 'apps/harmony/i18n-keys.txt');
const APPENDIX_PATH = join(ROOT, 'apps/harmony/i18n-harmony-only.json');
const HARMONY_RES_DIR = join(ROOT, 'apps/harmony/entry/src/main/resources');

const LOCALE_TARGETS = [
  { id: 'en', desktopFile: 'en.ts', harmonyDir: 'base' },
  { id: 'zh-CN', desktopFile: 'zh-CN.ts', harmonyDir: 'zh_CN' },
  { id: 'ja', desktopFile: 'ja.ts', harmonyDir: 'ja_JP' },
];

const INT_PLACEHOLDERS = new Set([
  'count',
  'n',
  'num',
  'total',
  'days',
  'minutes',
  'seconds',
  'percent',
  'size',
  'tokens',
  'pct',
  'online',
  'completed',
  'handled',
  'approved',
  'skipped',
  'resolved',
  'selected',
  'index',
  'rev',
  'hours',
  'files',
  'lines',
  'steps',
  'turns',
  'remaining',
  'max',
  'min',
  'port',
]);

const STRING_PLACEHOLDERS = new Set([
  'name',
  'host',
  'project',
  'group',
  'title',
  'id',
  'error',
  'label',
  'mode',
  'path',
  'provider',
  'query',
  'summary',
  'keys',
  'action',
  'panel',
  'version',
  'mod',
  'scope',
  'cwd',
  'agent',
  'status',
  'branch',
  'file',
  'dir',
  'url',
  'target',
  'source',
  'reason',
  'detail',
  'value',
  'text',
  'message',
  'user',
  'role',
  'model',
  'effort',
  'duration',
  'a',
  'b',
  'handle',
  'workspace',
  'command',
  'tool',
  'type',
  'kind',
  'date',
  'time',
]);

function fail(message) {
  console.error(`gen-harmony-strings: ERROR: ${message}`);
  process.exit(1);
}

/**
 * Decode a double-quoted, single-quoted, or backtick JS/TS string literal body.
 */
function decodeJsString(raw, quote, filePath, lineNo) {
  let out = '';
  for (let i = 0; i < raw.length; i++) {
    const ch = raw[i];
    if (ch !== '\\') {
      out += ch;
      continue;
    }
    i++;
    if (i >= raw.length) {
      fail(`Unterminated escape sequence in ${filePath}:${lineNo}`);
    }
    const esc = raw[i];
    switch (esc) {
      case '"':
      case "'":
      case '`':
      case '\\':
      case '/':
        out += esc;
        break;
      case 'n':
        out += '\n';
        break;
      case 'r':
        out += '\r';
        break;
      case 't':
        out += '\t';
        break;
      case 'b':
        out += '\b';
        break;
      case 'f':
        out += '\f';
        break;
      case '0':
        out += '\0';
        break;
      case 'u': {
        if (raw[i + 1] === '{') {
          const close = raw.indexOf('}', i + 2);
          if (close === -1) {
            fail(`Invalid unicode escape in ${filePath}:${lineNo}`);
          }
          const hex = raw.slice(i + 2, close);
          out += String.fromCodePoint(parseInt(hex, 16));
          i = close;
        } else {
          const hex = raw.slice(i + 1, i + 5);
          if (!/^[0-9a-fA-F]{4}$/.test(hex)) {
            fail(`Invalid \\u escape in ${filePath}:${lineNo}`);
          }
          out += String.fromCharCode(parseInt(hex, 16));
          i += 4;
        }
        break;
      }
      case 'x': {
        const hex = raw.slice(i + 1, i + 3);
        if (!/^[0-9a-fA-F]{2}$/.test(hex)) {
          fail(`Invalid \\x escape in ${filePath}:${lineNo}`);
        }
        out += String.fromCharCode(parseInt(hex, 16));
        i += 2;
        break;
      }
      case '\n':
        break;
      case '\r':
        if (raw[i + 1] === '\n') i++;
        break;
      default:
        if (esc === quote) {
          out += quote;
        } else {
          out += esc;
        }
        break;
    }
  }
  return out;
}

/**
 * Parse a Desktop TypeScript locale dictionary file (`export const xx = { "key": "val", ... }`).
 * Correctly handles comments, multiline values, escaped quotes, template literals, and `+` concatenations.
 */
function parseDesktopLocaleFile(filePath) {
  const src = readFileSync(filePath, 'utf8');
  const exportMatch = /export\s+const\s+\w+(?:\s*:\s*[^=]+)?\s*=\s*\{/.exec(src);
  if (!exportMatch) {
    fail(`Could not find exported locale object in ${filePath}`);
  }
  const map = new Map();
  let i = exportMatch.index + exportMatch[0].length;
  let line = src.slice(0, i).split('\n').length;
  const len = src.length;

  function skipWsAndComments() {
    while (i < len) {
      const ch = src[i];
      if (ch === ' ' || ch === '\t' || ch === '\r') {
        i++;
      } else if (ch === '\n') {
        i++;
        line++;
      } else if (ch === '/' && src[i + 1] === '/') {
        i += 2;
        while (i < len && src[i] !== '\n') i++;
      } else if (ch === '/' && src[i + 1] === '*') {
        i += 2;
        while (i < len && !(src[i] === '*' && src[i + 1] === '/')) {
          if (src[i] === '\n') line++;
          i++;
        }
        i += 2;
      } else {
        break;
      }
    }
  }

  function readQuotedString() {
    const quote = src[i];
    if (quote !== '"' && quote !== "'" && quote !== '`') {
      fail(`Expected string literal at ${filePath}:${line}, got ${JSON.stringify(src.slice(i, i + 16))}`);
    }
    const startLine = line;
    i++;
    let raw = '';
    while (i < len) {
      const ch = src[i];
      if (ch === '\\') {
        raw += ch;
        i++;
        if (i < len) {
          if (src[i] === '\n') line++;
          raw += src[i];
          i++;
        }
        continue;
      }
      if (ch === quote) {
        i++;
        if (quote === '`' && raw.includes('${')) {
          fail(`Dynamic template interpolation is not supported in ${filePath}:${startLine}`);
        }
        return decodeJsString(raw, quote, filePath, startLine);
      }
      if (ch === '\n') {
        if (quote !== '`') {
          fail(`Unterminated string literal at ${filePath}:${startLine}`);
        }
        line++;
      }
      raw += ch;
      i++;
    }
    fail(`Unterminated string literal starting at ${filePath}:${startLine}`);
  }

  while (i < len) {
    skipWsAndComments();
    if (i >= len) {
      fail(`Unexpected EOF before closing brace in ${filePath}`);
    }
    if (src[i] === '}') {
      break;
    }
    if (src[i] === ',') {
      i++;
      continue;
    }
    const keyLine = line;
    const key = readQuotedString();
    skipWsAndComments();
    if (src[i] !== ':') {
      fail(`Expected ':' after key ${JSON.stringify(key)} at ${filePath}:${keyLine}`);
    }
    i++;
    skipWsAndComments();
    let value = readQuotedString();
    skipWsAndComments();
    while (src[i] === '+') {
      i++;
      skipWsAndComments();
      value += readQuotedString();
      skipWsAndComments();
    }
    if (map.has(key)) {
      fail(`Duplicate desktop locale key ${JSON.stringify(key)} in ${filePath}:${keyLine}`);
    }
    map.set(key, value);
    if (src[i] === ',') {
      i++;
    }
  }
  return map;
}

function parseWhitelist(path) {
  const raw = readFileSync(path, 'utf8');
  const patterns = [];
  for (const line of raw.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (trimmed.length === 0 || trimmed.startsWith('#')) {
      continue;
    }
    patterns.push(trimmed);
  }
  return patterns;
}

function wildcardToRegex(pattern) {
  const escaped = pattern.replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*');
  return new RegExp(`^${escaped}$`);
}

function convertDesktopKey(desktopKey) {
  const converted = desktopKey.replace(/\./g, '_');
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(converted)) {
    fail(`Desktop key ${JSON.stringify(desktopKey)} converts to invalid Harmony resource name ${JSON.stringify(converted)}`);
  }
  return converted;
}

function classifyPlaceholder(name, desktopKey) {
  if (INT_PLACEHOLDERS.has(name)) {
    return 'd';
  }
  if (STRING_PLACEHOLDERS.has(name)) {
    return 's';
  }
  fail(
    `Cannot determine format specifier (%s vs %d) for placeholder {${name}} in key ${JSON.stringify(desktopKey)}. ` +
      'Add it to INT_PLACEHOLDERS or STRING_PLACEHOLDERS in scripts/gen-harmony-strings.mjs.'
  );
}

function extractPlaceholders(value, desktopKey) {
  const names = [];
  let i = 0;
  while (i < value.length) {
    const ch = value[i];
    if (ch === '{') {
      const close = value.indexOf('}', i + 1);
      if (close === -1) {
        fail(`Unclosed '{' placeholder in key ${JSON.stringify(desktopKey)}: ${JSON.stringify(value)}`);
      }
      const name = value.slice(i + 1, close);
      if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) {
        fail(`Invalid placeholder {${name}} in key ${JSON.stringify(desktopKey)}`);
      }
      names.push(name);
      i = close + 1;
    } else if (ch === '}') {
      fail(`Unmatched '}' in key ${JSON.stringify(desktopKey)}: ${JSON.stringify(value)}`);
    } else {
      i++;
    }
  }
  return names;
}

/**
 * Convert `{placeholder}` markers to HarmonyOS `%s` / `%d` (or `%N$s` / `%N$d` when a translation
 * reorders placeholders relative to the canonical English order).
 */
function mapPlaceholders(value, desktopKey, canonicalNames) {
  const localNames = extractPlaceholders(value, desktopKey);
  if (localNames.length !== canonicalNames.length) {
    fail(
      `Placeholder count mismatch for ${JSON.stringify(desktopKey)}: ` +
        `en has [${canonicalNames.join(', ')}], locale has [${localNames.join(', ')}]`
    );
  }
  const sameOrder = localNames.every((name, idx) => name === canonicalNames[idx]);
  if (!sameOrder) {
    const canonicalSet = new Set(canonicalNames);
    const localSet = new Set(localNames);
    if (canonicalSet.size !== canonicalNames.length || localSet.size !== localNames.length) {
      fail(`Cannot safely reorder repeated placeholders in ${JSON.stringify(desktopKey)}`);
    }
    for (const name of localNames) {
      if (!canonicalSet.has(name)) {
        fail(`Placeholder {${name}} in ${JSON.stringify(desktopKey)} does not exist in en.ts`);
      }
    }
  }

  let out = '';
  let idx = 0;
  let i = 0;
  while (i < value.length) {
    if (value[i] === '{') {
      const close = value.indexOf('}', i + 1);
      const name = value.slice(i + 1, close);
      const spec = classifyPlaceholder(name, desktopKey);
      if (sameOrder) {
        out += `%${spec}`;
      } else {
        const canonicalPos = canonicalNames.indexOf(name) + 1;
        out += `%${canonicalPos}$${spec}`;
      }
      idx++;
      i = close + 1;
    } else if (value[i] === '%' && localNames.length > 0) {
      out += '%%';
      i++;
    } else {
      out += value[i];
      i++;
    }
  }
  if (idx !== localNames.length) {
    fail(`Internal placeholder mapping error for ${JSON.stringify(desktopKey)}`);
  }
  return out;
}

function loadHarmonyOnlyAppendix(path) {
  if (!existsSync(path)) {
    return new Map();
  }
  const parsed = JSON.parse(readFileSync(path, 'utf8'));
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    fail(`${path} must be a JSON object mapping Harmony key -> { en, "zh-CN", ja }`);
  }
  const out = new Map();
  for (const [key, entry] of Object.entries(parsed)) {
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) {
      fail(`Invalid Harmony-only key ${JSON.stringify(key)} in ${path}`);
    }
    if (!entry || typeof entry !== 'object') {
      fail(`Invalid entry for ${JSON.stringify(key)} in ${path}`);
    }
    for (const loc of LOCALE_TARGETS) {
      if (typeof entry[loc.id] !== 'string') {
        fail(`Missing locale ${JSON.stringify(loc.id)} for ${JSON.stringify(key)} in ${path}`);
      }
    }
    out.set(key, {
      en: entry['en'],
      'zh-CN': entry['zh-CN'],
      ja: entry['ja'],
    });
  }
  return out;
}

function generateStrings() {
  const desktopMaps = new Map();
  for (const loc of LOCALE_TARGETS) {
    const filePath = join(DESKTOP_LOCALES_DIR, loc.desktopFile);
    desktopMaps.set(loc.id, parseDesktopLocaleFile(filePath));
  }

  const enMap = desktopMaps.get('en');
  const patterns = parseWhitelist(WHITELIST_PATH);
  const selectedDesktopKeys = [];
  const selectedSet = new Set();

  for (const pattern of patterns) {
    if (pattern.includes('*')) {
      const re = wildcardToRegex(pattern);
      let matched = 0;
      for (const key of enMap.keys()) {
        if (re.test(key)) {
          matched++;
          if (!selectedSet.has(key)) {
            selectedSet.add(key);
            selectedDesktopKeys.push(key);
          }
        }
      }
      if (matched === 0) {
        fail(`Whitelist wildcard ${JSON.stringify(pattern)} matched 0 desktop keys`);
      }
    } else {
      if (!enMap.has(pattern)) {
        fail(`Whitelist key ${JSON.stringify(pattern)} does not exist in desktop en.ts`);
      }
      if (!selectedSet.has(pattern)) {
        selectedSet.add(pattern);
        selectedDesktopKeys.push(pattern);
      }
    }
  }

  const appendix = loadHarmonyOnlyAppendix(APPENDIX_PATH);

  // Check collisions across desktop keys and Harmony-only appendix.
  const originByHarmonyKey = new Map();
  const conflicts = [];
  const registerKey = (harmonyKey, origin) => {
    if (originByHarmonyKey.has(harmonyKey)) {
      conflicts.push({
        harmonyKey,
        first: originByHarmonyKey.get(harmonyKey),
        second: origin,
      });
    } else {
      originByHarmonyKey.set(harmonyKey, origin);
    }
  };

  const merged = new Map(); // harmonyKey -> { en, zh-CN, ja }

  for (const desktopKey of selectedDesktopKeys) {
    const harmonyKey = convertDesktopKey(desktopKey);
    registerKey(harmonyKey, `desktop:${desktopKey}`);
    const enRaw = enMap.get(desktopKey);
    const canonicalPlaceholders = extractPlaceholders(enRaw, desktopKey);
    const values = {};
    for (const loc of LOCALE_TARGETS) {
      const locMap = desktopMaps.get(loc.id);
      if (!locMap.has(desktopKey)) {
        fail(`Desktop key ${JSON.stringify(desktopKey)} is missing in ${loc.desktopFile}`);
      }
      values[loc.id] = mapPlaceholders(locMap.get(desktopKey), desktopKey, canonicalPlaceholders);
    }
    merged.set(harmonyKey, values);
  }

  for (const [harmonyKey, values] of appendix.entries()) {
    registerKey(harmonyKey, `harmony-only:${harmonyKey}`);
    merged.set(harmonyKey, values);
  }

  if (conflicts.length > 0) {
    console.error('gen-harmony-strings: Key collision table:');
    for (const c of conflicts) {
      console.error(`  ${c.harmonyKey}  <=  ${c.first}  vs  ${c.second}`);
    }
    process.exit(1);
  }

  const sortedKeys = [...merged.keys()].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));

  const outputs = new Map();
  for (const loc of LOCALE_TARGETS) {
    const outPath = join(HARMONY_RES_DIR, loc.harmonyDir, 'element', 'string.json');
    const payload = {
      string: sortedKeys.map((name) => ({
        name,
        value: merged.get(name)[loc.id],
      })),
    };
    outputs.set(outPath, `${JSON.stringify(payload, null, 2)}\n`);
  }

  return { outputs, desktopCount: selectedDesktopKeys.length, appendixCount: appendix.size, totalCount: sortedKeys.length };
}

function main() {
  const args = process.argv.slice(2);
  const checkMode = args.includes('--check');
  const unknownArgs = args.filter((a) => a !== '--check');
  if (unknownArgs.length > 0) {
    fail(`Unknown arguments: ${unknownArgs.join(' ')}. Usage: node scripts/gen-harmony-strings.mjs [--check]`);
  }

  const { outputs, desktopCount, appendixCount, totalCount } = generateStrings();

  if (checkMode) {
    const diffs = [];
    for (const [outPath, expected] of outputs.entries()) {
      if (!existsSync(outPath)) {
        diffs.push(`Missing generated file: ${outPath}`);
        continue;
      }
      const actual = readFileSync(outPath, 'utf8');
      if (actual !== expected) {
        diffs.push(`Out of sync: ${outPath}`);
      }
    }
    if (diffs.length > 0) {
      for (const d of diffs) {
        console.error(d);
      }
      console.error('Run `node scripts/gen-harmony-strings.mjs` to regenerate HarmonyOS string.json resources.');
      process.exit(1);
    }
    console.log(
      `Verified ${totalCount} HarmonyOS i18n keys (${desktopCount} desktop + ${appendixCount} harmony-only) across 3 locales.`
    );
    return;
  }

  for (const [outPath, content] of outputs.entries()) {
    mkdirSync(dirname(outPath), { recursive: true });
    writeFileSync(outPath, content, 'utf8');
  }
  console.log(
    `Generated ${totalCount} HarmonyOS i18n keys (${desktopCount} desktop + ${appendixCount} harmony-only) across 3 locales.`
  );
}

main();
