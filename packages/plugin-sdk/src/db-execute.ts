/**
 * Typed Cap'n `ExecuteRequest` codec and host-mediated `DatabaseBinding`.
 *
 * `encodeExecuteRequest` emits the same unpacked Cap'n stream as the Rust SDK.
 * `DatabaseBinding.execute` builds that message (for `maxRequestBytes`) and
 * forwards the structured request through a host `executeAtomic` transport.
 */

import { CapnpMessage, CapnpReader, type CapnpStruct, type StructReader } from "./db-capnp.js";
import { readDbValue, writeDbValue, type DbType, type DbValue } from "./db-value.js";

/** Host-authored statement kind on `DbStatement.kind`. */
export type DbStatementKind = "query" | "execute" | "select" | "returning";

/** Which result fields the caller needs. */
export type DbResultSelection = "discard" | "affectedRows" | "rows" | "cursor";

const KIND_ORD: Record<DbStatementKind, number> = {
  query: 0,
  execute: 1,
  select: 2,
  returning: 3,
};

const KIND_FROM = ["query", "execute", "select", "returning"] as const;

const SELECT_ORD: Record<DbResultSelection, number> = {
  discard: 0,
  affectedRows: 1,
  rows: 2,
  cursor: 3,
};

const SELECT_FROM = ["discard", "affectedRows", "rows", "cursor"] as const;

const COL_TYPE_ORD: Record<DbType, number> = {
  unspecified: 0,
  bool: 1,
  int64: 2,
  float64: 3,
  text: 4,
  bytes: 5,
};

const COL_TYPE_FROM = ["unspecified", "bool", "int64", "float64", "text", "bytes"] as const;

/**
 * One statement in a typed atomic batch.
 */
export interface TypedDbStatement {
  /** Canonical Bookclerk SQL (`?` placeholders). */
  sql: string;
  /** Ordered typed binds. */
  parameters: DbValue[];
  /** Host-authored kind (adapters must not reparse SQL). */
  kind: DbStatementKind;
  /** Proven row upper bound (`0` = unproven). */
  maxRows: number;
  /** Which result fields the caller needs. */
  resultSelection: DbResultSelection;
}

/**
 * Typed `DatabaseSession.executeAtomic` request.
 */
export interface ExecuteRequest {
  /** Caller-chosen idempotency key. */
  operationId: string;
  /** SHA-256 hex of the idempotency-relevant request; empty when omitted. */
  requestHash: string;
  /** Ordered statements (must be non-empty). */
  statements: TypedDbStatement[];
  /** Index of the application-status statement. */
  outcomeIndex: number;
  /** Payload statement index when `hasPayloadIndex` is true. */
  payloadIndex: number;
  /** Whether `payloadIndex` is set. */
  hasPayloadIndex: boolean;
  /** Prior-receipt statement index when `hasPriorReceiptIndex` is true. */
  priorReceiptIndex: number;
  /** Whether `priorReceiptIndex` is set. */
  hasPriorReceiptIndex: boolean;
  /** Receipt-select index when `hasReceiptSelectIndex` is true. */
  receiptSelectIndex: number;
  /** Whether `receiptSelectIndex` is set. */
  hasReceiptSelectIndex: boolean;
  /** Guest-visible deadline (unix ms). Zero means omitted. */
  deadlineUnixMs: number;
}

/**
 * One column in a typed result set.
 */
export interface DbColumn {
  /** Column name. */
  name: string;
  /** Declared / inferred type. */
  dbType: DbType;
}

/**
 * One positional result row.
 */
export interface DbRow {
  /** Cells in column order. */
  values: DbValue[];
}

/**
 * Result of one statement in an atomic batch.
 */
export interface StatementResult {
  /** Positional rows. */
  rows: DbRow[];
  /** Column metadata aligned with each row. */
  columns: DbColumn[];
  /** Engine `rowsAffected` (0 for `Select`). */
  rowsAffected: number;
  /** Optional result cursor. */
  cursor: string;
}

/**
 * Handler/engine timing on `ExecuteReply`.
 */
export interface DbTiming {
  /** Whole-attempt elapsed microseconds. */
  attemptElapsedUs: number;
  /** Engine-reported SQL microseconds when available. */
  dbExecutionUs: number;
  /** Observability source (`sqlite_txn`, `d1_sql_duration`, …). */
  dbTimingSource: string;
}

/**
 * Typed `executeAtomic` reply.
 */
export interface ExecuteReply {
  /** Echo of the request `operationId`. */
  operationId: string;
  /** Per-statement results, in plan order. */
  statements: StatementResult[];
  /** Handler/engine timing. */
  timing: DbTiming;
}

