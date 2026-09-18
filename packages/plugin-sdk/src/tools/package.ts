/**
 * Packages a plugin directory into a distributable `.tar.gz` archive.
 */

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { parse as parseToml } from "smol-toml";
import { validateLogo, validateManifest, type Manifest } from "./validate.js";
import { assertPathInside, refuseSymlinkPath } from "../sparse-workerd/ensure.js";

function hostTarget(): string {
  const plat = process.platform;
  const arch = process.arch;
  if (plat === "linux" && arch === "x64") return "linux-x64-gnu";
  if (plat === "linux" && arch === "arm64") return "linux-arm64";
  if (plat === "darwin" && arch === "arm64") return "macos-arm64";
  if (plat === "darwin" && arch === "x64") return "macos-x64";
  if (plat === "win32" && arch === "x64") return "windows-x64";
  return `${plat}-${arch}`;
}

/**
 * Copy a directory tree, refusing symlinks and non-file/non-dir entries.
 *
 * @param src - Source directory (must not itself be a symlink).
 * @param dst - Destination directory to create.
 * @throws {Error} When a symlink or unsupported type is found.
 */
function copyRecursiveNoSymlinks(src: string, dst: string): void {
  const srcSt = fs.lstatSync(src);
  if (srcSt.isSymbolicLink()) {
    throw new Error(`refusing symlink package source: ${src}`);
  }
  if (!srcSt.isDirectory()) {
    throw new Error(`package source is not a directory: ${src}`);
  }
  fs.mkdirSync(dst, { recursive: true });
  for (const ent of fs.readdirSync(src, { withFileTypes: true })) {
    const from = path.join(src, ent.name);
    const to = path.join(dst, ent.name);
    if (ent.isSymbolicLink()) {
      throw new Error(`refusing symlink in package source: ${from}`);
    }
    if (ent.isDirectory()) {
      copyRecursiveNoSymlinks(from, to);
    } else if (ent.isFile()) {
      fs.copyFileSync(from, to);
    } else {
      throw new Error(`refusing unsupported package source type: ${from}`);
    }
  }
}

/**
 * Packages `pluginDir` into `outDir` and returns the archive path.
 *
 * Native archives include the command binary and are tagged with the host
 * target triple. Workerd archives include the modules tree (the SDK is injected
 * by `bookclerk-workerd` at serve time). Updates `SHA256SUMS` beside the
 * archive.
 *
 * Absolute native `command` paths are treated as operator-selected build
 * outputs. Relative package sources under the plugin tree refuse symlinks so
 * outside bytes cannot enter the archive.
 *
 * @param pluginDir - Plugin root containing `plugin.toml`.
 * @param outDir - Destination directory for the archive and checksums.
 * @returns Absolute path to the created `.tar.gz`.
 * @throws {Error} When validation fails, assets are missing, or `tar` fails.
 *
 * @example
 * ```ts
 * const archive = packagePlugin("./my-plugin", "./dist");
 * console.log(`packed ${archive}`);
 * ```
 */
export function packagePlugin(pluginDir: string, outDir: string): string {
  const root = path.resolve(pluginDir);
  const tomlPath = assertPathInside(root, "plugin.toml");
  const m = parseToml(fs.readFileSync(tomlPath, "utf8")) as Manifest;
  validateManifest(m);
  const version = m.version ?? "0.0.0";
  const id = m.id;
  fs.mkdirSync(outDir, { recursive: true });
  const staging = path.join(outDir, `.staging-${id}`);
  fs.rmSync(staging, { recursive: true, force: true });
  fs.mkdirSync(staging, { recursive: true });
  fs.copyFileSync(tomlPath, path.join(staging, "plugin.toml"));

  if (m.logo != null) {
    const logo = validateLogo(String(m.logo));
    if (logo.kind === "embedded") {
      const src = assertPathInside(root, logo.value);
      refuseSymlinkPath(root, src);
      if (!fs.existsSync(src) || fs.lstatSync(src).isSymbolicLink() || !fs.statSync(src).isFile()) {
        throw new Error(`embedded logo missing for package: ${src}`);
      }
      const dest = path.join(staging, logo.value);
      fs.mkdirSync(path.dirname(dest), { recursive: true });
      fs.copyFileSync(src, dest);
    }
  }

  const runtime = m.runtime ?? "native";
  let archiveStem: string;
  if (runtime === "native") {
    const cmd = m.command!;
    const absolute = path.isAbsolute(cmd);
    const src = absolute ? path.resolve(cmd) : assertPathInside(root, cmd);
    if (!absolute) {
      refuseSymlinkPath(root, src);
      if (fs.lstatSync(src).isSymbolicLink()) {
        throw new Error(`refusing symlink native command under plugin: ${src}`);
      }
    }
    if (!fs.existsSync(src) || !fs.statSync(src).isFile()) {
      throw new Error(`native binary not found for package: ${src}`);
    }
    const binName = path.basename(src);
    fs.copyFileSync(src, path.join(staging, binName));
    try {
      fs.chmodSync(path.join(staging, binName), 0o755);
    } catch {
      /* windows */
    }
    archiveStem = `bookclerk-plugin-${id}-${version}-${hostTarget()}`;
  } else {
    const modulesDir = m.workerd?.modules_dir ?? "modules";
    const srcModules = assertPathInside(root, modulesDir);
    refuseSymlinkPath(root, srcModules);
    copyRecursiveNoSymlinks(srcModules, path.join(staging, modulesDir));
    // Authors import `@bookclerk/plugin-sdk/workerd`; bookclerk-workerd injects it.
    archiveStem = `bookclerk-plugin-${id}-${version}-workerd`;
  }

  const archiveName = `${archiveStem}.tar.gz`;
  const archivePath = path.join(outDir, archiveName);
  const tar = spawnSync(
    "tar",
    ["-C", staging, "-czf", archivePath, "."],
    { encoding: "utf8" },
  );
  if (tar.status !== 0) {
    // Do not leave a partial archive advertised via SHA256SUMS.
    try {
      fs.rmSync(archivePath, { force: true });
    } catch {
      /* ignore */
    }
    fs.rmSync(staging, { recursive: true, force: true });
    throw new Error(`tar failed: ${tar.stderr || tar.stdout}`);
  }
  fs.rmSync(staging, { recursive: true, force: true });

  const digest = crypto
    .createHash("sha256")
    .update(fs.readFileSync(archivePath))
    .digest("hex");
  const sumsPath = path.join(outDir, "SHA256SUMS");
  let body = "";
  if (fs.existsSync(sumsPath)) {
    body = fs
      .readFileSync(sumsPath, "utf8")
      .split("\n")
      .filter((l) => l && !l.endsWith(archiveName))
      .join("\n");
    if (body && !body.endsWith("\n")) body += "\n";
  }
  body += `${digest}  ${archiveName}\n`;
  fs.writeFileSync(sumsPath, body);
  return archivePath;
}
