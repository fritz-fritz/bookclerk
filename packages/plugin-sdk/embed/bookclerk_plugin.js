/**
 * GENERATED FILE - do not edit. Bundled from `src/workerd.ts` by
 * `scripts/build-embed.mjs` (`npm run build` in packages/plugin-sdk).
 *
 * Workerd runtime for `@bookclerk/plugin-sdk` / `@bookclerk/plugin-sdk/workerd`
 * (@bookclerk/plugin-sdk@0.1.0). `bookclerk-workerd` injects this module into every
 * plugin isolate under those names; authors import the package, never a
 * relative embed path. Native guests use Rust `serve` / `PluginWorker` instead.
 */

// dist/plugin.js
import { WorkerEntrypoint, RpcTarget } from "cloudflare:workers";

// dist/abi.js
var PRODUCT_API_VERSION = 3;
var MAX_SCALAR_BYTES = 262144;
var MAX_STREAM_WINDOW_BYTES = 1048576;
var MAX_LIST_PAGE = 256;
var MAX_CHECKPOINT_BYTES = 65536;
var MAX_EVENT_PAYLOAD_BYTES = 65536;
var MAX_PLUGIN_MIGRATION_OPS = 256;
var MAX_PLUGIN_MIGRATION_TOTAL_OPS = 2048;
var MAX_PLUGIN_MIGRATION_REGISTRATION_BYTES = 262144;
var FEATURE_SCALAR_LIMITS = "rpc.scalarLimits";
var FEATURE_STREAMS = "rpc.streams";
var FEATURE_STORAGE_COPY = "storage.copy";
var PLUGIN_ERROR_CODES = ["invalid_params", "unauthorized", "forbidden", "not_found", "unavailable", "unsupported", "internal", "payload_too_large", "deadline_exceeded", "invalid_cursor", "cancelled", "conflict"];
var DB_TYPES = ["unspecified", "bool", "int64", "float64", "text", "bytes"];
var DB_STATEMENT_KINDS = ["execute", "select", "returning"];
var DB_RESULT_SELECTIONS = ["discard", "affectedRows", "rows"];

// dist/plugin-migrations.js
var utf8 = new TextEncoder();
function utf8Bytes(value) {
  return utf8.encode(value).byteLength;
}
function opSql(op) {
  if (op && typeof op === "object") {
    if ("schema" in op && typeof op.schema === "string") {
      return op.schema;
    }
    if ("data" in op && typeof op.data === "string") {
      return op.data;
    }
  }
  return "";
}
function requirePluginMigrationRegistration(migrations) {
  if (!Array.isArray(migrations)) {
    return [];
  }
  if (migrations.length > MAX_LIST_PAGE) {
    throw migrationTooLarge(`plugin migration count ${migrations.length} exceeds maxListPage (${MAX_LIST_PAGE})`);
  }
  let total = 0;
  let totalOps = 0;
  for (const migration of migrations) {
    const ops = Array.isArray(migration.operations) ? migration.operations : [];
    if (ops.length > MAX_PLUGIN_MIGRATION_OPS) {
      throw migrationTooLarge(`plugin migration \`${migration.id}\` has ${ops.length} operations; exceeds maxPluginMigrationOps (${MAX_PLUGIN_MIGRATION_OPS})`);
    }
    totalOps += ops.length;
    if (totalOps > MAX_PLUGIN_MIGRATION_TOTAL_OPS) {
      throw migrationTooLarge(`plugin migration registration has ${totalOps} operations; exceeds maxPluginMigrationTotalOps (${MAX_PLUGIN_MIGRATION_TOTAL_OPS})`);
    }
    total += utf8Bytes(String(migration.id ?? ""));
    if (total > MAX_PLUGIN_MIGRATION_REGISTRATION_BYTES) {
      throw migrationTooLarge(`plugin migration registration is ${total} bytes; exceeds maxPluginMigrationRegistrationBytes (${MAX_PLUGIN_MIGRATION_REGISTRATION_BYTES})`);
    }
    for (const op of ops) {
      const n = utf8Bytes(opSql(op));
      if (n > MAX_SCALAR_BYTES) {
        throw migrationTooLarge(`plugin migration \`${migration.id}\` SQL is ${n} bytes; exceeds maxScalarBytes (${MAX_SCALAR_BYTES})`);
      }
      total += n;
      if (total > MAX_PLUGIN_MIGRATION_REGISTRATION_BYTES) {
        throw migrationTooLarge(`plugin migration registration is ${total} bytes; exceeds maxPluginMigrationRegistrationBytes (${MAX_PLUGIN_MIGRATION_REGISTRATION_BYTES})`);
      }
    }
  }
  return migrations;
}
function migrationTooLarge(message) {
  const err = new Error(message);
  err.code = "payload_too_large";
  err.wireCode = "payload_too_large";
  return err;
}

