# Bookclerk plugin ABI v2 — object-capability Workers RPC.
#
# Authors never see transport-private capability table indexes. Public types are
# the interfaces below (Destination, Source, JobHandler, ByteSource) plus the
# TypeScript class projections. ByteSource is the Cap'n Proto realization of a
# transferred byte ReadableStream (pull window = flow control).
#
# Method results are typed success/error unions. SDKs map `err` onto a thrown
# PluginError so authors do not inspect unions. Unknown future `code` strings
# MUST be preserved (do not collapse to "internal").
@0x816df58cae22db0c;

const apiVersion :UInt32 = 2;
const envelopeVersion :UInt32 = 1;
const maxScalarBytes :UInt32 = 262144;
const maxStreamWindowBytes :UInt32 = 1048576;
const maxListPage :UInt32 = 256;
const maxCheckpointBytes :UInt32 = 65536;

struct ScalarLimits {
  maxScalarBytes @0 :UInt32;
  maxStreamWindowBytes @1 :UInt32;
  maxListPage @2 :UInt32;
}

# `code` is a snake_case string (`not_found`, `invalid_cursor`, …). Unknown
# codes are forwarded as-is; SDKs surface them as PluginErrorCode.unknown
# while retaining the raw wire code.
struct PluginError {
  code @0 :Text;
  message @1 :Text;
}

struct ObjectMetadata {
  key @0 :Text;
  size @1 :UInt64;
  contentType @2 :Text;
  etag @3 :Text;
  sha256 @4 :Data;
}

struct ObjectInfo {
  key @0 :Text;
  size @1 :UInt64;
}

struct ListOptions {
  prefix @0 :Text;
  cursor @1 :Text;
  limit @2 :UInt32;
}

struct ListPage {
  objects @0 :List(ObjectInfo);
  nextCursor @1 :Text;
}

struct ByteRange {
  offset @0 :UInt64;
  length @1 :UInt64;
}

struct ReadOptions {
  range @0 :ByteRange;
}

struct WriteOptions {
  contentType @0 :Text;
  contentLength @1 :UInt64;
  sha256 @2 :Data;
}

struct PutResult {
  key @0 :Text;
  bytesWritten @1 :UInt64;
  etag @2 :Text;
  sha256 @3 :Data;
}

struct CopyResult {
  bytesCopied @0 :UInt64;
}

struct PluginDescribe {
  apiVersion @0 :UInt32;
  id @1 :Text;
  kind @2 :Text;
  displayName @3 :Text;
  rpcFeatures @4 :List(Text);
  scalarLimits @5 :ScalarLimits;
}

# Opaque JSON knobs only. OS paths, FDs, and sockets are transport-private.
struct DestinationContext {
  json @0 :Text;
}

struct SourceContext {
  json @0 :Text;
}

struct WorkerContext {
  jobId @0 :Text;
  json @1 :Text;
}

# Durable command envelope (not a domain event). Envelope version and command
# payload schema version are independent. Idempotency keys are scoped to
# (account, plugin, commandType) until a terminal fenced outcome is committed.
struct JobInvocation {
  envelopeVersion @0 :UInt32;
  payloadSchemaVersion @1 :UInt32;
  invocationId @2 :Text;
  commandType @3 :Text;
  payloadJson @4 :Text;
  idempotencyKey @5 :Text;
  attempt @6 :UInt32;
  correlationId @7 :Text;
  causationId @8 :Text;
  # UTC Unix milliseconds. Host fence/lease is authoritative; this hint must
  # not outlive the fence (clock skew across VPS nodes).
  deadlineUnixMs @9 :UInt64;
  checkpointJson @10 :Text;
  checkpointSchemaVersion @11 :UInt32;
}

struct CompletedOutcome {
  message @0 :Text;
  bytesCopied @1 :UInt64;
}

struct RetryableOutcome {
  message @0 :Text;
  retryAfterUnixMs @1 :UInt64;
}

struct RejectedOutcome {
  message @0 :Text;
}