/**
 * Host `DatabaseSession.executeAtomic` projection.
 */
export interface AtomicTransport {
  /**
   * Runs one typed atomic batch.
   *
   * @param request Structured Cap'n `ExecuteRequest`.
   * @returns Structured `ExecuteReply`.
   */
  executeAtomic(request: ExecuteRequest): Promise<ExecuteReply>;
}

/**
 * Explicit retry identity. Reuses both `operationId` and `requestHash`.
 */
export interface RetryToken {
  /** Caller-chosen idempotency key. */
  operationId: string;
  /** Canonical Cap'n request hash stamped by the host. */
  requestHash: string;
}

/**
 * Options for {@link createDatabaseBinding}.
 */
export interface DatabaseBindingOptions {
  /** Negotiated `maxRequestBytes` (`0` = unlimited). */
  maxRequestBytes?: number;
  /** Default `maxRows` for `all()` (`0` = host adapter cap). */
  maxResultRows?: number;
  /** Default retry token; omitted calls mint a UUID and leave the hash empty. */
  retry?: RetryToken;
  /** Idempotency key; generated when omitted. */
  operationId?: string;
  /** SHA-256 hex of the request; empty when omitted. */
  requestHash?: string;
  /** Guest-visible deadline (unix ms). */
  deadlineUnixMs?: number;
}

/**
 * Cloudflare-style prepared statement. Kind, bounds, and request hash are
 * derived by the trusted host. Only universal {@link DbValue}s are public.
 */
export interface PreparedStatement {
  /**
   * Replace bound parameters.
   *
   * @param values Universal `DbValue`s only.
   */
  bind(...values: DbValue[]): PreparedStatement;
  /**
   * Execute as DML (`affectedRows`).
   *
   * @param options Optional retry token.
   */
  run(options?: { retry?: RetryToken }): Promise<ExecuteReply>;
  /**
   * First row as a name→value map, or `null`. Sends `maxRows = 1`.
   *
   * @param options Optional retry token.
   */
  first(options?: { retry?: RetryToken }): Promise<Record<string, DbValue> | null>;
  /**
   * Execute as a row-returning query.
   *
   * @param options Optional retry token.
   */
  all(options?: { retry?: RetryToken }): Promise<ExecuteReply>;
  /** Mark DML intent for {@link DatabaseBinding.batch}. */
  asRun(): PreparedStatement;
  /** Mark `maxRows = 1` row intent for {@link DatabaseBinding.batch}. */
  asFirst(): PreparedStatement;
  /** Mark row-returning intent for {@link DatabaseBinding.batch}. */
  asAll(): PreparedStatement;
}

/**
 * Host-mediated typed SQL surface for plugin guests.
 *
 * Public API is Cloudflare-style `prepare().bind().run()/first()/all()` plus
 * atomic `batch()`. Raw `executeAtomic` stays internal.
 */
export interface DatabaseBinding {
  /**
   * Prepare one canonical-SQL statement (`?` placeholders).
   *
   * @param sql Host-mediated SQL. Kind and bounds are derived by the host.
   */
  prepare(sql: string): PreparedStatement;
  /**
   * Run prepared statements as one typed atomic batch.
   *
   * Each statement must already carry a terminal intent (`asRun` / `asFirst` /
   * `asAll`, or a prior `run`/`first`/`all` builder). `batch` does not apply
   * a default `resultSelection`.
   *
   * @param statements Prepared statements (binds and intent already applied).
   * @param options Retry token.
   */
  batch(
    statements: PreparedStatement[],
    options?: { retry?: RetryToken },
  ): Promise<ExecuteReply>;
  /**
   * Internal typed-batch transport. Prefer {@link DatabaseBinding.prepare}.
   *
   * @param batch Ordered statements with typed `DbValue` parameters.
   * @param options Optional retry token.
   */
  execute(batch: TypedDbStatement[], options?: { retry?: RetryToken }): Promise<ExecuteReply>;
}

/**
 * Encodes a standalone unpacked Cap'n `ExecuteRequest` message.
 *
 * @param request Structured request (non-empty `statements`).
 * @returns Unpacked Cap'n stream bytes (same encoding as the Rust SDK).
 * @throws When `statements` is empty.
 */