// dist/plugin.js
var KNOWN_ERROR_CODES = new Set(PLUGIN_ERROR_CODES);
var PluginError = class _PluginError extends Error {
  /** Known `PluginErrorCode` wire string, or `unknown`. */
  code;
  /** Raw wire code, including codes this SDK does not know. */
  wireCode;
  constructor(code, message) {
    super(message);
    this.name = "PluginError";
    this.wireCode = code;
    this.code = KNOWN_ERROR_CODES.has(code) ? code : "unknown";
  }
  /**
   * Construct a {@link PluginError} from a Cap'n Proto / JSON wire code.
   *
   * Unknown codes become `unknown` on {@link PluginError.code} while
   * {@link PluginError.wireCode} keeps the raw value.
   *
   * @param code - Wire error code (known or unknown).
   * @param message - Operator-facing error text.
   * @returns Typed plugin error.
   */
  static fromWire(code, message) {
    return new _PluginError(code, message);
  }
};
function unsupported(method) {
  return PluginError.fromWire("unsupported", `${method} not implemented`);
}
function utf8Bytes2(value) {
  return new TextEncoder().encode(String(value ?? "")).byteLength;
}
function errorMessage(err) {
  return err instanceof Error ? err.message : String(err);
}
function schemaMigrationOp(sql) {
  return { schema: sql };
}
function dataMigrationOp(sql) {
  return { data: sql };
}
var Destination = class extends RpcTarget {
  /**
   * Metadata without a body; `null` when the key is missing.
   *
   * @param _key - Object key.
   * @returns Metadata or `null` when the key is missing.
   */
  head(_key) {
    return Promise.reject(unsupported("head"));
  }
  /**
   * One page of keys under `options.prefix`.
   *
   * @param _options - Prefix, cursor, and limit.
   * @returns One page of object keys.
   */
  list(_options) {
    return Promise.reject(unsupported("list"));
  }
  /**
   * Streamed read. The body is a transferred stream, not a scalar.
   *
   * @param _key - Object key.
   * @param _options - Optional byte range.
   * @returns Metadata plus a transferred body stream.
   */
  get(_key, _options) {
    return Promise.reject(unsupported("get"));
  }
  /**
   * Streamed write. `body` ownership is transferred to the destination.
   *
   * @param _key - Object key.
   * @param _body - Byte stream.
   * @param _options - Optional content type / length.
   * @returns Bytes written and optional etag / sha256.
   */
  put(_key, _body, _options) {
    return Promise.reject(unsupported("put"));
  }
  /**
   * Server-side copy when the backend supports it.
   *
   * @param _from - Source key.
   * @param _to - Destination key.
   * @returns Bytes copied.
   */
  copy(_from, _to) {
    return Promise.reject(unsupported("copy"));
  }
  /**
   * Delete a key (no-op if missing).
   *
   * @param _key - Object key.
   * @returns Resolves when the delete is complete.
   */
  delete(_key) {
    return Promise.reject(unsupported("delete"));
  }
  /**
   * Finalize a destination-side staged object.
   *
   * @param _key - Object key.
   * @param _commitToken - Idempotency / commit token.
   * @returns Published object metadata.
   */
  commit(_key, _commitToken) {
    return Promise.reject(unsupported("commit"));
  }
  /**
   * Abort a destination-side staged object.
   *
   * @param _key - Object key.
   * @param _commitToken - Staging token to discard.
   * @returns Rejects with typed `unsupported` unless overridden.
   */
  abortStage(_key, _commitToken) {
    return Promise.reject(unsupported("abortStage"));
  }
};
var Source = class extends RpcTarget {
  /**
   * Opens `key` for streamed reading.
   *
   * @param _key - Object key.
   * @returns Metadata plus a transferred body stream.
   */
  open(_key) {
    return Promise.reject(unsupported("open"));
  }
};
var ProgressSink = class extends RpcTarget {
  /**
   * Reports `percent` in `0..=100` and an operator-facing `message`.
   *
   * @param _percent - Completion percent.
   * @param _message - Operator-facing status.
   * @returns Resolves when the host records progress.
   */
  report(_percent, _message) {
    return Promise.reject(unsupported("report"));
  }
};
var AdapterDatabaseSession = class extends RpcTarget {
  /**
   * Typed SQL-contract advertisement.
   *
   * @returns Guest `DbCapabilities`.
   */
  capabilities() {
    return Promise.reject(unsupported("capabilities"));
  }
  /**
   * Typed atomic batch (`ExecuteRequest` → `ExecuteReply`).
   *
   * @param _request - Cap'n `ExecuteRequest` (structured Workers RPC object).
   * @returns `ExecuteReply`.
   */
  execute(_request) {
    return Promise.reject(unsupported("execute"));
  }
  /**
   * Close the adapter session.
   *
   * @returns Resolves when the session is closed.
   */
  close() {
    return Promise.resolve();
  }
};
function decodeExtensibleConfig(cfg) {
  if (cfg == null)
    return {};
  if (typeof cfg !== "object")
    return {};
  const record = cfg;
  const payload = record.payload;
  let text = "";
  if (payload instanceof Uint8Array) {
    text = new TextDecoder().decode(payload);
  } else if (payload instanceof ArrayBuffer) {
    text = new TextDecoder().decode(new Uint8Array(payload));
  } else if (typeof payload === "string") {
    text = payload;
  } else if (payload && typeof payload === "object" && !("schemaVersion" in record)) {
    return payload;
  }
  const mediaType = String(record.mediaType ?? "");
  if (!text.trim())
    return {};
  if (mediaType === "" || /json/i.test(mediaType)) {
    try {
      const parsed = JSON.parse(text);
      return parsed === null || typeof parsed !== "object" ? {} : parsed;
    } catch {
      return {};
    }
  }
  return { mediaType, text };
}
function jsonPayload(value) {
  return {
    schemaVersion: 1,
    mediaType: "application/json",
    payload: new TextEncoder().encode(JSON.stringify(value ?? null))
  };
}
function cliArgs(params) {
  const out = {};
  for (const arg of params?.args ?? []) {
    if (arg && typeof arg.name === "string")
      out[arg.name] = String(arg.value ?? "");
  }
  return out;
}
function invocationEnv(rawEnv, context) {
  const merged = { ...rawEnv ?? {} };
  if (context && typeof context === "object") {
    if (context.config !== void 0)
      merged.CONFIG = decodeExtensibleConfig(context.config);
    if (context.secrets !== void 0)
      merged.SECRETS = decodeExtensibleConfig(context.secrets);
    if (context.storage)
      merged.WORK_FS = context.storage;
    if (context.events)
      merged.EVENTS = context.events;
    if (Array.isArray(context.databases)) {
      for (const entry of context.databases) {
        if (entry && typeof entry.name === "string" && entry.database) {
          merged[entry.name] = entry.database;
        }
      }
    }
  }
  return Object.freeze(merged);
}
function applyInvocationEnv(instance, context) {
  const target = instance;
  const merged = invocationEnv(target.env, context);
  try {
    target.env = merged;
  } catch {
    Object.defineProperty(instance, "env", { value: merged, configurable: true });
  }
}
function invocationOf(context) {
  const inv = context && typeof context === "object" ? context.invocation ?? {} : {};
  return Object.freeze({
    id: String(inv.id ?? ""),
    accountId: String(inv.accountId ?? ""),
    deadlineUnixMs: Number(inv.deadlineUnixMs ?? 0) || 0,
    correlationId: String(inv.correlationId ?? ""),
    causationId: String(inv.causationId ?? "")
  });
}
function unixMs(value) {
  if (value instanceof Date)
    return value.getTime();
  const n = Number(value ?? 0);
  return Number.isFinite(n) && n > 0 ? Math.floor(n) : 0;
}
function checkpointText(checkpoint) {
  if (checkpoint === void 0 || checkpoint === null)
    return "";
  const text = typeof checkpoint === "string" ? checkpoint : JSON.stringify(checkpoint);
  if (utf8Bytes2(text) > MAX_CHECKPOINT_BYTES) {
    throw PluginError.fromWire("payload_too_large", `checkpoint is ${utf8Bytes2(text)} bytes; exceeds maxCheckpointBytes (${MAX_CHECKPOINT_BYTES})`);
  }
  return text;
}
var EventMessage = class {
  #result = null;
  /** Outbox event id. */
  id;
  /** Event type (`book_acquired`, …). */
  type;
  /** Schema version of {@link EventMessage.body}. */
  schemaVersion;
  /** When the producer observed the fact. */
  timestamp;
  /** Delivery counter, starting at 1. */
  attempts;
  /** Account scope; empty for host-wide events. */
  accountId;
  /** Producer plugin id; empty when unknown. */
  source;
  /** Trace correlation id. */
  correlationId;
  /** Id of the command or event that caused this one. */
  causationId;
  /** Consumer-side idempotency key; stable across redeliveries. */
  deduplicationKey;
  /** Encoded payload bytes. */
  body;
  /** Resume ordinal; distinct from `attempts`. */
  invocationSequence;
  /** True when this delivery resumes a prior `suspend()`. */
  resumePending;
  /** Checkpoint from the prior `suspend()`, or `null`. */
  checkpoint;
  /** Raw wire envelope. */
  raw;
  constructor(event) {
    const e = event && typeof event === "object" ? event : {};
    this.id = String(e.eventId ?? "");
    this.type = String(e.eventType ?? "");
    this.schemaVersion = Number(e.schemaVersion ?? 0) || 0;
    this.timestamp = new Date(Number(e.occurredAtUnixMs ?? 0) || 0);
    this.attempts = Number(e.deliveryAttempt ?? 1) || 1;
    this.accountId = String(e.accountId ?? "");
    this.source = String(e.source ?? "");
    this.correlationId = String(e.correlationId ?? "");
    this.causationId = String(e.causationId ?? "");
    this.deduplicationKey = String(e.deduplicationKey ?? "");
    const payload = e.payload;
    this.body = payload instanceof Uint8Array ? payload : payload instanceof ArrayBuffer ? new Uint8Array(payload) : new Uint8Array();
    this.invocationSequence = Number(e.invocationSequence ?? 0) || 0;
    this.resumePending = Boolean(e.resumePending);
    const checkpointJson = String(e.checkpointJson ?? "");
    this.checkpoint = checkpointJson ? Object.freeze({
      json: checkpointJson,
      schemaVersion: Number(e.checkpointSchemaVersion ?? 0) || 0
    }) : null;
    this.raw = e;
  }
  /**
   * Decode {@link EventMessage.body} as JSON.
   *
   * @returns Parsed payload (`{}` for an empty body).
   */
  json() {
    if (this.body.byteLength === 0)
      return {};
    return JSON.parse(new TextDecoder().decode(this.body));
  }
  /**
   * Recorded outcome.
   *
   * @returns The outcome, or `null` while undecided.
   */
  get result() {
    return this.#result;
  }
  #record(result) {
    if (this.#result === null)
      this.#result = Object.freeze(result);
  }
  /** Mark handled; the host marks the event delivered. */
  ack() {
    this.#record({ kind: "ack" });
  }
  /**
   * Redeliver later: at `retryAt`, after `delaySeconds`, or — when both are
   * omitted — whenever the host's backoff chooses.
   *
   * @param options - Redelivery timing and reason.
   */
  retry(options) {
    const explicit = unixMs(options?.retryAt);
    const delay = Number(options?.delaySeconds ?? 0) || 0;
    this.#record({
      kind: "retry",
      retryAtUnixMs: explicit || (delay > 0 ? Date.now() + Math.floor(delay * 1e3) : 0),
      reason: String(options?.reason ?? "")
    });
  }
  /**
   * Stop delivering; the host records `reason`.
   *
   * @param reason - Short operator-facing reason.
   */
  reject(reason) {
    this.#record({ kind: "reject", reason: String(reason ?? "") });
  }
  /**
   * Park for operator review.
   *
   * @param reason - Short operator-facing reason.
   */
  deadLetter(reason) {
    this.#record({ kind: "deadLetter", reason: String(reason ?? "") });
  }
  /**
   * Release with a bounded checkpoint; the host redelivers at `wakeAt` or
   * when a `wakeOnEventType` event arrives.
   *
   * @param options - Checkpoint and wake conditions.
   */
  suspend(options) {
    const opts = options ?? {};
    this.#record({
      kind: "suspended",
      checkpointJson: checkpointText(opts.checkpoint),
      checkpointSchemaVersion: Number(opts.checkpointSchemaVersion ?? 1) || 1,
      wakeAtUnixMs: unixMs(opts.wakeAt),
      wakeOnEventType: String(opts.wakeOnEventType ?? ""),
      wakeOnFilterJson: opts.wakeOnFilter === void 0 || opts.wakeOnFilter === null ? "" : typeof opts.wakeOnFilter === "string" ? opts.wakeOnFilter : JSON.stringify(opts.wakeOnFilter)
    });
  }
};
var EventBatch = class {
  /** Messages in delivery order. */
  messages;
  /** Invocation identity of this delivery. */
  invocation;
  constructor(events, invocation) {
    this.messages = Object.freeze((Array.isArray(events) ? events : []).map((event) => new EventMessage(event)));
    this.invocation = invocation ?? invocationOf(void 0);
  }
  /**
   * Event type shared by the batch.
   *
   * @returns The shared type, or `""` when mixed or empty.
   */
  get type() {
    const first = this.messages[0]?.type ?? "";
    return this.messages.every((m) => m.type === first) ? first : "";
  }
  /** Ack every message. */
  ackAll() {
    for (const m of this.messages)
      m.ack();
  }
  /**
   * Retry every message.
   *
   * @param options - Redelivery timing and reason.
   */
  retryAll(options) {
    for (const m of this.messages)
      m.retry(options);
  }
};
function eventBatchResults(batch, failed) {
  return batch.messages.map((m) => {
    if (m.result)
      return m.result;
    if (failed !== void 0) {
      return { kind: "retry", retryAtUnixMs: 0, reason: errorMessage(failed) };
    }
    return { kind: "ack" };
  });
}
var JobController = class {
  #result = null;
  #progress;
  #abort;
  /** Full durable envelope as delivered. */
  invocation;
  /** Unique id of this invocation attempt. */
  id;
  /** Command type the handler dispatches on. */
  type;
  /** Command payload JSON text. */
  payloadJson;
  /** Schema version of {@link JobController.payloadJson}. */
  payloadSchemaVersion;
  /** Caller idempotency key. */
  idempotencyKey;
  /** Failure retry counter, starting at 1. */
  attempt;
  /** Deadline hint (Unix ms); the host fence is authoritative. */
  deadlineUnixMs;
  /** Trace correlation id. */
  correlationId;
  /** Id of the event or command that caused this job. */
  causationId;
  /** Resume ordinal. */
  invocationSequence;
  /** Step id within a multi-step command; empty when single-step. */
  stepId;
  /** Checkpoint from the prior `suspend()`, or `null`. */
  checkpoint;
  /** Granted byte source, when the job has input. */
  input;
  /** Granted object store, when the job has output. */
  output;
  /** Aborts when the host cancels the invocation. */
  signal;
  constructor(invocation, granted) {
    const inv = invocation && typeof invocation === "object" ? invocation : {};
    this.invocation = Object.freeze({ ...inv });
    this.id = String(inv.invocationId ?? "");
    this.type = String(inv.commandType ?? "");
    this.payloadJson = String(inv.payloadJson ?? "");
    this.payloadSchemaVersion = Number(inv.payloadSchemaVersion ?? 0) || 0;
    this.idempotencyKey = String(inv.idempotencyKey ?? "");
    this.attempt = Number(inv.attempt ?? 1) || 1;
    this.deadlineUnixMs = Number(inv.deadlineUnixMs ?? 0) || 0;
    this.correlationId = String(inv.correlationId ?? "");
    this.causationId = String(inv.causationId ?? "");
    this.invocationSequence = Number(inv.invocationSequence ?? 0) || 0;
    this.stepId = String(inv.stepId ?? "");
    const checkpointJson = String(inv.checkpointJson ?? "");
    this.checkpoint = checkpointJson ? Object.freeze({
      json: checkpointJson,
      schemaVersion: Number(inv.checkpointSchemaVersion ?? 0) || 0
    }) : null;
    this.input = granted?.input ?? null;
    this.output = granted?.output ?? null;
    this.#progress = granted?.progress ?? null;
    this.#abort = new AbortController();
    this.signal = this.#abort.signal;
    const cancel = granted?.cancel;
    if (cancel && typeof cancel.wait === "function") {
      Promise.resolve().then(() => cancel.wait()).then(() => this.#abort.abort(PluginError.fromWire("cancelled", "job cancelled by host")), () => {
      });
    }
  }
  /**
   * Decode {@link JobController.payloadJson}.
   *
   * @returns Parsed payload (`{}` when empty).
   */
  json() {
    return this.payloadJson ? JSON.parse(this.payloadJson) : {};
  }
  /**
   * Report `percent` in `0..=100` with an operator-facing `message`.
   *
   * @param percent - Completion percent.
   * @param message - Operator-facing status.
   * @returns Resolves when the host records progress.
   */
  async progress(percent, message) {
    if (!this.#progress)
      return;
    await this.#progress.report(Number(percent) || 0, String(message ?? ""));
  }
  /**
   * Recorded terminal outcome.
   *
   * @returns The outcome, or `null` while the job is still running.
   */
  get result() {
    return this.#result;
  }
  #record(result) {
    if (this.#result === null)
      this.#result = Object.freeze(result);
  }
  /**
   * Release with a bounded checkpoint; the host resumes at `wakeAt`.
   *
   * @param options - Checkpoint and wake time.
   */
  suspend(options) {
    const opts = options ?? {};
    this.#record({
      kind: "suspended",
      checkpoint: {
        schemaVersion: Number(opts.checkpointSchemaVersion ?? 1) || 1,
        json: checkpointText(opts.checkpoint)
      },
      wakeAtUnixMs: unixMs(opts.wakeAt)
    });
  }
  /**
   * Give up this attempt and let the host retry at `retryAt` (or its default).
   *
   * @param options - Retry time and reason.
   */
  retryLater(options) {
    const opts = options ?? {};
    this.#record({
      kind: "retryable",
      message: String(opts.reason ?? ""),
      retryAfterUnixMs: unixMs(opts.retryAt)
    });
  }
  /** Host-side cancellation observed (adapter-internal). */
  cancel() {
    this.#abort.abort(PluginError.fromWire("cancelled", "job cancelled by host"));
  }
};
function jobOutcomeFor(job, returned, failed) {
  if (job.result)
    return job.result;
  if (failed !== void 0) {
    const code = failed && typeof failed === "object" ? failed.wireCode ?? failed.code : void 0;
    if (job.signal.aborted || code === "cancelled") {
      return { kind: "cancelled", message: errorMessage(failed) };
    }
    if (code === "unavailable" || code === "deadline_exceeded") {
      return { kind: "retryable", message: errorMessage(failed), retryAfterUnixMs: 0 };
    }
    return { kind: "rejected", message: errorMessage(failed) };
  }
  const extra = returned && typeof returned === "object" ? returned : {};
  return {
    kind: "completed",
    message: String(extra.message ?? ""),
    bytesCopied: Number(extra.bytesCopied ?? 0) || 0
  };
}
var BookclerkEntrypoint = class extends WorkerEntrypoint {
  /**
   * Rejects HTTP fetch — workerd guests are Workers-RPC only.
   *
   * @param _request - Incoming HTTP request when the entrypoint is fetch-facing.
   * @returns Always a 404 empty response.
   */
  async fetch(_request) {
    return new Response(null, { status: 404 });
  }
  /**
   * Adapter dispatch for `describe()`.
   *
   * @returns Author refinement or `null` when not implemented.
   * @internal
   */
  async bookclerkDescribe() {
    if (typeof this.describe !== "function")
      return null;
    return await this.describe();
  }
  /**
   * Adapter dispatch for the `event(batch)` trigger.
   *
   * @param context - Granted bindings for this invocation.
   * @param wireBatch - Wire `EventBatch`.
   * @returns One outcome per event, in order.
   * @internal
   */
  async bookclerkEvent(context, wireBatch) {
    if (typeof this.event !== "function") {
      throw unsupported("event");
    }
    applyInvocationEnv(this, context);
    const events = Array.isArray(wireBatch?.events) ? wireBatch.events : [];
    const batch = new EventBatch(events, invocationOf(context));
    try {
      await this.event(batch);
      return eventBatchResults(batch, void 0);
    } catch (err) {
      return eventBatchResults(batch, err ?? new Error("event handler failed"));
    }
  }
  /**
   * Adapter dispatch for the `job(controller)` trigger.
   *
   * @param context - Granted bindings for this invocation.
   * @param invocation - Durable command envelope.
   * @param granted - Granted input / output / progress / cancel stubs.
   * @returns Wire job outcome.
   * @internal
   */
  async bookclerkJob(context, invocation, granted) {
    if (typeof this.job !== "function") {
      throw unsupported("job");
    }
    applyInvocationEnv(this, context);
    const job = new JobController(invocation, granted);
    try {
      const returned = await this.job(job);
      return jobOutcomeFor(job, returned, void 0);
    } catch (err) {
      return jobOutcomeFor(job, void 0, err ?? new Error("job handler failed"));
    }
  }
  /**
   * Adapter dispatch for `databaseMigrations(binding)`.
   *
   * @param binding - Binding name from `[[databases]]`.
   * @returns Bounded registration (`[]` when not implemented).
   * @internal
   */
  async bookclerkDatabaseMigrations(binding) {
    if (typeof this.databaseMigrations !== "function")
      return [];
    const migrations = await this.databaseMigrations(String(binding ?? ""));
    return requirePluginMigrationRegistration(Array.isArray(migrations) ? migrations : []);
  }
  /**
   * Adapter dispatch for `shutdown()`.
   *
   * @returns Resolves when the author hook has run.
   * @internal
   */
  async bookclerkShutdown() {
    if (typeof this.shutdown === "function")
      await this.shutdown();
  }
};
var NamedEntrypoint = class extends WorkerEntrypoint {
  /** Methods the adapter may dispatch on this entrypoint. */
  static bookclerkMethods = [];
  /** Invocation identity of the current call (set by `bookclerkInvoke`). */
  invocation = invocationOf(void 0);
  /**
   * Rejects HTTP fetch — workerd guests are Workers-RPC only.
   *
   * @param _request - Incoming HTTP request when the entrypoint is fetch-facing.
   * @returns Always a 404 empty response.
   */
  async fetch(_request) {
    return new Response(null, { status: 404 });
  }
  /**
   * Adapter dispatch: install granted bindings, then call `method`.
   *
   * @param context - Granted bindings for this invocation.
   * @param method - Method name from the static allowlist.
   * @param args - Method arguments.
   * @returns Method result.
   * @internal
   */
  async bookclerkInvoke(context, method, ...args) {
    const allowed = this.constructor.bookclerkMethods ?? [];
    const target = this[method];
    if (!allowed.includes(method) || typeof target !== "function") {
      throw unsupported(method);
    }
    applyInvocationEnv(this, context);
    this.invocation = invocationOf(context);
    return await target.apply(this, args);
  }
};
var STOREFRONT_METHODS = Object.freeze([
  "login",
  "scan",
  "fetchTitle",
  "listAccounts",
  "loginStart",
  "loginComplete",
  "searchCatalog",
  "expandCandidates",
  "purchaseHint",
  "listDeals",
  "catalogDetail",
  "diagnose",
  "health"
]);
var StorefrontEntrypoint = class extends NamedEntrypoint {
  static bookclerkMethods = STOREFRONT_METHODS;
  /**
   * Password or one-shot OAuth login. The host seals
   * `LoginResult.credentials` into `encrypted_secrets`.
   *
   * @param _params - Login parameters.
   * @returns Account identity plus opaque credentials.
   */
  login(_params) {
    return Promise.reject(unsupported("login"));
  }
  /**
   * Library scan; the host upserts `ScanSummary.books`.
   *
   * @param _params - Scan parameters (accounts + credentials).
   * @returns Books discovered plus per-account counters.
   */
  scan(_params) {
    return Promise.reject(unsupported("scan"));
  }
  /**
   * Fetches one title into `params.cacheDir` and returns plain media paths.
   *
   * @param _params - Title identifiers, credentials, fetch options.
   * @returns Plain media parts plus metadata.
   */
  fetchTitle(_params) {
    return Promise.reject(unsupported("fetchTitle"));
  }
  /**
   * Accounts the guest knows about.
   *
   * @returns Account list.
   */
  listAccounts() {
    return Promise.reject(unsupported("listAccounts"));
  }
  /**
   * Begins an interactive OAuth login.
   *
   * @param _params - Login parameters.
   * @returns Continuation for {@link StorefrontEntrypoint.loginComplete}.
   */
  loginStart(_params) {
    return Promise.reject(unsupported("loginStart"));
  }
  /**
   * Finishes a login started by {@link StorefrontEntrypoint.loginStart}.
   *
   * @param _params - Continuation plus user input.
   * @returns Account identity plus opaque credentials.
   */
  loginComplete(_params) {
    return Promise.reject(unsupported("loginComplete"));
  }
  /**
   * Free-text storefront catalog search.
   *
   * @param _params - Query parameters.
   * @returns Catalog hits.
   */
  searchCatalog(_params) {
    return Promise.reject(unsupported("searchCatalog"));
  }
  /**
   * Related-title expansion from a seed title.
   *
   * @param _params - Seed title plus limits.
   * @returns Candidate hits.
   */
  expandCandidates(_params) {
    return Promise.reject(unsupported("expandCandidates"));
  }
  /**
   * Purchase link / price hint; `null` when the title is unknown.
   *
   * @param _params - Title identifiers.
   * @returns Hint or `null`.
   */
  purchaseHint(_params) {
    return Promise.reject(unsupported("purchaseHint"));
  }
  /**
   * Current storefront deals.
   *
   * @param _params - Optional filters.
   * @returns Deal hits.
   */
  listDeals(_params) {
    return Promise.reject(unsupported("listDeals"));
  }
  /**
   * Full catalog record for one product; `null` when unknown.
   *
   * @param _params - Product identifiers.
   * @returns Catalog hit or `null`.
   */
  catalogDetail(_params) {
    return Promise.reject(unsupported("catalogDetail"));
  }
  /**
   * Operator-facing diagnostic lines.
   *
   * @returns Probe lines.
   */
  diagnose() {
    return Promise.resolve([]);
  }
  /**
   * Reports whether the storefront session is usable.
   *
   * @returns Health flag plus detail.
   */
  health() {
    return Promise.resolve({ ok: true, detail: "" });
  }
};
var STORAGE_METHODS = Object.freeze([
  "head",
  "list",
  "get",
  "put",
  "copy",
  "delete",
  "commit",
  "abortStage"
]);
var StorageEntrypoint = class extends NamedEntrypoint {
  static bookclerkMethods = STORAGE_METHODS;
  /**
   * Metadata without a body; `null` when the key is missing.
   *
   * @param _key - Object key.
   * @returns Metadata or `null` when the key is missing.
   */
  head(_key) {
    return Promise.reject(unsupported("head"));
  }
  /**
   * One page of keys under `options.prefix`.
   *
   * @param _options - Prefix, cursor, and limit.
   * @returns One page of object keys.
   */
  list(_options) {
    return Promise.reject(unsupported("list"));
  }
  /**
   * Streamed read. The body is a transferred stream, not a scalar.
   *
   * @param _key - Object key.
   * @param _options - Optional byte range.
   * @returns Metadata plus a transferred body stream.
   */
  get(_key, _options) {
    return Promise.reject(unsupported("get"));
  }
  /**
   * Streamed write. `body` ownership is transferred to the destination.
   *
   * @param _key - Object key.
   * @param _body - Byte stream.
   * @param _options - Optional content type / length.
   * @returns Bytes written and optional etag / sha256.
   */
  put(_key, _body, _options) {
    return Promise.reject(unsupported("put"));
  }
  /**
   * Server-side copy when the backend supports it.
   *
   * @param _from - Source key.
   * @param _to - Destination key.
   * @returns Bytes copied.
   */
  copy(_from, _to) {
    return Promise.reject(unsupported("copy"));
  }
  /**
   * Delete a key (no-op if missing).
   *
   * @param _key - Object key.
   * @returns Resolves when the delete is complete.
   */
  delete(_key) {
    return Promise.reject(unsupported("delete"));
  }
  /**
   * Finalize a destination-side staged object.
   *
   * @param _key - Object key.
   * @param _commitToken - Idempotency / commit token.
   * @returns Published object metadata.
   */
  commit(_key, _commitToken) {
    return Promise.reject(unsupported("commit"));
  }
  /**
   * Abort a destination-side staged object.
   *
   * @param _key - Object key.
   * @param _commitToken - Staging token to discard.
   * @returns Rejects with typed `unsupported` unless overridden.
   */
  abortStage(_key, _commitToken) {
    return Promise.reject(unsupported("abortStage"));
  }
};
var REMOTE_LIBRARY_METHODS = Object.freeze([
  "health",
  "diagnose",
  "start",
  "stop",
  "scanLibrary",
  "syncListening",
  "pollEvents"
]);
var RemoteLibraryEntrypoint = class extends NamedEntrypoint {
  static bookclerkMethods = REMOTE_LIBRARY_METHODS;
  /**
   * Reports whether the remote library session is usable.
   *
   * @returns Health flag plus detail.
   */
  health() {
    return Promise.resolve({ ok: true, detail: "" });
  }
  /**
   * Operator-facing diagnostic lines.
   *
   * @returns Probe lines.
   */
  diagnose() {
    return Promise.resolve([]);
  }
  /**
   * Starts long-running work for this invocation.
   *
   * @returns Resolves when running.
   */
  start() {
    return Promise.resolve();
  }
  /**
   * Stops long-running work for this invocation.
   *
   * @returns Resolves when stopped.
   */
  stop() {
    return Promise.resolve();
  }
  /**
   * Re-syncs the remote library.
   *
   * @param _params - Scan scope.
   * @returns Resolves when the scan has been accepted.
   */
  scanLibrary(_params) {
    return Promise.reject(unsupported("scanLibrary"));
  }
  /**
   * Push / pull listening progress; the host upserts the rows.
   *
   * @returns Progress rows.
   */
  syncListening() {
    return Promise.reject(unsupported("syncListening"));
  }
  /**
   * Drains external users observed since the last poll.
   *
   * @returns Newly observed users.
   */
  pollEvents() {
    return Promise.reject(unsupported("pollEvents"));
  }
};
var DatabaseAdapterEntrypoint = class extends NamedEntrypoint {
  static bookclerkMethods = Object.freeze(["openSession"]);
  /**
   * Opens a session. Sessions cannot survive suspension.
   *
   * @returns Adapter session capability.
   */
  openSession() {
    return Promise.reject(unsupported("openSession"));
  }
};
var CliEntrypoint = class extends NamedEntrypoint {
  static bookclerkMethods = Object.freeze(["describe", "invoke"]);
  /**
   * Guest CLI schema.
   *
   * @returns Declared commands (`{ commands: [] }` when the guest has no CLI).
   */
  describe() {
    return Promise.resolve({ commands: [] });
  }
  /**
   * Invokes a guest CLI command.
   *
   * @param _params - Command name plus named argument values.
   * @returns Exit code, captured output, optional structured payload.
   */
  invoke(_params) {
    return Promise.reject(unsupported("invoke"));
  }
};
var OidcEntrypoint = class extends NamedEntrypoint {
  static bookclerkMethods = Object.freeze(["clients", "authenticateUser"]);
  /**
   * Plugin-provided OIDC authorization-server client templates. The host
   * materializes `oidc_clients` rows; plugins never mint tokens.
   *
   * @returns Templates (`[]` when unused).
   */
  clients() {
    return Promise.resolve([]);
  }
  /**
   * Verifies remote credentials on behalf of the host.
   *
   * @param _params - Username / password pair.
   * @returns External user identity.
   */
  authenticateUser(_params) {
    return Promise.reject(unsupported("authenticateUser"));
  }
};
var ENTRYPOINT_BINDINGS = Object.freeze({
  storefront: "PLUGIN_STOREFRONT",
  storage: "PLUGIN_STORAGE",
  databaseAdapter: "PLUGIN_DATABASE_ADAPTER",
  remoteLibrary: "PLUGIN_REMOTE_LIBRARY",
  cli: "PLUGIN_CLI",
  oidc: "PLUGIN_OIDC"
});
var ENTRYPOINT_CLASSES = Object.freeze({
  storefront: "Storefront",
  storage: "Storage",
  databaseAdapter: "DatabaseAdapter",
  remoteLibrary: "RemoteLibrary",
  cli: "Cli",
  oidc: "Oidc"
});
function exactLengthBody(body, expected) {
  if (body == null || expected == null || !Number.isFinite(expected)) {
    return body;
  }
  const reader = body.getReader();
  let n = 0;
  return new ReadableStream({
    async pull(controller) {
      const { done, value } = await reader.read();
      if (done) {
        if (n !== expected) {
          controller.error(PluginError.fromWire("invalid_params", `content-length ${expected} got ${n}`));
          return;
        }
        controller.close();
        return;
      }
      n += value.byteLength;
      if (n > expected) {
        controller.error(PluginError.fromWire("payload_too_large", `body exceeded Content-Length ${expected}`));
        return;
      }
      controller.enqueue(value);
    },
    cancel(reason) {
      return reader.cancel(reason);
    }
  });
}
function streamedRead(resp, key) {
  return {
    meta: {
      key: resp.headers.get("x-bookclerk-key") || key,
      size: Number(resp.headers.get("x-bookclerk-size") || "0"),
      contentType: resp.headers.get("x-bookclerk-content-type") || void 0,
      etag: resp.headers.get("x-bookclerk-etag") || void 0
    },
    body: resp.body
  };
}
var GrantedSource = class extends Source {
  #granted;
  #auth;
  #signal;
  constructor(granted, auth, signal) {
    super();
    this.#granted = granted;
    this.#auth = auth;
    this.#signal = signal;
  }
  async open(key) {
    const resp = await this.#granted.fetch(`http://granted/open?key=${encodeURIComponent(key)}`, {
      headers: this.#auth,
      signal: this.#signal
    });
    if (!resp.ok) {
      throw PluginError.fromWire("internal", await resp.text());
    }
    return streamedRead(resp, key);
  }
};
var GrantedDestination = class extends Destination {
  #granted;
  #auth;
  #signal;
  constructor(granted, auth, signal) {
    super();
    this.#granted = granted;
    this.#auth = auth;
    this.#signal = signal;
  }
  async put(key, body, options) {
    const headers = { ...this.#auth };
    if (options?.contentType)
      headers["content-type"] = options.contentType;
    if (options?.contentLength != null) {
      headers["content-length"] = String(options.contentLength);
    }
    const resp = await this.#granted.fetch(`http://granted/put?key=${encodeURIComponent(key)}`, {
      method: "PUT",
      headers,
      body,
      signal: this.#signal
    });
    if (!resp.ok) {
      throw PluginError.fromWire("internal", await resp.text());
    }
    return await resp.json();
  }
};
var GrantedProgress = class extends ProgressSink {
  #granted;
  #auth;
  #signal;
  constructor(granted, auth, signal) {
    super();
    this.#granted = granted;
    this.#auth = auth;
    this.#signal = signal;
  }
  async report(percent, message) {
    await this.#granted.fetch(`http://granted/progress`, {
      method: "POST",
      headers: { ...this.#auth, "content-type": "application/json" },
      body: JSON.stringify({ percent, message: message || "" }),
      signal: this.#signal
    });
  }
};
function publishPayloadBytes(payload) {
  if (payload === void 0 || payload === null)
    return new Uint8Array(0);
  if (payload instanceof Uint8Array)
    return payload;
  if (payload instanceof ArrayBuffer)
    return new Uint8Array(payload);
  if (typeof payload === "string")
    return new TextEncoder().encode(payload);
  return new TextEncoder().encode(JSON.stringify(payload));
}
var GrantedEvents = class extends RpcTarget {
  #granted;
  #auth;
  /**
   * @param granted - Granted reverse channel.
   * @param auth - Bearer header for the events grant.
   */
  constructor(granted, auth) {
    super();
    this.#granted = granted;
    this.#auth = auth;
  }
  /**
   * Publish one domain event through the host outbox.
   *
   * @param event - Event type, schema version, payload, dedup key.
   * @returns Outbox row identity.
   */
  async publish(event) {
    if (!event || typeof event.eventType !== "string" || !event.eventType) {
      throw PluginError.fromWire("invalid_params", "publish requires eventType");
    }
    const payload = publishPayloadBytes(event.payload);
    if (payload.byteLength > MAX_EVENT_PAYLOAD_BYTES) {
      throw PluginError.fromWire("payload_too_large", `event payload of ${payload.byteLength} bytes exceeds ${MAX_EVENT_PAYLOAD_BYTES}`);
    }
    const wire = toBridgeJson({
      eventType: event.eventType,
      schemaVersion: Number(event.schemaVersion ?? 1) || 1,
      deduplicationKey: String(event.deduplicationKey ?? ""),
      payload,
      occurredAtUnixMs: Number(event.occurredAtUnixMs ?? 0) || 0,
      correlationId: String(event.correlationId ?? ""),
      causationId: String(event.causationId ?? "")
    });
    const resp = await this.#granted.fetch("http://granted/events/publish", {
      method: "POST",
      headers: { ...this.#auth, "content-type": "application/json" },
      body: JSON.stringify(wire)
    });
    const value = await resp.json().catch(() => ({}));
    if (value && value.error) {
      throw PluginError.fromWire(value.error.code || "internal", value.error.message || "");
    }
    if (!resp.ok) {
      throw PluginError.fromWire("internal", `events publish HTTP ${resp.status}`);
    }
    return { eventId: String(value.eventId ?? ""), duplicate: Boolean(value.duplicate) };
  }
};
var CancelWatch = class extends RpcTarget {
  #promise;
  constructor(signal) {
    super();
    this.#promise = new Promise((resolve) => {
      if (signal.aborted)
        resolve();
      else
        signal.addEventListener("abort", () => resolve(), { once: true });
    });
  }
  wait() {
    return this.#promise;
  }
};
function grantedJobCapabilities(env, grantToken, controller) {
  const granted = env.GRANTED;
  if (!granted || typeof grantToken !== "string" || !grantToken) {
    throw PluginError.fromWire("internal", "granted reverse channel missing");
  }
  const auth = { Authorization: `Bearer ${grantToken}` };
  return {
    input: new GrantedSource(granted, auth, controller.signal),
    output: new GrantedDestination(granted, auth, controller.signal),
    progress: new GrantedProgress(granted, auth, controller.signal),
    cancel: new CancelWatch(controller.signal)
  };
}
function bytesToBase64(bytes) {
  let bin = "";
  for (let i = 0; i < bytes.length; i++)
    bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
}
function toBridgeJson(value) {
  if (value instanceof Uint8Array)
    return bytesToBase64(value);
  if (value instanceof ArrayBuffer)
    return bytesToBase64(new Uint8Array(value));
  if (Array.isArray(value))
    return value.map(toBridgeJson);
  if (value === null || typeof value !== "object")
    return value;
  const out = {};
  for (const [key, inner] of Object.entries(value)) {
    if (inner === void 0)
      continue;
    out[key] = toBridgeJson(inner);
  }
  return out;
}
function parseJsonBinding(value) {
  if (value == null)
    return null;
  if (typeof value === "string") {
    try {
      return JSON.parse(value);
    } catch {
      return null;
    }
  }
  return typeof value === "object" ? value : null;
}
function wrapPluginFromBinding() {
  return createInvocationAdapter();
}
function wrapPluginFromNative() {
  return createInvocationAdapter();
}
var TRIGGER_FAMILIES = Object.freeze({
  eventConsumer: "eventConsumer",
  jobRunner: "jobRunner"
});
function allowedEntrypointFamilies(capabilities, requested) {
  if (!capabilities || typeof capabilities !== "object")
    return [];
  const entrypoints = Array.isArray(capabilities.entrypoints) ? capabilities.entrypoints : [];
  const consumes = Array.isArray(capabilities.consumes) ? capabilities.consumes : [];
  const jobs = Array.isArray(capabilities.jobs) ? capabilities.jobs : [];
  const exportsStorage = entrypoints.includes("storage");
  const out = [];
  for (const name of requested) {
    if (typeof name !== "string" || out.includes(name))
      continue;
    if (name === TRIGGER_FAMILIES.eventConsumer) {
      if (consumes.length > 0)
        out.push(name);
    } else if (name === TRIGGER_FAMILIES.jobRunner) {
      if (jobs.length > 0 || exportsStorage)
        out.push(name);
    } else if (name in ENTRYPOINT_BINDINGS && entrypoints.includes(name)) {
      out.push(name);
    }
  }
  return out;
}
function mergeDescribe(base, guest) {
  const guestCli = guest.cli;
  const cli = guestCli && Array.isArray(guestCli.commands) && guestCli.commands.length > 0 ? guestCli : base.cli ?? guestCli;
  return {
    ...base,
    ...guest,
    apiVersion: base.apiVersion ?? guest.apiVersion ?? 3,
    id: base.id ?? guest.id,
    capabilities: base.capabilities ?? guest.capabilities,
    ...cli === void 0 ? {} : { cli }
  };
}
function createInvocationAdapter() {
  return class InvocationAdapter extends WorkerEntrypoint {
    #manifest() {
      return parseJsonBinding(this.env.PLUGIN_DESCRIBE) ?? {};
    }
    #author() {
      if (this.env.PLUGIN)
        return this.env.PLUGIN;
      throw PluginError.fromWire("unavailable", "PLUGIN binding missing");
    }
    #named(name) {
      const binding = ENTRYPOINT_BINDINGS[name];
      const stub = binding ? this.env[binding] : void 0;
      if (!stub) {
        throw PluginError.fromWire("unsupported", `${name} entrypoint not exported`);
      }
      return stub;
    }
    /**
     * Turn the bridge context into the author-facing one: the host's events
     * grant token becomes an `EVENTS` stub and never reaches the author.
     *
     * @param ctx - Bridge context from the launcher.
     * @returns Author-facing granted context.
     */
    #bindContext(ctx) {
      const source = ctx && typeof ctx === "object" ? ctx : {};
      const { eventsToken, ...rest } = source;
      const bound = rest;
      if (typeof eventsToken === "string" && eventsToken && this.env.GRANTED) {
        bound.events = new GrantedEvents(this.env.GRANTED, {
          Authorization: `Bearer ${eventsToken}`
        });
      }
      return bound;
    }
    async fetch(_request) {
      return new Response(null, { status: 404 });
    }
    /**
     * `PluginDescribe`: the manifest projection the launcher binds as
     * `PLUGIN_DESCRIBE`, refined by the guest's describe. Author mode calls
     * the author's optional `describe()`; native-behind-workerd passes the
     * native guest's typed describe as `native`. Identity and capabilities
     * always come from the manifest.
     *
     * @param native - Native guest describe (native-behind-workerd only).
     * @returns Wire describe.
     */
    async describe(native) {
      const base = this.#manifest();
      if (native !== void 0) {
        if (native === null || typeof native !== "object") {
          throw PluginError.fromWire("invalid_params", "native describe must be an object");
        }
        return mergeDescribe(base, native);
      }
      const author = await this.#author().bookclerkDescribe() ?? {};
      return mergeDescribe(base, author);
    }
    /**
     * `/open` policy for native-behind-workerd: validates the invocation
     * envelope and returns the requested entrypoint / trigger families the
     * manifest (`PLUGIN_DESCRIBE`) lets this plugin export. The launcher
     * nulls every family that is not returned.
     *
     * @param ctx - Open context (`invocation`, `config`, `secrets`).
     * @param requested - Family names the launcher wants to export.
     * @returns Authorized subset of `requested`.
     */
    async openInvocation(ctx, requested = []) {
      const invocation = ctx && typeof ctx === "object" ? ctx.invocation : void 0;
      if (!invocation || typeof invocation.id !== "string" || !invocation.id) {
        throw PluginError.fromWire("invalid_params", "open requires a non-empty invocation id");
      }
      const list = Array.isArray(requested) ? requested : [];
      return allowedEntrypointFamilies(this.#manifest().capabilities, list);
    }
    /**
     * Call `method` on the named entrypoint `name` with the granted context.
     *
     * @param name - Entrypoint wire name.
     * @param ctx - Granted context.
     * @param method - Method name.
     * @param args - Method arguments.
     * @returns Method result.
     */
    async invokeEntrypoint(name, ctx, method, args = []) {
      const list = Array.isArray(args) ? args : [];
      return this.#named(name).bookclerkInvoke(this.#bindContext(ctx), method, ...list);
    }
    /**
     * Deliver one domain event to the default entrypoint's `event(batch)`.
     *
     * @param ctx - Granted context.
     * @param event - Wire domain event.
     * @returns Recorded outcome.
     */
    async invokeEvent(ctx, event) {
      const results = await this.#author().bookclerkEvent(this.#bindContext(ctx), { events: [event] });
      const first = Array.isArray(results) ? results[0] : void 0;
      if (!first || typeof first.kind !== "string") {
        throw PluginError.fromWire("internal", "event handler returned no result");
      }
      return first;
    }
    /**
     * `/destination/<op>` route → `storage` entrypoint.
     *
     * @param op - Destination method name.
     * @param ctx - Granted context.
     * @param args - Route arguments.
     * @param body - Stream body for `put`.
     * @returns Method result.
     */
    async invokeDestination(op, ctx, args = {}, body) {
      switch (op) {
        case "head":
          return this.invokeEntrypoint("storage", ctx, "head", [String(args.key ?? "")]);
        case "list":
          return this.invokeEntrypoint("storage", ctx, "list", [args.options ?? {}]);
        case "get":
          return this.invokeEntrypoint("storage", ctx, "get", [String(args.key ?? ""), args.options]);
        case "put": {
          if (!body) {
            throw PluginError.fromWire("invalid_params", "put missing body stream");
          }
          const options = args.options;
          const bounded = exactLengthBody(body, options?.contentLength);
          return this.invokeEntrypoint("storage", ctx, "put", [String(args.key ?? ""), bounded, options]);
        }
        case "copy":
          return this.invokeEntrypoint("storage", ctx, "copy", [String(args.from ?? ""), String(args.to ?? "")]);
        case "delete":
          await this.invokeEntrypoint("storage", ctx, "delete", [String(args.key ?? "")]);
          return { ok: true };
        case "commit":
          return this.invokeEntrypoint("storage", ctx, "commit", [
            String(args.key ?? ""),
            String(args.commitToken ?? "")
          ]);
        case "abortStage":
          await this.invokeEntrypoint("storage", ctx, "abortStage", [
            String(args.key ?? ""),
            String(args.commitToken ?? "")
          ]);
          return { ok: true };
        default:
          throw PluginError.fromWire("unsupported", `destination.${op}`);
      }
    }
    /**
     * `/source/open` route → `storage.get`.
     *
     * @param ctx - Granted context.
     * @param key - Object key.
     * @returns Opened byte source result.
     */
    async invokeSourceOpen(ctx, key) {
      return this.invokeEntrypoint("storage", ctx, "get", [String(key ?? "")]);
    }
    /**
     * `/worker/handle` route → default entrypoint `job(controller)`.
     *
     * @param ctx - Granted context.
     * @param invocation - Durable command envelope.
     * @param grantToken - Per-invocation grant token.
     * @param _databases - Per-binding grant tokens (bound in a later ABI step).
     * @returns Wire job outcome.
     */
    async invokeHandle(ctx, invocation, grantToken, _databases) {
      const controller = new AbortController();
      try {
        const granted = grantedJobCapabilities(this.env, grantToken, controller);
        return await this.#author().bookclerkJob(this.#bindContext(ctx), invocation ?? {}, granted);
      } finally {
        controller.abort();
      }
    }
    /**
     * `/cliDescribe` route → `cli.describe`.
     *
     * @returns CLI schema.
     */
    async cliDescribe() {
      return await this.invokeEntrypoint("cli", {}, "describe", []);
    }
    /**
     * `/cliInvoke` route → `cli.invoke`.
     *
     * @param params - Command and arguments.
     * @returns Invocation result.
     */
    async cliInvoke(params) {
      return await this.invokeEntrypoint("cli", {}, "invoke", [params ?? {}]);
    }
    /**
     * `/oidcClients` route → `oidc.clients`.
     *
     * @returns Client templates.
     */
    async oidcClients() {
      const clients = await this.invokeEntrypoint("oidc", {}, "clients", []);
      return Array.isArray(clients) ? clients : [];
    }
    /**
     * `/databaseMigrations` route → default entrypoint `databaseMigrations`.
     *
     * @param binding - Binding name.
     * @returns Bounded registration.
     */
    async databaseMigrations(binding) {
      return this.#author().bookclerkDatabaseMigrations(String(binding ?? ""));
    }
    /**
     * `/shutdown` route → default entrypoint `shutdown`. Without an author
     * `PLUGIN` binding (native-behind-workerd) there is nothing to run here:
     * the launcher shuts the native guest down over Cap'n Proto.
     *
     * @returns Resolves when the author hook has run.
     */
    async shutdown() {
      if (!this.env.PLUGIN)
        return;
      await this.#author().bookclerkShutdown();
    }
  };
}

