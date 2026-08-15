/**
 * Bookclerk bridge worker — HTTP ↔ Workers RPC service binding.
 *
 * v1: bookclerk-workerd POSTs `{ id, method, params }` to `/rpc`.
 * v2: role factories keep RpcTarget stubs in this isolate; byte payloads move
 * as request/response bodies (`ReadableStream`) with runtime flow control.
 * Adapter-private dest ids never appear in the public ABI.
 *
 * All `/rpc`, `/v2/*`, and `/health` requests require `Authorization: Bearer`
 * matching the per-isolate `BRIDGE_TOKEN` binding.
 */

function timingSafeEqual(a, b) {
  if (typeof a !== "string" || typeof b !== "string") return false;
  let out = a.length === b.length ? 0 : 1;
  for (let i = 0; i < b.length; i++) {
    const ac = i < a.length ? a.charCodeAt(i) : 0;
    out |= ac ^ b.charCodeAt(i);
  }
  return out === 0;
}

function authorize(request, env) {
  const expected = env.BRIDGE_TOKEN;
  if (typeof expected !== "string" || !expected) {
    return false;
  }
  const header = request.headers.get("Authorization") || "";
  const prefix = "Bearer ";
  if (!header.startsWith(prefix) && !header.startsWith("bearer ")) {
    return false;
  }
  const provided = header.slice(prefix.length).trim();
  return timingSafeEqual(provided, expected);
}

function errJson(id, code, message, status) {
  return Response.json(
    { id: id ?? null, error: { code, message } },
    { status: status ?? 200 },
  );
}

function catchErr(err) {
  const code =
    err && typeof err === "object" && typeof err.wireCode === "string"
      ? err.wireCode
      : err && typeof err === "object" && typeof err.code === "string"
        ? err.code
        : "internal";
  const message =
    err instanceof Error
      ? err.message
      : typeof err === "string"
        ? err
        : String(err);
  return { code, message };
}

