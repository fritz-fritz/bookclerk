#!/usr/bin/env node
/** Symlink containment regressions for package + sparse-workerd helpers. */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

function mustThrow(label, fn, pattern = /symlink|\.\.|unsafe|escape|must not contain/i) {
  let threw = false;
  try {
    fn();
  } catch (e) {
    threw = pattern.test(String(e));
    if (!threw) console.error(`unexpected ${label} error:`, e);
  }
  if (!threw) {
    console.error(`FAIL: ${label}`);
    process.exit(1);
  }
  console.log(`ok ${label}`);
}

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

  mustThrow("packagePlugin refuses module symlink", () =>
    packagePlugin(plugin, path.join(tmp, "dist")),
  );

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
  mustThrow("materializeConfig refuses .bookclerk symlink", () =>
    materializeConfig(plugin, manifest, { listenPort: 0, bridgeToken: "tok" }),
  );
  if (fs.existsSync(path.join(outsideDir, "bridge.js"))) {
    console.error("FAIL: outside bridge.js was written");
    process.exit(1);
  }

  fs.rmSync(path.join(plugin, ".bookclerk"), { force: true });
  fs.rmSync(path.join(modules, "main.js"));
  fs.symlinkSync(outside, path.join(modules, "main.js"));
  mustThrow("materializeConfig refuses main module symlink", () =>
    materializeConfig(plugin, manifest, { listenPort: 0, bridgeToken: "tok" }),
  );

  // Intermediate directory symlink under modules/.
  fs.rmSync(path.join(modules, "main.js"), { force: true });
  fs.writeFileSync(path.join(modules, "main.js"), "export default class X {}");
  const nestedOutside = path.join(tmp, "nested_out");
  fs.mkdirSync(nestedOutside);
  fs.writeFileSync(path.join(nestedOutside, "x.js"), "export default 1");
  fs.symlinkSync(nestedOutside, path.join(modules, "vendor"));
  mustThrow("packagePlugin refuses intermediate modules dir symlink", () =>
    packagePlugin(plugin, path.join(tmp, "dist-mid")),
  );

  // Manifest `..` path via assertPathInside (same contract package uses).
  mustThrow(
    "assertPathInside rejects .. manifest path",
    () => assertPathInside(plugin, "../outside.txt"),
    /\.\.|escape|must not contain/i,
  );

  // Symlinked plugin root must still package a valid tree.
  fs.rmSync(path.join(modules, "vendor"), { force: true });
  const linkPlugin = path.join(tmp, "link_plugin");
  fs.symlinkSync(plugin, linkPlugin);
  const archive = packagePlugin(linkPlugin, path.join(tmp, "dist-link"));
  if (!fs.existsSync(archive)) {
    console.error("FAIL: symlinked plugin root did not package");
    process.exit(1);
  }
  console.log("ok packagePlugin allows symlinked plugin root");

  console.log("path security regressions passed");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