export function encodeExecuteRequest(request: ExecuteRequest): Uint8Array {
  if (request.statements.length === 0) {
    throw new Error("executeAtomic statements must be non-empty");
  }
  const msg = new CapnpMessage();
  const root = msg.initRoot(4, 3);
  root.setText(0, request.operationId);
  root.setText(1, request.requestHash);
  root.setUint32(0, request.outcomeIndex);
  root.setUint32(1, request.payloadIndex);
  root.setBool(64, request.hasPayloadIndex);
  root.setUint32(3, request.priorReceiptIndex);
  root.setBool(65, request.hasPriorReceiptIndex);
  root.setUint32(4, request.receiptSelectIndex);
  root.setBool(66, request.hasReceiptSelectIndex);
  root.setUint64(3, BigInt(request.deadlineUnixMs));
  const stmts = root.initStructList(2, request.statements.length, 1, 2);
  for (let i = 0; i < request.statements.length; i++) {
    writeStatement(stmts[i], request.statements[i]);
  }
  return msg.finish();
}

/**
 * Decodes a standalone unpacked Cap'n `ExecuteRequest` message.
 *
 * @param bytes Unpacked Cap'n stream.
 * @returns Structured request.
 * @throws When the buffer is not a valid non-empty `ExecuteRequest`.
 */
export function decodeExecuteRequest(bytes: Uint8Array): ExecuteRequest {
  const reader = new CapnpReader(bytes);
  const root = reader.root(4, 3);
  const stmtStructs = root.getStructList(2, 1, 2);
  if (stmtStructs.length === 0) {
    throw new Error("executeAtomic statements must be non-empty");
  }
  return {
    operationId: root.getText(0),
    requestHash: root.getText(1),
    statements: stmtStructs.map(readStatement),
    outcomeIndex: root.getUint32(0),
    payloadIndex: root.getUint32(1),
    hasPayloadIndex: root.getBool(64),
    priorReceiptIndex: root.getUint32(3),
    hasPriorReceiptIndex: root.getBool(65),
    receiptSelectIndex: root.getUint32(4),
    hasReceiptSelectIndex: root.getBool(66),
    deadlineUnixMs: Number(root.getUint64(3)),
  };
}

/**
 * Encodes a standalone unpacked Cap'n `ExecuteReply` message.
 *
 * @param reply Structured reply.
 * @returns Unpacked Cap'n stream bytes.
 */
export function encodeExecuteReply(reply: ExecuteReply): Uint8Array {
  const msg = new CapnpMessage();
  const root = msg.initRoot(0, 3);
  writeExecuteReply(root, reply);
  return msg.finish();
}

/**
 * Encodes `ExecuteAtomicReply` (`ok` or `err`).
 *
 * @param outcome Successful reply or a wire `PluginError`.
 * @returns Unpacked Cap'n stream bytes.
 */
export function encodeExecuteAtomicReply(
  outcome: { ok: ExecuteReply } | { err: { code: string; message: string } },
): Uint8Array {
  const msg = new CapnpMessage();
  const root = msg.initRoot(1, 1);
  if ("ok" in outcome) {
    root.setUint16(0, 0);
    writeExecuteReply(root.initStruct(0, 0, 3), outcome.ok);
  } else {
    root.setUint16(0, 1);
    const err = root.initStruct(0, 0, 2);
    err.setText(0, outcome.err.code);
    err.setText(1, outcome.err.message);
  }
  return msg.finish();
}

/**
 * Decodes a standalone `ExecuteAtomicReply`. `err` is thrown as `PluginError`.
 *
 * @param bytes Unpacked Cap'n stream.
 * @returns Structured `ExecuteReply`.
 * @throws When the union is `err` or the buffer is invalid.
 */
export function decodeExecuteAtomicReply(bytes: Uint8Array): ExecuteReply {
  const root = new CapnpReader(bytes).root(1, 1);
  const disc = root.getUint16(0);
  if (disc === 0) {
    return readExecuteReply(root.getStruct(0, 0, 3));
  }
  if (disc === 1) {
    const err = root.getStruct(0, 0, 2);
    const code = err.getText(0);
    const message = err.getText(1);
    const known = new Set([
      "invalid_params",
      "unauthorized",
      "forbidden",
      "not_found",
      "unavailable",
      "unsupported",
      "internal",
      "payload_too_large",
      "deadline_exceeded",
      "invalid_cursor",
      "cancelled",
      "conflict",
    ]);
    throw Object.assign(new Error(message), {
      name: "PluginError",
      code: known.has(code) ? code : "unknown",
      wireCode: code,
    });
  }
  throw new Error("unknown ExecuteAtomicReply union member");
}

