/**
 * Downloads / refreshes the pinned Cloudflare `workerd` binary (mirrors `ensure.rs`).
 */

import { createHash } from "node:crypto";
import fs from "node:fs";
import { createGunzip } from "node:zlib";
import { pipeline } from "node:stream/promises";
import { Readable } from "node:stream";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import os from "node:os";

/**
 * One platform asset entry inside {@link WorkerdPin}.
 */
export type WorkerdAsset = {
  /** Gzip-compressed release artifact filename on the Cloudflare workerd GitHub release. */
  artifact: string;
  /** Expected SHA-256 hex digest of the compressed artifact bytes (not the gunzipped binary). */
  sha256_hex: string;
};

/**
 * Contents of `workerd-pin.json` shipping with this package.
 */
export type WorkerdPin = {
  /** GitHub release tag (for example `v1.20250310.0`) that identifies this pin. */
  release_tag: string;
  /** Compatibility date bundled with this pin for Cap'n Proto configs. */
  bundled_compat_date: string;
  /** Stamp filename written beside the installed binary to record `release_tag`. */
  version_stamp: string;
  /** Platform key → artifact map (`linux-x86_64`, `macos-aarch64`, `windows-x86_64`, …). */
  assets: Record<string, WorkerdAsset>;
};

/**
 * Resolves the `@bookclerk/plugin-sdk` package root.
 *
 * Works whether this module is loaded from `src/sparse-workerd` or
 * `dist/sparse-workerd`.
 *
 * @returns Absolute path to `packages/plugin-sdk`.
 */
export function packageRoot(): string {
  const here = path.dirname(fileURLToPath(import.meta.url));
  // dist/sparse-workerd or src/sparse-workerd → package root
  return path.resolve(here, "../..");
}

/**
 * Loads and parses `workerd-pin.json` from the package root.
 *
 * @param root - Package root override (defaults to {@link packageRoot}).
 * @returns Parsed pin document.
 */
export function loadPin(root = packageRoot()): WorkerdPin {
  const pinPath = assertPathInside(path.resolve(root), "workerd-pin.json");
  return JSON.parse(fs.readFileSync(pinPath, "utf8")) as WorkerdPin;
}

/**
 * Maps Node `process.platform` / `process.arch` to a pin asset key.
 *
 * @param platform - Node platform string (default `process.platform`).
 * @param arch - Node arch string (default `process.arch`).
 * @returns Pin asset key, or `undefined` when the host is unsupported.
 */
export function platformKey(
  platform = process.platform,
  arch = process.arch,
): string | undefined {
  const map: Record<string, string> = {
    "linux-x64": "linux-x86_64",
    "linux-arm64": "linux-aarch64",
    "darwin-x64": "macos-x86_64",
    "darwin-arm64": "macos-aarch64",
    "win32-x64": "windows-x86_64",
  };
  return map[`${platform}-${arch}`];
}

/**
 * Returns the on-disk workerd binary basename for the current platform.
 *
 * @returns `workerd.exe` on Windows, otherwise `workerd`.
 */
export function binaryName(): string {
  return process.platform === "win32" ? "workerd.exe" : "workerd";
}

/**
 * Builds the GitHub release download URL for a pin artifact.
 *
 * @param pin - Loaded workerd pin.
 * @param artifact - Artifact filename from {@link WorkerdAsset.artifact}.
 * @returns Absolute HTTPS download URL.
 */
export function downloadUrl(pin: WorkerdPin, artifact: string): string {
  return `https://github.com/cloudflare/workerd/releases/download/${pin.release_tag}/${artifact}`;
}

/**
 * Resolves the preferred workerd cache directory.
 *
 * Honors `BOOKCLERK_WORKERD_CACHE`, otherwise uses
 * `~/.cache/bookclerk/workerd`.
 *
 * @returns Absolute cache directory path.
 */
export function defaultCacheDir(): string {
  if (process.env.BOOKCLERK_WORKERD_CACHE) {
    return process.env.BOOKCLERK_WORKERD_CACHE;
  }
  const home = os.homedir();
  return path.join(home, ".cache", "bookclerk", "workerd");
}

