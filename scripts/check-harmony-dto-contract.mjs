#!/usr/bin/env node
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';

const harmony = fileURLToPath(new URL('../apps/harmony/', import.meta.url));
const model = join(harmony, 'entry/src/main/ets/model');
const fixtures = join(harmony, 'contract/fixtures');
const files = [join(model, 'OrbitTypes.ets')].filter(existsSync);
const split = join(model, 'dto');
if (existsSync(split)) {
  files.push(...readdirSync(split).filter(name => name.endsWith('.ets')).sort().map(name => join(split, name)));
}

// Ignore delimiters inside comments and string literal types. A nested
// object type belongs to its parent field.
const interfaces = new Map();
for (const path of files) {
  const source = readFileSync(path, 'utf8').replace(
    /"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|\/\/[^\n]*|\/\*[\s\S]*?\*\//g,
    token => token.startsWith('/') ? ' ' : "''",
  );
  for (const match of source.matchAll(/\binterface\s+(\w+)\s*\{/g)) {
    const name = match[1];
    const start = match.index + match[0].length;
    let depth = 1;
    let body = '';
    for (let i = start; depth && i < source.length; i++) {
      const c = source[i];
      if (c === '{') {
        if (depth === 1) body += '{}';
        depth++;
      } else if (c === '}') {
        depth--;
      } else if (depth === 1) {
        body += c;
      }
    }
    if (depth) throw new Error('Unclosed interface ' + name + ' in ' + path);
    if (interfaces.has(name)) throw new Error('Duplicate interface ' + name + ' in ' + path);
    interfaces.set(name, body);
  }
}

const views = [
  'AgentStageDelta', 'StageTurn', 'FleetOverview', 'HostDetail',
  'FleetSidebar', 'PersonalAgentBriefing', 'TerminalScreen',
];
const variants = [
  'user_prompt', 'assistant_thinking', 'assistant_message', 'tool_call',
  'permission_request', 'question_request', 'notice', 'agent_message',
];
const expected = [
  ...views.map(view => view + 'View.json'),
  ...variants.map(tag => 'TimelineItemView.' + tag + '.json'),
];
const errors = [];
const keys = new Map();
const jsonFiles = readdirSync(fixtures).filter(name => name.endsWith('.json')).sort();
for (const file of expected) {
  if (!jsonFiles.includes(file)) errors.push('Missing fixture ' + file);
}
for (const file of jsonFiles) {
  const match = /^(\w+)View(?:\.(\w+))?\.json$/.exec(file);
  if (!match) throw new Error('Unexpected fixture name ' + file);
  const [, view, tag] = match;
  const value = JSON.parse(readFileSync(join(fixtures, file), 'utf8'));
  if (!value || Array.isArray(value) || typeof value !== 'object') throw new Error('Expected an object in ' + file);
  if (tag !== undefined && value.item_type !== tag) throw new Error('Wrong discriminator in ' + file);
  // TimelineItemDto represents all variants in one interface.
  const union = keys.get(view) ?? new Set();
  for (const key of Object.keys(value)) union.add(key);
  keys.set(view, union);
}
for (const [view, rust] of keys) {
  const aliases = [view + 'Dto', 'Orbit' + view + 'Dto', view + 'View'];
  const candidates = aliases.filter(name => interfaces.has(name));
  if (candidates.length !== 1) {
    errors.push(candidates.length ? 'Ambiguous interfaces for ' + view : 'Missing interface ' + aliases[0]);
    continue;
  }
  const [name] = candidates;
  const ark = new Set();
  for (const field of interfaces.get(name).split(/;|\n(?=\s*(?:readonly\s+)?\w+\??\s*:)/).map(s => s.trim()).filter(Boolean)) {
    const parsed = /^(?:readonly\s+)?(\w+)\??\s*:[\s\S]+$/.exec(field);
    if (!parsed) throw new Error('Unsupported declaration in ' + name + ': ' + field);
    if (ark.has(parsed[1])) throw new Error('Duplicate field ' + name + '.' + parsed[1]);
    ark.add(parsed[1]);
  }
  const missing = [...rust].filter(k => !ark.has(k)).sort();
  const extra = [...ark].filter(k => !rust.has(k)).sort();
  if (missing.length) errors.push(name + ' missing in ArkTS: ' + missing.join(', '));
  if (extra.length) errors.push(name + ' extra in ArkTS: ' + extra.join(', '));
}
if (errors.length) {
  console.error(errors.join('\n'));
  process.exitCode = 1;
} else {
  console.log('Verified ' + jsonFiles.length + ' HarmonyOS DTO fixtures against ' + keys.size + ' ArkTS interfaces.');
}
