import assert from "node:assert/strict";

const KINDS = new Set(["null", "boolean", "int64", "float64", "text", "bytes"]);
const TYPES = new Set([
  "unspecified",
  "bool",
  "int64",
  "float64",
  "text",
  "bytes",
]);

function parseDbValue(raw) {
  if (raw === null || typeof raw !== "object" || Array.isArray(raw)) {
    throw new Error("DbValue must be an object");
  }
  if (typeof raw.kind !== "string" || !KINDS.has(raw.kind)) {
    throw new Error(`unknown DbValue union member: ${String(raw.kind)}`);
  }
  switch (raw.kind) {
    case "null":
      if (typeof raw.value !== "string" || !TYPES.has(raw.value)) {
        throw new Error("typed null requires a DbType");
      }
      return { kind: "null", value: raw.value };
    case "boolean":
      if (typeof raw.value !== "boolean") {
        throw new Error("boolean DbValue requires a boolean");
      }
      return { kind: "boolean", value: raw.value };
    case "int64":
      if (typeof raw.value !== "number" || !Number.isInteger(raw.value)) {
        throw new Error("int64 DbValue requires an integer");
      }
      return { kind: "int64", value: raw.value };
    case "float64":
      if (typeof raw.value !== "number" || !Number.isFinite(raw.value)) {
        throw new Error("float64 value is not finite");
      }
      return { kind: "float64", value: raw.value };
    case "text":
      if (typeof raw.value !== "string") {
        throw new Error("text DbValue requires a string");
      }
      return { kind: "text", value: raw.value };
    case "bytes":
      if (typeof raw.value !== "string") {
        throw new Error("bytes DbValue requires a string");
      }
      return { kind: "bytes", value: raw.value };
    default:
      throw new Error(`unknown DbValue union member: ${raw.kind}`);
  }
}

const nullBytes = parseDbValue({ kind: "null", value: "bytes" });
assert.deepEqual(nullBytes, { kind: "null", value: "bytes" });

for (const n of [-1, 0, 1]) {
  assert.deepEqual(parseDbValue({ kind: "int64", value: n }), {
    kind: "int64",
    value: n,
  });
}

const text = parseDbValue({ kind: "text", value: "héllo\u0000world" });
assert.equal(text.kind, "text");
assert.equal(text.value, "héllo\u0000world");

const blob = parseDbValue({ kind: "bytes", value: "b64:AAEC" });
assert.equal(blob.kind, "bytes");

assert.throws(
  () => parseDbValue({ kind: "xml", value: "<a/>" }),
  /unknown DbValue union member/,
);

console.log("db-value goldens ok");