/**
 * Resolves `candidate` and requires it to stay under `root`.
 *
 * Rejects NUL bytes and `..` path components (names like `..draft` and
 * `edition..2` are allowed). Does not follow symlinks — callers that write or
 * embed must also use {@link refuseSymlinkPath}.
 *
 * @param root - Trusted directory (resolved).
 * @param candidate - Absolute path or path relative to `root`.
 * @returns Absolute path under `root`.
 * @throws {Error} When the path is empty, contains NUL/`..`, or escapes `root`.
 */
export function assertPathInside(root: string, candidate: string): string {
  if (!root || root.includes("\0") || !candidate || candidate.includes("\0")) {
    throw new Error("path is empty or contains NUL");
  }
  const normalized = candidate.replace(/\\/g, "/");
  if (normalized.split("/").includes("..")) {
    throw new Error(`path must not contain '..': ${candidate}`);
  }
  const resolvedRoot = path.resolve(root);
  const resolved = path.isAbsolute(candidate)
    ? path.resolve(candidate)
    : path.resolve(resolvedRoot, candidate);
  const rel = path.relative(resolvedRoot, resolved);
  if (path.isAbsolute(rel)) {
    throw new Error(`path ${resolved} escapes root ${resolvedRoot}`);
  }
  const segments = rel.split(path.sep).filter((s) => s.length > 0);
  if (segments.some((s) => s === "..")) {
    throw new Error(`path ${resolved} escapes root ${resolvedRoot}`);
  }
  return resolved;
}

/**
 * Require `candidate` under `trustedRoot` with no symlink suffix components.
 *
 * Resolves the trusted root identity once (a symlinked operator/plugin root is
 * allowed). Only path components *below* that identity are refused when they
 * are symlinks.
 *
 * @param trustedRoot - Original operator/plugin root.
 * @param candidate - Path previously produced by {@link assertPathInside}.
 * @returns The validated absolute path.
 * @throws {Error} When a suffix component is a symlink or escapes `trustedRoot`.
 */
export function refuseSymlinkPath(trustedRoot: string, candidate: string): string {
  const rootLex = path.resolve(trustedRoot);
  const target = path.resolve(candidate);
  const rel = path.relative(rootLex, target);
  if (path.isAbsolute(rel) || rel.split(path.sep).includes("..")) {
    throw new Error(`path ${target} escapes root ${rootLex}`);
  }
  // Allow the operator-selected root itself to be a symlink; constrain children.
  const root = fs.realpathSync(rootLex);
  const parts = rel === "" ? [] : rel.split(path.sep);
  let cur = root;
  for (let i = 0; i < parts.length; i++) {
    cur = path.join(cur, parts[i]!);
    let st: fs.Stats;
    try {
      st = fs.lstatSync(cur);
    } catch (err) {
      const code = (err as NodeJS.ErrnoException).code;
      if (code === "ENOENT" && i === parts.length - 1) {
        break;
      }
      throw err;
    }
    if (st.isSymbolicLink()) {
      throw new Error(`refusing symlink in path: ${cur}`);
    }
  }
  return target;
}

/**
 * Validates a URL before `fetch` / download.
 *
 * Allows `https:` anywhere, or `http:` only to loopback hosts.
 *
 * @param url - Absolute URL string.
 * @returns Canonical href safe to request.
 * @throws {Error} When the URL is invalid or uses a disallowed scheme/host.
 */
export function validateFetchUrl(url: string): string {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    throw new Error(`invalid URL: ${url}`);
  }
  if (parsed.protocol === "https:") {
    return parsed.href;
  }
  if (parsed.protocol === "http:") {
    const host = parsed.hostname.toLowerCase();
    if (
      host === "127.0.0.1" ||
      host === "localhost" ||
      host === "::1" ||
      host === "[::1]"
    ) {
      return parsed.href;
    }
  }
  throw new Error(
    `refusing non-HTTPS (or non-loopback HTTP) URL: ${parsed.protocol}//${parsed.host}`,
  );
}

/**
 * Validates a workerd (or helper) binary path before spawn.
 *
 * Requires an absolute path with no NUL bytes. When `trustedRoot` is set,
 * resolves both paths and requires the binary to stay under that root.
 *
 * @param bin - Candidate executable path.
 * @param trustedRoot - Optional directory the binary must remain under.
 * @returns Absolute validated path.
 * @throws {Error} When the path is relative, contains NUL, or escapes `trustedRoot`.
 */
