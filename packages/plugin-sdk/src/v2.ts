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

/** Injected destination knobs. */
export interface DestinationContext {
  pluginDataDir: string;
  json?: string;
}

/** Injected source knobs. */
export interface SourceContext {
  pluginDataDir: string;
  json?: string;
}

/** Job worker instantiation knobs. */
export interface WorkerContext {
  jobId: string;
  pluginDataDir: string;
  json?: string;
}

/** Typed domain job event (bytes never belong here). */
export interface JobEvent {
  eventType: string;
  json?: string;
}

/** Handler completion. */
export interface JobOutcome {
  ok: boolean;
  message?: string;
  bytesCopied?: number;
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
   */
  head(_key: string): Promise<ObjectMetadata | null> {
    return Promise.reject(unsupported("head"));
  }

  /**
   * One page of keys under `options.prefix`.
   *
   * @param _options - Prefix, cursor, and limit.
   */
  list(_options: ListOptions): Promise<ListPage> {
    return Promise.reject(unsupported("list"));
  }

  /**
   * Streamed read. The body is a transferred stream, not a scalar.
   *
   * @param _key - Object key.
   * @param _options - Optional byte range.
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
   */
  copy?(_from: string, _to: string): Promise<CopyResult> {
    return Promise.reject(unsupported("copy"));
  }

  /**
   * Delete a key (no-op if missing).
   *
   * @param _key - Object key.
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
   */
  handle(_event: JobEvent, _context: JobContext): Promise<JobOutcome> {
    return Promise.reject(unsupported("handle"));
  }
}

type V2Env = {
  GRANTED?: { fetch: (input: string, init?: RequestInit) => Promise<Response> };
  BRIDGE_TOKEN?: string;
};

/**
 * Adapter-private granted stubs: the plugin isolate fetches the host reverse
 * channel. Bridge RpcTargets cannot call back into the bridge while `handle`
 * HTTP is still open.
 */
function v2GrantedContext(env: V2Env, invocationId: string): JobContext {
  const granted = env.GRANTED;
  const token = env.BRIDGE_TOKEN;
  if (!granted || typeof token !== "string" || !token) {
    throw unsupported("granted reverse channel missing");
  }
  const auth = { Authorization: `Bearer ${token}` };
  return {
    input: {
      async open(key: string) {
        const resp = await granted.fetch(
          `http://granted/open?invocation=${encodeURIComponent(invocationId)}&key=${encodeURIComponent(key)}`,
          { headers: auth },
        );
        if (!resp.ok) {
          throw Object.assign(new Error(await resp.text()), { code: "internal" });
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
          `http://granted/put?invocation=${encodeURIComponent(invocationId)}&key=${encodeURIComponent(key)}`,
          { method: "PUT", headers, body },
        );
        if (!resp.ok) {
          throw Object.assign(new Error(await resp.text()), { code: "internal" });
        }
        return resp.json();
      },
    } as Destination,
    progress: {
      async report(percent: number, message?: string) {
        await granted.fetch(
          `http://granted/progress?invocation=${encodeURIComponent(invocationId)}`,
          {
            method: "POST",
            headers: { ...auth, "content-type": "application/json" },
            body: JSON.stringify({ percent, message: message || "" }),
          },
        );
      },
    } as ProgressSink,
  };
}

/**
 * Product `apiVersion` 2 guest base — `describe` / role factories / shutdown.
 *
 * Abort is stream cancel / RpcTarget disposal. Authors never see `handleId`,
 * `readChunk`, `writeChunk`, `finalize`, or `abort` as ABI methods.
 */
export abstract class BookclerkPluginV2 extends WorkerEntrypoint<V2Env> {
  /**
   * Rejects HTTP fetch — workerd guests are Workers-RPC only.
   *
   * @returns Always a 404 empty response.
   */
  async fetch(): Promise<Response> {
    return new Response(null, { status: 404 });
  }

  /**
   * Advertises identity, features, and scalar limits.
   */
  abstract describe(): Promise<PluginDescribe>;

