/**
 * Libation diagnostics Worker (Cloudflare Builds → Workers).
 *
 * POST /submit  — clients send redacted JSON; validated + enriched → B2
 * GET  /report  — GitHub Action pulls new objects since `since` (secret key)
 * GET  /health  — liveness
 *
 * Secrets (Cloudflare dashboard / wrangler secret):
 *   B2_KEY_ID, B2_APPLICATION_KEY, B2_BUCKET_ID
 *   REPORT_API_KEY          — required for /report
 *   CLIENT_IP_HASH_SALT     — optional; enables hashed client IP in enrichment
 *
 * Cloudflare Builds: set project root to `tools/diagnostics-collector`.
 */

const MAX_BODY_BYTES = 256 * 1024;
const INCOMING_PREFIX = "diagnostics/incoming/";
const MAX_REPORT_FILES = 100;

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const path = url.pathname.replace(/\/+$/, "") || "/";

    try {
      if (request.method === "GET" && (path === "/" || path === "/health")) {
        const origin = new URL(request.url).origin;
        return json({
          ok: true,
          service: "libation-diagnostics",
          url: origin,
        });
      }

      if (request.method === "POST" && path === "/submit") {
        return handleSubmit(request, env);
      }

      if (request.method === "GET" && path === "/report") {
        return handleReport(request, env, url);
      }

      return json({ error: "not found" }, 404);
    } catch (err) {
      return json({ error: "internal error" }, 500);
    }
  },
};

async function handleSubmit(request, env) {
  const len = Number(request.headers.get("content-length") || "0");
  if (len > MAX_BODY_BYTES) {
    return json({ error: "payload too large" }, 413);
  }

  const raw = await request.arrayBuffer();
  if (raw.byteLength === 0 || raw.byteLength > MAX_BODY_BYTES) {
    return json({ error: "invalid payload size" }, 400);
  }

  let payload;
  try {
    payload = JSON.parse(new TextDecoder().decode(raw));
  } catch {
    return json({ error: "invalid json" }, 400);
  }

  const verr = validateClientPayload(payload);
  if (verr) {
    return json({ error: verr }, 400);
  }

  const asText = JSON.stringify(payload);
  if (looksLikeSecretLeak(asText)) {
    return json({ error: "payload rejected by secret heuristics" }, 400);
  }

  const receivedAt = Date.now();
  const enriched = {
    schema_version: 1,
    received_at_unix_ms: receivedAt,
    worker: "libation-diagnostics",
    client_ip_hash: await maybeHashIp(request, env),
    user_agent: truncate(request.headers.get("user-agent") || "", 200),
    payload,
  };

  const bodyText = JSON.stringify(enriched);
  if (looksLikeSecretLeak(bodyText)) {
    return json({ error: "enriched payload rejected" }, 400);
  }

  const objectKey = `${INCOMING_PREFIX}${safeToken(payload.trigger)}-${receivedAt}-${crypto.randomUUID()}.json`;
  const auth = await b2Authorize(env);
  const upload = await b2GetUploadUrl(auth, env.B2_BUCKET_ID);
  await b2Upload(upload, objectKey, bodyText, {
    received_at_unix_ms: String(receivedAt),
    trigger: safeToken(payload.trigger),
    libation_version: truncate(String(payload.version || ""), 64),
  });

  return json({ ok: true, key: objectKey, received_at_unix_ms: receivedAt }, 201);
}

async function handleReport(request, env, url) {
  if (!requireReportAuth(request, env)) {
    return json({ error: "unauthorized" }, 401);
  }

  const sinceRaw = url.searchParams.get("since");
  const since = sinceRaw != null && sinceRaw !== "" ? Number(sinceRaw) : 0;
  if (!Number.isFinite(since) || since < 0) {
    return json({ error: "invalid since" }, 400);
  }

  const auth = await b2Authorize(env);
  const files = await b2ListIncoming(auth, env.B2_BUCKET_ID);
  const newer = files
    .filter((f) => f.action === "upload" && Number(f.uploadTimestamp) > since)
    .sort((a, b) => Number(a.uploadTimestamp) - Number(b.uploadTimestamp))
    .slice(0, MAX_REPORT_FILES);

  const reports = [];
  let maxTs = since;
  for (const f of newer) {
    const ts = Number(f.uploadTimestamp);
    if (ts > maxTs) maxTs = ts;
    const downloaded = await b2DownloadById(auth, f.fileId);
    let parsed;
    try {
      parsed = JSON.parse(downloaded);
    } catch {
      continue;
    }
    reports.push({
      file_name: f.fileName,
      file_id: f.fileId,
      upload_timestamp_ms: ts,
      content_length: f.contentLength,
      report: parsed,
    });
  }

  return json({
    ok: true,
    since,
    next_since: reports.length ? maxTs : since,
    count: reports.length,
    truncated: files.filter((f) => Number(f.uploadTimestamp) > since).length > MAX_REPORT_FILES,
    reports,
  });
}

function requireReportAuth(request, env) {
  const expected = env.REPORT_API_KEY;
  if (!expected || typeof expected !== "string" || expected.length < 16) {
    return false;
  }
  const header = request.headers.get("authorization") || "";
  const bearer = header.toLowerCase().startsWith("bearer ")
    ? header.slice(7).trim()
    : "";
  const alt = (request.headers.get("x-libation-report-key") || "").trim();
  return timingSafe.equal(bearer, expected) || timingSafe.equal(alt, expected);
}

/** Constant-time string compare for equal-length secrets. */
const timingSafe = {
  equal(a, b) {
    if (typeof a !== "string" || typeof b !== "string" || a.length !== b.length) {
      return false;
    }
    let out = 0;
    for (let i = 0; i < a.length; i++) {
      out |= a.charCodeAt(i) ^ b.charCodeAt(i);
    }
    return out === 0;
  },
};