export function validateSpawnExecutable(
  bin: string,
  trustedRoot?: string,
): string {
  if (!bin || bin.includes("\0")) {
    throw new Error("spawn executable path is empty or contains NUL");
  }
  if (!path.isAbsolute(bin)) {
    throw new Error(`spawn executable must be absolute: ${bin}`);
  }
  if (trustedRoot) {
    return assertPathInside(trustedRoot, bin);
  }
  return path.resolve(bin);
}

function isCurrent(bin: string, pin: WorkerdPin): boolean {
  const dir = path.resolve(path.dirname(bin));
  const stamp = assertPathInside(dir, pin.version_stamp);
  if (fs.existsSync(stamp)) {
    const text = fs.readFileSync(stamp, "utf8").trim();
    if (text === pin.release_tag) return true;
  }
  const validated = validateSpawnExecutable(bin);
  // Probe the selected path directly (do not rewrite PATH / spawn by basename).
  const out = spawnSync(validated, ["--version"], {
    encoding: "utf8",
    shell: false,
  });
  if (out.status !== 0) return false;
  const combined = `${out.stdout ?? ""}${out.stderr ?? ""}`;
  const pinBare = pin.release_tag.replace(/^v/, "");
  return combined.includes(pin.release_tag) || combined.includes(pinBare);
}

/**
 * Ensures `cacheDir/workerd` matches the pin, downloading if needed.
 *
 * Honors `BOOKCLERK_WORKERD_BIN` when that binary exists and matches the pin.
 *
 * @param cacheDir - Directory that will hold the binary (default {@link defaultCacheDir}).
 * @param root - Package root for loading the pin (default {@link packageRoot}).
 * @returns Absolute path to a current `workerd` binary.
 * @throws {Error} When the platform has no pin, download fails, or the digest mismatches.
 */
export async function ensureWorkerd(
  cacheDir = defaultCacheDir(),
  root = packageRoot(),
): Promise<string> {
  const pin = loadPin(root);
  const override = process.env.BOOKCLERK_WORKERD_BIN;
  if (override && fs.existsSync(override) && isCurrent(override, pin)) {
    // Env override: absolute file only (may live outside the cache dir).
    return validateSpawnExecutable(override);
  }

  const absCache = path.resolve(cacheDir);
  fs.mkdirSync(absCache, { recursive: true });
  const dest = assertPathInside(absCache, binaryName());
  if (fs.existsSync(dest) && isCurrent(dest, pin)) {
    return validateSpawnExecutable(dest, absCache);
  }

  const key = platformKey();
  if (!key || !pin.assets[key]) {
    throw new Error(
      `no pinned workerd asset for ${process.platform}-${process.arch}`,
    );
  }
  const asset = pin.assets[key]!;
  const url = validateFetchUrl(downloadUrl(pin, asset.artifact));
  console.error(`bookclerk-plugin: fetching ${url}`);
  // HTTPS (or loopback HTTP) URL validated by validateFetchUrl above.
  const res = await fetch(url);
  if (!res.ok) {
    throw new Error(`GET ${url} returned ${res.status}`);
  }
  const compressed = Buffer.from(await res.arrayBuffer());
  const got = createHash("sha256").update(compressed).digest("hex");
  if (got !== asset.sha256_hex) {
    throw new Error(
      `workerd download sha256 mismatch: got ${got}, expected ${asset.sha256_hex}`,
    );
  }

  const tmp = assertPathInside(absCache, `${binaryName()}.tmp`);
  await pipeline(
    Readable.from(compressed),
    createGunzip(),
    fs.createWriteStream(tmp),
  );
  if (process.platform !== "win32") {
    fs.chmodSync(tmp, 0o755);
  }
  fs.renameSync(tmp, dest);
  const stampPath = assertPathInside(absCache, pin.version_stamp);
  fs.writeFileSync(stampPath, `${pin.release_tag}\n`);
  console.error(
    `bookclerk-plugin: installed ${pin.release_tag} → ${dest}`,
  );
  return validateSpawnExecutable(dest, absCache);
}