// dist/db-capnp.js
var WORD = 8;
var MAX_TRAVERSAL_WORDS = 64 * 1024;
var textEncoder = new TextEncoder();
var textDecoder = new TextDecoder();
var CapnpMessage = class {
  buf;
  view;
  /** Words used in the segment, including the root pointer at word 0. */
  usedWords = 1;
  constructor() {
    this.buf = new Uint8Array(256);
    this.view = new DataView(this.buf.buffer, this.buf.byteOffset, this.buf.byteLength);
  }
  alloc(nWords) {
    const off = this.usedWords;
    this.usedWords += nWords;
    this.ensure((this.usedWords + 1) * WORD);
    return off;
  }
  initRoot(dataWords, pointerWords) {
    const off = this.alloc(dataWords + pointerWords);
    this.writeStructPointer(0, off, dataWords, pointerWords);
    return new CapnpStruct(this, off, dataWords, pointerWords);
  }
  finish() {
    const segBytes = this.usedWords * WORD;
    const out = new Uint8Array(WORD + segBytes);
    const view = new DataView(out.buffer);
    view.setUint32(0, 0, true);
    view.setUint32(4, this.usedWords, true);
    out.set(this.buf.subarray(0, segBytes), WORD);
    return out;
  }
  writeStructPointer(ptrWord, targetWord, dataWords, pointerWords) {
    const offset = targetWord - (ptrWord + 1);
    const word = 0n | BigInt(offset & 1073741823) << 2n | BigInt(dataWords) << 32n | BigInt(pointerWords) << 48n;
    this.setWord(ptrWord, word);
  }
  writeListPointer(ptrWord, targetWord, elementSize, listLength) {
    const offset = targetWord - (ptrWord + 1);
    const word = 1n | BigInt(offset & 1073741823) << 2n | BigInt(elementSize) << 32n | BigInt(listLength) << 35n;
    this.setWord(ptrWord, word);
  }
  writeCapPointer(ptrWord, index) {
    this.setWord(ptrWord, 3n | BigInt(index) << 32n);
  }
  writeEmptyCompositeList(ptrWord, dataWords, pointerWords) {
    const tagWord = this.alloc(1);
    this.writeListPointer(ptrWord, tagWord, 7, 0);
    const tag = 0n | BigInt(dataWords) << 32n | BigInt(pointerWords) << 48n;
    this.setWord(tagWord, tag);
  }
  initStructList(ptrWord, count, dataWords, pointerWords) {
    if (count === 0) {
      this.writeEmptyCompositeList(ptrWord, dataWords, pointerWords);
      return [];
    }
    const elemWords = dataWords + pointerWords;
    const payloadWords = count * elemWords;
    const tagWord = this.alloc(1 + payloadWords);
    this.writeListPointer(ptrWord, tagWord, 7, payloadWords);
    const tag = 0n | BigInt(count) << 2n | BigInt(dataWords) << 32n | BigInt(pointerWords) << 48n;
    this.setWord(tagWord, tag);
    const out = [];
    for (let i = 0; i < count; i++) {
      out.push(new CapnpStruct(this, tagWord + 1 + i * elemWords, dataWords, pointerWords));
    }
    return out;
  }
  setText(ptrWord, value) {
    const encoded = textEncoder.encode(value);
    const withNul = new Uint8Array(encoded.length + 1);
    withNul.set(encoded, 0);
    this.setByteList(ptrWord, withNul);
  }
  setData(ptrWord, value) {
    this.setByteList(ptrWord, value);
  }
  setByteList(ptrWord, bytes) {
    if (bytes.length === 0) {
      this.writeListPointer(ptrWord, ptrWord + 1, 2, 0);
      return;
    }
    const nWords = Math.ceil(bytes.length / WORD);
    const target = this.alloc(nWords);
    this.buf.set(bytes, target * WORD);
    this.writeListPointer(ptrWord, target, 2, bytes.length);
  }
  /**
   * Allocates a list of `count` pointers.
   *
   * @param ptrWord - Word holding the list pointer.
   * @param count - Number of pointer elements.
   * @returns The word of each element, in order.
   */
  setPointerList(ptrWord, count) {
    if (count === 0) {
      this.writeListPointer(ptrWord, ptrWord + 1, 6, 0);
      return [];
    }
    const target = this.alloc(count);
    this.writeListPointer(ptrWord, target, 6, count);
    const out = [];
    for (let i = 0; i < count; i++) {
      out.push(target + i);
    }
    return out;
  }
  setTextList(ptrWord, values) {
    const words = this.setPointerList(ptrWord, values.length);
    for (let i = 0; i < values.length; i++) {
      this.setText(words[i], values[i]);
    }
  }
  setDataList(ptrWord, values) {
    const words = this.setPointerList(ptrWord, values.length);
    for (let i = 0; i < values.length; i++) {
      this.setData(words[i], values[i]);
    }
  }
  /**
   * Writes a `List(UInt16)` / `List(UInt32)` (element size codes 3 / 4).
   *
   * @param ptrWord - Word holding the list pointer.
   * @param values - Unsigned integers in range for `byteWidth`.
   * @param byteWidth - `2` for UInt16 (enums), `4` for UInt32.
   */
  setUintList(ptrWord, values, byteWidth) {
    const count = values.length;
    const sizeCode = byteWidth === 2 ? 3 : 4;
    if (count === 0) {
      this.writeListPointer(ptrWord, ptrWord + 1, sizeCode, 0);
      return;
    }
    const target = this.alloc(Math.ceil(count * byteWidth / WORD));
    const view = new DataView(this.buf.buffer, this.buf.byteOffset + target * WORD);
    for (let i = 0; i < count; i++) {
      if (byteWidth === 2) {
        view.setUint16(i * 2, values[i], true);
      } else {
        view.setUint32(i * 4, values[i], true);
      }
    }
    this.writeListPointer(ptrWord, target, sizeCode, count);
  }
  setBoolList(ptrWord, values) {
    const count = values.length;
    if (count === 0) {
      this.writeListPointer(ptrWord, ptrWord + 1, 1, 0);
      return;
    }
    const target = this.alloc(Math.ceil(count / 64));
    const base = target * WORD;
    for (let i = 0; i < count; i++) {
      if (values[i]) {
        this.buf[base + (i >> 3)] |= 1 << (i & 7);
      }
    }
    this.writeListPointer(ptrWord, target, 1, count);
  }
  setUint16(word, fieldIndex, value) {
    this.view.setUint16(word * WORD + fieldIndex * 2, value, true);
  }
  setInt32(word, fieldIndex, value) {
    this.view.setInt32(word * WORD + fieldIndex * 4, value | 0, true);
  }
  setUint32(word, fieldIndex, value) {
    this.view.setUint32(word * WORD + fieldIndex * 4, value >>> 0, true);
  }
  setInt64(word, fieldIndex, value) {
    this.view.setBigInt64(word * WORD + fieldIndex * 8, value, true);
  }
  setUint64(word, fieldIndex, value) {
    this.view.setBigUint64(word * WORD + fieldIndex * 8, value, true);
  }
  setFloat32(word, fieldIndex, value) {
    this.view.setFloat32(word * WORD + fieldIndex * 4, value, true);
  }
  setFloat64(word, fieldIndex, value) {
    this.view.setFloat64(word * WORD + fieldIndex * 8, value, true);
  }
  setBool(word, bitIndex, value) {
    const byteOff = word * WORD + (bitIndex >> 3);
    const mask = 1 << (bitIndex & 7);
    if (value) {
      this.buf[byteOff] |= mask;
    } else {
      this.buf[byteOff] &= ~mask;
    }
  }
  setWord(word, value) {
    this.view.setBigUint64(word * WORD, value, true);
  }
  ensure(bytes) {
    if (this.buf.byteLength >= bytes) {
      return;
    }
    let n = this.buf.byteLength;
    while (n < bytes) {
      n *= 2;
    }
    const next = new Uint8Array(n);
    next.set(this.buf);
    this.buf = next;
    this.view = new DataView(this.buf.buffer, this.buf.byteOffset, this.buf.byteLength);
  }
};
var CapnpStruct = class _CapnpStruct {
  msg;
  word;
  dataWords;
  pointerWords;
  constructor(msg, word, dataWords, pointerWords) {
    this.msg = msg;
    this.word = word;
    this.dataWords = dataWords;
    this.pointerWords = pointerWords;
  }
  pointerWord(index) {
    if (index >= this.pointerWords) {
      throw new Error("pointer index outside struct pointer section");
    }
    return this.word + this.dataWords + index;
  }
  setUint16(fieldIndex, value) {
    this.msg.setUint16(this.word, fieldIndex, value);
  }
  setInt32(fieldIndex, value) {
    this.msg.setInt32(this.word, fieldIndex, value);
  }
  setUint32(fieldIndex, value) {
    this.msg.setUint32(this.word, fieldIndex, value);
  }
  setInt64(fieldIndex, value) {
    this.msg.setInt64(this.word, fieldIndex, value);
  }
  setUint64(fieldIndex, value) {
    this.msg.setUint64(this.word, fieldIndex, value);
  }
  setFloat32(fieldIndex, value) {
    this.msg.setFloat32(this.word, fieldIndex, value);
  }
  setFloat64(fieldIndex, value) {
    this.msg.setFloat64(this.word, fieldIndex, value);
  }
  setBool(bitIndex, value) {
    this.msg.setBool(this.word, bitIndex, value);
  }
  setText(pointerIndex, value) {
    this.msg.setText(this.pointerWord(pointerIndex), value);
  }
  setData(pointerIndex, value) {
    this.msg.setData(this.pointerWord(pointerIndex), value);
  }
  setCap(pointerIndex, index) {
    this.msg.writeCapPointer(this.pointerWord(pointerIndex), index);
  }
  setTextList(pointerIndex, values) {
    this.msg.setTextList(this.pointerWord(pointerIndex), values);
  }
  setDataList(pointerIndex, values) {
    this.msg.setDataList(this.pointerWord(pointerIndex), values);
  }
  setBoolList(pointerIndex, values) {
    this.msg.setBoolList(this.pointerWord(pointerIndex), values);
  }
  setUint16List(pointerIndex, values) {
    this.msg.setUintList(this.pointerWord(pointerIndex), values, 2);
  }
  setUint32List(pointerIndex, values) {
    this.msg.setUintList(this.pointerWord(pointerIndex), values, 4);
  }
  initStructList(pointerIndex, count, dataWords, pointerWords) {
    return this.msg.initStructList(this.pointerWord(pointerIndex), count, dataWords, pointerWords);
  }
  initStruct(pointerIndex, dataWords, pointerWords) {
    const ptrWord = this.pointerWord(pointerIndex);
    const off = this.msg.alloc(dataWords + pointerWords);
    this.msg.writeStructPointer(ptrWord, off, dataWords, pointerWords);
    return new _CapnpStruct(this.msg, off, dataWords, pointerWords);
  }
};
var CapnpReader = class {
  view;
  segOff;
  size0;
  constructor(bytes) {
    if (bytes.byteLength < WORD) {
      throw new Error("truncated Cap'n message");
    }
    this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const nsegMinus = this.view.getUint32(0, true);
    if (nsegMinus + 1 !== 1) {
      throw new Error("multi-segment Cap'n messages are not supported");
    }
    const size0 = this.view.getUint32(4, true);
    this.segOff = WORD;
    this.size0 = size0;
    if (this.segOff + size0 * WORD > bytes.byteLength) {
      throw new Error("truncated Cap'n segment");
    }
    if (size0 > MAX_TRAVERSAL_WORDS) {
      throw new Error("Cap'n segment exceeds traversal budget");
    }
  }
  root(dataWords, pointerWords) {
    return this.structAt(0, dataWords, pointerWords);
  }
  structAt(ptrWord, dataWords, pointerWords) {
    void dataWords;
    void pointerWords;
    const word = this.readWord(ptrWord);
    if (word === 0n) {
      return new StructReader(this, 0, 0, 0);
    }
    const a = Number(word & 3n);
    if (a === 2 || a === 3) {
      throw new Error("far pointers are not supported");
    }
    if (a !== 0) {
      throw new Error("expected struct pointer");
    }
    const offset = signExtend30(Number(word >> 2n & 0x3fffffffn));
    const dw = Number(word >> 32n & 0xffffn);
    const pw = Number(word >> 48n & 0xffffn);
    const target = ptrWord + 1 + offset;
    this.checkRange(target, dw + pw);
    return new StructReader(this, target, dw, pw);
  }
  readWord(word) {
    this.checkRange(word, 1);
    return this.view.getBigUint64(this.segOff + word * WORD, true);
  }
  checkRange(word, nWords) {
    if (word < 0 || nWords < 0 || word + nWords > this.size0) {
      throw new Error("Cap'n pointer out of segment");
    }
  }
  listPointer(ptrWord, expectSize) {
    const word = this.readWord(ptrWord);
    if (word === 0n) {
      return null;
    }
    const a = Number(word & 3n);
    if (a === 2 || a === 3) {
      throw new Error("far pointers are not supported");
    }
    if (a !== 1) {
      throw new Error("expected list pointer");
    }
    const offset = signExtend30(Number(word >> 2n & 0x3fffffffn));
    const c = Number(word >> 32n & 7n);
    const d = Number(word >> 35n);
    if (c !== expectSize) {
      throw new Error(`expected list element size ${expectSize}, found ${c}`);
    }
    return { target: ptrWord + 1 + offset, length: d };
  }
  getUint16(word, fieldIndex) {
    return this.view.getUint16(this.segOff + word * WORD + fieldIndex * 2, true);
  }
  getInt32(word, fieldIndex) {
    return this.view.getInt32(this.segOff + word * WORD + fieldIndex * 4, true);
  }
  getUint32(word, fieldIndex) {
    return this.view.getUint32(this.segOff + word * WORD + fieldIndex * 4, true);
  }
  getInt64(word, fieldIndex) {
    return this.view.getBigInt64(this.segOff + word * WORD + fieldIndex * 8, true);
  }
  getUint64(word, fieldIndex) {
    return this.view.getBigUint64(this.segOff + word * WORD + fieldIndex * 8, true);
  }
  getFloat32(word, fieldIndex) {
    return this.view.getFloat32(this.segOff + word * WORD + fieldIndex * 4, true);
  }
  getFloat64(word, fieldIndex) {
    return this.view.getFloat64(this.segOff + word * WORD + fieldIndex * 8, true);
  }
  getBool(word, bitIndex) {
    const byteOff = this.segOff + word * WORD + (bitIndex >> 3);
    return (this.view.getUint8(byteOff) & 1 << (bitIndex & 7)) !== 0;
  }
  readByteList(ptrWord) {
    const lp = this.listPointer(ptrWord, 2);
    if (lp === null) {
      return new Uint8Array(0);
    }
    const nWords = lp.length === 0 ? 0 : Math.ceil(lp.length / WORD);
    this.checkRange(lp.target, nWords);
    const start = this.segOff + lp.target * WORD;
    if (start + lp.length > this.view.byteLength) {
      throw new Error("truncated Cap'n byte list");
    }
    return new Uint8Array(this.view.buffer, this.view.byteOffset + start, lp.length);
  }
  readText(ptrWord) {
    const bytes = this.readByteList(ptrWord);
    const end = bytes.length > 0 && bytes[bytes.length - 1] === 0 ? bytes.length - 1 : bytes.length;
    return textDecoder.decode(bytes.subarray(0, end));
  }
  readCapIndex(ptrWord) {
    const word = this.readWord(ptrWord);
    if (word === 0n) {
      return null;
    }
    if ((word & 3n) !== 3n) {
      throw new Error("expected capability pointer");
    }
    return Number(word >> 32n);
  }
  pointerList(ptrWord) {
    const lp = this.listPointer(ptrWord, 6);
    if (lp === null || lp.length === 0) {
      return [];
    }
    if (lp.length > MAX_TRAVERSAL_WORDS) {
      throw new Error("pointer list exceeds traversal budget");
    }
    this.checkRange(lp.target, lp.length);
    const out = [];
    for (let i = 0; i < lp.length; i++) {
      out.push(lp.target + i);
    }
    return out;
  }
  readTextList(ptrWord) {
    return this.pointerList(ptrWord).map((w) => this.readText(w));
  }
  readDataList(ptrWord) {
    return this.pointerList(ptrWord).map((w) => this.readByteList(w));
  }
  /**
   * Reads a `List(UInt16)` / `List(UInt32)`.
   *
   * @param ptrWord - Word holding the list pointer.
   * @param byteWidth - `2` for UInt16 (enums), `4` for UInt32.
   * @returns Decoded unsigned integers.
   */
  readUintList(ptrWord, byteWidth) {
    const lp = this.listPointer(ptrWord, byteWidth === 2 ? 3 : 4);
    if (lp === null || lp.length === 0) {
      return [];
    }
    this.checkRange(lp.target, Math.ceil(lp.length * byteWidth / WORD));
    const base = this.segOff + lp.target * WORD;
    const out = [];
    for (let i = 0; i < lp.length; i++) {
      out.push(byteWidth === 2 ? this.view.getUint16(base + i * 2, true) : this.view.getUint32(base + i * 4, true));
    }
    return out;
  }
  readBoolList(ptrWord) {
    const lp = this.listPointer(ptrWord, 1);
    if (lp === null || lp.length === 0) {
      return [];
    }
    this.checkRange(lp.target, Math.ceil(lp.length / 64));
    const base = this.segOff + lp.target * WORD;
    const out = [];
    for (let i = 0; i < lp.length; i++) {
      out.push((this.view.getUint8(base + (i >> 3)) >> (i & 7) & 1) === 1);
    }
    return out;
  }
  readStructList(ptrWord, dataWords, pointerWords) {
    void dataWords;
    void pointerWords;
    const lp = this.listPointer(ptrWord, 7);
    if (lp === null || lp.length === 0) {
      return [];
    }
    const tagWord = lp.target;
    this.checkRange(tagWord, 1);
    const tag = this.readWord(tagWord);
    const count = Number(tag >> 2n & 0x3fffffffn);
    const dw = Number(tag >> 32n & 0xffffn);
    const pw = Number(tag >> 48n & 0xffffn);
    const elemWords = dw + pw;
    if (elemWords > 0 && count > Math.floor(lp.length / elemWords)) {
      throw new Error("composite list count exceeds payload");
    }
    if (count > MAX_TRAVERSAL_WORDS) {
      throw new Error("composite list count exceeds traversal budget");
    }
    this.checkRange(tagWord, 1 + count * elemWords);
    const out = [];
    for (let i = 0; i < count; i++) {
      out.push(new StructReader(this, tagWord + 1 + i * elemWords, dw, pw));
    }
    return out;
  }
};
function signExtend30(n) {
  const v = n & 1073741823;
  return v & 536870912 ? v - 1073741824 : v;
}
var StructReader = class _StructReader {
  reader;
  word;
  dataWords;
  pointerWords;
  constructor(reader, word, dataWords, pointerWords) {
    this.reader = reader;
    this.word = word;
    this.dataWords = dataWords;
    this.pointerWords = pointerWords;
  }
  hasData(fieldIndex, size) {
    return (fieldIndex + 1) * size <= this.dataWords * WORD;
  }
  pointerWord(index) {
    if (index >= this.pointerWords) {
      return null;
    }
    return this.word + this.dataWords + index;
  }
  getUint16(fieldIndex) {
    return this.hasData(fieldIndex, 2) ? this.reader.getUint16(this.word, fieldIndex) : 0;
  }
  getInt32(fieldIndex) {
    return this.hasData(fieldIndex, 4) ? this.reader.getInt32(this.word, fieldIndex) : 0;
  }
  getUint32(fieldIndex) {
    return this.hasData(fieldIndex, 4) ? this.reader.getUint32(this.word, fieldIndex) : 0;
  }
  getInt64(fieldIndex) {
    return this.hasData(fieldIndex, 8) ? this.reader.getInt64(this.word, fieldIndex) : 0n;
  }
  getUint64(fieldIndex) {
    return this.hasData(fieldIndex, 8) ? this.reader.getUint64(this.word, fieldIndex) : 0n;
  }
  getFloat32(fieldIndex) {
    return this.hasData(fieldIndex, 4) ? this.reader.getFloat32(this.word, fieldIndex) : 0;
  }
  getFloat64(fieldIndex) {
    return this.hasData(fieldIndex, 8) ? this.reader.getFloat64(this.word, fieldIndex) : 0;
  }
  getBool(bitIndex) {
    if (bitIndex >= this.dataWords * 64) {
      return false;
    }
    return this.reader.getBool(this.word, bitIndex);
  }
  getText(pointerIndex) {
    const ptr = this.pointerWord(pointerIndex);
    return ptr === null ? "" : this.reader.readText(ptr);
  }
  getData(pointerIndex) {
    const ptr = this.pointerWord(pointerIndex);
    return ptr === null ? new Uint8Array(0) : this.reader.readByteList(ptr);
  }
  getCapIndex(pointerIndex) {
    const ptr = this.pointerWord(pointerIndex);
    return ptr === null ? null : this.reader.readCapIndex(ptr);
  }
  getTextList(pointerIndex) {
    const ptr = this.pointerWord(pointerIndex);
    return ptr === null ? [] : this.reader.readTextList(ptr);
  }
  getDataList(pointerIndex) {
    const ptr = this.pointerWord(pointerIndex);
    return ptr === null ? [] : this.reader.readDataList(ptr);
  }
  getBoolList(pointerIndex) {
    const ptr = this.pointerWord(pointerIndex);
    return ptr === null ? [] : this.reader.readBoolList(ptr);
  }
  getUint16List(pointerIndex) {
    const ptr = this.pointerWord(pointerIndex);
    return ptr === null ? [] : this.reader.readUintList(ptr, 2);
  }
  getUint32List(pointerIndex) {
    const ptr = this.pointerWord(pointerIndex);
    return ptr === null ? [] : this.reader.readUintList(ptr, 4);
  }
  getStructList(pointerIndex, dataWords, pointerWords) {
    const ptr = this.pointerWord(pointerIndex);
    return ptr === null ? [] : this.reader.readStructList(ptr, dataWords, pointerWords);
  }
  getStruct(pointerIndex, dataWords, pointerWords) {
    const ptr = this.pointerWord(pointerIndex);
    if (ptr === null) {
      return new _StructReader(this.reader, 0, 0, 0);
    }
    return this.reader.structAt(ptr, dataWords, pointerWords);
  }
};

