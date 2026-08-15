/**
 * Workerd runtime for `@bookclerk/plugin-sdk` / `@bookclerk/plugin-sdk/workerd`.
 *
 * Authors import the package — never a relative embed path:
 *
 *   import { BookclerkPlugin } from "@bookclerk/plugin-sdk/workerd";
 *   // or: import { BookclerkPlugin } from "@bookclerk/plugin-sdk";
 *
 *   import { wasmBookclerkPlugin } from "@bookclerk/plugin-sdk/workerd"; // Rust/Wasm glue
 *
 * `bookclerk-workerd` injects this module into the isolate under those names.
 * Native guests use `@bookclerk/plugin-sdk/native` (`BookclerkPluginGuest`) instead.
 */

import { WorkerEntrypoint, RpcTarget } from "cloudflare:workers";

function unsupported(method) {
  return Object.assign(new Error(`${method} not implemented`), {
    code: "unsupported",
  });
}

export class BookclerkPlugin extends WorkerEntrypoint {
  /** Required by workerd when the entrypoint is not HTTP-facing. */
  async fetch() {
    return new Response(null, { status: 404 });
  }

  /** Identity, capabilities, CLI schema, brand — required. */
  async handshake(_params) {
    throw unsupported("handshake");
  }

  async shutdown() {}

  async health() {
    return { ok: true };
  }

  async diagnose() {
    return { lines: [] };
  }

  async onEvent(_event) {
    throw unsupported("onEvent");
  }

  async cliDescribe() {
    return { commands: [] };
  }

  async cliInvoke(_params) {
    throw unsupported("cliInvoke");
  }
}

/**
 * BookclerkPlugin subclass that forwards Workers RPC methods to a Wasm
 * `dispatch(method, paramsJson) -> resultJson` export (wasm-bindgen).
 *
 * @param {(method: string, paramsJson: string) => string} dispatch
 * @returns {typeof BookclerkPlugin}
 */
export function wasmBookclerkPlugin(dispatch) {
  return class WasmBookclerkPlugin extends BookclerkPlugin {
    #call(method, params) {
      const paramsJson =
        params === undefined || params === null ? "{}" : JSON.stringify(params);
      const out = dispatch(method, paramsJson);
      return out === "null" ? null : JSON.parse(out);
    }

    async handshake(params) {
      return this.#call("handshake", params);
    }

    async shutdown() {
      this.#call("shutdown", {});
    }

    async health() {
      return this.#call("health", {});
    }

    async diagnose() {
      return this.#call("diagnose", {});
    }

    async onEvent(event) {
      this.#call("onEvent", event);
    }

    async cliDescribe() {
      return this.#call("cliDescribe", {});
    }

    async cliInvoke(params) {
      return this.#call("cliInvoke", params);
    }
  };
}

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

/** Thrown when a wire union carries `err`. Unknown codes are kept on `wireCode`. */
export class PluginError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "PluginError";
    this.wireCode = code;
    this.code = KNOWN_ERROR_CODES.has(code) ? code : "unknown";
  }

  static fromWire(code, message) {
    return new PluginError(code, message);
  }
}

function v2Unsupported(method) {
  return PluginError.fromWire("unsupported", `${method} not implemented`);
}

/** Product ABI version 2 (`describe().apiVersion`). */
export const PRODUCT_API_VERSION = 2;
export const MAX_SCALAR_BYTES = 262144;
export const MAX_STREAM_WINDOW_BYTES = 1048576;
export const MAX_LIST_PAGE = 256;
export const FEATURE_SCALAR_LIMITS = "rpc.scalarLimits";
export const FEATURE_STREAMS = "rpc.streams";
export const FEATURE_STORAGE_COPY = "storage.copy";
export const ENVELOPE_VERSION = 1;
export const MAX_CHECKPOINT_BYTES = 65536;

/** Destination capability — subclass and override methods. Abort is stream cancel. */
export class Destination extends RpcTarget {
  async head(_key) {
    throw v2Unsupported("head");
  }
  async list(_options) {
    throw v2Unsupported("list");
  }
  async get(_key, _options) {
    throw v2Unsupported("get");
  }
  async put(_key, _body, _options) {
    throw v2Unsupported("put");
  }
  async copy(_from, _to) {
    throw v2Unsupported("copy");
  }
  async delete(_key) {
    throw v2Unsupported("delete");
  }
}

