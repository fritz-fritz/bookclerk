#!/usr/bin/env node
/**
 * Minimal ABI drift guard: assert the authoritative schema file exists and
 * carries the expected title / apiVersion const.
 */
import { readFileSync, existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const schemaPath = resolve(
  here,
  "../../../crates/bookclerk-plugin-abi/schema/abi.json",
);

if (!existsSync(schemaPath)) {
  console.error(`missing ABI schema: ${schemaPath}`);
  process.exit(1);
}

const schema = JSON.parse(readFileSync(schemaPath, "utf8"));
if (schema.title !== "BookclerkPluginAbi") {
  console.error(`unexpected schema title: ${schema.title}`);
  process.exit(1);
}
if (schema.properties?.apiVersion?.const !== 1) {
  console.error("expected properties.apiVersion.const === 1");
  process.exit(1);
}
for (const key of [
  "HandshakeParams",
  "HandshakeResult",
  "HostToPluginEvent",
  "PluginToHostEvent",
  "CliInvokeParams",
  "PluginError",
]) {
  if (!schema.$defs?.[key]) {
    console.error(`schema missing $defs.${key}`);
    process.exit(1);
  }
}

console.log(`ok: ${schemaPath}`);