function validateClientPayload(p) {
  if (!p || typeof p !== "object" || Array.isArray(p)) return "body must be an object";
  if (typeof p.trigger !== "string" || !p.trigger || p.trigger.length > 64) {
    return "trigger required";
  }
  if (typeof p.version !== "string" || p.version.length > 64) return "version required";
  if (typeof p.os !== "string" || p.os.length > 64) return "os required";
  if (typeof p.archived_at_unix_ms !== "number" && typeof p.archived_at_unix_ms !== "undefined") {
    return "archived_at_unix_ms invalid";
  }
  if (!Array.isArray(p.events)) return "events must be an array";
  if (p.events.length > 500) return "too many events";
  for (const ev of p.events) {
    if (!ev || typeof ev !== "object") return "invalid event";
    if (typeof ev.message === "string" && ev.message.length > 8000) {
      return "event message too long";
    }
    if (typeof ev.level === "string" && ev.level.length > 32) return "event level too long";
    if (typeof ev.target === "string" && ev.target.length > 256) return "event target too long";
  }
  return null;
}

function looksLikeSecretLeak(text) {
  return (
    /\bAtna\|[A-Za-z0-9._\-+/=]+/.test(text) ||
    /\bAtnr\|[A-Za-z0-9._\-+/=]+/.test(text) ||
    /\bBearer\s+[A-Za-z0-9._\-+/=]{20,}/.test(text) ||
    /\bgh[pousr]_[A-Za-z0-9_]{20,}/.test(text) ||
    /\bgithub_pat_[A-Za-z0-9_]{20,}/.test(text) ||
    /\bAKIA[0-9A-Z]{16}\b/.test(text) ||
    /-----BEGIN [A-Z ]*PRIVATE KEY-----/.test(text)
  );
}

async function maybeHashIp(request, env) {
  const salt = env.CLIENT_IP_HASH_SALT;
  if (!salt) return null;
  const ip = request.headers.get("cf-connecting-ip") || "";
  if (!ip) return null;
  const data = new TextEncoder().encode(`${salt}:${ip}`);
  const digest = await crypto.subtle.digest("SHA-256", data);
  return [...new Uint8Array(digest)]
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("")
    .slice(0, 32);
}

function safeToken(s) {
  return String(s || "report")
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48) || "report";
}

function truncate(s, n) {
  return s.length <= n ? s : s.slice(0, n);
}

async function b2Authorize(env) {
  const endpoint = (env.B2_ENDPOINT || "https://api.backblazeb2.com").replace(/\/$/, "");
  const id = env.B2_KEY_ID;
  const key = env.B2_APPLICATION_KEY;
  if (!id || !key || !env.B2_BUCKET_ID) {
    throw new Error("missing B2 credentials");
  }
  const basic = btoa(`${id}:${key}`);
  const res = await fetch(`${endpoint}/b2api/v2/b2_authorize_account`, {
    headers: { Authorization: `Basic ${basic}` },
  });
  if (!res.ok) throw new Error("b2 authorize failed");
  return res.json();
}

async function b2GetUploadUrl(auth, bucketId) {
  const res = await fetch(`${auth.apiUrl}/b2api/v2/b2_get_upload_url`, {
    method: "POST",
    headers: {
      Authorization: auth.authorizationToken,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ bucketId }),
  });
  if (!res.ok) throw new Error("b2 get_upload_url failed");
  return res.json();
}

async function b2Upload(upload, fileName, bodyText, info = {}) {
  const bytes = new TextEncoder().encode(bodyText);
  const hashBuffer = await crypto.subtle.digest("SHA-1", bytes);
  const sha1 = [...new Uint8Array(hashBuffer)].map((b) => b.toString(16).padStart(2, "0")).join("");

  const headers = {
    Authorization: upload.authorizationToken,
    "X-Bz-File-Name": encodeURIComponent(fileName),
    "Content-Type": "application/json",
    "Content-Length": String(bytes.byteLength),
    "X-Bz-Content-Sha1": sha1,
  };
  for (const [k, v] of Object.entries(info)) {
    headers[`X-Bz-Info-${k}`] = encodeURIComponent(String(v));
  }

  const res = await fetch(upload.uploadUrl, {
    method: "POST",
    headers,
    body: bytes,
  });
  if (!res.ok) throw new Error("b2 upload failed");
}

async function b2ListIncoming(auth, bucketId) {
  const files = [];
  let startFileName = null;
  for (let i = 0; i < 20; i++) {
    const body = {
      bucketId,
      prefix: INCOMING_PREFIX,
      maxFileCount: 1000,
    };
    if (startFileName) body.startFileName = startFileName;
    const res = await fetch(`${auth.apiUrl}/b2api/v2/b2_list_file_names`, {
      method: "POST",
      headers: {
        Authorization: auth.authorizationToken,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(body),
    });
    if (!res.ok) throw new Error("b2 list failed");
    const data = await res.json();
    files.push(...(data.files || []));
    if (!data.nextFileName) break;
    startFileName = data.nextFileName;
  }
  return files;
}

async function b2DownloadById(auth, fileId) {
  const res = await fetch(`${auth.downloadUrl}/b2api/v2/b2_download_file_by_id?fileId=${encodeURIComponent(fileId)}`, {
    headers: { Authorization: auth.authorizationToken },
  });
  if (!res.ok) throw new Error("b2 download failed");
  return res.text();
}

function json(obj, status = 200) {
  return new Response(JSON.stringify(obj), {
    status,
    headers: {
      "Content-Type": "application/json",
      "Cache-Control": "no-store",
    },
  });
}
