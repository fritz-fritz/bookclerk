#!/usr/bin/env bash
# Architecture invariants for the ABI v3 plugin host.
#
# Production hosts must not:
#   - link ordinary plugin implementation crates (in-process execution)
#   - select the diagnostic direct-native Cap'n Proto transport
#   - treat a bare manifest id as platform trust
#   - persist grants under a bare manifest id without PluginKey
#   - leave authority revision empty on a product spawn
#
# Database adapter crates remain linked for host-owned SQL lowering.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

status=0

fail() {
  echo "FAIL: $*" >&2
  status=1
}

ok() {
  echo "ok: $*"
}

for toml in crates/bookclerk-cli/Cargo.toml crates/bookclerkd/Cargo.toml crates/bookclerk-plugin-host/Cargo.toml; do
  if grep -n 'bundled-plugins' "$toml" >/dev/null; then
    fail "$toml still declares bundled-plugins (in-process product path)"
  else
    ok "$toml has no bundled-plugins feature"
  fi
done

FORBIDDEN_LIBS='^bookclerk_plugin_(source|integration|destination)_'
for host in bookclerk-cli bookclerkd bookclerk-plugin-host; do
  tree="$(cargo tree -p "$host" --edges normal --prefix none --format '{lib}')"
  hits="$(grep -E "$FORBIDDEN_LIBS" <<<"$tree" | sort -u || true)"
  if [[ -n "$hits" ]]; then
    echo "FAIL: $host links ordinary plugin implementation crates:" >&2
    sed 's/^/  /' <<<"$hits" >&2
    status=1
  else
    ok "$host does not link source/integration/destination plugin crates"
  fi
done

if grep -RIn --include='*.rs' 'DirectNativeDiagnostic\|direct_native_diagnostic' \
  crates/bookclerk-cli/src crates/bookclerkd/src >/dev/null 2>&1; then
  fail "product binaries mention DirectNativeDiagnostic"
else
  ok "bookclerk / bookclerkd do not select DirectNativeDiagnostic"
fi

prod_hits="$(grep -RIn --include='*.rs' 'SpawnTransport::DirectNativeDiagnostic' \
  crates/bookclerk-plugin-host/src \
  | grep -v 'spawn_plan.rs' \
  | grep -v 'rpc_session.rs' \
  || true)"
if [[ -n "$prod_hits" ]]; then
  # jail.rs may mention it inside #[cfg(test)].
  leftover="$(echo "$prod_hits" | grep -v 'src/jail.rs' || true)"
  if [[ -n "$leftover" ]]; then
    echo "FAIL: production plugin-host selects DirectNativeDiagnostic:" >&2
    echo "$leftover" >&2
    status=1
  else
    ok "plugin-host production spawn does not select DirectNativeDiagnostic"
  fi
else
  ok "plugin-host production spawn does not select DirectNativeDiagnostic"
fi

if grep -RIn --include='*.rs' 'is_platform_plugin_id' crates >/dev/null 2>&1; then
  fail "is_platform_plugin_id reappeared (platform trust must use provenance)"
else
  ok "no is_platform_plugin_id"
fi

if grep -RIn --include='*.rs' 'fn is_trusted_platform_plugin' \
  crates/bookclerk-plugin-host/src/jail.rs >/dev/null; then
  ok "platform extra jail grants require verified provenance"
else
  fail "is_trusted_platform_plugin missing from jail.rs"
fi

if ! grep -n 'grant.plugin_key = plugin_key' crates/bookclerk-plugin-host/src/consent.rs >/dev/null; then
  fail "consent.rs no longer stamps PluginKey onto grants"
else
  ok "grants are stamped with PluginKey"
fi

if ! grep -n 'grant_revision.is_empty()' crates/bookclerk-plugin-host/src/rpc_session.rs >/dev/null; then
  fail "rpc_session.rs no longer fail-closes on an empty authority revision"
else
  ok "product spawn fail-closes on an empty authority revision"
fi

if grep -RIn --include='*.rs' 'publisher_key_id' crates >/dev/null 2>&1; then
  fail "publisher_key_id reappeared (no publisher PKI in the identity model)"
else
  ok "no publisher_key_id"
fi

if grep -RIn --include='*.rs' 'register_builtin_sources\|register_builtin_integrations' \
  crates/bookclerk-plugin-host crates/bookclerk-cli crates/bookclerkd >/dev/null 2>&1; then
  fail "register_builtin_* in-process registration reappeared"
else
  ok "no in-process register_builtin_* on hosts"
fi

if grep -n 'coarse jail' crates/bookclerk-plugins/optional/database-postgres/plugin.toml \
    docs/plugin-registry.md >/dev/null 2>&1; then
  fail "postgres/native docs still describe coarse jail outbound"
else
  ok "native postgres networking is SDK sockets, not coarse jail"
fi

if grep -A20 'pub fn bind_socket_proxy' crates/bookclerk-workerd/src/unix_bind.rs \
    | grep -q 'SOCKET_PROXY_ABSTRACT_PREFIX'; then
  fail "socket proxy still binds an abstract socket (nested Landlock ABI 6 scopes those out)"
else
  ok "native SOCKET_PROXY is a pathname (nested Landlock can connect)"
fi

if grep -B8 'NESTED_NATIVE_JAIL_ENV, "1"' crates/bookclerk-plugin-host/src/spawn_stdio.rs \
    | grep -q 'Start::Confined'; then
  fail "nested native jail is still gated on outer confinement"
else
  ok "nested native jail is requested for Unix native-behind-workerd"
fi

# Native guests must not open ambient TCP (reqwest::Client) in product src.
hits="$(find crates/bookclerk-plugins/optional crates/bookclerk-enrich/src \
  -name '*.rs' ! -path '*/tests/*' -print0 \
  | xargs -0 grep -n 'reqwest::Client' || true)"
if [[ -n "$hits" ]]; then
  echo "FAIL: ambient reqwest::Client remains in native guest / enrich src:" >&2
  echo "$hits" >&2
  status=1
else
  ok "native storefront/enrich HTTP is not ambient reqwest::Client"
fi

if grep -n 'reqwest::Client' crates/bookclerk-storage/src >/dev/null 2>&1; then
  fail "bookclerk-storage still uses ambient reqwest::Client"
else
  ok "S3 storage HTTP is not ambient reqwest::Client"
fi

exit "$status"
