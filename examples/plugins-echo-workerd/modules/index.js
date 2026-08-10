/**
 * Echo workerd guest module (MVP loadable artifact).
 *
 * Branding: this class is the JS sibling of `EchoPlugin extends BookclerkPlugin`
 * from `@bookclerk/plugin-sdk` / `src/index.ts`. Authors write against
 * BookclerkPlugin — not raw WorkerEntrypoint. bookclerk-workerd loads this
 * module from `modules/` per plugin.toml.
 *
 * Until a full TS→workerd build pipeline ships, keep this file in sync with
 * `src/index.ts`.
 */

const API_VERSION = 1;
const PLUGIN_ID = "echo";
const KIND = "integration";

const CLI = {
  commands: [
    {
      name: "ping",
      about: "Probe echo plugin",
      args: [
        {
          name: "message",
          long: "message",
          kind: "string",
          default: "hi",
        },
      ],
    },
  ],
};

/** @implements {import("@bookclerk/plugin-sdk").BookclerkPlugin} surface */
export default class EchoPlugin {
  /**
   * @param {unknown} ctx
   * @param {{ HOST?: { notify(event: unknown): Promise<void> } }} env
   */
  constructor(ctx, env) {
    this.ctx = ctx;
    this.env = env ?? {};
  }

  /** @param {{ apiVersion: number, config?: object }} _params */
  async handshake(_params) {
    return {
      apiVersion: API_VERSION,
      id: PLUGIN_ID,
      kind: KIND,
      displayName: "Echo Integration",
      capabilities: ["health", "diagnose", "onEvent", "cli"],
      cli: CLI,
    };
  }

  async shutdown() {}

  async health() {
    return {
      ok: true,
      id: PLUGIN_ID,
      enabled: true,
      detail: "echo workerd plugin ready",
    };
  }

  async diagnose() {
    return { lines: ["echo: ok"] };
  }

  /**
   * @param {{ type: string, payload?: { titleId?: string } }} event
   */
  async onEvent(event) {
    if (event?.type === "book_acquired") {
      const titleId = event.payload?.titleId ?? "";
      if (this.env?.HOST?.notify) {
        await this.env.HOST.notify({
          type: "plugin_log",
          payload: {
            level: "info",
            message: `echo saw book_acquired titleId=${titleId}`,
          },
        });
      }
    }
  }

  async cliDescribe() {
    return CLI;
  }

  /**
   * @param {{ command: string, args?: Record<string, unknown> }} params
   */
  async cliInvoke(params) {
    if (params?.command !== "ping") {
      return {
        exitCode: 2,
        stderr: `unknown command ${params?.command ?? ""}`,
      };
    }
    const message =
      typeof params.args?.message === "string" ? params.args.message : "hi";
    return {
      exitCode: 0,
      stdout: `pong: ${message}\n`,
      json: { pong: message },
    };
  }
}
