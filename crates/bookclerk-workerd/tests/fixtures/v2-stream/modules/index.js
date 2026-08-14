/**
 * ABI v2 contract fixture: Destination streams + JobHandler copy.
 *
 * Large objects are generated/consumed lazily. `count:` puts discard bytes;
 * `pattern:<n>` gets emit a repeating counter stream of `n` bytes.
 */

import {
  BookclerkPluginV2,
  PRODUCT_API_VERSION,
  FEATURE_SCALAR_LIMITS,
  FEATURE_STREAMS,
} from "@bookclerk/plugin-sdk/workerd";

/**
 * Plain in-isolate objects (not RpcTarget). The HTTP bridge invokes methods
 * from later `/v2/*` requests; RpcTarget I/O is bound to the factory request.
 */

function patternStream(size) {
  let pos = 0;
  return new ReadableStream({
    pull(controller) {
      if (pos >= size) {
        controller.close();
        return;
      }
      const n = Math.min(64 * 1024, size - pos);
      const buf = new Uint8Array(n);
      for (let i = 0; i < n; i++) {
        buf[i] = (pos + i) % 251;
      }
      pos += n;
      controller.enqueue(buf);
    },
  });
}

function bytesStream(buf) {
  return new ReadableStream({
    start(controller) {
      if (buf.byteLength > 0) controller.enqueue(buf);
      controller.close();
    },
  });
}

class MemDest {
  constructor() {
    this.store = new Map();
  }

  async head(key) {
    if (this.store.has(key)) {
      const buf = this.store.get(key);
      return { key, size: buf.byteLength };
    }
    if (key.startsWith("pattern:")) {
      return { key, size: Number(key.slice("pattern:".length)) || 0 };
    }
    return null;
  }

  async list(options) {
    const prefix = options?.prefix || "";
    const objects = [];
    for (const [key, buf] of this.store) {
      if (key.startsWith(prefix)) {
        objects.push({ key, size: buf.byteLength });
      }
    }
    return { objects };
  }

  async get(key) {
    if (this.store.has(key)) {
      const buf = this.store.get(key);
      return { meta: { key, size: buf.byteLength }, body: bytesStream(buf) };
    }
    if (key.startsWith("pattern:")) {
      const size = Number(key.slice("pattern:".length)) || 0;
      return { meta: { key, size }, body: patternStream(size) };
    }
    throw Object.assign(new Error(`not found: ${key}`), { code: "not_found" });
  }

  async put(key, body) {
    const reader = body.getReader();
    const chunks = [];
    let n = 0;
    const keep = !key.startsWith("count:");
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      n += value.byteLength;
      if (keep) chunks.push(value);
    }
    if (keep) {
      const buf = new Uint8Array(n);
      let off = 0;
      for (const c of chunks) {
        buf.set(c, off);
        off += c.byteLength;
      }
      this.store.set(key, buf);
    }
    return { key, bytesWritten: n };
  }

  async delete(key) {
    this.store.delete(key);
  }
}

class MemSource {
  constructor(dest) {
    this.dest = dest;
  }

  open(key) {
    return this.dest.get(key);
  }
}

class CopyHandler {
  async handle(event, context) {
    const spec = JSON.parse(event.json || "{}");
    await context.progress.report(0, "opening");
    const opened = await context.input.open(spec.from);
    await context.progress.report(10, "copying");
    const put = await context.output.put(spec.to, opened.body, {
      contentType: opened.meta?.contentType,
      contentLength: opened.meta?.size || undefined,
    });
    await context.progress.report(100, "done");
    return {
      ok: true,
      message: `copied ${spec.from} -> ${spec.to}`,
      bytesCopied: put.bytesWritten,
    };
  }
}

export default class StreamPlugin extends BookclerkPluginV2 {
  constructor(ctx, env) {
    super(ctx, env);
    this.mem = new MemDest();
  }

  async describe() {
    return {
      apiVersion: PRODUCT_API_VERSION,
      id: "v2_stream",
      kind: "output",
      displayName: "ABI v2 stream contract",
      rpcFeatures: [FEATURE_SCALAR_LIMITS, FEATURE_STREAMS],
      scalarLimits: {
        maxScalarBytes: 262144,
        maxStreamWindowBytes: 1048576,
        maxListPage: 256,
      },
    };
  }

  destination() {
    return this.mem;
  }

  source() {
    return new MemSource(this.mem);
  }

  worker() {
    return new CopyHandler();
  }
}
