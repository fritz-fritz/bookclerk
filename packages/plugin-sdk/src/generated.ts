/**
 * TypeScript projection of `crates/bookclerk-plugin-abi/schema/abi.json`.
 *
 * Field names are camelCase to match the Workers RPC wire format. Keep in sync
 * with the schema and the Rust types in `bookclerk-plugin-abi`. Run
 * `npm run check-schema` to assert the authoritative JSON still exists.
 *
 * See also `docs/plugins.md` for the guest contract narrative.
 */

/**
 * Wire API version negotiated during handshake for all guests.
 */
export const API_VERSION = 1 as const;

/**
 * Plugin surface kind advertised in handshake and `plugin.toml`.
 */
export type PluginKind = "source" | "integration" | "output" | "database";

/**
 * Stable error codes carried on {@link PluginError.code}.
 */
export type PluginErrorCode =
  | "invalid_params"
  | "unauthorized"
  | "forbidden"
  | "not_found"
  | "unavailable"
  | "unsupported"
  | "internal";

/**
 * RPC or plugin failure payload returned on the wire.
 */
export interface PluginError {
  /** Stable machine-readable failure code. */
  code: PluginErrorCode;
  /** Human-readable error message (secrets must already be redacted). */
  message: string;
  /** Optional structured details for operators or host UIs. */
  details?: Record<string, unknown>;
}

/**
 * Loose JSON object used for config blobs and CLI argument maps.
 */
export type JsonObject = Record<string, unknown>;

/**
 * Parameters for the required `handshake` RPC method.
 */
export interface HandshakeParams {
  /** Host-offered ABI version (must be {@link API_VERSION}). */
  apiVersion: typeof API_VERSION | 1;
  /** Install / operator config object passed into the guest. */
  config: JsonObject;
}

/**
 * Brand colors and icon for UI chrome.
 */
export interface BrandDto {
  /** Stable brand identifier (often matches plugin id). */
  id: string;
  /** Display name shown beside the brand mark. */
  name: string;
  /** Background color (CSS color string). */
  bg: string;
  /** Foreground / text color (CSS color string). */
  fg: string;
  /** Accent color (CSS color string). */
  accent: string;
  /** Absolute or relative URL for the brand icon. */
  iconUrl: string;
}

/**
 * One selectable value under a {@link ConfigOptionDto}.
 */
export interface ConfigOptionValueDto {
  /** Machine id for the option value. */
  id: string;
  /** Operator-facing label. */
  label: string;
}

/**
 * Config option descriptor discovered during handshake for sources.
 */
export interface ConfigOptionDto {
  /** Config key written when the operator picks a value. */
  key: string;
  /** Operator-facing label for the option group. */
  label: string;
  /** Allowed values for this option. */
  values: ConfigOptionValueDto[];
}

/**
 * CLI argument value kind for plugin-declared commands.
 */
export type CliArgKind = "string" | "bool" | "int" | "path";

/**
 * One CLI argument declared by a plugin command.
 */
export interface CliArgSpec {
  /** Internal argument name (also the default long flag). */
  name: string;
  /** Long flag without leading `--`. */
  long?: string;
  /** Short flag character without leading `-`. */
  short?: string;
  /** Value kind; defaults to `string` when omitted. */
  kind?: CliArgKind;
  /** Whether the host must supply this argument. */
  required?: boolean;
  /** Default value as a string when the flag is omitted. */
  default?: string;
  /** Short help text for the argument. */
  about?: string;
  /** When true, the argument is positional rather than a flag. */
  positional?: boolean;
}

/**
 * One plugin CLI command declared in handshake / `cliDescribe`.
 */
export interface CliCommandSpec {
  /** Command name invoked via `cliInvoke`. */
  name: string;
  /** Short help text for the command. */
  about?: string;
  /** Arguments accepted by this command. */
  args?: CliArgSpec[];
}

/**
 * Declared CLI surface returned from handshake or `cliDescribe`.
 */
export interface CliSchema {
  /** Commands the guest exposes to `bookclerk plugin` / host plumbing. */
  commands?: CliCommandSpec[];
}

/**
 * Result of a successful `handshake` negotiation.
 */
