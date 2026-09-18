#!/usr/bin/env node
/** Symlink containment regressions for package + sparse-workerd helpers. */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

async function main() {
  let assertPathInside, packagePlugin, materializeConfig;
  try {
    ({ assertPathInside } = await import("../dist/sparse-workerd/ensure.js"));
    ({ packagePlugin } = await import("../dist/tools/package.js"));
    ({ materializeConfig } = await import("../dist/sparse-workerd/config.js"));
  } catch {
    console.error("dist/ missing — run npm run build first");
    process.exit(1);
  }

  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "bc-path-"));
  const plugin = path.join(tmp, "plugin");
  const modules = path.join(plugin, "modules");
  fs.mkdirSync(modules, { recursive: true });
  const manifest = {
    api_version: 3,
    id: "sym_test",
    runtime: "workerd",
    entrypoints: ["cli"],
    workerd: {
      compatibility_date: "2026-08-01",
      main_module: "main.js",
      modules_dir: "modules",
      entrypoint: "default",
    },
    capabilities: { network: { mode: "deny" } },
  };
  fs.writeFileSync(
    path.join(plugin, "plugin.toml"),
    `api_version = 3
id = "sym_test"
runtime = "workerd"
entrypoints = ["cli"]
[workerd]
compatibility_date = "2026-08-01"
main_module = "main.js"
modules_dir = "modules"
entrypoint = "default"
[capabilities.network]
mode = "deny"
`,
  );
  fs.writeFileSync(path.join(modules, "main.js"), "export default class X {}");
  const outside = path.join(tmp, "outside.txt");
  fs.writeFileSync(outside, "SECRET_OUTSIDE");
  fs.symlinkSync(outside, path.join(modules, "leak.txt"));

  let threw = false;
  try {
    packagePlugin(plugin, path.join(tmp, "dist"));
  } catch (e) {
    threw = /symlink/i.test(String(e));
    if (!threw) console.error("unexpected package error:", e);
  }
  if (!threw) {
    console.error("FAIL: packagePlugin accepted module symlink");
    process.exit(1);
  }
  console.log("ok packagePlugin refuses module symlink");

  const draftRoot = path.join(tmp, "draftroot");
  fs.mkdirSync(draftRoot);
  const draft = assertPathInside(draftRoot, "..draft");
  if (!draft.endsWith(`${path.sep}..draft`)) {
    console.error("FAIL: ..draft rejected", draft);
    process.exit(1);
  }
  console.log("ok assertPathInside allows ..draft name");

  const outsideDir = path.join(tmp, "out_dir");
  fs.mkdirSync(outsideDir);
  fs.symlinkSync(outsideDir, path.join(plugin, ".bookclerk"));
  fs.rmSync(path.join(modules, "leak.txt"));
  threw = false;
  try {
    materializeConfig(plugin, manifest, { listenPort: 0, bridgeToken: "tok" });
  } catch (e) {
    threw = /symlink/i.test(String(e));
    if (!threw) console.error("unexpected materialize error:", e);
  }
  if (!threw) {
    console.error("FAIL: materializeConfig accepted .bookclerk symlink");
    process.exit(1);
  }
  if (fs.existsSync(path.join(outsideDir, "bridge.js"))) {
    console.error("FAIL: outside bridge.js was written");
    process.exit(1);
  }
  console.log("ok materializeConfig refuses .bookclerk symlink");

  fs.rmSync(path.join(plugin, ".bookclerk"), { force: true });
  fs.rmSync(path.join(modules, "main.js"));
  fs.symlinkSync(outside, path.join(modules, "main.js"));
  threw = false;
  try {
    materializeConfig(plugin, manifest, { listenPort: 0, bridgeToken: "tok" });
  } catch (e) {
    threw = /symlink/i.test(String(e));
    if (!threw) console.error("unexpected main symlink error:", e);
  }
  if (!threw) {
    console.error("FAIL: materializeConfig accepted main symlink");
    process.exit(1);
  }
  console.log("ok materializeConfig refuses main module symlink");
  console.log("path security regressions passed");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
