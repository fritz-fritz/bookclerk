# Echo plugin — workerd / `@bookclerk/plugin-sdk`

Greenfield Workers RPC guest that **extends `BookclerkPlugin`** (not bare
`WorkerEntrypoint`). Same ABI as the native Echo integration; isolation is via
`bookclerk-jail` + `bookclerk-workerd`.

| | |
| --- | --- |
| `id` | `echo` |
| `kind` | `integration` |
| `runtime` | `workerd` |
| Network | `deny` |
| CLI | `ping` |

## Layout

```
plugin.toml          # install descriptor
src/index.ts         # authoring source (extends BookclerkPlugin)
modules/index.js     # loadable MVP module for bookclerk-workerd
```

## Develop

```bash
# Build / typecheck the SDK, then this example
cd packages/plugin-sdk && npm install && npm run build && npm run check-schema
cd ../../examples/plugins-echo-workerd && npm install && npm run typecheck
```

Install layout when packaged: `plugin.toml` + `modules/` under
`$BOOKCLERK_FILES_DIR/plugins/echo/`. Enable with `[integrations.echo]` in
config (host spawns `bookclerk-workerd`, not a SEA binary).

## Related

- SDK: [`packages/plugin-sdk`](../../packages/plugin-sdk/)
- ADR: [`docs/adr/plugin-workers-rpc-workerd.md`](../../docs/adr/plugin-workers-rpc-workerd.md)
- ABI schema: [`crates/bookclerk-plugin-abi/schema/abi.json`](../../crates/bookclerk-plugin-abi/schema/abi.json)
- Native Echo: `crates/bookclerk-plugin-examples/echo-integration`
- Experimental Node SEA Echo (legacy JSON-RPC): [`../plugins-echo-ts/`](../plugins-echo-ts/)