export interface HandshakeResult {
  /** Negotiated ABI version (must match {@link API_VERSION}). */
  apiVersion: typeof API_VERSION | 1;
  /** Globally unique plugin id (`[a-z0-9_]{2,32}` grammar). */
  id: string;
  /** Plugin kind (`source`, `integration`, `output`, or `database`). */
  kind: PluginKind | string;
  /** Optional operator-facing display name. */
  displayName?: string;
  /** Capability method names this guest implements. */
  capabilities: string[];
  /** Optional UI brand block. */
  brand?: BrandDto;
  /** Optional config options for the Accounts / Settings UI. */
  configOptions?: ConfigOptionDto[];
  /** Optional CLI schema (may also be returned from `cliDescribe`). */
  cli?: CliSchema;
  /** Portal login mode when the guest supports connect-portal auth. */
  portalAuthMode?: "oauth" | "password";
  /** Env var name holding a password for password-mode portal auth. */
  passwordEnvVar?: string;
  /** Alternate ids accepted by the host for this install. */
  aliases?: string[];
  /** Sort key for stable ordering in UI lists (lower first). */
  sortKey?: number;
}

/**
 * Parameters for the `cliInvoke` RPC method.
 */
export interface CliInvokeParams {
  /** Command name matching a {@link CliCommandSpec.name}. */
  command: string;
  /** Named argument map (flag names → values). */
  args?: JsonObject;
}

/**
 * Result of a `cliInvoke` call.
 */
export interface CliInvokeResult {
  /** Process-style exit code (0 = success). */
  exitCode?: number;
  /** Captured stdout text. */
  stdout?: string;
  /** Captured stderr text. */
  stderr?: string;
  /** Optional structured JSON payload alongside text output. */
  json?: unknown;
}

/**
 * Result of the `health` RPC method.
 */
export interface HealthResult {
  /** Whether the guest considers itself healthy. */
  ok: boolean;
  /** Optional plugin id echo for host adapters. */
  id?: string;
  /** Optional enablement flag for host adapters. */
  enabled?: boolean;
  /** Optional human-readable health detail. */
  detail?: string;
}

/**
 * Result of the `diagnose` RPC method (`plugins doctor`).
 */
export interface DiagnoseResult {
  /** Operator-facing diagnostic lines (empty when nothing to report). */
  lines: string[];
}

/**
 * Host → plugin payload when a title has been acquired.
 */
export interface BookAcquiredPayload {
  /** Library title id. */
  titleId: string;
  /** Source plugin / storefront id that produced the title. */
  source: string;
  /** Optional Audible ASIN. */
  asin?: string;
  /** Optional ISBN. */
  isbn?: string;
  /** Destination object keys written for this acquire. */
  pathKeys: string[];
}

/**
 * Host → plugin payload when a library scan finishes.
 */
export interface LibraryScanCompletedPayload {
  /** Source that was scanned. */
  source: string;
  /** Number of titles upserted into the library. */
  upserted: number;
}

/**
 * Host → plugin payload when operator config changes.
 */
export interface ConfigChangedPayload {
  /** Updated config object for the guest install. */
  config: JsonObject;
}

/**
 * Host → plugin event envelope delivered via `onEvent`.
 *
 * Discriminated by `type` using snake_case event names from the ABI schema.
 */
export type HostToPluginEvent =
  | { type: "book_acquired"; payload: BookAcquiredPayload }
  | { type: "library_scan_completed"; payload: LibraryScanCompletedPayload }
  | { type: "config_changed"; payload: ConfigChangedPayload };

/**
 * Severity for {@link PluginLogPayload} events pushed to the host.
 */
export type PluginLogLevel = "debug" | "info" | "warn" | "error";

/**
 * Plugin → host payload listing external users for portal sync.
 */
export interface ExternalUsersPayload {
  /** Opaque user records understood by the host integration layer. */
  users: unknown[];
}

/**
 * Plugin → host payload for listening-progress sync.
 */
export interface ListeningProgressPayload {
  /** Opaque progress items understood by the host. */
  items: unknown[];
}

/**
 * Plugin → host structured log line.
 */
export interface PluginLogPayload {
  /** Log severity. */
  level: PluginLogLevel;
  /** Message text (secrets must already be redacted). */
  message: string;
}

/**
 * Plugin → host event envelope sent through `env.HOST.notify`.
 *
 * Discriminated by `type` using snake_case event names from the ABI schema.
 */
export type PluginToHostEvent =
  | { type: "external_users"; payload: ExternalUsersPayload }
  | { type: "listening_progress"; payload: ListeningProgressPayload }
  | { type: "plugin_log"; payload: PluginLogPayload };

/**
 * Host binding used by guests to push {@link PluginToHostEvent} notifications.
 */
