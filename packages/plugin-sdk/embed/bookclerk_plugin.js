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

function v2Unsupported(method) {
  return Object.assign(new Error(`${method} not implemented`), {
    code: "unsupported",
  });
}

/** Product ABI version 2 (`describe().apiVersion`). */
export const PRODUCT_API_VERSION = 2;
export const MAX_SCALAR_BYTES = 262144;
export const MAX_STREAM_WINDOW_BYTES = 1048576;
export const MAX_LIST_PAGE = 256;
export const FEATURE_SCALAR_LIMITS = "rpc.scalarLimits";
export const FEATURE_STREAMS = "rpc.streams";
export const FEATURE_STORAGE_COPY = "storage.copy";

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
  async handle(_event, _context) {
    throw v2Unsupported("handle");
  }
}

const v2Dests = new Map();
const v2Sources = new Map();
const v2Handlers = new Map();
let v2Seq = 0;

function v2Next(prefix) {
  v2Seq += 1;
  return `${prefix}${v2Seq}`;
}

function v2GrantedContext(env, invocationId) {
  const granted = env.GRANTED;
  const token = env.BRIDGE_TOKEN;
  if (!granted || typeof token !== "string" || !token) {
    throw Object.assign(new Error("granted reverse channel missing"), {
      code: "internal",
    });
  }
  const auth = { Authorization: `Bearer ${token}` };
  return {
    invocationId,
    input: {
      async open(key) {
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
          `http://granted/put?invocation=${encodeURIComponent(invocationId)}&key=${encodeURIComponent(key)}`,
          { method: "PUT", headers, body },
        );
        if (!resp.ok) {
          throw Object.assign(new Error(await resp.text()), { code: "internal" });
        }
        return resp.json();
      },
    },
    progress: {
      async report(percent, message) {
        await granted.fetch(
          `http://granted/progress?invocation=${encodeURIComponent(invocationId)}`,
          {
            method: "POST",
            headers: { ...auth, "content-type": "application/json" },
            body: JSON.stringify({ percent, message: message || "" }),
          },
        );
      },
    },
    signal: { aborted: false },
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

/**
 * Product `apiVersion` 2 guest base. Authors never see `handleId` / `writeChunk`.
 *
 * `__v2*` methods are the workerd HTTP adapter (private): dest/source/handler
 * objects stay in this isolate so streams are taken on the current RPC.
 * Tables are module-scoped — each RPC constructs a new entrypoint instance.
 * Streamed get/put/open use `fetch()` (service-binding bodies) rather than
 * JSRPC stream arguments, which cannot cross a different request's I/O.
 */
export class BookclerkPluginV2 extends WorkerEntrypoint {
  async fetch(request) {
    const url = new URL(request.url);
    try {
      if (url.pathname === "/__v2/put" && request.method === "PUT") {
        const dest = v2Dests.get(url.searchParams.get("id"));
        if (!dest) {
          throw Object.assign(new Error("destination stub expired"), { code: "not_found" });
        }
        const key = url.searchParams.get("key") || "";
        const lenHeader = request.headers.get("content-length");
        const result = await dest.put(key, request.body, {
          contentType: request.headers.get("content-type") || undefined,
          contentLength: lenHeader != null ? Number(lenHeader) : undefined,
        });
        return Response.json(result);
      }
      if (url.pathname === "/__v2/get" && request.method === "GET") {
        const dest = v2Dests.get(url.searchParams.get("id"));
        if (!dest) {
          throw Object.assign(new Error("destination stub expired"), { code: "not_found" });
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
        const src = v2Sources.get(url.searchParams.get("id"));
        if (!src) {
          throw Object.assign(new Error("source stub expired"), { code: "not_found" });
        }
        const result = await src.open(url.searchParams.get("key") || "");
        return new Response(result.body, { headers: v2MetaHeaders(result.meta) });
      }
    } catch (err) {
      const code = err && typeof err === "object" && err.code ? err.code : "internal";
      const message = err instanceof Error ? err.message : String(err);
      return Response.json({ error: { code, message } }, { status: 200 });
    }
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

  async __v2CreateDestination(ctx) {
    const dest = await this.destination(ctx ?? {});
    const id = v2Next("d");
    v2Dests.set(id, dest);
    return { id };
  }
  async __v2CreateSource(ctx) {
    const src = await this.source(ctx ?? {});
    const id = v2Next("s");
    v2Sources.set(id, src);
    return { id };
  }
  async __v2CreateWorker(ctx) {
    const handler = await this.worker(ctx ?? {});
    const id = v2Next("h");
    v2Handlers.set(id, handler);
    return { id };
  }
  async __v2DestHead(id, key) {
    const dest = v2Dests.get(id);
    if (!dest) throw Object.assign(new Error("destination stub expired"), { code: "not_found" });
    const meta = await dest.head(key);
    return { found: meta != null, meta: meta ?? null };
  }
  async __v2DestList(id, options) {
    const dest = v2Dests.get(id);
    if (!dest) throw Object.assign(new Error("destination stub expired"), { code: "not_found" });
    return dest.list(options ?? {});
  }
  async __v2DestGet(id, key, options) {
    const dest = v2Dests.get(id);
    if (!dest) throw Object.assign(new Error("destination stub expired"), { code: "not_found" });
    return dest.get(key, options);
  }
  async __v2DestPut(id, key, body, options) {
    const dest = v2Dests.get(id);
    if (!dest) throw Object.assign(new Error("destination stub expired"), { code: "not_found" });
    return dest.put(key, body, options);
  }
  async __v2DestCopy(id, from, to) {
    const dest = v2Dests.get(id);
    if (!dest) throw Object.assign(new Error("destination stub expired"), { code: "not_found" });
    return dest.copy(from, to);
  }
  async __v2DestDelete(id, key) {
    const dest = v2Dests.get(id);
    if (!dest) throw Object.assign(new Error("destination stub expired"), { code: "not_found" });
    await dest.delete(key);
    return { ok: true };
  }
  async __v2SourceOpen(id, key) {
    const src = v2Sources.get(id);
    if (!src) throw Object.assign(new Error("source stub expired"), { code: "not_found" });
    return src.open(key);
  }
  async __v2Handle(id, event, invocationId) {
    const handler = v2Handlers.get(id);
    if (!handler) throw Object.assign(new Error("job handler stub expired"), { code: "not_found" });
    const context = v2GrantedContext(this.env, invocationId);
    return handler.handle(event, context);
  }
}