function writeExecuteReply(root: CapnpStruct, reply: ExecuteReply): void {
  root.setText(0, reply.operationId);
  const stmts = root.initStructList(1, reply.statements.length, 1, 3);
  for (let i = 0; i < reply.statements.length; i++) {
    writeStatementResult(stmts[i], reply.statements[i]);
  }
  const timing = root.initStruct(2, 2, 1);
  timing.setUint64(0, BigInt(reply.timing.attemptElapsedUs));
  timing.setUint64(1, BigInt(reply.timing.dbExecutionUs));
  timing.setText(0, reply.timing.dbTimingSource);
}

function readExecuteReply(root: StructReader): ExecuteReply {
  return {
    operationId: root.getText(0),
    statements: root.getStructList(1, 1, 3).map(readStatementResult),
    timing: (() => {
      const t = root.getStruct(2, 2, 1);
      return {
        attemptElapsedUs: Number(t.getUint64(0)),
        dbExecutionUs: Number(t.getUint64(1)),
        dbTimingSource: t.getText(0),
      };
    })(),
  };
}

function writeStatementResult(s: CapnpStruct, stmt: StatementResult): void {
  s.setUint64(0, BigInt(stmt.rowsAffected));
  const rows = s.initStructList(0, stmt.rows.length, 0, 1);
  for (let i = 0; i < stmt.rows.length; i++) {
    const cells = rows[i].initStructList(0, stmt.rows[i].values.length, 2, 1);
    for (let j = 0; j < stmt.rows[i].values.length; j++) {
      writeDbValue(cells[j], stmt.rows[i].values[j]);
    }
  }
  const cols = s.initStructList(1, stmt.columns.length, 1, 1);
  for (let i = 0; i < stmt.columns.length; i++) {
    cols[i].setText(0, stmt.columns[i].name);
    cols[i].setUint16(0, COL_TYPE_ORD[stmt.columns[i].dbType]);
  }
  s.setText(2, stmt.cursor);
}

function readStatementResult(s: StructReader): StatementResult {
  const columns = s.getStructList(1, 1, 1).map((c) => {
    const ty = COL_TYPE_FROM[c.getUint16(0)];
    if (ty === undefined) {
      throw new Error("unknown DbType");
    }
    return { name: c.getText(0), dbType: ty };
  });
  const rows = s.getStructList(0, 0, 1).map((row) => ({
    values: row.getStructList(0, 2, 1).map(readDbValue),
  }));
  return {
    rows,
    columns,
    rowsAffected: Number(s.getUint64(0)),
    cursor: s.getText(2),
  };
}

/**
 * Builds a host-mediated {@link DatabaseBinding} over an `executeAtomic` transport.
 *
 * @param transport Host session projection.
 * @param options Request-budget and idempotency knobs.
 * @returns Binding whose `execute(batch)` encodes Cap'n then calls the host.
 */
export function createDatabaseBinding(
  transport: AtomicTransport,
  options: DatabaseBindingOptions = {},
): DatabaseBinding {
  const executeAtomic = async (
    batch: TypedDbStatement[],
    retry?: RetryToken,
  ): Promise<ExecuteReply> => {
    if (!Array.isArray(batch) || batch.length === 0) {
      throw new Error("executeAtomic statements must be non-empty");
    }
    const request = await executeRequestFromBatch(batch, options, retry);
    const encoded = encodeExecuteRequest(request);
    const cap = options.maxRequestBytes ?? 0;
    if (cap > 0 && encoded.byteLength > cap) {
      throw new Error(
        `atomic request is ${encoded.byteLength} bytes; guest maxRequestBytes is ${cap}`,
      );
    }
    return transport.executeAtomic(request);
  };

  const binding: DatabaseBinding = {
    prepare(sql: string): PreparedStatement {
      return makePrepared(binding, sql, [], options.maxResultRows ?? 0);
    },
    batch(statements, opts) {
      const typed = statements.map((s) => {
        const intent = (s as PreparedInternal)._intent;
        if (!intent) {
          throw new Error(
            "batch statement is missing terminal intent (asRun/asFirst/asAll)",
          );
        }
        return (s as PreparedInternal)._asTyped();
      });
      return executeAtomic(typed, opts?.retry);
    },
    execute(batch, opts) {
      return executeAtomic(batch, opts?.retry);
    },
  };
  return binding;
}

interface TerminalIntent {
  resultSelection: DbResultSelection;
  maxRows: number;
}

interface PreparedInternal extends PreparedStatement {
  _intent?: TerminalIntent;
  _asTyped(): TypedDbStatement;
}

