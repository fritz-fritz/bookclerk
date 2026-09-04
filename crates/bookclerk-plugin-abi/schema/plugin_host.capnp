# Bookclerk host-private plugin ABI extensions (`api_version = 2`).
#
# Not part of the public plugin author contract. Host ↔ first-party database
# adapter guests only (SeaORM interactive transaction proxy).
@0xb7c4e2f91a0835d2;

using Plugin = import "plugin.capnp";

struct AdapterTransactionReply {
    union {
        ok @0 :AdapterTransaction;
        err @1 :Plugin.PluginError;
    }
}

interface AdapterTransaction {
  execute @0 (request :Plugin.AdapterExecuteRequest) -> (result :Plugin.ExecuteResultReply);
  commit @1 () -> (result :Plugin.EmptyReply);
  rollback @2 () -> (result :Plugin.EmptyReply);
  # Same payload as `execute` (abiMinor 20). Ordinal kept; do not reuse.
  executeEnvelope @3 (request :Plugin.AdapterExecuteRequest) -> (result :Plugin.ExecuteResultReply);
}

interface HostAdapterDatabaseSession {
  begin @0 () -> (result :AdapterTransactionReply);
  executeEnvelope @1 (request :Plugin.AdapterExecuteRequest) -> (result :Plugin.ExecuteResultReply);
}
