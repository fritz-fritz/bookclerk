/**
 * TypeScript class ABI for `apiVersion` 2 (object-capability Workers RPC).
 *
 * Authors subclass {@link BookclerkPluginV2} and return {@link Destination} /
 * {@link Source} / {@link JobHandler} RpcTargets. Byte payloads move as
 * `ReadableStream` — never as base64 scalars, `handleId`, or `writeChunk`.
 */

import "./cloudflare-workers.d.ts";
import { WorkerEntrypoint, RpcTarget } from "cloudflare:workers";

/** Product ABI version (`plugin.toml` `api_version` and `describe().apiVersion`). */
export const PRODUCT_API_VERSION = 2 as const;

/** Maximum decoded size of an ordinary RPC scalar value (not a stream window). */
export const MAX_SCALAR_BYTES = 262_144;

/** Maximum bytes returned by one stream pull window. */
export const MAX_STREAM_WINDOW_BYTES = 1_048_576;

/** Maximum objects in one `Destination.list` page. */
export const MAX_LIST_PAGE = 256;

/** Guest honors scalar / stream-window / list-page caps. */
export const FEATURE_SCALAR_LIMITS = "rpc.scalarLimits";

/** Media moves through transferred ReadableStream / ByteSource streams. */
export const FEATURE_STREAMS = "rpc.streams";

/** Guest implements server-side {@link Destination.copy}. */
export const FEATURE_STORAGE_COPY = "storage.copy";

/** Negotiated numeric limits advertised at {@link PluginDescribe}. */
export interface ScalarLimits {
  maxScalarBytes: number;
  maxStreamWindowBytes: number;
  maxListPage: number;
}

/** Guest identity returned by `BookclerkPlugin.describe`. */
export interface PluginDescribe {
  apiVersion: typeof PRODUCT_API_VERSION | 2;
  id: string;
  kind: string;
  displayName?: string;
  rpcFeatures: string[];
  scalarLimits: ScalarLimits;
}

/** Injected destination knobs. Opaque JSON only — no OS paths. */
export interface DestinationContext {
  json?: string;
}

/** Injected source knobs. Opaque JSON only — no OS paths. */
export interface SourceContext {
  json?: string;
}

/** Job worker instantiation knobs. Opaque JSON only — no OS paths. */
export interface WorkerContext {
  jobId?: string;
  json?: string;
}

/** Current envelope schema version for {@link JobInvocation}. */
export const ENVELOPE_VERSION = 1 as const;

/** Maximum checkpoint payload size (bytes). */
export const MAX_CHECKPOINT_BYTES = 65_536;

/** Bounded, versioned checkpoint. */
export interface JobCheckpoint {
  schemaVersion: number;
  json?: string;
}

/**
 * Versioned durable command envelope (not a domain event).
 *
 * Idempotency keys are scoped to `(account, plugin, commandType)` until a
 * terminal fenced outcome is committed. `deadlineUnixMs` is a guest hint; the
 * host fence/lease is authoritative.
 */
export interface JobInvocation {
  envelopeVersion: number;
  payloadSchemaVersion: number;
  invocationId: string;
  commandType: string;
  payloadJson?: string;
  idempotencyKey: string;
  attempt: number;
  correlationId?: string;
  causationId?: string;
  deadlineUnixMs: number;
  checkpoint?: JobCheckpoint;
}

/** Handler completion. Suspension is durable only after a fenced commit. */
export type JobOutcome =
  | { kind: "completed"; message?: string; bytesCopied?: number }
  | { kind: "retryable"; message?: string; retryAfterUnixMs?: number }
  | { kind: "rejected"; message?: string }
  | { kind: "cancelled"; message?: string }
  | {
      kind: "suspended";
      checkpoint: JobCheckpoint;
      wakeAtUnixMs: number;
    };