// dist/db-value.js
var KINDS = /* @__PURE__ */ new Set(["null", "boolean", "int64", "float64", "text", "bytes"]);
var TYPES = new Set(DB_TYPES);
function requirePortableText(text) {
  if (text.includes("\0")) {
    throw new Error("BookclerkSQL TEXT cannot contain U+0000 (use BYTES/BLOB for binary)");
  }
  return text;
}
var I64_MIN = -0x8000000000000000n;
var I64_MAX = 0x7fffffffffffffffn;
var DB_TYPE_FROM_ORD = DB_TYPES;
var DB_TYPE_ORD = Object.fromEntries(DB_TYPES.map((ty, ord) => [ty, ord]));
function parseDbValue(raw) {
  if (raw === null || typeof raw !== "object" || Array.isArray(raw)) {
    throw new Error("DbValue must be an object");
  }
  const obj = raw;
  if (typeof obj.kind !== "string" || !KINDS.has(obj.kind)) {
    throw new Error(`unknown DbValue union member: ${String(obj.kind)}`);
  }
  switch (obj.kind) {
    case "null":
      if (typeof obj.value !== "string" || !TYPES.has(obj.value)) {
        throw new Error("typed null requires a DbType");
      }
      return { kind: "null", value: obj.value };
    case "boolean":
      if (typeof obj.value !== "boolean") {
        throw new Error("boolean DbValue requires a boolean");
      }
      return { kind: "boolean", value: obj.value };
    case "int64":
      return { kind: "int64", value: parseInt64(obj.value) };
    case "float64":
      if (typeof obj.value !== "number" || !Number.isFinite(obj.value)) {
        throw new Error("float64 value is not finite");
      }
      return { kind: "float64", value: obj.value };
    case "text":
      if (typeof obj.value !== "string") {
        throw new Error("text DbValue requires a string");
      }
      requirePortableText(obj.value);
      return { kind: "text", value: obj.value };
    case "bytes":
      return { kind: "bytes", value: parseBytes(obj.value) };
    default:
      throw new Error(`unknown DbValue union member: ${obj.kind}`);
  }
}
function encodeDbValue(value) {
  const msg = new CapnpMessage();
  const root = msg.initRoot(2, 1);
  writeDbValue(root, value);
  return msg.finish();
}
function decodeDbValue(bytes) {
  const reader = new CapnpReader(bytes);
  return readDbValue(reader.root(2, 1));
}
function writeDbValue(root, value) {
  switch (value.kind) {
    case "null":
      root.setUint16(0, DB_TYPE_ORD[value.value]);
      root.setUint16(1, 0);
      return;
    case "boolean":
      root.setBool(0, value.value);
      root.setUint16(1, 1);
      return;
    case "int64":
      if (value.value < I64_MIN || value.value > I64_MAX) {
        throw new Error("int64 DbValue is out of range");
      }
      root.setInt64(1, value.value);
      root.setUint16(1, 2);
      return;
    case "float64":
      if (!Number.isFinite(value.value)) {
        throw new Error("float64 value is not finite");
      }
      root.setFloat64(1, value.value);
      root.setUint16(1, 3);
      return;
    case "text":
      requirePortableText(value.value);
      root.setUint16(1, 4);
      root.setText(0, value.value);
      return;
    case "bytes":
      root.setUint16(1, 5);
      root.setData(0, value.value);
      return;
    default: {
      const _exhaustive = value;
      throw new Error(`unknown DbValue union member: ${JSON.stringify(_exhaustive)}`);
    }
  }
}
function readDbValue(root) {
  const disc = root.getUint16(1);
  switch (disc) {
    case 0: {
      const ty = DB_TYPE_FROM_ORD[root.getUint16(0)];
      if (ty === void 0) {
        throw new Error("unknown DbType");
      }
      return { kind: "null", value: ty };
    }
    case 1:
      return { kind: "boolean", value: root.getBool(0) };
    case 2:
      return { kind: "int64", value: root.getInt64(1) };
    case 3: {
      const n = root.getFloat64(1);
      if (!Number.isFinite(n)) {
        throw new Error("float64 value is not finite");
      }
      return { kind: "float64", value: n };
    }
    case 4:
      return { kind: "text", value: requirePortableText(root.getText(0)) };
    case 5:
      return { kind: "bytes", value: root.getData(0) };
    default:
      throw new Error(`unknown DbValue union member: ${disc}`);
  }
}
function parseInt64(raw) {
  let n;
  if (typeof raw === "bigint") {
    n = raw;
  } else if (typeof raw === "number") {
    if (!Number.isInteger(raw) || !Number.isFinite(raw)) {
      throw new Error("int64 DbValue requires an integer");
    }
    n = BigInt(raw);
  } else if (typeof raw === "string") {
    if (!/^-?\d+$/.test(raw)) {
      throw new Error("int64 DbValue requires an integer");
    }
    n = BigInt(raw);
  } else {
    throw new Error("int64 DbValue requires an integer");
  }
  if (n < I64_MIN || n > I64_MAX) {
    throw new Error("int64 DbValue is out of range");
  }
  return n;
}
function parseBytes(raw) {
  if (raw instanceof Uint8Array) {
    return raw;
  }
  if (typeof raw !== "string") {
    throw new Error("bytes DbValue requires bytes");
  }
  if (!raw.startsWith("b64:")) {
    throw new Error("bytes DbValue requires bytes");
  }
  return decodeBase64(raw.slice(4));
}
function decodeBase64(b64) {
  const bin = globalThis.atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) {
    out[i] = bin.charCodeAt(i);
  }
  return out;
}