function metaHeaders(meta) {
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

async function handleV2(request, env, url) {
  const plugin = env.PLUGIN;
  if (!plugin) {
    return errJson(null, "unavailable", "PLUGIN binding missing", 500);
  }

  if (request.method === "POST" && url.pathname === "/v2/describe") {
    try {
      const result = await plugin.describe();
      return Response.json(result);
    } catch (err) {
      const { code, message } = catchErr(err);
      return errJson(null, code, message);
    }
  }

  if (request.method === "POST" && url.pathname === "/v2/destination") {
    try {
      const ctx = await request.json();
      return Response.json(await plugin.__v2CreateDestination(ctx ?? {}));
    } catch (err) {
      const { code, message } = catchErr(err);
      return errJson(null, code, message);
    }
  }

  if (request.method === "POST" && url.pathname === "/v2/source") {
    try {
      const ctx = await request.json();
      return Response.json(await plugin.__v2CreateSource(ctx ?? {}));
    } catch (err) {
      const { code, message } = catchErr(err);
      return errJson(null, code, message);
    }
  }

  if (request.method === "POST" && url.pathname === "/v2/worker") {
    try {
      const ctx = await request.json();
      return Response.json(await plugin.__v2CreateWorker(ctx ?? {}));
    } catch (err) {
      const { code, message } = catchErr(err);
      return errJson(null, code, message);
    }
  }

  if (request.method === "POST" && url.pathname === "/v2/stub-counts") {
    try {
      return Response.json(await plugin.__v2StubCounts());
    } catch (err) {
      const { code, message } = catchErr(err);
      return errJson(null, code, message);
    }
  }

  const destMatch = url.pathname.match(/^\/v2\/dest\/([^/]+)\/(head|list|get|put|copy|delete|dispose)$/);
  if (destMatch) {
    const id = destMatch[1];
    const op = destMatch[2];
    try {
      if (op === "head" && request.method === "POST") {
        const { key } = await request.json();
        return Response.json(await plugin.__v2DestHead(id, key));
      }
      if (op === "list" && request.method === "POST") {
        const options = await request.json();
        return Response.json(await plugin.__v2DestList(id, options ?? {}));
      }
      if (op === "get" && request.method === "GET") {
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
        const result = await plugin.__v2DestGet(id, key, options);
        return new Response(result.body, { headers: metaHeaders(result.meta) });
      }
      if (op === "put" && request.method === "PUT") {
        const key = url.searchParams.get("key") || "";
        const contentType = request.headers.get("content-type") || undefined;
        const lenHeader = request.headers.get("content-length");
        const options = {
          contentType,
          contentLength: lenHeader != null ? Number(lenHeader) : undefined,
        };
        const result = await plugin.__v2DestPut(id, key, request.body, options);
        return Response.json(result);
      }
      if (op === "copy" && request.method === "POST") {
        const { from, to } = await request.json();
        return Response.json(await plugin.__v2DestCopy(id, from, to));
      }
      if (op === "delete" && request.method === "POST") {
        const { key } = await request.json();
        return Response.json(await plugin.__v2DestDelete(id, key));
      }
      if (op === "dispose" && request.method === "POST") {
        return Response.json(await plugin.__v2DisposeDestination(id));
      }
    } catch (err) {
      const { code, message } = catchErr(err);
      return errJson(null, code, message);
    }
  }

  const srcMatch = url.pathname.match(/^\/v2\/source\/([^/]+)\/open$/);
  if (srcMatch && request.method === "GET") {
    try {
      const key = url.searchParams.get("key") || "";
      const result = await plugin.__v2SourceOpen(srcMatch[1], key);
      return new Response(result.body, { headers: metaHeaders(result.meta) });
    } catch (err) {
      const { code, message } = catchErr(err);
      return errJson(null, code, message);
    }
  }

  const srcDispose = url.pathname.match(/^\/v2\/source\/([^/]+)\/dispose$/);
  if (srcDispose && request.method === "POST") {
    try {
      return Response.json(await plugin.__v2DisposeSource(srcDispose[1]));
    } catch (err) {
      const { code, message } = catchErr(err);
      return errJson(null, code, message);
    }
  }

  const handleMatch = url.pathname.match(/^\/v2\/handler\/([^/]+)\/handle$/);
  if (handleMatch && request.method === "POST") {
    try {
      const body = await request.json();
      const grantToken = body.grantToken;
      const invocation = body.invocation ?? body.event ?? {};
      const outcome = await plugin.__v2Handle(
        handleMatch[1],
        invocation,
        grantToken,
      );
      return Response.json(outcome);
    } catch (err) {
      const { code, message } = catchErr(err);
      return errJson(null, code, message);
    }
  }

  return new Response("not found", { status: 404 });
}

export default {
  async fetch(request, env) {
    if (!authorize(request, env)) {
      return new Response("unauthorized", { status: 401 });
    }

    const url = new URL(request.url);
    if (request.method === "GET" && url.pathname === "/health") {
      return new Response("ok", { status: 200 });
    }
    if (url.pathname.startsWith("/v2/")) {
      return handleV2(request, env, url);
    }
    if (request.method !== "POST" || url.pathname !== "/rpc") {
      return new Response("not found", { status: 404 });
    }

    let body;
    try {
      body = await request.json();
    } catch {
      return Response.json(
        { error: { code: "invalid_params", message: "invalid JSON body" } },
        { status: 400 },
      );
    }

    const id = body?.id ?? null;
    const method = body?.method;
    if (typeof method !== "string" || !method) {
      return Response.json(
        {
          id,
          error: { code: "invalid_params", message: "method required" },
        },
        { status: 400 },
      );
    }

    const plugin = env.PLUGIN;
    if (!plugin || typeof plugin[method] !== "function") {
      return Response.json({
        id,
        error: {
          code: "unsupported",
          message: `method \`${method}\` not exported by plugin entrypoint`,
        },
      });
    }

    try {
      const params = body.params;
      const result =
        params === undefined || params === null
          ? await plugin[method]()
          : await plugin[method](params);
      return Response.json({ id, result: result ?? null });
    } catch (err) {
      const { code, message } = catchErr(err);
      return Response.json({
        id,
        error: { code, message },
      });
    }
  },
};
