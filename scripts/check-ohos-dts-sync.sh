#!/usr/bin/env bash
set -euo pipefail
python3 - "${BASH_SOURCE[0]}" "$@" <<'PY'
"""Generate declarations from the safe Rust N-API surface. Unknown signatures fail CI."""
import difflib
from pathlib import Path
import re
import sys

root = Path(sys.argv[1]).resolve().parent.parent
source = (root / "crates/orbit-ffi-ohos/src/lib.rs").read_text()
dest = root / "apps/harmony/entry/src/main/cpp/types/liborbit_ohos/index.d.ts"
# Keep quoted URLs and braces in Rust strings intact while dropping comments.
tokens = re.compile(r'"(?:\\.|[^"\\])*"|//[^\n]*|/\*.*?\*/', re.S)
source = tokens.sub(lambda m: m[0] if m[0].startswith('"') else "", source)
exports = re.compile(r"#\[napi(?:\(([^)]*)\))?\]\s*(?:#\[[^\]]*\]\s*)*")
tasks = dict(re.findall(r"impl Task for (\w+)\s*\{.*?type JsValue\s*=\s*(\w+|\(\))\s*;", source, re.S))
scopes = {}
classes = {}
functions = []
aliases = []

def camel(name):
    return re.sub(r"_([a-z0-9])", lambda m: m[1].upper(), name)

def ts_type(raw, owner):
    raw = raw.strip()
    primitive = {"()": "void", "Self": owner, "String": "string", "bool": "boolean",
                 "u32": "number", "i64": "number", "Uint8Array": "Uint8Array",
                 "ThreadsafeFunction<String, ()>": "(err: Error | null, payloadJson: string) => void"}
    if raw in primitive:
        return primitive[raw]
    generic = re.fullmatch(r"(\w+)<(.+)>", raw)
    if generic:
        kind, inner = generic.groups()
        if kind == "Result":
            return ts_type(inner, owner)
        if kind == "Option":
            return ts_type(inner, owner) + " | null"
        if kind == "Vec":
            return "Array<" + ts_type(inner, owner) + ">"
        if kind == "AsyncTask":
            if inner.startswith("MobileRpcTask<"):
                inner = inner[len("MobileRpcTask<"):-1]
            else:
                inner = tasks[inner]
            return "Promise<" + ts_type(inner, owner) + ">"
    sys.exit(f"Unsupported N-API type {raw!r}")

def end_brace(start):
    depth = 0
    # Skip strings so braces inside messages cannot terminate the impl.
    for token in re.finditer(r'"(?:\\.|[^"\\])*"|[{}]', source[start:]):
        if token[0] == "{":
            depth += 1
        elif token[0] == "}":
            depth -= 1
            if depth == 0:
                return start + token.end()
    sys.exit("Unclosed napi impl")

for attr in exports.finditer(source):
    options = attr[1] or ""
    item = source[attr.end():]
    if options == "module_exports":
        body_start = source.index("{", attr.end())
        body = source[body_start:end_brace(body_start)]
        alias = re.fullmatch(r'\s*\{\s*let constructor: Unknown = exports\.get_named_property\("(\w+)"\)\?;\s*exports\.set_named_property\("(\w+)", constructor\)\s*\}\s*', body)
        if not alias:
            sys.exit("Unsupported N-API module exports hook")
        aliases.append(alias.groups())
        continue
    obj = re.match(r"pub struct (\w+)\s*\{", item)
    if obj:
        classes[obj[1]] = []
        continue
    impl = re.match(r"impl (\w+)\s*\{", item)
    if impl:
        scopes[attr.end()] = (end_brace(attr.end() + impl.end() - 1), impl[1])
        continue
    method = re.match(r"pub (?:async )?fn (\w+)\s*\((.*?)\)\s*(?:->\s*([^{]+?))?\s*\{", item, re.S)
    if not method:
        sys.exit("Unsupported napi export at line " + str(source[:attr.start()].count("\n") + 1))
    owner = next((name for start, (end, name) in scopes.items() if start < attr.start() < end), None)
    override = re.search(r'js_name\s*=\s*"(\w+)"', options)
    name = override[1] if override else camel(method[1])
    constructor = "constructor" in options
    if constructor:
        name = "constructor"
    args = " ".join(method[2].split()).strip(", ")
    parts = re.split(r",\s*(?=\w+\s*:)", args) if args else []
    params = []
    required = False
    for part in reversed(parts):
        if part == "&self":
            continue
        key, raw = (v.strip() for v in part.split(":", 1))
        typ = ts_type(raw, owner)
        opt = raw.startswith("Option<")
        label = camel(key) + ("?" if opt and not required else "")
        params.append(label + ": " + typ + (" | undefined" if opt and required else ""))
        required |= not opt
    args_ts = ", ".join(reversed(params))
    prefix = ("static " if "factory" in options else "") if owner else "export function "
    ret = "" if constructor else ": " + ts_type(method[3] or "()", owner)
    decl = f"{prefix}{name}({args_ts}){ret};"
    entries = classes[owner] if owner else functions
    if any(n == name for n, _ in entries):
        sys.exit(f"Duplicate N-API export {owner}.{name}")
    entries.append((name, decl))

if not classes or not scopes:
    sys.exit("No exported N-API class found")
lines = ["// Generated from orbit-ffi-ohos. Run scripts/check-ohos-dts-sync.sh --write."]
for owner, entries in classes.items():
    lines.append(f"export class {owner} {{")
    lines.extend("  " + decl for _, decl in entries)
    lines.append("}")
lines.extend(decl for _, decl in functions)
for original, alias in aliases:
    if original not in classes or alias in classes:
        sys.exit(f"Invalid class alias {alias}")
    lines.append(f"export {{ {original} as {alias} }};")
expected = "\n".join(lines) + "\n"
if sys.argv[2:] == ["--write"]:
    dest.write_text(expected)
elif sys.argv[2:]:
    sys.exit("Usage: check-ohos-dts-sync.sh [--write]")
elif expected != dest.read_text():
    sys.stderr.writelines(difflib.unified_diff(dest.read_text().splitlines(True), expected.splitlines(True), fromfile="checked-in", tofile="Rust N-API"))
    sys.exit("Run scripts/check-ohos-dts-sync.sh --write to synchronize the declarations.")
print(f"Verified {sum(map(len, classes.values())) + len(functions)} HarmonyOS N-API exports.")
PY
