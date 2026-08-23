//! Bookclerk Cap'n Proto stdio adapter over the workerd v2 HTTP bridge.
//!
//! Isolates keep `RpcTarget` stubs; this process maps them onto the host-facing
//! [`PluginRoot`] / [`Destination`] traits with streamed HTTP bodies.

#![allow(clippy::missing_docs_in_private_items)]
#![allow(clippy::arc_with_non_send_sync)]

use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

use async_trait::async_trait;
use bookclerk_plugin_abi::v2::{
    serve_plugin_stdio, ByteRange, CopyResult, DatabaseSession, Destination, DestinationContext,
    DomainEvent, EventResult, HealthOk, Integration, IntegrationContext, JobHandler,
    JobHandlerContext, JobInvocation, JobOutcome, ListOptions, ListPage, ObjectInfo,
    ObjectMetadata, OidcClientTemplate, PluginDescribe, PluginRoot, PutResult, ReadResult,
    ScalarLimitsDto, Source, SourceContext, WorkerContext, WriteOptions, MAX_LIST_PAGE,
    MAX_SCALAR_BYTES, MAX_STREAM_WINDOW_BYTES, PRODUCT_API_VERSION,
};
use bookclerk_plugin_abi::{PluginError, Result as AbiResult};
use tokio::io::AsyncRead;

use crate::granted::{GrantedSlot, GrantedTable};
use crate::v2_http::BridgeHttp;

/// Serves Bookclerk capnp on stdio while talking HTTP/JSRPC to the isolate.
///
/// Must run inside a `LocalSet` (same thread as the granted HTTP server).
///
/// # Errors
///
/// Returns a plugin error when the vat fails.
pub async fn mediate_v2_stdio(http: BridgeHttp, table: GrantedTable) -> anyhow::Result<()> {
    let plugin = WorkerdV2Root { http, table };
    serve_plugin_stdio(Arc::new(plugin), MAX_STREAM_WINDOW_BYTES)
        .await
        .map_err(|err| anyhow::anyhow!("{err}"))
}

struct WorkerdV2Root {
    http: BridgeHttp,
    table: GrantedTable,
}

fn map_http(err: anyhow::Error) -> PluginError {
    let msg = err.to_string();
    if let Some((code, rest)) = msg.split_once(": ") {
        return PluginError::from_wire(code, rest);
    }
    PluginError::internal(msg)
}

#[async_trait(?Send)]
impl PluginRoot for WorkerdV2Root {
    async fn describe(&self) -> AbiResult<PluginDescribe> {
        let v = self
            .http
            .json_post("/v2/describe", &serde_json::json!({}))
            .await
            .map_err(map_http)?;
        let api_version = v.get("apiVersion").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        if api_version != PRODUCT_API_VERSION {
            return Err(PluginError::unsupported(format!(
                "unsupported apiVersion {api_version}"
            )));
        }
        Ok(PluginDescribe {
            api_version,
            id: v.get("id").and_then(|x| x.as_str()).unwrap_or("").into(),
            kind: v.get("kind").and_then(|x| x.as_str()).unwrap_or("").into(),
            display_name: {
                let name = v
                    .get("displayName")
                    .and_then(|x| x.as_str())
                    .map(str::to_string);
                match v.get("stubCounts") {
                    Some(counts) => {
                        let dests = counts.get("dests").and_then(|x| x.as_u64()).unwrap_or(0);
                        let sources = counts.get("sources").and_then(|x| x.as_u64()).unwrap_or(0);
                        let handlers = counts.get("handlers").and_then(|x| x.as_u64()).unwrap_or(0);
                        let suffix = format!("stubs=d:{dests},s:{sources},h:{handlers}");
                        Some(match name {
                            Some(n) if !n.is_empty() => format!("{n} {suffix}"),
                            _ => suffix,
                        })
                    }
                    None => name,
                }
            },
            rpc_features: v
                .get("rpcFeatures")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            scalar_limits: {
                let sl = v.get("scalarLimits");
                ScalarLimitsDto {
                    max_scalar_bytes: sl
                        .and_then(|x| x.get("maxScalarBytes"))
                        .and_then(|x| x.as_u64())
                        .unwrap_or(u64::from(MAX_SCALAR_BYTES))
                        as u32,
                    max_stream_window_bytes: sl
                        .and_then(|x| x.get("maxStreamWindowBytes"))
                        .and_then(|x| x.as_u64())
                        .unwrap_or(u64::from(MAX_STREAM_WINDOW_BYTES))
                        as u32,
                    max_list_page: sl
                        .and_then(|x| x.get("maxListPage"))
                        .and_then(|x| x.as_u64())
                        .unwrap_or(u64::from(MAX_LIST_PAGE))
                        as u32,
                }
            },
            abi_major: v
                .get("abiMajor")
                .and_then(|x| x.as_u64())
                .unwrap_or(u64::from(api_version)) as u32,
            abi_minor: v.get("abiMinor").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
            supported_roles: v
                .get("supportedRoles")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            metadata_json: v
                .get("metadataJson")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
        })
    }