function makePrepared(
  binding: DatabaseBinding,
  sql: string,
  parameters: DbValue[],
  defaultAllRows: number,
  intent?: TerminalIntent,
): PreparedInternal {
  const stmt: PreparedInternal = {
    _intent: intent,
    bind(...values: DbValue[]) {
      return makePrepared(binding, sql, values, defaultAllRows, intent);
    },
    asRun() {
      return makePrepared(binding, sql, parameters, defaultAllRows, {
        resultSelection: "affectedRows",
        maxRows: 0,
      });
    },
    asFirst() {
      const inner = sql.trim().replace(/;+\s*$/, "");
      return makePrepared(
        binding,
        `SELECT * FROM (${inner}) AS _bc_first LIMIT 1`,
        parameters,
        defaultAllRows,
        {
          resultSelection: "rows",
          maxRows: 1,
        },
      );
    },
    asAll() {
      return makePrepared(binding, sql, parameters, defaultAllRows, {
        resultSelection: "rows",
        maxRows: defaultAllRows,
      });
    },
    run(options) {
      return binding.batch([stmt.asRun()], { retry: options?.retry });
    },
    async first(options) {
      const reply = await binding.batch([stmt.asFirst()], { retry: options?.retry });
      const result = reply.statements[0];
      if (!result || result.rows.length === 0) {
        return null;
      }
      const row: Record<string, DbValue> = {};
      for (let i = 0; i < result.columns.length; i++) {
        row[result.columns[i].name] = result.rows[0].values[i];
      }
      return row;
    },
    all(options) {
      return binding.batch([stmt.asAll()], { retry: options?.retry });
    },
    _asTyped(): TypedDbStatement {
      const used = intent;
      if (!used) {
        throw new Error("prepared statement is missing terminal intent");
      }
      return {
        sql,
        parameters,
        kind:
          used.resultSelection === "affectedRows" || used.resultSelection === "discard"
            ? "execute"
            : "select",
        maxRows: used.maxRows,
        resultSelection: used.resultSelection,
      };
    },
  };
  return stmt;
}

async function executeRequestFromBatch(
  batch: TypedDbStatement[],
  options: DatabaseBindingOptions,
  retry?: RetryToken,
): Promise<ExecuteRequest> {
  const token = retry ?? options.retry;
  return {
    operationId: token?.operationId ?? options.operationId ?? newOperationId(),
    requestHash: token?.requestHash ?? options.requestHash ?? "",
    statements: batch,
    outcomeIndex: 0,
    payloadIndex: 0,
    hasPayloadIndex: false,
    priorReceiptIndex: 0,
    hasPriorReceiptIndex: false,
    receiptSelectIndex: 0,
    hasReceiptSelectIndex: false,
    deadlineUnixMs: options.deadlineUnixMs ?? 0,
  };
}

/**
 * SHA-256 hex of the Cap'n request with id and hash cleared.
 *
 * @param request Structured request.
 * @returns Hex digest matching the host canonical hash.
 */
export async function canonicalExecuteRequestHash(request: ExecuteRequest): Promise<string> {
  const canonical: ExecuteRequest = {
    ...request,
    operationId: "",
    requestHash: "",
    deadlineUnixMs: 0,
  };
  const bytes = encodeExecuteRequest(canonical);
  if (globalThis.crypto?.subtle) {
    const digest = await globalThis.crypto.subtle.digest("SHA-256", bytes);
    return hexBytes(new Uint8Array(digest));
  }
  const { createHash } = await import("node:crypto");
  return createHash("sha256").update(bytes).digest("hex");
}

function hexBytes(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function newOperationId(): string {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return globalThis.crypto.randomUUID();
  }
  return `op-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function writeStatement(s: CapnpStruct, stmt: TypedDbStatement): void {
  s.setText(0, stmt.sql);
  s.setUint16(0, KIND_ORD[stmt.kind]);
  s.setUint16(1, SELECT_ORD[stmt.resultSelection]);
  s.setUint32(1, stmt.maxRows);
  const params = s.initStructList(1, stmt.parameters.length, 2, 1);
  for (let i = 0; i < stmt.parameters.length; i++) {
    writeDbValue(params[i], stmt.parameters[i]);
  }
}

function readStatement(s: StructReader): TypedDbStatement {
  const kind = KIND_FROM[s.getUint16(0)];
  const selection = SELECT_FROM[s.getUint16(1)];
  if (kind === undefined) {
    throw new Error("unknown DbStatementKind");
  }
  if (selection === undefined) {
    throw new Error("unknown DbResultSelection");
  }
  return {
    sql: s.getText(0),
    parameters: s.getStructList(1, 2, 1).map(readDbValue),
    kind,
    maxRows: s.getUint32(1),
    resultSelection: selection,
  };
}
