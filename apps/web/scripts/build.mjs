import { existsSync, readFileSync } from "node:fs";
import { resolve, join } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const web = fileURLToPath(new URL("../", import.meta.url));
const root = resolve(web, "../..");
const desktop = resolve(web, "../desktop");
const run = (command, args, cwd = root) =>
  execFileSync(command, args, { cwd, stdio: "inherit" });
if (!existsSync(join(desktop, "node_modules/typescript/bin/tsc")))
  run("npm", ["ci"], desktop);
const meta = JSON.parse(
  execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], {
    cwd: root,
  }),
);
const version = /name = "wasm-bindgen"\nversion = "([^"]+)"/.exec(
  readFileSync(join(root, "Cargo.lock"), "utf8"),
)[1];
let bindgen = "wasm-bindgen";
function matches(command) {
  try {
    return (
      execFileSync(command, ["--version"], { encoding: "utf8" }).trim() ===
      "wasm-bindgen " + version
    );
  } catch {
    return false;
  }
}
if (!matches(bindgen)) {
  const local = join(meta.target_directory, "wasm-tools");
  bindgen = join(
    local,
    "bin",
    process.platform === "win32" ? "wasm-bindgen.exe" : "wasm-bindgen",
  );
  if (!matches(bindgen))
    run("cargo", [
      "install",
      "--locked",
      "--root",
      local,
      "--version",
      version,
      "wasm-bindgen-cli",
    ]);
}
run("cargo", [
  "build",
  "--locked",
  "--release",
  "--target",
  "wasm32-unknown-unknown",
  "-p",
  "orbit-client-wasm",
]);
run(bindgen, [
  join(
    meta.target_directory,
    "wasm32-unknown-unknown/release/orbit_client_wasm.wasm",
  ),
  "--target",
  "web",
  "--out-dir",
  join(web, "src/generated"),
]);
run(
  process.execPath,
  [join(desktop, "node_modules/typescript/bin/tsc"), "-p", "tsconfig.json"],
  web,
);
run(
  process.execPath,
  [join(web, "node_modules/vite/bin/vite.js"), "build"],
  web,
);