    async fn destination(&self, context: DestinationContext) -> AbiResult<Box<dyn Destination>> {
        Ok(Box::new(HttpDestination {
            http: self.http.clone(),
            ctx: context,
        }))
    }

    async fn source(&self, context: SourceContext) -> AbiResult<Box<dyn Source>> {
        Ok(Box::new(HttpSource {
            http: self.http.clone(),
            ctx: context,
        }))
    }

    async fn worker(&self, context: WorkerContext) -> AbiResult<Box<dyn JobHandler>> {
        Ok(Box::new(HttpJobHandler {
            http: self.http.clone(),
            ctx: context,
            table: Rc::clone(&self.table),
        }))
    }

    async fn integration(&self, context: IntegrationContext) -> AbiResult<Box<dyn Integration>> {
        Ok(Box::new(HttpIntegration {
            http: self.http.clone(),
            ctx: context,
        }))
    }

    async fn cli_describe(&self) -> AbiResult<String> {
        let v = self
            .http
            .json_post("/v2/cliDescribe", &serde_json::json!({}))
            .await
            .map_err(map_http)?;
        Ok(v.get("json")
            .and_then(|x| x.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| v.to_string()))
    }

    async fn cli_invoke(&self, params_json: &str) -> AbiResult<String> {
        let v = self
            .http
            .json_post(
                "/v2/cliInvoke",
                &serde_json::json!({ "paramsJson": params_json }),
            )
            .await
            .map_err(map_http)?;
        Ok(v.get("json")
            .and_then(|x| x.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| v.to_string()))
    }

    async fn oidc_clients(&self) -> AbiResult<Vec<OidcClientTemplate>> {
        let v = self
            .http
            .json_post("/v2/oidcClients", &serde_json::json!({}))
            .await
            .map_err(map_http)?;
        let clients = v
            .get("clients")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        serde_json::from_value(clients).map_err(|err| PluginError::internal(err.to_string()))
    }
}

struct HttpDestination {
    http: BridgeHttp,
    ctx: DestinationContext,
}

struct HttpIntegration {
    http: BridgeHttp,
    ctx: IntegrationContext,
}

fn json_string(v: &serde_json::Value) -> String {
    v.get("json")
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| v.to_string())
}

#[async_trait(?Send)]
impl Integration for HttpIntegration {
    async fn health(&self) -> AbiResult<HealthOk> {
        let v = self
            .http
            .json_post(
                "/v2/integration/health",
                &serde_json::json!({ "json": self.ctx.json }),
            )
            .await
            .map_err(map_http)?;
        Ok(HealthOk {
            ok: v.get("ok").and_then(|x| x.as_bool()).unwrap_or(true),
            detail: v
                .get("detail")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
        })
    }

    async fn diagnose(&self) -> AbiResult<String> {
        let v = self
            .http
            .json_post(
                "/v2/integration/diagnose",
                &serde_json::json!({ "json": self.ctx.json }),
            )
            .await
            .map_err(map_http)?;
        if let Some(s) = v.as_str() {
            return Ok(s.to_string());
        }
        if let Some(lines) = v.get("lines") {
            return Ok(lines.to_string());
        }
        if v.is_array() {
            return Ok(v.to_string());
        }
        Ok(json_string(&v))
    }

    async fn on_event(&self, event: DomainEvent) -> AbiResult<EventResult> {
        let v = self
            .http
            .json_post(
                "/v2/integration/onEvent",
                &serde_json::json!({
                    "json": self.ctx.json,
                    "event": event,
                }),
            )
            .await
            .map_err(map_http)?;
        EventResult::from_json_value(&v)
    }

