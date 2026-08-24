//! Plugin ABI v2: object-capability classes, transferable byte streams, jobs.

#![allow(clippy::missing_docs_in_private_items)]

mod db_rpc;
mod features;
mod host_rpc;
mod host_roles;
mod jobs;
mod limits;
mod roles;
mod rpc;
mod sdk_wire;
mod types;

/// Generated Cap'n Proto RPC interfaces (`schema/plugin_v2.capnp`).
pub use crate::plugin_v2_capnp;

/// Host-private Cap'n Proto RPC interfaces (`schema/plugin_v2_host.capnp`).
pub use crate::plugin_v2_host_capnp;

pub use db_rpc::{
    canonical_execute_request_hash, decode_db_value_bytes, decode_execute_request_bytes,
    decode_execute_result_reply_bytes, encoded_db_value_bytes, encoded_execute_reply_bytes,
    encoded_execute_request_bytes, encoded_execute_result_reply_bytes,
    encoded_statement_result_bytes,
};
pub use features::{
    negotiate_rpc_features, RpcFeature, FEATURE_SCALAR_LIMITS, FEATURE_STORAGE_COPY,
    FEATURE_STREAMS,
};
pub use jobs::{read_all, stream_copy_keys, StreamCopyHandler, StreamCopySpec};
pub use limits::{
    ScalarLimits, ABI_MAJOR, ABI_MINOR, MAX_EVENT_PAYLOAD_BYTES, MAX_LIST_PAGE, MAX_SCALAR_BYTES,
    MAX_STREAM_WINDOW_BYTES, PRODUCT_API_VERSION,
};
pub use host_rpc::HostAdapterDatabaseSessionClient;
pub use host_roles::{AdapterTransaction, HostAdapterDatabaseSession};
pub use roles::{
    AdapterDatabaseSession, ByteRange, Cancellation, ContentSource, ContentSourceContext, Database,
    DatabaseContext, Destination, GuestDatabase, Integration, IntegrationContext, JobHandler,
    JobHandlerContext, NeverCancel, PluginRoot, ProgressSink, ReadResult, Source,
};
pub use rpc::{
    byte_source_from_async_read, connect_plugin, pull_byte_source_to_writer, serve_plugin,
    serve_plugin_stdio, AdapterSessionHandle, ContentSourceClient, DatabaseClient,
    DestinationClient, DestinationServer, IntegrationClient, PluginClient, PluginServer,
    SourceClient, SourceServer,
};
pub use types::{
    CopyResult, DestinationContext, DomainEvent, EventResult, ExecResult, ExtensibleConfig,
    HealthOk, JobCheckpoint, JobInvocation, JobInvocationLease, JobOutcome, ListOptions, ListPage,
    ObjectInfo, ObjectMetadata, OidcClientTemplate, PluginDescribe, PutResult, QueryPage,
    ScalarLimitsDto, SourceContext, Statement, WorkerContext, WriteOptions, ENVELOPE_VERSION,
    MAX_CHECKPOINT_BYTES,
};