struct CancelledOutcome {
  message @0 :Text;
}

struct SuspendedOutcome {
  checkpointJson @0 :Text;
  checkpointSchemaVersion @1 :UInt32;
  wakeAtUnixMs @2 :UInt64;
}

struct JobOutcome {
  union {
    completed @0 :CompletedOutcome;
    retryable @1 :RetryableOutcome;
    rejected @2 :RejectedOutcome;
    cancelled @3 :CancelledOutcome;
    suspended @4 :SuspendedOutcome;
  }
}

struct HeadOk {
  found @0 :Bool;
  meta @1 :ObjectMetadata;
}

struct HeadReply {
  union {
    ok @0 :HeadOk;
    err @1 :PluginError;
  }
}

struct ListReply {
  union {
    ok @0 :ListPage;
    err @1 :PluginError;
  }
}

struct GetOk {
  meta @0 :ObjectMetadata;
  body @1 :ByteSource;
}

struct GetReply {
  union {
    ok @0 :GetOk;
    err @1 :PluginError;
  }
}

struct PutReply {
  union {
    ok @0 :PutResult;
    err @1 :PluginError;
  }
}

struct CopyReply {
  union {
    ok @0 :CopyResult;
    err @1 :PluginError;
  }
}

struct EmptyReply {
  union {
    ok @0 :Void;
    err @1 :PluginError;
  }
}

struct PullOk {
  chunk @0 :Data;
  done @1 :Bool;
}

struct PullReply {
  union {
    ok @0 :PullOk;
    err @1 :PluginError;
  }
}

struct OpenOk {
  meta @0 :ObjectMetadata;
  body @1 :ByteSource;
}

struct OpenReply {
  union {
    ok @0 :OpenOk;
    err @1 :PluginError;
  }
}

struct DescribeReply {
  union {
    ok @0 :PluginDescribe;
    err @1 :PluginError;
  }
}

struct DestinationReply {
  union {
    ok @0 :Destination;
    err @1 :PluginError;
  }
}

struct SourceReply {
  union {
    ok @0 :Source;
    err @1 :PluginError;
  }
}

struct WorkerReply {
  union {
    ok @0 :JobHandler;
    err @1 :PluginError;
  }
}

struct HandleReply {
  union {
    ok @0 :JobOutcome;
    err @1 :PluginError;
  }
}

# Transferred readable byte stream. The capability *is* the stream; callers
# pull bounded windows. Abort is capability drop / RPC cancel. A failed pull
# MUST set `err` — never a successful empty EOF.
interface ByteSource {
  pull @0 (maxBytes :UInt32) -> (result :PullReply);
}

interface Destination {
  head @0 (key :Text) -> (result :HeadReply);
  list @1 (options :ListOptions) -> (result :ListReply);
  get @2 (key :Text, options :ReadOptions) -> (result :GetReply);
  put @3 (key :Text, body :ByteSource, options :WriteOptions) -> (result :PutReply);
  copy @4 (from :Text, to :Text) -> (result :CopyReply);
  delete @5 (key :Text) -> (result :EmptyReply);
}

interface Source {
  open @0 (key :Text) -> (result :OpenReply);
}

interface ProgressSink {
  report @0 (percent :Float32, message :Text) -> (result :EmptyReply);
}

# Transport cancellation. SDKs project this into a locally created AbortSignal
# (AbortSignal is not a serializable Workers RPC value).
interface Cancellation {
  poll @0 () -> (cancelled :Bool);
}

interface JobHandler {
  handle @0 (
      invocation :JobInvocation,
      input :Source,
      output :Destination,
      progress :ProgressSink,
      cancel :Cancellation)
      -> (result :HandleReply);
}

interface BookclerkPlugin {
  describe @0 () -> (result :DescribeReply);
  destination @1 (context :DestinationContext) -> (result :DestinationReply);
  source @2 (context :SourceContext) -> (result :SourceReply);
  worker @3 (context :WorkerContext) -> (result :WorkerReply);
  shutdown @4 () -> (result :EmptyReply);
}