    async fn start(&self) -> AbiResult<()> {
        let _ = self
            .http
            .json_post(
                "/v2/integration/start",
                &serde_json::json!({ "json": self.ctx.json }),
            )
            .await
            .map_err(map_http)?;
        Ok(())
    }

    async fn stop(&self) -> AbiResult<()> {
        let _ = self
            .http
            .json_post(
                "/v2/integration/stop",
                &serde_json::json!({ "json": self.ctx.json }),
            )
            .await
            .map_err(map_http)?;
        Ok(())
    }
}

fn dest_ctx_json(ctx: &DestinationContext) -> String {
    serde_json::json!({ "json": ctx.json }).to_string()
}

#[async_trait(?Send)]
impl Destination for HttpDestination {
    async fn head(&self, key: &str) -> AbiResult<Option<ObjectMetadata>> {
        let v = self
            .http
            .json_post(
                "/v2/destination/head",
                &serde_json::json!({ "key": key, "json": self.ctx.json }),
            )
            .await
            .map_err(map_http)?;
        if v.get("found").and_then(|x| x.as_bool()) != Some(true) {
            return Ok(None);
        }
        Ok(v.get("meta").and_then(meta_from_json))
    }

    async fn list(&self, options: ListOptions) -> AbiResult<ListPage> {
        let v = self
            .http
            .json_post(
                "/v2/destination/list",
                &serde_json::json!({
                    "json": self.ctx.json,
                    "options": {
                        "prefix": options.prefix,
                        "cursor": options.cursor,
                        "limit": options.limit,
                    },
                }),
            )
            .await
            .map_err(map_http)?;
        let objects: Vec<ObjectInfo> = v
            .get("objects")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|o| {
                        Some(ObjectInfo {
                            key: o.get("key")?.as_str()?.to_string(),
                            size: o.get("size")?.as_u64().unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if objects.len() > MAX_LIST_PAGE as usize {
            return Err(PluginError::payload_too_large(format!(
                "list page of {} objects exceeds {MAX_LIST_PAGE}",
                objects.len()
            )));
        }
        Ok(ListPage {
            objects,
            next_cursor: v
                .get("nextCursor")
                .and_then(|x| x.as_str())
                .map(str::to_string),
        })
    }

    async fn get(&self, key: &str, range: Option<ByteRange>) -> AbiResult<ReadResult> {
        let mut path = format!("/v2/destination/get?key={}", percent_encode(key));
        if let Some(r) = range {
            path.push_str(&format!("&offset={}", r.offset));
            if let Some(len) = r.length {
                path.push_str(&format!("&length={len}"));
            }
        }
        let ctx = dest_ctx_json(&self.ctx);
        let (meta, body) = self
            .http
            .get_stream_headers(&path, &[("x-bookclerk-context", ctx.as_str())])
            .await
            .map_err(map_http)?;
        Ok(ReadResult { meta, body })
    }

    async fn put(
        &self,
        key: &str,
        body: Pin<Box<dyn AsyncRead + Send>>,
        options: WriteOptions,
    ) -> AbiResult<PutResult> {
        let path = format!("/v2/destination/put?key={}", percent_encode(key));
        let ctx = dest_ctx_json(&self.ctx);
        let mut extra = vec![("x-bookclerk-context", ctx.as_str())];
        let token;
        if let Some(t) = &options.commit_token {
            token = t.clone();
            extra.push(("x-bookclerk-commit-token", token.as_str()));
        }
        let stage = if options.stage_only { "1" } else { "0" };
        extra.push(("x-bookclerk-stage-only", stage));
        self.http
            .put_stream_headers(
                &path,
                body,
                options.content_type.as_deref(),
                options.content_length,
                &extra,
            )
            .await
            .map_err(map_http)
    }

    async fn copy(&self, from: &str, to: &str) -> AbiResult<CopyResult> {
        let v = self
            .http
            .json_post(
                "/v2/destination/copy",
                &serde_json::json!({ "from": from, "to": to, "json": self.ctx.json }),
            )
            .await
            .map_err(map_http)?;
        Ok(CopyResult {
            bytes_copied: v.get("bytesCopied").and_then(|x| x.as_u64()).unwrap_or(0),
        })
    }

    async fn delete(&self, key: &str) -> AbiResult<()> {
        self.http
            .json_post(
                "/v2/destination/delete",
                &serde_json::json!({ "key": key, "json": self.ctx.json }),
            )
            .await
            .map_err(map_http)?;
        Ok(())
    }

    async fn commit(&self, key: &str, commit_token: &str) -> AbiResult<PutResult> {
        let v = self
            .http
            .json_post(
                "/v2/destination/commit",
                &serde_json::json!({
                    "key": key,
                    "commitToken": commit_token,
                    "json": self.ctx.json
                }),
            )
            .await
            .map_err(map_http)?;
        Ok(PutResult {
            key: v
                .get("key")
                .and_then(|x| x.as_str())
                .unwrap_or(key)
                .to_string(),
            bytes_written: v.get("bytesWritten").and_then(|x| x.as_u64()).unwrap_or(0),
            etag: v.get("etag").and_then(|x| x.as_str()).map(str::to_string),
            sha256: None,
        })
    }

    async fn abort_stage(&self, key: &str, commit_token: &str) -> AbiResult<()> {
        self.http
            .json_post(
                "/v2/destination/abortStage",
                &serde_json::json!({
                    "key": key,
                    "commitToken": commit_token,
                    "json": self.ctx.json
                }),
            )
            .await
            .map_err(map_http)?;
        Ok(())
    }
}

struct HttpSource {
    http: BridgeHttp,
    ctx: SourceContext,
}

#[async_trait(?Send)]
impl Source for HttpSource {
    async fn open(&self, key: &str) -> AbiResult<ReadResult> {
        let path = format!("/v2/source/open?key={}", percent_encode(key));
        let ctx = serde_json::json!({ "json": self.ctx.json }).to_string();
        let (meta, body) = self
            .http
            .get_stream_headers(&path, &[("x-bookclerk-context", ctx.as_str())])
            .await
            .map_err(map_http)?;
        Ok(ReadResult { meta, body })
    }
}

struct HttpJobHandler {
    http: BridgeHttp,
    ctx: WorkerContext,
    table: GrantedTable,
}

#[async_trait(?Send)]
impl JobHandler for HttpJobHandler {
    async fn handle(
        &self,
        invocation: JobInvocation,
        context: JobHandlerContext,
    ) -> AbiResult<JobOutcome> {
        let (allow_database, max_atomic_request_bytes) =
            granted_database_budget(context.database.as_deref()).await?;
        let grant = format!("{:032x}", rand::random::<u128>());
        self.table.borrow_mut().insert(
            grant.clone(),
            GrantedSlot {
                input: Some(context.input),
                output: Some(context.output),
                progress: Some(context.progress),
                expires: std::time::Instant::now() + std::time::Duration::from_secs(3600),
                allow_open: true,
                allow_put: true,
                allow_progress: true,
                database: context.database.map(Rc::from),
                allow_database,
                max_atomic_request_bytes,
            },
        );
        let _revoke = RevokeGrant {
            table: Rc::clone(&self.table),
            grant: grant.clone(),
        };
        let result = self
            .http
            .json_post(
                "/v2/worker/handle",
                &serde_json::json!({
                    "grantToken": grant,
                    "invocation": invocation,
                    "jobId": self.ctx.job_id,
                    "json": self.ctx.json,
                }),
            )
            .await;
        let v = result.map_err(map_http)?;
        outcome_from_json(&v)
    }
}

fn meta_from_json(v: &serde_json::Value) -> Option<ObjectMetadata> {
    Some(ObjectMetadata {
        key: v.get("key")?.as_str()?.to_string(),
        size: v.get("size")?.as_u64().unwrap_or(0),
        content_type: v
            .get("contentType")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        etag: v.get("etag").and_then(|x| x.as_str()).map(str::to_string),
        sha256: None,
    })
}

fn outcome_from_json(v: &serde_json::Value) -> AbiResult<JobOutcome> {
    let kind = v
        .get("kind")
        .and_then(|x| x.as_str())
        .or_else(|| {
            v.get("ok")
                .and_then(|ok| ok.as_bool())
                .and_then(|ok| ok.then_some("completed"))
        })
        .unwrap_or("completed");
    let message = v
        .get("message")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    Ok(match kind {
        "retryable" => JobOutcome::Retryable {
            message,
            retry_after_unix_ms: v.get("retryAfterUnixMs").and_then(|x| x.as_u64()),
        },
        "rejected" => JobOutcome::Rejected { message },
        "cancelled" => JobOutcome::Cancelled { message },
        "suspended" => JobOutcome::Suspended {
            checkpoint: bookclerk_plugin_abi::v2::JobCheckpoint {
                schema_version: v
                    .get("checkpoint")
                    .and_then(|c| c.get("schemaVersion"))
                    .and_then(|x| x.as_u64())
                    .unwrap_or(1) as u32,
                json: v
                    .get("checkpoint")
                    .and_then(|c| c.get("json"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
            },
            wake_at_unix_ms: v.get("wakeAtUnixMs").and_then(|x| x.as_u64()).unwrap_or(0),
        },
        _ => JobOutcome::Completed {
            message,
            bytes_copied: v.get("bytesCopied").and_then(|x| x.as_u64()).unwrap_or(0),
        },
    })
}

/// Negotiates the granted `executeAtomic` body budget.
///
/// When no session is granted, the isolate cannot call `/db/executeAtomic`.
/// When a session is granted, `capabilities()` must succeed and
/// `maxRequestBytes` must be in `1..=MAX_SCALAR_BYTES` (`0` is not unlimited).
///
/// # Errors
///
/// Returns when capabilities cannot be obtained or the advertised budget is
/// zero or larger than the host scalar ceiling.
async fn granted_database_budget(database: Option<&dyn DatabaseSession>) -> AbiResult<(bool, u32)> {
    let Some(db) = database else {
        return Ok((false, 0));
    };
    let caps = db
        .capabilities()
        .await
        .map_err(|err| PluginError::unavailable(format!("database capabilities: {err}")))?;
    if caps.max_request_bytes == 0 || caps.max_request_bytes > MAX_SCALAR_BYTES {
        return Err(PluginError::invalid_params(format!(
            "database maxRequestBytes {} is not in 1..={MAX_SCALAR_BYTES}",
            caps.max_request_bytes
        )));
    }
    Ok((true, caps.max_request_bytes))
}

struct RevokeGrant {
    table: GrantedTable,
    grant: String,
}

impl Drop for RevokeGrant {
    fn drop(&mut self) {
        self.table.borrow_mut().remove(&self.grant);
    }
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc as StdArc;
    use std::time::Duration;

    use bookclerk_plugin_abi::v2::{
        ExecResult, NeverCancel, ProgressSink, QueryPage, Statement, Transaction, ENVELOPE_VERSION,
    };
    use bookclerk_plugin_abi::{DbCapabilities, DbConnectResult};

    struct NoopSource;

    #[async_trait(?Send)]
    impl Source for NoopSource {
        async fn open(&self, _key: &str) -> AbiResult<ReadResult> {
            Err(PluginError::unsupported("open"))
        }
    }

    struct NoopDest;

    #[async_trait(?Send)]
    impl Destination for NoopDest {
        async fn head(&self, _key: &str) -> AbiResult<Option<ObjectMetadata>> {
            Ok(None)
        }
        async fn list(&self, _options: ListOptions) -> AbiResult<ListPage> {
            Ok(ListPage {
                objects: Vec::new(),
                next_cursor: None,
            })
        }
        async fn get(&self, _key: &str, _range: Option<ByteRange>) -> AbiResult<ReadResult> {
            Err(PluginError::unsupported("get"))
        }
        async fn put(
            &self,
            _key: &str,
            _body: Pin<Box<dyn AsyncRead + Send>>,
            _options: WriteOptions,
        ) -> AbiResult<PutResult> {
            Err(PluginError::unsupported("put"))
        }
        async fn copy(&self, _from: &str, _to: &str) -> AbiResult<CopyResult> {
            Err(PluginError::unsupported("copy"))
        }
        async fn delete(&self, _key: &str) -> AbiResult<()> {
            Ok(())
        }
    }

    struct NoopProgress;

    #[async_trait(?Send)]
    impl ProgressSink for NoopProgress {
        async fn report(&self, _percent: f32, _message: &str) -> AbiResult<()> {
            Ok(())
        }
    }

    struct BudgetSession {
        fail: bool,
        max_request_bytes: u32,
    }

    #[async_trait(?Send)]
    impl DatabaseSession for BudgetSession {
        async fn execute(&self, _statement: Statement) -> AbiResult<ExecResult> {
            Err(PluginError::unsupported("execute"))
        }

        async fn query(
            &self,
            _statement: Statement,
            _cursor: &str,
            _limit: u32,
        ) -> AbiResult<QueryPage> {
            Err(PluginError::unsupported("query"))
        }

        async fn begin(&self) -> AbiResult<Box<dyn Transaction>> {
            Err(PluginError::unsupported("begin"))
        }

        async fn capabilities(&self) -> AbiResult<DbCapabilities> {
            if self.fail {
                return Err(PluginError::internal("capabilities boom"));
            }
            let mut caps = DbCapabilities::from_connect(&DbConnectResult::sqlite());
            caps.max_request_bytes = self.max_request_bytes;
            Ok(caps)
        }
    }

    fn job_context(database: Option<Box<dyn DatabaseSession>>) -> JobHandlerContext {
        JobHandlerContext {
            input: Box::new(NoopSource),
            output: Box::new(NoopDest),
            progress: Box::new(NoopProgress),
            database,
            cancel: Box::new(NeverCancel),
        }
    }

    fn test_invocation() -> JobInvocation {
        JobInvocation {
            envelope_version: ENVELOPE_VERSION,
            payload_schema_version: 1,
            invocation_id: "inv-1".into(),
            command_type: "stream_copy".into(),
            payload_json: "{}".into(),
            idempotency_key: "idem".into(),
            attempt: 1,
            correlation_id: "corr".into(),
            causation_id: None,
            deadline_unix_ms: 0,
            checkpoint: None,
            invocation_sequence: 1,
            step_id: None,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn granted_budget_fails_closed_on_missing_or_invalid_capabilities() {
        let err = granted_database_budget(Some(&BudgetSession {
            fail: true,
            max_request_bytes: MAX_SCALAR_BYTES,
        }))
        .await
        .expect_err("capabilities errors must fail the grant");
        assert!(err.to_string().contains("capabilities"), "{err}");

        let err = granted_database_budget(Some(&BudgetSession {
            fail: false,
            max_request_bytes: 0,
        }))
        .await
        .expect_err("zero maxRequestBytes must fail the grant");
        assert!(err.to_string().contains("maxRequestBytes"), "{err}");

        let err = granted_database_budget(Some(&BudgetSession {
            fail: false,
            max_request_bytes: MAX_SCALAR_BYTES + 1,
        }))
        .await
        .expect_err("oversized maxRequestBytes must fail the grant");
        assert!(err.to_string().contains("maxRequestBytes"), "{err}");

        assert_eq!(granted_database_budget(None).await.unwrap(), (false, 0));
        assert_eq!(
            granted_database_budget(Some(&BudgetSession {
                fail: false,
                max_request_bytes: 4096,
            }))
            .await
            .unwrap(),
            (true, 4096)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn failing_capabilities_does_not_insert_grant_or_dispatch_worker() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        let accepted = StdArc::new(AtomicBool::new(false));
        let flag = StdArc::clone(&accepted);
        let accept = tokio::spawn(async move {
            if listener.accept().await.is_ok() {
                flag.store(true, Ordering::SeqCst);
            }
        });

        let table: GrantedTable = Rc::new(RefCell::new(HashMap::new()));
        let handler = HttpJobHandler {
            http: BridgeHttp {
                port,
                token: "no-dispatch".into(),
            },
            ctx: WorkerContext {
                job_id: "job".into(),
                json: "{}".into(),
            },
            table: Rc::clone(&table),
        };
        let err = handler
            .handle(
                test_invocation(),
                job_context(Some(Box::new(BudgetSession {
                    fail: true,
                    max_request_bytes: MAX_SCALAR_BYTES,
                }))),
            )
            .await
            .expect_err("must fail closed");
        assert!(
            err.to_string().contains("capabilities"),
            "must not fall through to the worker bridge: {err}"
        );
        assert!(
            table.borrow().is_empty(),
            "grant must not be inserted when capabilities fail"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(75), accept)
                .await
                .is_err(),
            "worker HTTP must not be dispatched"
        );
        assert!(!accepted.load(Ordering::SeqCst));
    }
}
