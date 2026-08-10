# `@bookclerk/plugin-sdk`

TypeScript guest SDK for Bookclerk workerd plugins. Authors extend
**`BookclerkPlugin`** (never bare `WorkerEntrypoint`) and implement Workers RPC
methods from the shared ABI (`crates/bookclerk-plugin-abi/schema/abi.json`).

## Usage

```ts
import {
  BookclerkPlugin,
  type HandshakeParams,
  type HandshakeResult,
} from "@bookclerk/plugin-sdk";

export default class MyPlugin extends BookclerkPlugin {
  async handshake(_params: HandshakeParams): Promise<HandshakeResult> {
    return {
      apiVersion: 1,
      id: "my-plugin",
      kind: "integration",
      capabilities: ["health"],
    };
  }
}
```

Host bindings live on `this.env` (`HOST.notify`, optional `CONFIG` /
`PLUGIN_KV` / …). See the [ADR](../../docs/adr/plugin-workers-rpc-workerd.md)
and the Echo example at [`examples/plugins-echo-workerd`](../../examples/plugins-echo-workerd/).

## Scripts

```bash
npm run build         # emit dist/
npm run check-schema  # assert abi.json is present and keyed
```
