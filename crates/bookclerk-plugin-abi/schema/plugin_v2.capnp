# Bookclerk plugin ABI v2 — object-capability Workers RPC.
#
# Authors never see transport-private capability table indexes. Public types are
# the interfaces below (Destination, Source, JobHandler, ByteSource) plus the
# TypeScript class projections. ByteSource is the Cap'n Proto realization of a
# transferred byte ReadableStream (pull window = flow control).
@0x816df58cae22db0c;

const apiVersion :UInt32 = 2;
const maxScalarBytes :UInt32 = 262144;
const maxStreamWindowBytes :UInt32 = 1048576;
const maxListPage :UInt32 = 256;

struct ScalarLimits {
  maxScalarBytes @0 :UInt32;
  maxStreamWindowBytes @1 :UInt32;
  maxListPage @2 :UInt32;
}

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

struct DestinationContext {
  pluginDataDir @0 :Text;
  json @1 :Text;
}

struct SourceContext {
  pluginDataDir @0 :Text;
  json @1 :Text;
}

struct WorkerContext {
  jobId @0 :Text;
  pluginDataDir @1 :Text;
  json @2 :Text;
}

struct JobEvent {
  eventType @0 :Text;
  json @1 :Text;
}

struct JobOutcome {
  ok @0 :Bool;
  message @1 :Text;
  bytesCopied @2 :UInt64;
}

# Transferred readable byte stream. The capability *is* the stream; callers
# pull bounded windows. Abort is capability drop / RPC cancel.
interface ByteSource {
  pull @0 (maxBytes :UInt32) -> (chunk :Data, done :Bool);
}

interface Destination {
  head @0 (key :Text) -> (found :Bool, meta :ObjectMetadata);
  list @1 (options :ListOptions) -> (page :ListPage);
  get @2 (key :Text, options :ReadOptions) -> (meta :ObjectMetadata, body :ByteSource);
  put @3 (key :Text, body :ByteSource, options :WriteOptions) -> (result :PutResult);
  copy @4 (from :Text, to :Text) -> (result :CopyResult);
  delete @5 (key :Text);
}

interface Source {
  open @0 (key :Text) -> (meta :ObjectMetadata, body :ByteSource);
}

interface ProgressSink {
  report @0 (percent :Float32, message :Text);
}

interface JobHandler {
  handle @0 (event :JobEvent, input :Source, output :Destination, progress :ProgressSink)
      -> (outcome :JobOutcome);
}

interface BookclerkPlugin {
  describe @0 () -> (manifest :PluginDescribe);
  destination @1 (context :DestinationContext) -> (dest :Destination);
  source @2 (context :SourceContext) -> (src :Source);
  worker @3 (context :WorkerContext) -> (handler :JobHandler);
  shutdown @4 ();
}
