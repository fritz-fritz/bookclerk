# ADR: Plugin-provided OIDC clients

- **Status:** Accepted
- **Date:** 2026-08-15
- **Related:** [First-party identity](first-party-identity.md),
  [Workers RPC + workerd](plugin-workers-rpc-workerd.md)

## Context

Bookclerk is the OpenID Connect **authorization server** for players such as
Audiobookshelf. Plugins do not implement the OIDC protocol; they are relying
parties (or they help the operator talk to one).

The host previously **seeded** a public PKCE client `audiobookshelf` at every
daemon start, including Bookclerk’s own origin (`{issuer}/` on port 8787) and
hardcoded `http://127.0.0.1:13378/auth/openid/callback` URIs. That was wrong
on three counts:

1. The client existed even when the Audiobookshelf **plugin was not
   installed**.
2. Redirect URIs mixed Bookclerk’s listen port with ABS’s UI port. ABS
   callbacks live on the **player** origin (operator-configured
   `[integrations.audiobookshelf].base_url`, default ABS UI port 13378), not
   on `daemon.listen` (default 8787).
3. There was no enable/disable control. Installing or seeding a client
   immediately made it a live relying party.

A plugin ABI redesign is in flight ([Workers RPC ADR](plugin-workers-rpc-workerd.md)).
Guests should eventually **declare** OIDC client templates (callback path,
public vs confidential, default scopes) on handshake so the host can
materialize them without first-party hardcoding. That handshake field is
**not** added in this cut.

## Decision

### Ownership

| Kind | Who defines identity + redirects | Operator can |
| --- | --- | --- |
| **Plugin-provided** | The plugin (callback path and client id from plugin data; origin from the plugin’s operator settings, e.g. ABS `base_url`) | Enable/disable, display name, refresh/scopes/confidential; **not** redirect URIs or delete |
| **Custom** | The operator (Settings → Add client) | Full edit and delete |

The host remains the authorization server. Plugins never mint tokens.

### Lifecycle

- A plugin-provided client **appears only when that plugin is installed**
  (discovered on disk and/or loaded in the integration registry). It does not
  appear merely because a `[integrations.<id>]` table exists in `config.toml`.
- Newly materialized plugin clients are **disabled**. Authorize and token
  endpoints treat a disabled client like an unknown client. The operator
  turns the client on after reviewing redirects.
- Uninstalling a plugin does **not** auto-delete its OIDC row (reinstall
  should keep the enable flag and secret). The card may remain until a later
  cleanup pass; it stays disabled unless the operator left it on.
- Custom clients default **enabled** (the operator just created them).

### Redirect URIs

Plugin redirect URIs are **deterministic**:

```
{plugin_origin}{callback_path}
```

`plugin_origin` is the operator-set server URL in plugin settings (ABS:
`integrations.audiobookshelf.base_url`). `callback_path` is plugin data (ABS:
`/auth/openid/callback`). When the origin host is loopback (`127.0.0.1` /
`localhost` / `::1`), the host also registers the usual loopback hostname
alias so either form works. Empty `base_url` yields an empty redirect list
until the operator sets the server URL.

The Settings UI shows plugin redirects **read-only** and points the operator
at the plugin’s server URL field. `PUT /api/auth/oidc/clients/{id}` ignores
redirect changes for plugin-owned rows. Custom clients keep an editable
textarea.

Bookclerk’s public origin / listen port is the **issuer**, not a player
callback, and must not be copied onto plugin redirect lists.

### Enable toggle

Every client card (plugin and custom) has an **Enabled** control. Disabled
clients cannot complete `/oidc/authorize` or `/oidc/token`.

### Transitional host catalog (no ABI yet)

Until handshake can carry an `oidcClients` (name TBD) declaration:

- The host keeps a **first-party template table** for plugins we already
  ship (today: Audiobookshelf → client id `audiobookshelf`, public PKCE,
  `/auth/openid/callback`).
- On startup and config reload the host upserts those templates **only**
  for installed plugin ids, refreshes redirect URIs from current plugin
  settings, and never clobbers `enabled`, secrets, or operator-edited name /
  scopes.

This catalog is an acknowledged stopgap. It must not grow into a second
plugin API.

### Existing rows

Migration V19 adds `enabled` (default 1) and `plugin_id`. Rows with
`client_id = 'audiobookshelf'` are stamped `plugin_id = 'audiobookshelf'` so
their redirects become host-managed. Existing rows stay **enabled** so
already-working ABS SSO is not silently turned off; only **new** plugin
materializations start disabled.

## Consequences

- Settings → Sign-in no longer implies an Audiobookshelf client on a
  Bookclerk install that never staged that plugin (`cargo dev` without
  `--optional`, slim packages, etc.).
- Operators who want ABS login enable the Audiobookshelf plugin, set its
  server URL, then enable the OIDC client and paste Bookclerk’s issuer into
  ABS OpenID settings.
- Custom players (other than ABS) remain fully operator-defined.
- The forthcoming ABI should add a structured client template (id, display
  name, callback path, public/confidential, default scopes, which config
  key supplies the origin) on handshake / `plugin.toml`. Implementation of
  that field is explicitly **out of scope** here so it can land with the
  ABI redesign rather than as a one-off JSON-RPC shape.

## Non-goals

- Plugin-implemented authorization servers or token passthrough to an
  upstream issuer.
- Binding Bookclerk to ABS’s default UI port 13378.
- Handshake / `abi.json` changes in this cut.
- Auto-deleting OIDC rows when a plugin is removed.
- Per-account OIDC clients.