  /**
   * Returns a destination capability for this invocation.
   *
   * @param _context - Data dir and opaque JSON knobs.
   */
  destination(_context: DestinationContext): Destination | Promise<Destination> {
    throw unsupported("destination");
  }

  /**
   * Returns a source capability for this invocation.
   *
   * @param _context - Data dir and opaque JSON knobs.
   */
  source(_context: SourceContext): Source | Promise<Source> {
    throw unsupported("source");
  }

  /**
   * Returns a job handler for this invocation.
   *
   * @param _context - Durable job id and data dir.
   */
  worker(_context: WorkerContext): JobHandler | Promise<JobHandler> {
    throw unsupported("worker");
  }

  /**
   * Releases guest resources.
   */
  async shutdown(): Promise<void> {}

  /**
   * HTTP-adapter: store a destination in this isolate and return a private id.
   *
   * @param ctx - Destination factory context.
   */
  async __v2CreateDestination(
    ctx: DestinationContext,
  ): Promise<{ id: string }> {
    const dest = await this.destination(ctx ?? { pluginDataDir: "" });
    const id = BookclerkPluginV2.next("d");
    BookclerkPluginV2.dests.set(id, dest);
    return { id };
  }

  /**
   * HTTP-adapter: store a source in this isolate.
   *
   * @param ctx - Source factory context.
   */
  async __v2CreateSource(ctx: SourceContext): Promise<{ id: string }> {
    const src = await this.source(ctx ?? { pluginDataDir: "" });
    const id = BookclerkPluginV2.next("s");
    BookclerkPluginV2.sources.set(id, src);
    return { id };
  }

  /**
   * HTTP-adapter: store a job handler in this isolate.
   *
   * @param ctx - Worker factory context.
   */
  async __v2CreateWorker(ctx: WorkerContext): Promise<{ id: string }> {
    const handler = await this.worker(ctx ?? { jobId: "", pluginDataDir: "" });
    const id = BookclerkPluginV2.next("h");
    BookclerkPluginV2.handlers.set(id, handler);
    return { id };
  }

  /**
   * HTTP-adapter streamed get (body is transferred on this RPC).
   *
   * @param id - Private dest id.
   * @param key - Object key.
   * @param options - Optional range.
   */
  async __v2DestGet(
    id: string,
    key: string,
    options?: ReadOptions,
  ): Promise<ReadResult> {
    return BookclerkPluginV2.dest(id).get(key, options);
  }

  /**
   * HTTP-adapter streamed put (body belongs to this RPC).
   *
   * @param id - Private dest id.
   * @param key - Object key.
   * @param body - Byte stream.
   * @param options - Write options.
   */
  async __v2DestPut(
    id: string,
    key: string,
    body: ReadableStream<Uint8Array>,
    options?: WriteOptions,
  ): Promise<PutResult> {
    return BookclerkPluginV2.dest(id).put(key, body, options);
  }

  /**
   * HTTP-adapter job invocation. Granted Source/Destination/Progress are
   * built in this isolate from `env.GRANTED` (no bridge RpcTarget round-trip).
   *
   * @param id - Private handler id.
   * @param event - Domain event.
   * @param invocationId - Host invocation key for the granted reverse channel.
   */
  async __v2Handle(
    id: string,
    event: JobEvent,
    invocationId: string,
  ): Promise<JobOutcome> {
    const handler = BookclerkPluginV2.handlers.get(id);
    if (!handler) {
      throw unsupported("job handler stub expired");
    }
    const context = v2GrantedContext(this.env, invocationId);
    return handler.handle(event, context);
  }

  private static dests = new Map<string, Destination>();
  private static sources = new Map<string, Source>();
  private static handlers = new Map<string, JobHandler>();
  private static seq = 0;

  private static next(prefix: string): string {
    BookclerkPluginV2.seq += 1;
    return `${prefix}${BookclerkPluginV2.seq}`;
  }

  private static dest(id: string): Destination {
    const dest = BookclerkPluginV2.dests.get(id);
    if (!dest) {
      throw unsupported("destination stub expired");
    }
    return dest;
  }
}

function unsupported(method: string): Error {
  return Object.assign(new Error(`${method} not implemented`), {
    code: "unsupported" as const,
  });
}
