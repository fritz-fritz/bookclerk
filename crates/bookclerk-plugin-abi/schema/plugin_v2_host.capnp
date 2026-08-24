# Bookclerk host-private plugin ABI extensions (`api_version = 2`).
#
# Not part of the public plugin author contract. Host ↔ first-party database
# adapter guests only (SeaORM interactive transaction proxy).
@0xb7c4e2f91a0835d2;

using PluginV2 = import "plugin_v2.capnp";

struct AdapterTransactionReply {
  union {
    ok @0 :AdapterTransaction;
    err @1 :PluginV2.PluginError;
  }
}

interface AdapterTransaction {
  execute @0 (request :PluginV2.ExecuteRequest) -> (result :PluginV2.ExecuteResultReply);
  commit @1 () -> (result :PluginV2.EmptyReply);
  rollback @2 () -> (result :PluginV2.EmptyReply);
}

struct HostGuestReceiptPersist {
  guestLen @0 :UInt32;
  guestHash @1 :Text;
}

struct HostExecuteEnvelope {
  request @0 :PluginV2.ExecuteRequest;
  guestReceipt @1 :HostGuestReceiptPersist;
}

interface HostAdapterDatabaseSession {
  begin @0 () -> (result :AdapterTransactionReply);
  executeEnvelope @1 (envelope :HostExecuteEnvelope) -> (result :PluginV2.ExecuteResultReply);
}