/** Source capability. */
export class Source extends RpcTarget {
  async open(_key) {
    throw v2Unsupported("open");
  }
}

/** Progress reports (never media). */
export class ProgressSink extends RpcTarget {
  async report(_percent, _message) {
    throw v2Unsupported("report");
  }
}

/** Job handler for one durable invocation. */
export class JobHandler extends RpcTarget {
  async handle(_invocation, _context) {
    throw v2Unsupported("handle");
  }
}

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
          controller.error(
            PluginError.fromWire("invalid_params", `content-length ${expected} got ${n}`),
          );
          return;
        }
        controller.close();
        return;
      }
      n += value.byteLength;
      if (n > expected) {
        controller.error(
          PluginError.fromWire("payload_too_large", `body exceeded Content-Length ${expected}`),
        );
        return;
      }
      controller.enqueue(value);
    },
    cancel(reason) {
      return reader.cancel(reason);
    },
  });
}

function v2GrantedContext(env, grantToken, controller) {
  const granted = env.GRANTED;
  if (!granted || typeof grantToken !== "string" || !grantToken) {
    throw PluginError.fromWire("internal", "granted reverse channel missing");
  }
  const auth = { Authorization: `Bearer ${grantToken}` };
  return {
    input: {
      async open(key) {
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
          body: resp.body,
        };
      },
    },
    output: {
      async put(key, body, options) {
        const headers = { ...auth };
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
    },
    progress: {
      async report(percent, message) {
        await granted.fetch(`http://granted/progress`, {
          method: "POST",
          headers: { ...auth, "content-type": "application/json" },
          body: JSON.stringify({ percent, message: message || "" }),
          signal: controller.signal,
        });
      },
    },
    signal: controller.signal,
  };
}

function v2MetaHeaders(meta) {
  const headers = {
    "x-bookclerk-key": meta?.key || "",
    "x-bookclerk-size": String(meta?.size ?? 0),
  };
  if (meta?.contentType) {
    headers["x-bookclerk-content-type"] = meta.contentType;
    headers["content-type"] = meta.contentType;
  }
  if (meta?.etag) headers["x-bookclerk-etag"] = meta.etag;
  if (meta?.size != null && Number(meta.size) > 0) {
    headers["content-length"] = String(meta.size);
  }
  return headers;
}

function v2ErrResponse(err) {
  const code =
    err && typeof err === "object" && typeof err.wireCode === "string"
      ? err.wireCode
      : err && typeof err === "object" && typeof err.code === "string"
        ? err.code
        : "internal";
  const message = err instanceof Error ? err.message : String(err);
  return Response.json({ error: { code, message } }, { status: 200 });
}

/**
 * Product `apiVersion` 2 guest base — authors subclass this.
 * Adapter-private GRANTED / BRIDGE_TOKEN / `__v2*` live on {@link wrapV2Plugin}.
 */
export class BookclerkPluginV2 extends WorkerEntrypoint {
  async fetch() {
    return new Response(null, { status: 404 });
  }
  async describe() {
    throw v2Unsupported("describe");
  }
  destination(_context) {
    throw v2Unsupported("destination");
  }
  source(_context) {
    throw v2Unsupported("source");
  }
  worker(_context) {
    throw v2Unsupported("worker");
  }
  async shutdown() {}
}

/**
 * First-party wrapper: owns dest/source/handler maps and per-invocation grants.
 * Export `wrapV2Plugin(MyPlugin)` as the workerd default.
 */