// dist/guest-sql.js
function identStart(c) {
  const code = c.charCodeAt(0);
  return code >= 65 && code <= 90 || code >= 97 && code <= 122 || c === "_";
}
function identCont(c) {
  return identStart(c) || c >= "0" && c <= "9";
}
function skipWsComments(sql, start) {
  let i = start;
  while (i < sql.length) {
    while (i < sql.length && /\s/.test(sql[i])) {
      i += 1;
    }
    if (sql[i] === "-" && sql[i + 1] === "-") {
      i += 2;
      while (i < sql.length && sql[i] !== "\n") {
        i += 1;
      }
      continue;
    }
    if (sql[i] === "/" && sql[i + 1] === "*") {
      i += 2;
      while (i + 1 < sql.length && !(sql[i] === "*" && sql[i + 1] === "/")) {
        i += 1;
      }
      i = Math.min(i + 2, sql.length);
      continue;
    }
    break;
  }
  return i;
}
function keywordAt(sql, i, kw) {
  if (i + kw.length > sql.length) {
    return false;
  }
  if (sql.slice(i, i + kw.length).toLowerCase() !== kw.toLowerCase()) {
    return false;
  }
  const beforeOk = i === 0 || !identCont(sql[i - 1]);
  const after = sql[i + kw.length] ?? " ";
  return beforeOk && !identCont(after);
}
function skipIdentOrQuoted(sql, i) {
  const c = sql[i];
  if (c === void 0) {
    return null;
  }
  if (c === '"' || c === "`" || c === "[") {
    const end = c === "[" ? "]" : c;
    let j = i + 1;
    while (j < sql.length) {
      if (sql[j] === end) {
        if (end !== "]" && sql[j + 1] === end) {
          j += 2;
          continue;
        }
        return j + 1;
      }
      j += 1;
    }
    return null;
  }
  if (identStart(c)) {
    let j = i + 1;
    while (j < sql.length && identCont(sql[j])) {
      j += 1;
    }
    return j;
  }
  return null;
}
function skipBalancedParens(sql, start) {
  if (sql[start] !== "(") {
    return null;
  }
  let depth = 0;
  let end = null;
  forEachUnquoted(sql.slice(start), (slice, idx) => {
    if (end !== null) {
      return 1;
    }
    const c = slice[idx];
    if (c === "(") {
      depth += 1;
    } else if (c === ")") {
      depth -= 1;
      if (depth === 0) {
        end = start + idx + 1;
      }
    }
    return 1;
  });
  return end;
}
function sqlAfterLeadingCtes(sql) {
  let i = skipWsComments(sql, 0);
  if (!keywordAt(sql, i, "WITH")) {
    return sql;
  }
  i += 4;
  i = skipWsComments(sql, i);
  if (keywordAt(sql, i, "RECURSIVE")) {
    i += 9;
    i = skipWsComments(sql, i);
  }
  for (; ; ) {
    const next = skipIdentOrQuoted(sql, i);
    if (next === null) {
      return sql;
    }
    i = skipWsComments(sql, next);
    if (sql[i] === "(") {
      const after = skipBalancedParens(sql, i);
      if (after === null) {
        return sql;
      }
      i = skipWsComments(sql, after);
    }
    if (!keywordAt(sql, i, "AS")) {
      return sql;
    }
    i = skipWsComments(sql, i + 2);
    if (keywordAt(sql, i, "NOT")) {
      const afterNot = skipWsComments(sql, i + 3);
      if (keywordAt(sql, afterNot, "MATERIALIZED")) {
        i = skipWsComments(sql, afterNot + 12);
      }
    } else if (keywordAt(sql, i, "MATERIALIZED")) {
      i = skipWsComments(sql, i + 12);
    }
    if (sql[i] !== "(") {
      return sql;
    }
    const afterBody = skipBalancedParens(sql, i);
    if (afterBody === null) {
      return sql;
    }
    i = skipWsComments(sql, afterBody);
    if (sql[i] === ",") {
      i = skipWsComments(sql, i + 1);
      continue;
    }
    return sql.slice(i);
  }
}
function forEachUnquoted(sql, step) {
  let i = 0;
  let inS = false;
  let inD = false;
  let inLine = false;
  let inBlock = false;
  while (i < sql.length) {
    const c = sql[i];
    if (inLine) {
      if (c === "\n") {
        inLine = false;
      }
      i += 1;
      continue;
    }
    if (inBlock) {
      if (c === "*" && sql[i + 1] === "/") {
        inBlock = false;
        i += 2;
        continue;
      }
      i += 1;
      continue;
    }
    if (inS) {
      if (c === "'") {
        if (sql[i + 1] === "'") {
          i += 2;
          continue;
        }
        inS = false;
      }
      i += 1;
      continue;
    }
    if (inD) {
      if (c === '"') {
        if (sql[i + 1] === '"') {
          i += 2;
          continue;
        }
        inD = false;
      }
      i += 1;
      continue;
    }
    if (c === "-" && sql[i + 1] === "-") {
      inLine = true;
      i += 2;
      continue;
    }
    if (c === "/" && sql[i + 1] === "*") {
      inBlock = true;
      i += 2;
      continue;
    }
    if (c === "'") {
      inS = true;
      i += 1;
      continue;
    }
    if (c === '"') {
      inD = true;
      i += 1;
      continue;
    }
    const n = Math.max(1, step(sql, i));
    i += n;
  }
}
function forEachTopLevelKeyword(sql, onKeyword) {
  let depth = 0;
  forEachUnquoted(sql, (slice, idx) => {
    const c = slice[idx];
    if (c === "(") {
      depth += 1;
      return 1;
    }
    if (c === ")") {
      depth = Math.max(0, depth - 1);
      return 1;
    }
    if (depth === 0 && identStart(c)) {
      let j = idx + 1;
      while (j < slice.length && identCont(slice[j])) {
        j += 1;
      }
      onKeyword(idx, slice.slice(idx, j));
      return j - idx;
    }
    return 1;
  });
}
function hasTopLevelKeyword(sql, keyword) {
  const want = keyword.toUpperCase();
  let found = false;
  forEachTopLevelKeyword(sql, (_, kw) => {
    if (kw.toUpperCase() === want) {
      found = true;
    }
  });
  return found;
}
function firstTopLevelKeyword(sql) {
  let first;
  forEachTopLevelKeyword(sql, (_, kw) => {
    if (first === void 0) {
      first = kw.toUpperCase();
    }
  });
  return first;
}
function guestStatementKind(sql) {
  const main = sqlAfterLeadingCtes(sql);
  if (hasTopLevelKeyword(main, "RETURNING")) {
    return "returning";
  }
  const verb = firstTopLevelKeyword(main);
  if (verb === "SELECT" || verb === "VALUES") {
    return "select";
  }
  return "execute";
}
function splitExecQueries(query) {
  return query.split("\n").map((line) => line.trim()).filter((line) => line.length > 0).map((line) => stripTrailingSemicolons(line).trim()).filter((line) => line.length > 0);
}
function stripTrailingSemicolons(line) {
  let end = line.length;
  while (end > 0 && line[end - 1] === ";") {
    end -= 1;
  }
  return line.slice(0, end);
}

