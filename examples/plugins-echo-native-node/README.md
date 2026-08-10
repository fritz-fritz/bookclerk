# Echo Integration (native Node)

Reference **native** Bookclerk guest. Subclasses `BookclerkPlugin` and runs with
`BookclerkPluginGuest.serve` (`api_version = 1`, id `echo-native-node`).

**Dev / CI staging** (`cargo stage-plugins --examples`) installs a small shell
launcher that runs `node src/echo.mjs` with a vendored
`@bookclerk/plugin-sdk/native` under `sdk/`. Requires `npm run build` in
`packages/plugin-sdk` first. Override the Node binary with `BOOKCLERK_NODE`.

**Publisher path:** experimental Node SEA via `npm run sea:build` (see
`scripts/build-sea.mjs`) — pack `plugin.toml` + the SEA binary as
`bookclerk-plugin-echo-native-node`.

```bash
cd packages/plugin-sdk && npm run build
cd examples/plugins-echo-native-node
node src/echo.mjs
# or after staging:
# target/plugin-artifacts/echo-native-node/bookclerk-plugin-echo-native-node
```

Sibling examples: `plugins-echo-native-rust`, `plugins-echo-native-python`,
`plugins-echo-workerd-*`.
