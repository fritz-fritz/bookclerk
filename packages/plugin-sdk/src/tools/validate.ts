/**
 * plugin.toml semantic validation (mirrors bookclerk-plugin-manifest rules).
 */

export type Manifest = {
  api_version: number;
  id: string;
  name?: string;
  kind: string;
  version?: string;
  logo?: string;
  runtime?: string;
  command?: string;
  args?: string[];
  workerd?: {
    compatibility_date: string;
    compatibility_flags?: string[];
    main_module: string;
    modules_dir?: string;
    entrypoint?: string;
    limits?: { cpu_ms?: number; subrequests?: number };
  };
  modules?: Array<{ name: string; path: string; type?: string }>;
  capabilities: {
    network: { mode: string; domains?: string[] };
    bindings?: Record<string, boolean>;
    methods?: { list?: string[] };
  };
  cli?: unknown;
};

const LOGO_EXTENSIONS = [
  ".png",
  ".jpg",
  ".jpeg",
  ".gif",
  ".webp",
  ".svg",
  ".ico",
] as const;

/** Classify and validate `plugin.toml` `logo` (mirrors Rust `validate_logo`). */
export function validateLogo(
  raw: string,
): { kind: "remote"; value: string } | { kind: "embedded"; value: string } {
  const trimmed = raw.trim();
  if (!trimmed) {
    throw new Error("plugin.toml: `logo` must not be empty (omit the key instead)");
  }
  if (trimmed.includes("\0")) {
    throw new Error("plugin.toml: `logo` must not contain NUL");
  }
  const lower = trimmed.toLowerCase();
  if (lower.includes("://") || lower.startsWith("javascript:") || lower.startsWith("data:")) {
    if (!(lower.startsWith("https://") || lower.startsWith("http://"))) {
      throw new Error(
        "plugin.toml: `logo` URL must use http:// or https:// (no javascript:/data:/file:)",
      );
    }
    const afterScheme = trimmed.replace(/^https?:\/\//i, "");
    if (!afterScheme) {
      throw new Error("plugin.toml: `logo` URL is missing a host");
    }
    const authority = afterScheme.split(/[/?#]/, 1)[0] ?? "";
    if (authority.includes("@")) {
      throw new Error(
        "plugin.toml: `logo` URL must not include userinfo (user:pass@host)",
      );
    }
    const host = (authority.split("%", 1)[0] ?? "").split(":", 1)[0]?.trim() ?? "";
    if (!host || host === "." || host === "..") {
      throw new Error("plugin.toml: `logo` URL is missing a host");
    }
    return { kind: "remote", value: trimmed };
  }
  const path = trimmed.replace(/\\/g, "/");
  if (path.startsWith("/") || path.startsWith("~")) {
    throw new Error(
      "plugin.toml: embedded `logo` must be a relative path under the plugin root",
    );
  }
  if (path.length >= 2 && path.charCodeAt(1) === 58 /* : */) {
    throw new Error(
      "plugin.toml: embedded `logo` must be a relative path (no drive letter)",
    );
  }
  if (path.startsWith("//")) {
    throw new Error("plugin.toml: embedded `logo` must be a relative path (no UNC)");
  }
  const segments: string[] = [];
  for (const seg of path.split("/")) {
    if (!seg || seg === ".") continue;
    if (seg === "..") {
      throw new Error("plugin.toml: embedded `logo` must not contain `..` segments");
    }
    segments.push(seg);
  }
  if (segments.length === 0) {
    throw new Error("plugin.toml: embedded `logo` path is empty after normalization");
  }
  const normalized = segments.join("/");
  const lowerPath = normalized.toLowerCase();
  if (!LOGO_EXTENSIONS.some((ext) => lowerPath.endsWith(ext))) {
    throw new Error(
      `plugin.toml: embedded \`logo\` must end with one of ${LOGO_EXTENSIONS.join(", ")}`,
    );
  }
  return { kind: "embedded", value: normalized };
}

export function validateManifest(m: Manifest): void {
  if (!m.id || !String(m.id).trim()) {
    throw new Error("plugin.toml: `id` is required");
  }
  if (m.api_version !== 1) {
    throw new Error("plugin.toml: `api_version` must be 1");
  }
  if (m.logo != null) {
    validateLogo(String(m.logo));
  }
  const kind = m.kind;
  if (!["source", "integration", "output", "database"].includes(kind)) {
    throw new Error(`plugin.toml: invalid kind ${kind}`);
  }
  const runtime = m.runtime ?? "native";
  if (runtime === "native") {
    if (!m.command || !String(m.command).trim()) {
      throw new Error(
        'plugin.toml: `command` is required when runtime = "native"',
      );
    }
    if (
      m.capabilities?.network?.domains &&
      m.capabilities.network.domains.length > 0
    ) {
      throw new Error(
        'plugin.toml: capabilities.network.domains is only valid for runtime = "workerd" (native outbound is coarse jail networking with no hostname filter — omit domains)',
      );
    }
  } else if (runtime === "workerd") {
    if (!m.workerd) {
      throw new Error(
        'plugin.toml: `[workerd]` is required when runtime = "workerd"',
      );
    }
    if (!m.workerd.compatibility_date?.trim()) {
      throw new Error("plugin.toml: workerd.compatibility_date is required");
    }
    if (!m.workerd.main_module?.trim()) {
      throw new Error("plugin.toml: workerd.main_module is required");
    }
    if (
      m.capabilities?.network?.mode === "outbound" &&
      (!m.capabilities.network.domains ||
        m.capabilities.network.domains.length === 0)
    ) {
      throw new Error(
        'plugin.toml: capabilities.network.domains is required when runtime = "workerd" and mode = "outbound"',
      );
    }
  } else {
    throw new Error(`plugin.toml: unknown runtime ${runtime}`);
  }
}
