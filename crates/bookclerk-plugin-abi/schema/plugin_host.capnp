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
  # abiMinor 21: same primitives as AdapterDatabaseSession, on the open txn.
  exportIdentity @4 () -> (result :Plugin.IdentityExportReply);
  importIdentity @5 (rows :List(Plugin.IdentityHighWater)) -> (result :Plugin.EmptyReply);
  listUserRelations @6 () -> (result :Plugin.UserRelationsReply);
  prepareUnitRestore @7 () -> (result :Plugin.EmptyReply);
  dropUserRelations @8 (names :List(Text)) -> (result :Plugin.EmptyReply);
  assertRestoreConstraints @9 () -> (result :Plugin.EmptyReply);
}

interface HostAdapterDatabaseSession {
  # isolation defaults to atomicBatch when omitted (Cap'n zero).
  begin @0 (isolation :Plugin.IsolationReq) -> (result :AdapterTransactionReply);
  executeEnvelope @1 (request :Plugin.AdapterExecuteRequest) -> (result :Plugin.ExecuteResultReply);
}