export interface HostBinding {
  /**
   * Delivers a plugin → host event on the reverse notify channel.
   *
   * @param event - Event envelope to send.
   * @returns Resolves when the host acknowledges the notify.
   */
  notify(event: PluginToHostEvent): Promise<void>;
}

/**
 * Guest `env` bindings declared by the ABI / `capabilities.bindings`.
 *
 * Only `HOST` is required for event push; other bindings appear when the
 * operator has consented to them in `plugin.toml`.
 */
export interface BookclerkEnv {
  /** Reverse channel for plugin → host events. */
  HOST: HostBinding;
  /** Operator config object when the `config` binding is enabled. */
  CONFIG?: JsonObject;
  /** Sealed secrets binding when the `secrets` capability is enabled. */
  SECRETS?: unknown;
  /** Per-plugin KV store when the `plugin_kv` binding is enabled. */
  PLUGIN_KV?: unknown;
  /** Work filesystem binding when the `work_fs` capability is enabled. */
  WORK_FS?: unknown;
  /** OAuth helper binding when the `oauth` capability is enabled. */
  OAUTH?: unknown;
}

/**
 * Parameters for integration `scanLibrary` (schema placeholder; widen as schema grows).
 */
export type ScanLibraryParams = JsonObject;
/**
 * Parameters for `authenticateUser` (schema placeholder).
 */
export type AuthenticateUserParams = JsonObject;
/**
 * Parameters for one-shot `login` (schema placeholder).
 */
export type LoginParams = JsonObject;
/**
 * Parameters for interactive `loginStart` (schema placeholder).
 */
export type LoginStartParams = JsonObject;
/**
 * Parameters for interactive `loginComplete` (schema placeholder).
 */
export type LoginCompleteParams = JsonObject;
/**
 * Parameters for source `scan` (schema placeholder).
 */
export type ScanParams = JsonObject;
/**
 * Parameters for source `fetchTitle` (schema placeholder).
 */
export type FetchTitleParams = JsonObject;
/**
 * Parameters for `searchCatalog` (schema placeholder).
 */
export type SearchCatalogParams = JsonObject;
/**
 * Parameters for `expandCandidates` (schema placeholder).
 */
export type ExpandCandidatesParams = JsonObject;
/**
 * Parameters for `purchaseHint` (schema placeholder).
 */
export type PurchaseHintParams = JsonObject;
/**
 * Parameters for `listDeals` (schema placeholder).
 */
export type ListDealsParams = JsonObject;
/**
 * Parameters for destination `put` (schema placeholder).
 */
export type PutParams = JsonObject;
/**
 * Parameters for destination `putFile` (schema placeholder).
 */
export type PutFileParams = JsonObject;
/**
 * Parameters that carry a storage object key (schema placeholder).
 */
export type KeyParams = JsonObject;
/**
 * Parameters for destination `list` (schema placeholder).
 */
export type ListParams = JsonObject;
/**
 * Parameters for destination `copy` (schema placeholder).
 */
export type CopyParams = JsonObject;
/**
 * Parameters for destination `touchFile` (schema placeholder).
 */
export type TouchFileParams = JsonObject;
/**
 * Parameters for database `dbConnect` (schema placeholder).
 */
export type DbConnectParams = JsonObject;
/**
 * SQL statement DTO for `dbQuery` / `dbExecute` (schema placeholder).
 */
export type StatementDto = JsonObject;

/**
 * Known core method names on the Workers RPC wire.
 */
export const METHOD_NAMES = [
  "handshake",
  "shutdown",
  "health",
  "diagnose",
  "start",
  "onEvent",
  "pollEvents",
  "scanLibrary",
  "syncListening",
  "authenticateUser",
  "cliDescribe",
  "cliInvoke",
  "login",
  "loginStart",
  "loginComplete",
  "credentialsUpdate",
  "scan",
  "fetchTitle",
  "searchCatalog",
  "expandCandidates",
  "purchaseHint",
  "listDeals",
  "listAccounts",
  "catalogDetail",
  "put",
  "putFile",
  "get",
  "exists",
  "list",
  "probe",
  "copy",
  "delete",
  "touchFile",
  "dbConnect",
  "dbPing",
  "dbQuery",
  "dbExecute",
] as const;

/**
 * Union of known core RPC method names from {@link METHOD_NAMES}.
 */
export type MethodName = (typeof METHOD_NAMES)[number];
