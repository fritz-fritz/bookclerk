/// <reference path="./cloudflare-workers.d.ts" />
import { WorkerEntrypoint } from "cloudflare:workers";
import type {
  BookclerkEnv,
  CliInvokeParams,
  CliInvokeResult,
  CliSchema,
  DiagnoseResult,
  HandshakeParams,
  HandshakeResult,
  HealthResult,
  HostToPluginEvent,
} from "./generated.js";

/**
 * Branded guest base — extends `WorkerEntrypoint` with Bookclerk env + contract hooks.
 *
 * Authors subclass this type (never bare `WorkerEntrypoint`) and override the
 * methods their `capabilities.methods` declare.
 *
 * Required: {@link handshake}.
 *
 * Defaults: {@link shutdown}, {@link health}, {@link diagnose}.
 *
 * Optional overrides (omit or leave unimplemented until needed):
 * `onEvent`, `cliDescribe`, `cliInvoke`, `start`, `pollEvents`, kind-specific
 * methods (`scan`, `fetchTitle`, storage/db ops, …).
 */
export abstract class BookclerkPlugin extends WorkerEntrypoint<BookclerkEnv> {
  /** Bookclerk host bindings (`HOST`, `CONFIG`, `PLUGIN_KV`, …). */
  declare readonly env: BookclerkEnv;

  /** Identity, capabilities, CLI schema, brand. */
  abstract handshake(params: HandshakeParams): Promise<HandshakeResult>;

  /** Graceful teardown. */
  async shutdown(): Promise<void> {}

  /** Liveness probe. */
  async health(): Promise<HealthResult> {
    return { ok: true };
  }

  /** Human-readable diagnostic lines. */
  async diagnose(): Promise<DiagnoseResult> {
    return { lines: [] };
  }

  /**
   * Host → plugin event delivery (`onEvent`).
   * Override when advertising the `onEvent` capability.
   */
  async onEvent(_event: HostToPluginEvent): Promise<void> {
    throw unsupported("onEvent");
  }

  /** Declared CLI surface (mirrors handshake `cli` when present). */
  async cliDescribe(): Promise<CliSchema> {
    return { commands: [] };
  }

  /**
   * Run a plugin CLI command (`cliInvoke`).
   * Override when advertising the `cli` capability.
   */
  async cliInvoke(_params: CliInvokeParams): Promise<CliInvokeResult> {
    throw unsupported("cliInvoke");
  }
}

function unsupported(method: string): Error {
  return Object.assign(new Error(`${method} not implemented`), {
    code: "unsupported" as const,
  });
}