// dist/db-execute.js
var KIND_FROM = DB_STATEMENT_KINDS;
var KIND_ORD = Object.fromEntries(DB_STATEMENT_KINDS.map((kind, ord) => [kind, ord]));
var SELECT_FROM = DB_RESULT_SELECTIONS;
var SELECT_ORD = Object.fromEntries(DB_RESULT_SELECTIONS.map((sel, ord) => [sel, ord]));
var COL_TYPE_FROM = DB_TYPES;
var COL_TYPE_ORD = Object.fromEntries(DB_TYPES.map((ty, ord) => [ty, ord]));
function encodeExecuteRequest(request) {
  if (request.statements.length === 0) {
    throw new Error("execute statements must be non-empty");
  }
  const msg = new CapnpMessage();
  const root = msg.initRoot(4, 3);
  root.setText(0, request.operationId);
  root.setText(1, request.requestHash);
  root.setUint64(3, BigInt(request.deadlineUnixMs));
  const stmts = root.initStructList(2, request.statements.length, 1, 2);
  for (let i = 0; i < request.statements.length; i++) {
    writeStatement(stmts[i], request.statements[i]);
  }
  return msg.finish();
}
function decodeExecuteRequest(bytes) {
  const reader = new CapnpReader(bytes);
  const root = reader.root(4, 3);
  const stmtStructs = root.getStructList(2, 1, 2);
  if (stmtStructs.length === 0) {
    throw new Error("execute statements must be non-empty");
  }
  return {
    operationId: root.getText(0),
    requestHash: root.getText(1),
    statements: stmtStructs.map(readStatement),
    deadlineUnixMs: Number(root.getUint64(3))
  };
}
function encodeExecuteResultReply(outcome) {
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
function decodeExecuteResultReply(bytes) {
  const root = new CapnpReader(bytes).root(1, 1);
  const disc = root.getUint16(0);
  if (disc === 0) {
    return readExecuteReply(root.getStruct(0, 0, 3));
  }
  if (disc === 1) {
    const err = root.getStruct(0, 0, 2);
    const code = err.getText(0);
    const message = err.getText(1);
    const known = /* @__PURE__ */ new Set([
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
      "conflict"
    ]);
    throw Object.assign(new Error(message), {
      name: "PluginError",
      code: known.has(code) ? code : "unknown",
      wireCode: code
    });
  }
  throw new Error("unknown ExecuteResultReply union member");
}
function statementResultToD1Result(stmt, timing) {
  const changes = stmt.rowsAffected;
  const durationMs = timing.dbExecutionUs / 1e3;
  const results = stmt.columns.length > 0 ? stmt.rows.map((row) => {
    const obj = {};
    for (let i = 0; i < stmt.columns.length; i++) {
      obj[stmt.columns[i].name] = row.values[i];
    }
    return obj;
  }) : null;
  return {
    success: true,
    results,
    meta: {
      duration: durationMs,
      changes,
      last_row_id: 0,
      changed_db: changes > 0,
      rows_read: stmt.rows.length,
      rows_written: changes
    }
  };
}
function executeReplyToD1Results(reply) {
  return reply.statements.map((stmt) => statementResultToD1Result(stmt, reply.timing));
}
function rowMapFromStatement(result) {
  if (result.rows.length === 0) {
    return null;
  }
  const row = {};
  for (let i = 0; i < result.columns.length; i++) {
    row[result.columns[i].name] = result.rows[0].values[i];
  }
  return row;
}
function columnValueFromRow(row, colName) {
  if (colName in row) {
    return row[colName];
  }
  const lower = colName.toLowerCase();
  for (const [name, value] of Object.entries(row)) {
    if (name.toLowerCase() === lower) {
      return value;
    }
  }
  throw new Error(`column ${colName} not found in first() result`);
}
function writeExecuteReply(root, reply) {
  root.setText(0, reply.operationId);
  const stmts = root.initStructList(1, reply.statements.length, 1, 2);
  for (let i = 0; i < reply.statements.length; i++) {
    writeStatementResult(stmts[i], reply.statements[i]);
  }
  const timing = root.initStruct(2, 2, 1);
  timing.setUint64(0, BigInt(reply.timing.attemptElapsedUs));
  timing.setUint64(1, BigInt(reply.timing.dbExecutionUs));
  timing.setText(0, reply.timing.dbTimingSource);
}
function readExecuteReply(root) {
  return {
    operationId: root.getText(0),
    statements: root.getStructList(1, 1, 2).map(readStatementResult),
    timing: (() => {
      const t = root.getStruct(2, 2, 1);
      return {
        attemptElapsedUs: Number(t.getUint64(0)),
        dbExecutionUs: Number(t.getUint64(1)),
        dbTimingSource: t.getText(0)
      };
    })()
  };
}
function writeStatementResult(s, stmt) {
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
}
function readStatementResult(s) {
  const columns = s.getStructList(1, 1, 1).map((c) => {
    const ty = COL_TYPE_FROM[c.getUint16(0)];
    if (ty === void 0) {
      throw new Error("unknown DbType");
    }
    return { name: c.getText(0), dbType: ty };
  });
  const rows = s.getStructList(0, 0, 1).map((row) => ({
    values: row.getStructList(0, 2, 1).map(readDbValue)
  }));
  return {
    rows,
    columns,
    rowsAffected: Number(s.getUint64(0))
  };
}
function createDatabaseBinding(transport, options = {}) {
  const runExecute = async (batch, retry) => {
    if (!Array.isArray(batch) || batch.length === 0) {
      throw new Error("execute statements must be non-empty");
    }
    const request = await executeRequestFromBatch(batch, options, retry);
    const encoded = encodeExecuteRequest(request);
    const cap = options.maxRequestBytes ?? 0;
    if (cap > 0 && encoded.byteLength > cap) {
      throw new Error(`atomic request is ${encoded.byteLength} bytes; guest maxRequestBytes is ${cap}`);
    }
    return transport.execute(request);
  };
  const binding = {
    prepare(sql) {
      return makePrepared(binding, sql, [], options.maxResultRows ?? 0, defaultIntent(options));
    },
    batch(statements, opts) {
      const typed = statements.map((s) => s._asTyped());
      return runExecute(typed, opts?.retry).then(executeReplyToD1Results);
    },
    exec(query, opts) {
      const queries = splitExecQueries(query);
      if (queries.length === 0) {
        return Promise.reject(new Error("exec query is empty"));
      }
      const prepared = queries.map((sql) => binding.prepare(sql));
      return binding.batch(prepared, opts).then((results) => ({
        count: results.length,
        duration: results.reduce((sum, r) => sum + r.meta.duration, 0)
      }));
    },
    execute(batch, opts) {
      return runExecute(batch, opts?.retry);
    }
  };
  return binding;
}
function defaultIntent(options) {
  return {
    resultSelection: "rows",
    maxRows: options.maxResultRows ?? 0
  };
}
function makePrepared(binding, sql, parameters, defaultAllRows, intent) {
  const stmt = {
    _intent: intent,
    bind(...values) {
      return makePrepared(binding, sql, values, defaultAllRows, intent);
    },
    asRun() {
      return makePrepared(binding, sql, parameters, defaultAllRows, {
        resultSelection: "affectedRows",
        maxRows: 0
      });
    },
    asFirst() {
      return makePrepared(binding, sql, parameters, defaultAllRows, {
        resultSelection: "rows",
        maxRows: 1
      });
    },
    asAll() {
      return makePrepared(binding, sql, parameters, defaultAllRows, {
        resultSelection: "rows",
        maxRows: defaultAllRows
      });
    },
    run(options) {
      return binding.execute([this.asAll()._asTyped()], options).then((reply) => statementResultToD1Result(reply.statements[0], reply.timing));
    },
    first(colName, options) {
      return binding.execute([this.asFirst()._asTyped()], options).then((reply) => {
        const result = reply.statements[0];
        if (!result) {
          return null;
        }
        const row = rowMapFromStatement(result);
        if (row === null) {
          return null;
        }
        if (colName !== void 0) {
          return columnValueFromRow(row, colName);
        }
        return row;
      });
    },
    raw(options) {
      return binding.execute([this.asAll()._asTyped()], options).then((reply) => {
        const result = reply.statements[0];
        if (!result) {
          return [];
        }
        return result.rows.map((row) => row.values);
      });
    },
    all(options) {
      return binding.execute([this.asAll()._asTyped()], options).then((reply) => statementResultToD1Result(reply.statements[0], reply.timing));
    },
    _asTyped() {
      const used = intent ?? defaultIntent({ maxResultRows: defaultAllRows });
      return {
        sql,
        parameters,
        kind: used.resultSelection === "affectedRows" || used.resultSelection === "discard" ? "execute" : guestStatementKind(sql),
        maxRows: used.maxRows,
        resultSelection: used.resultSelection
      };
    }
  };
  return stmt;
}
async function executeRequestFromBatch(batch, options, retry) {
  const token = retry ?? options.retry;
  return {
    operationId: token?.operationId ?? options.operationId ?? newOperationId(),
    requestHash: token?.requestHash ?? options.requestHash ?? "",
    statements: batch,
    deadlineUnixMs: options.deadlineUnixMs ?? 0
  };
}
async function canonicalExecuteRequestHash(request) {
  const canonical = {
    ...request,
    operationId: "",
    requestHash: "",
    deadlineUnixMs: 0
  };
  const bytes = encodeExecuteRequest(canonical);
  if (globalThis.crypto?.subtle) {
    const digest = await globalThis.crypto.subtle.digest("SHA-256", bytes);
    return hexBytes(new Uint8Array(digest));
  }
  const { createHash } = await import("node:crypto");
  return createHash("sha256").update(bytes).digest("hex");
}
function hexBytes(bytes) {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}
function newOperationId() {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return globalThis.crypto.randomUUID();
  }
  return `op-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}
function writeStatement(s, stmt) {
  s.setText(0, stmt.sql);
  s.setUint16(0, KIND_ORD[stmt.kind]);
  s.setUint16(1, SELECT_ORD[stmt.resultSelection]);
  s.setUint32(1, stmt.maxRows);
  const params = s.initStructList(1, stmt.parameters.length, 2, 1);
  for (let i = 0; i < stmt.parameters.length; i++) {
    writeDbValue(params[i], stmt.parameters[i]);
  }
}
function readStatement(s) {
  const kind = KIND_FROM[s.getUint16(0)];
  const selectionRaw = s.getUint16(1);
  const selection = SELECT_FROM[selectionRaw] ?? "rows";
  if (kind === void 0) {
    throw new Error("unknown DbStatementKind");
  }
  return {
    sql: s.getText(0),
    parameters: s.getStructList(1, 2, 1).map(readDbValue),
    kind,
    maxRows: s.getUint32(1),
    resultSelection: selection
  };
}
export {
  AdapterDatabaseSession,
  BookclerkEntrypoint,
  CliEntrypoint,
  DatabaseAdapterEntrypoint,
  Destination,
  ENTRYPOINT_BINDINGS,
  ENTRYPOINT_CLASSES,
  EventBatch,
  EventMessage,
  FEATURE_SCALAR_LIMITS,
  FEATURE_STORAGE_COPY,
  FEATURE_STREAMS,
  JobController,
  MAX_CHECKPOINT_BYTES,
  MAX_EVENT_PAYLOAD_BYTES,
  MAX_LIST_PAGE,
  MAX_PLUGIN_MIGRATION_OPS,
  MAX_PLUGIN_MIGRATION_REGISTRATION_BYTES,
  MAX_PLUGIN_MIGRATION_TOTAL_OPS,
  MAX_SCALAR_BYTES,
  MAX_STREAM_WINDOW_BYTES,
  NamedEntrypoint,
  OidcEntrypoint,
  PRODUCT_API_VERSION,
  PluginError,
  ProgressSink,
  RemoteLibraryEntrypoint,
  Source,
  StorageEntrypoint,
  StorefrontEntrypoint,
  TRIGGER_FAMILIES,
  allowedEntrypointFamilies,
  canonicalExecuteRequestHash,
  cliArgs,
  createDatabaseBinding,
  dataMigrationOp,
  decodeDbValue,
  decodeExecuteRequest,
  decodeExecuteResultReply,
  decodeExtensibleConfig,
  encodeDbValue,
  encodeExecuteRequest,
  encodeExecuteResultReply,
  eventBatchResults,
  executeReplyToD1Results,
  invocationOf,
  jobOutcomeFor,
  jsonPayload,
  parseDbValue,
  requirePluginMigrationRegistration,
  schemaMigrationOp,
  statementResultToD1Result,
  toBridgeJson,
  wrapPluginFromBinding,
  wrapPluginFromNative
};
