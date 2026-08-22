/**
 * Universal Cap'n database value domain (`DbValue`).
 *
 * Baseline cells are typed null, bool, int64, finite float64, UTF-8 text, and
 * bytes. Unknown `kind` values fail closed.
 */

export type DbType =
  | "unspecified"
  | "bool"
  | "int64"
  | "float64"
  | "text"
  | "bytes";

export type DbValue =
  | { kind: "null"; value: DbType }
  | { kind: "boolean"; value: boolean }
  | { kind: "int64"; value: number }
  | { kind: "float64"; value: number }
  | { kind: "text"; value: string }
  | { kind: "bytes"; value: string };

const KINDS = new Set(["null", "boolean", "int64", "float64", "text", "bytes"]);
const TYPES = new Set([
  "unspecified",
  "bool",
  "int64",
  "float64",
  "text",
  "bytes",
]);

/**
 * Parses a JSON `DbValue`. Unknown union members throw.
 *
 * @param raw Parsed JSON object.
 * @returns Typed value.
 */
export function parseDbValue(raw: unknown): DbValue {
  if (raw === null || typeof raw !== "object" || Array.isArray(raw)) {
    throw new Error("DbValue must be an object");
  }
  const obj = raw as { kind?: unknown; value?: unknown };
  if (typeof obj.kind !== "string" || !KINDS.has(obj.kind)) {
    throw new Error(`unknown DbValue union member: ${String(obj.kind)}`);
  }
  switch (obj.kind) {
    case "null":
      if (typeof obj.value !== "string" || !TYPES.has(obj.value)) {
        throw new Error("typed null requires a DbType");
      }
      return { kind: "null", value: obj.value as DbType };
    case "boolean":
      if (typeof obj.value !== "boolean") {
        throw new Error("boolean DbValue requires a boolean");
      }
      return { kind: "boolean", value: obj.value };
    case "int64":
      if (typeof obj.value !== "number" || !Number.isInteger(obj.value)) {
        throw new Error("int64 DbValue requires an integer");
      }
      return { kind: "int64", value: obj.value };
    case "float64":
      if (typeof obj.value !== "number" || !Number.isFinite(obj.value)) {
        throw new Error("float64 value is not finite");
      }
      return { kind: "float64", value: obj.value };
    case "text":
      if (typeof obj.value !== "string") {
        throw new Error("text DbValue requires a string");
      }
      return { kind: "text", value: obj.value };
    case "bytes":
      if (typeof obj.value !== "string") {
        throw new Error("bytes DbValue requires a string");
      }
      return { kind: "bytes", value: obj.value };
    default:
      throw new Error(`unknown DbValue union member: ${obj.kind}`);
  }
}
