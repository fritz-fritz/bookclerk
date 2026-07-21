/**
 * Libation diagnostics collector — Cloudflare Worker.
 *
 * Accepts redacted crash/ERROR reports from Libation clients and opens a
 * GitHub Issue. Clients only need `diagnostics.share_reports = true`.
 *
 * Secrets (set via `wrangler secret put`):
 *   GITHUB_TOKEN  — fine-grained PAT or classic token with Issues: write
 *
 * Vars (wrangler.toml [vars]):
 *   GITHUB_REPO   — owner/repo (default fritz-fritz/libation-rs)
 *   ISSUE_LABELS  — comma-separated labels that already exist (optional)
 */

const DEFAULT_REPO = "fritz-fritz/libation-rs";

export default {
  async fetch(request, env) {
    const url = new URL(request.url);

    if (request.method === "GET" && (url.pathname === "/" || url.pathname === "/health")) {
      return json({ ok: true, service: "libation-diagnostics" });
    }

    if (request.method !== "POST" || url.pathname !== "/v1/report") {
      return json({ error: "not found" }, 404);
    }

    if (!env.GITHUB_TOKEN) {
      return json({ error: "collector misconfigured" }, 503);
    }

    let payload;
    try {
      payload = await request.json();
    } catch {
      return json({ error: "invalid json" }, 400);
    }

    const repo = (env.GITHUB_REPO || DEFAULT_REPO).trim();
    const [owner, name] = repo.split("/");
    if (!owner || !name) {
      return json({ error: "invalid GITHUB_REPO" }, 503);
    }

    const title = `[diagnostics] ${payload.trigger || "report"} · libation ${
      payload.version || "?"
    } · ${payload.os || "?"}`;

    const events = Array.isArray(payload.events) ? payload.events : [];
    const body = [
      "<!-- libation-diagnostics: auto-filed via collector; client redacts secrets -->",
      "## Libation diagnostics report",
      "",
      `- **Trigger:** \`${payload.trigger || "?"}\``,
      `- **Version:** \`${payload.version || "?"}\``,
      `- **OS:** \`${payload.os || "?"}\``,
      `- **Archived at (unix ms):** \`${payload.archived_at_unix_ms || "?"}\``,
      `- **Events:** ${events.length}`,
      "",
      "Submitted because an operator set `diagnostics.share_reports = true`.",
      "Auth tokens, passwords, DRM material, titles, and paths are redacted client-side.",
      "",
      "### Recent log events (redacted)",
      "",
      "```json",
      JSON.stringify(events, null, 2).slice(0, 55000),
      "```",
      "",
    ].join("\n");

    const issue = { title, body };
    const labels = (env.ISSUE_LABELS || "")
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);
    if (labels.length) issue.labels = labels;

    const gh = await fetch(`https://api.github.com/repos/${owner}/${name}/issues`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${env.GITHUB_TOKEN}`,
        Accept: "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "libation-diagnostics-collector",
        "Content-Type": "application/json",
      },
      body: JSON.stringify(issue),
    });

    const text = await gh.text();
    if (!gh.ok) {
      return json({ error: "github issue create failed", status: gh.status }, 502);
    }

    let htmlUrl = `https://github.com/${owner}/${name}/issues`;
    try {
      htmlUrl = JSON.parse(text).html_url || htmlUrl;
    } catch {
      /* ignore */
    }
    return json({ ok: true, html_url: htmlUrl }, 201);
  },
};

function json(obj, status = 200) {
  return new Response(JSON.stringify(obj), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}