const KNOWN_ERROR_CODES = new Set([
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

/** Thrown by the SDK when a wire union carries `err`. Unknown codes are kept. */
export class PluginError extends Error {
  readonly code: string;
  readonly wireCode: string;
  constructor(code: string, message: string) {
    super(message);
    this.name = "PluginError";
    this.wireCode = code;
    this.code = KNOWN_ERROR_CODES.has(code) ? code : "unknown";
  }

  static fromWire(code: string, message: string): PluginError {
    return new PluginError(code, message);
  }
}

/** Author-visible bindings. Adapter-private tokens are not present. */
export interface BookclerkPluginEnv {
  HTTP?: { fetch: typeof fetch };
  STORAGE?: unknown;
  SECRETS?: unknown;
  OAUTH?: unknown;
}

/** First-party wrapper env. Authors never see this type on their class. */
export interface AdapterEnv {
  PLUGIN_BACKEND?: unknown;
  GRANTED?: { fetch: (input: string, init?: RequestInit) => Promise<Response> };
  BRIDGE_TOKEN?: string;
}

/** Object listing entry. */
export interface ObjectInfo {
  key: string;
  size: number;
}

/** Metadata without a body. */
export interface ObjectMetadata {
  key: string;
  size: number;
  contentType?: string;
  etag?: string;
  sha256?: Uint8Array;
}

/** Paginated list request. */
export interface ListOptions {
  prefix?: string;
  cursor?: string;
  limit?: number;
}

/** One page of keys. */
export interface ListPage {
  objects: ObjectInfo[];
  nextCursor?: string;
}

/** Optional byte range for a streamed read. */
export interface ByteRange {
  offset: number;
  length?: number;
}

/** Read options for {@link Destination.get}. */
export interface ReadOptions {
  range?: ByteRange;
}

/** Write options for {@link Destination.put}. */
export interface WriteOptions {
  contentType?: string;
  contentLength?: number;
  sha256?: Uint8Array;
}

/** Streamed read result. `body` is a transferred ReadableStream. */
export interface ReadResult {
  meta: ObjectMetadata;
  body: ReadableStream<Uint8Array>;
}

/** Result of a streamed put. */
export interface PutResult {
  key: string;
  bytesWritten: number;
  etag?: string;
  sha256?: Uint8Array;
}

/** Result of a server-side copy. */
export interface CopyResult {
  bytesCopied: number;
}

/** Granted stubs for one {@link JobHandler.handle} invocation. */
export interface JobContext {
  input: Source;
  output: Destination;
  progress: ProgressSink;
  signal?: AbortSignal;
}

/**
 * Destination capability (storage). The runtime stub *is* the capability.
 */
export class Destination extends RpcTarget {
  /**
   * Metadata without a body; `null` when the key is missing.
   *
   * @param _key - Object key.
   * @returns Metadata or `null` when the key is missing.
   */
  head(_key: string): Promise<ObjectMetadata | null> {
    return Promise.reject(unsupported("head"));
  }

  /**
   * One page of keys under `options.prefix`.
   *
   * @param _options - Prefix, cursor, and limit.
   * @returns One page of object keys.
   */
  list(_options: ListOptions): Promise<ListPage> {
    return Promise.reject(unsupported("list"));
  }

  /**
   * Streamed read. The body is a transferred stream, not a scalar.
   *
   * @param _key - Object key.
   * @param _options - Optional byte range.
   * @returns Metadata plus a transferred body stream.
   */
  get(_key: string, _options?: ReadOptions): Promise<ReadResult> {
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
  put(
    _key: string,
    _body: ReadableStream<Uint8Array>,
    _options?: WriteOptions,
  ): Promise<PutResult> {
    return Promise.reject(unsupported("put"));
  }

  /**
   * Server-side copy when the backend supports it.
   *
   * @param _from - Source key.
   * @param _to - Destination key.
   * @returns Bytes copied.
   */
  copy?(_from: string, _to: string): Promise<CopyResult> {
    return Promise.reject(unsupported("copy"));
  }

  /**
   * Delete a key (no-op if missing).
   *
   * @param _key - Object key.
   * @returns Resolves when the delete is complete.
   */
  delete(_key: string): Promise<void> {
    return Promise.reject(unsupported("delete"));
  }
}

/**
 * Source capability that can open a named object as a stream.
 */
export class Source extends RpcTarget {
  /**
   * Opens `key` for streamed reading.
   *
   * @param _key - Object key.
   * @returns Metadata plus a transferred body stream.
   */
  open(_key: string): Promise<ReadResult> {
    return Promise.reject(unsupported("open"));
  }
}

/**
 * Progress reports for a job invocation (never carries media).
 */
export class ProgressSink extends RpcTarget {
  /**
   * Reports `percent` in `0..=100` and an operator-facing `message`.
   *
   * @param _percent - Completion percent.
   * @param _message - Operator-facing status.
   * @returns Resolves when the host records progress.
   */
  report(_percent: number, _message: string): Promise<void> {
    return Promise.reject(unsupported("report"));
  }
}

/**
 * Plugin worker that handles one durable job invocation.
 */
export class JobHandler extends RpcTarget {
  /**
   * Runs `event` using granted capabilities until completion or cancel.
   *
   * @param _event - Typed domain event (no media bytes).
   * @param _context - Granted source, destination, and progress stubs.
   * @returns Job outcome.
   */
  handle(_invocation: JobInvocation, _context: JobContext): Promise<JobOutcome> {
    return Promise.reject(unsupported("handle"));
  }
}

function v2GrantedContext(env: AdapterEnv, grantToken: string): JobContext {
  const granted = env.GRANTED;
  if (!granted || typeof grantToken !== "string" || !grantToken) {
    throw PluginError.fromWire("internal", "granted reverse channel missing");
  }
  const auth = { Authorization: `Bearer ${grantToken}` };
  const controller = new AbortController();
  return {
    input: {
      async open(key: string) {
        const resp = await granted.fetch(
          `http://granted/open?key=${encodeURIComponent(key)}`,
          { headers: auth, signal: controller.signal },
        );
        if (!resp.ok) {
          throw PluginError.fromWire("internal", await resp.text());
        }
        return {
          meta: {
            key: resp.headers.get("x-bookclerk-key") || key,
            size: Number(resp.headers.get("x-bookclerk-size") || "0"),
            contentType: resp.headers.get("x-bookclerk-content-type") || undefined,
            etag: resp.headers.get("x-bookclerk-etag") || undefined,
          },
          body: resp.body as ReadableStream<Uint8Array>,
        };
      },
    } as Source,
    output: {
      async put(
        key: string,
        body: ReadableStream<Uint8Array>,
        options?: WriteOptions,
      ) {
        const headers: Record<string, string> = { ...auth };
        if (options?.contentType) headers["content-type"] = options.contentType;
        if (options?.contentLength != null) {
          headers["content-length"] = String(options.contentLength);
        }
        const resp = await granted.fetch(
          `http://granted/put?key=${encodeURIComponent(key)}`,
          { method: "PUT", headers, body, signal: controller.signal },
        );
        if (!resp.ok) {
          throw PluginError.fromWire("internal", await resp.text());
        }
        return resp.json();
      },
    } as Destination,
    progress: {
      async report(percent: number, message?: string) {
        await granted.fetch(`http://granted/progress`, {
          method: "POST",
          headers: { ...auth, "content-type": "application/json" },
          body: JSON.stringify({ percent, message: message || "" }),
          signal: controller.signal,
        });
      },
    } as ProgressSink,
    signal: controller.signal,
  };
}

/**
 * Product `apiVersion` 2 guest base — `describe` / role factories / shutdown.
 *
 * Authors may `extend WorkerEntrypoint<BookclerkPluginEnv>`. Adapter-private
 * `GRANTED` / `BRIDGE_TOKEN` / `__v2*` bookkeeping lives on
 * {@link wrapV2Plugin}.
 */
export abstract class BookclerkPluginV2 extends WorkerEntrypoint<BookclerkPluginEnv> {
  /**
   * Rejects HTTP fetch — workerd guests are Workers-RPC only.
   *
   * @returns Always a 404 empty response.
   */
  async fetch(): Promise<Response> {
    return new Response(null, { status: 404 });
  }

  /** Advertises identity, features, and scalar limits. */
  abstract describe(): Promise<PluginDescribe>;

  /** Returns a destination capability for this invocation. */
  destination(_context: DestinationContext): Destination | Promise<Destination> {
    throw unsupported("destination");
  }

  /** Returns a source capability for this invocation. */
  source(_context: SourceContext): Source | Promise<Source> {
    throw unsupported("source");
  }

  /** Returns a job handler for this invocation. */
  worker(_context: WorkerContext): JobHandler | Promise<JobHandler> {
    throw unsupported("worker");
  }

  /** Releases guest resources. */
  async shutdown(): Promise<void> {}
}

type AuthorCtor = new (
  ctx: ExecutionContext,
  env: BookclerkPluginEnv,
) => BookclerkPluginV2;

/**
 * First-party wrapper: owns dest/source/handler maps, per-invocation grants,
 * and adapter env. Export `wrapV2Plugin(MyPlugin)` as the workerd default.
 *
 * @param Author - Author class extending {@link BookclerkPluginV2}.
 * @returns Wrapper entrypoint class bound to {@link AdapterEnv}.
 */
export function wrapV2Plugin(Author: AuthorCtor) {
  const dests = new Map<string, Destination>();
  const sources = new Map<string, Source>();
  const handlers = new Map<string, JobHandler>();
  let seq = 0;
  const next = (prefix: string) => {
    seq += 1;
    return `${prefix}${seq}`;
  };

  return class V2Wrapper extends WorkerEntrypoint<AdapterEnv> {
    #author: BookclerkPluginV2;

    constructor(ctx: ExecutionContext, env: AdapterEnv) {
      super(ctx, env);
      const authorEnv: BookclerkPluginEnv = { ...env };
      delete (authorEnv as AdapterEnv).GRANTED;
      delete (authorEnv as AdapterEnv).BRIDGE_TOKEN;
      delete (authorEnv as AdapterEnv).PLUGIN_BACKEND;
      this.#author = new Author(ctx, authorEnv);
    }

    async fetch(request: Request): Promise<Response> {
      const url = new URL(request.url);
      try {
        if (url.pathname === "/__v2/put" && request.method === "PUT") {
          const dest = dests.get(url.searchParams.get("id") ?? "");
          if (!dest) {
            throw PluginError.fromWire("not_found", "destination stub expired");
          }
          const key = url.searchParams.get("key") || "";
          const lenHeader = request.headers.get("content-length");
          const result = await dest.put(key, request.body as ReadableStream<Uint8Array>, {
            contentType: request.headers.get("content-type") || undefined,
            contentLength: lenHeader != null ? Number(lenHeader) : undefined,
          });
          return Response.json(result);
        }
        if (url.pathname === "/__v2/get" && request.method === "GET") {
          const dest = dests.get(url.searchParams.get("id") ?? "");
          if (!dest) {
            throw PluginError.fromWire("not_found", "destination stub expired");
          }
          const key = url.searchParams.get("key") || "";
          const offset = url.searchParams.get("offset");
          const length = url.searchParams.get("length");
          const options =
            offset != null
              ? {
                  range: {
                    offset: Number(offset),
                    length: length != null ? Number(length) : undefined,
                  },
                }
              : undefined;
          const result = await dest.get(key, options);
          return new Response(result.body, {
            headers: {
              "x-bookclerk-key": result.meta.key,
              "x-bookclerk-size": String(result.meta.size),
            },
          });
        }
        if (url.pathname === "/__v2/open" && request.method === "GET") {
          const src = sources.get(url.searchParams.get("id") ?? "");
          if (!src) {
            throw PluginError.fromWire("not_found", "source stub expired");
          }
          const result = await src.open(url.searchParams.get("key") || "");
          return new Response(result.body, {
            headers: {
              "x-bookclerk-key": result.meta.key,
              "x-bookclerk-size": String(result.meta.size),
            },
          });
        }
      } catch (err) {
        const pe =
          err instanceof PluginError
            ? err
            : PluginError.fromWire(
                "internal",
                err instanceof Error ? err.message : String(err),
              );
        return Response.json(
          { error: { code: pe.wireCode, message: pe.message } },
          { status: 200 },
        );
      }
      return this.#author.fetch(request);
    }

    describe(): Promise<PluginDescribe> {
      return this.#author.describe();
    }

    async shutdown(): Promise<void> {
      dests.clear();
      sources.clear();
      handlers.clear();
      await this.#author.shutdown();
    }

    async __v2CreateDestination(ctx: DestinationContext): Promise<{ id: string }> {
      const dest = await this.#author.destination(ctx ?? {});
      const id = next("d");
      dests.set(id, dest);
      return { id };
    }

    async __v2CreateSource(ctx: SourceContext): Promise<{ id: string }> {
      const src = await this.#author.source(ctx ?? {});
      const id = next("s");
      sources.set(id, src);
      return { id };
    }

    async __v2CreateWorker(ctx: WorkerContext): Promise<{ id: string }> {
      const handler = await this.#author.worker(ctx ?? {});
      const id = next("h");
      handlers.set(id, handler);
      return { id };
    }

    async __v2DestHead(id: string, key: string) {
      const dest = dests.get(id);
      if (!dest) throw PluginError.fromWire("not_found", "destination stub expired");
      const meta = await dest.head(key);
      return { found: meta != null, meta: meta ?? null };
    }

    async __v2DestList(id: string, options: ListOptions) {
      const dest = dests.get(id);
      if (!dest) throw PluginError.fromWire("not_found", "destination stub expired");
      return dest.list(options ?? {});
    }

    async __v2DestGet(id: string, key: string, options?: ReadOptions) {
      const dest = dests.get(id);
      if (!dest) throw PluginError.fromWire("not_found", "destination stub expired");
      return dest.get(key, options);
    }

    async __v2DestPut(
      id: string,
      key: string,
      body: ReadableStream<Uint8Array>,
      options?: WriteOptions,
    ) {
      const dest = dests.get(id);
      if (!dest) throw PluginError.fromWire("not_found", "destination stub expired");
      return dest.put(key, body, options);
    }

    async __v2DestCopy(id: string, from: string, to: string) {
      const dest = dests.get(id);
      if (!dest) throw PluginError.fromWire("not_found", "destination stub expired");
      return dest.copy?.(from, to);
    }

    async __v2DestDelete(id: string, key: string) {
      const dest = dests.get(id);
      if (!dest) throw PluginError.fromWire("not_found", "destination stub expired");
      await dest.delete(key);
      return { ok: true };
    }

    async __v2SourceOpen(id: string, key: string) {
      const src = sources.get(id);
      if (!src) throw PluginError.fromWire("not_found", "source stub expired");
      return src.open(key);
    }

    async __v2Handle(
      id: string,
      invocation: JobInvocation,
      grantToken: string,
    ): Promise<JobOutcome> {
      const handler = handlers.get(id);
      if (!handler) {
        throw PluginError.fromWire("not_found", "job handler stub expired");
      }
      try {
        const context = v2GrantedContext(this.env, grantToken);
        return await handler.handle(invocation, context);
      } finally {
        handlers.delete(id);
      }
    }
  };
}

function unsupported(method: string): Error {
  return PluginError.fromWire("unsupported", `${method} not implemented`);
}
