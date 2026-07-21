/**
 * Write-only diagnostics ingress → Backblaze B2.
 *
 * Libation clients POST redacted JSON here. This Worker validates the payload
 * and writes an object to B2. It does **not** create GitHub Issues and does
 * **not** expose read/list/delete to clients.
 *
 * A separate GitHub Action (`.github/workflows/diagnostics-ingest.yml`) lists
 * the bucket with server-side credentials and opens Issues (Copilot-ready).
 *
 * Secrets / bindings (wrangler):
 *   B2_KEY_ID, B2_APPLICATION_KEY, B2_BUCKET_ID
 *   Optional: B2_ENDPOINT (default https://api.backblazeb2.com)
 *
 * Deploy:
 *   wrangler secret put B2_KEY_ID
 *   wrangler secret put B2_APPLICATION_KEY
 *   wrangler secret put B2_BUCKET_ID
 *   wrangler deploy
 */

const MAX_BODY_BYTES = 256 * 1024;
const PREFIX = "diagnostics/incoming/";

export default {
  async fetch(request, env) {
    const url = new URL(request.url);

    if (request.method === "GET" && (url.pathname === "/" || url.pathname === "/health")) {
      return json({ ok: true, service: "libation-diagnostics-b2-ingress" });
    }

    if (request.method !== "POST" || url.pathname !== "/v1/report") {
      return json({ error: "not found" }, 404);
    }

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

    const err = validatePayload(payload);
    if (err) {
      return json({ error: err }, 400);
    }

    // Defense: reject obvious secret shapes that should never leave the client.
    const asText = JSON.stringify(payload);
    if (looksLikeSecretLeak(asText)) {
      return json({ error: "payload rejected by secret heuristics" }, 400);
    }

    try {
      const auth = await b2Authorize(env);
      const upload = await b2GetUploadUrl(auth, env.B2_BUCKET_ID);
      const objectKey = `${PREFIX}${payload.trigger || "report"}-${payload.archived_at_unix_ms || Date.now()}-${crypto.randomUUID()}.json`;
      await b2Upload(upload, objectKey, asText);
      return json({ ok: true, key: objectKey }, 201);
    } catch (e) {
      return json({ error: "storage write failed" }, 502);
    }
  },
};

function validatePayload(p) {
  if (!p || typeof p !== "object") return "body must be an object";
  if (typeof p.trigger !== "string" || !p.trigger) return "trigger required";
  if (typeof p.version !== "string") return "version required";
  if (typeof p.os !== "string") return "os required";
  if (!Array.isArray(p.events)) return "events must be an array";
  if (p.events.length > 500) return "too many events";
  for (const ev of p.events) {
    if (!ev || typeof ev !== "object") return "invalid event";
    if (typeof ev.message === "string" && ev.message.length > 8000) return "event message too long";
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

async function b2Upload(upload, fileName, bodyText) {
  const bytes = new TextEncoder().encode(bodyText);
  const hashBuffer = await crypto.subtle.digest("SHA-1", bytes);
  const sha1 = [...new Uint8Array(hashBuffer)].map((b) => b.toString(16).padStart(2, "0")).join("");

  const res = await fetch(upload.uploadUrl, {
    method: "POST",
    headers: {
      Authorization: upload.authorizationToken,
      "X-Bz-File-Name": encodeURIComponent(fileName),
      "Content-Type": "application/json",
      "Content-Length": String(bytes.byteLength),
      "X-Bz-Content-Sha1": sha1,
    },
    body: bytes,
  });
  if (!res.ok) throw new Error("b2 upload failed");
}

function json(obj, status = 200) {
  return new Response(JSON.stringify(obj), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}