export function wrapV2Plugin(Author) {
  const dests = new Map();
  const sources = new Map();
  const handlers = new Map();
  let seq = 0;
  const next = (prefix) => {
    seq += 1;
    return `${prefix}${seq}`;
  };

  return class V2Wrapper extends WorkerEntrypoint {
    constructor(ctx, env) {
      super(ctx, env);
      const authorEnv = { ...env };
      delete authorEnv.GRANTED;
      delete authorEnv.BRIDGE_TOKEN;
      delete authorEnv.PLUGIN_BACKEND;
      this.author = new Author(ctx, authorEnv);
    }

    async fetch(request) {
      const url = new URL(request.url);
      try {
        if (url.pathname === "/__v2/put" && request.method === "PUT") {
          const dest = dests.get(url.searchParams.get("id"));
          if (!dest) {
            throw PluginError.fromWire("not_found", "destination stub expired");
          }
          const key = url.searchParams.get("key") || "";
          const lenHeader = request.headers.get("content-length");
          const options = {
            contentType: request.headers.get("content-type") || undefined,
            contentLength: lenHeader != null ? Number(lenHeader) : undefined,
          };
          const result = await dest.put(
            key,
            exactLengthBody(request.body, options.contentLength),
            options,
          );
          return Response.json(result);
        }
        if (url.pathname === "/__v2/get" && request.method === "GET") {
          const dest = dests.get(url.searchParams.get("id"));
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
          return new Response(result.body, { headers: v2MetaHeaders(result.meta) });
        }
        if (url.pathname === "/__v2/open" && request.method === "GET") {
          const src = sources.get(url.searchParams.get("id"));
          if (!src) {
            throw PluginError.fromWire("not_found", "source stub expired");
          }
          const result = await src.open(url.searchParams.get("key") || "");
          return new Response(result.body, { headers: v2MetaHeaders(result.meta) });
        }
      } catch (err) {
        return v2ErrResponse(err);
      }
      return this.author.fetch(request);
    }

    describe() {
      return this.author.describe();
    }

    async shutdown() {
      dests.clear();
      sources.clear();
      handlers.clear();
      await this.author.shutdown();
    }

    async __v2CreateDestination(ctx) {
      const dest = await this.author.destination(ctx ?? {});
      const id = next("d");
      dests.set(id, dest);
      return { id };
    }
    async __v2CreateSource(ctx) {
      const src = await this.author.source(ctx ?? {});
      const id = next("s");
      sources.set(id, src);
      return { id };
    }
    async __v2CreateWorker(ctx) {
      const handler = await this.author.worker(ctx ?? {});
      const id = next("h");
      handlers.set(id, handler);
      return { id };
    }
    async __v2DestHead(id, key) {
      const dest = dests.get(id);
      if (!dest) throw PluginError.fromWire("not_found", "destination stub expired");
      const meta = await dest.head(key);
      return { found: meta != null, meta: meta ?? null };
    }
    async __v2DestList(id, options) {
      const dest = dests.get(id);
      if (!dest) throw PluginError.fromWire("not_found", "destination stub expired");
      return dest.list(options ?? {});
    }
    async __v2DestGet(id, key, options) {
      const dest = dests.get(id);
      if (!dest) throw PluginError.fromWire("not_found", "destination stub expired");
      return dest.get(key, options);
    }
    async __v2DestPut(id, key, body, options) {
      const dest = dests.get(id);
      if (!dest) throw PluginError.fromWire("not_found", "destination stub expired");
      return dest.put(key, exactLengthBody(body, options?.contentLength), options);
    }
    async __v2DestCopy(id, from, to) {
      const dest = dests.get(id);
      if (!dest) throw PluginError.fromWire("not_found", "destination stub expired");
      if (typeof dest.copy === "function") {
        return dest.copy(from, to);
      }
      throw PluginError.fromWire("unsupported", "copy not implemented");
    }
    async __v2DestDelete(id, key) {
      const dest = dests.get(id);
      if (!dest) throw PluginError.fromWire("not_found", "destination stub expired");
      await dest.delete(key);
      return { ok: true };
    }
    async __v2SourceOpen(id, key) {
      const src = sources.get(id);
      if (!src) throw PluginError.fromWire("not_found", "source stub expired");
      return src.open(key);
    }
    async __v2Handle(id, invocation, grantToken) {
      const handler = handlers.get(id);
      if (!handler) {
        throw PluginError.fromWire("not_found", "job handler stub expired");
      }
      const controller = new AbortController();
      try {
        const context = v2GrantedContext(this.env, grantToken, controller);
        return await handler.handle(invocation, context);
      } finally {
        controller.abort();
        handlers.delete(id);
      }
    }
  };
}


