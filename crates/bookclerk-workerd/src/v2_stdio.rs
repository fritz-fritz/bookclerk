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
    serve_plugin_stdio, ByteRange, CopyResult, Destination, DestinationContext, JobHandler,
    JobHandlerContext, JobInvocation, JobOutcome, ListOptions, ListPage, ObjectInfo,
    ObjectMetadata, PluginDescribe, PluginRoot, PutResult, ReadResult, ScalarLimitsDto, Source,
    SourceContext, WorkerContext, WriteOptions, MAX_LIST_PAGE, MAX_SCALAR_BYTES,
    MAX_STREAM_WINDOW_BYTES, PRODUCT_API_VERSION,
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
        })
    }

    async fn destination(&self, context: DestinationContext) -> AbiResult<Box<dyn Destination>> {
        let v = self
            .http
            .json_post(
                "/v2/destination",
                &serde_json::json!({
                    "json": context.json,
                }),
            )
            .await
            .map_err(map_http)?;
        let id = v
            .get("id")
            .and_then(|x| x.as_str())
            .ok_or_else(|| PluginError::internal("destination id missing"))?
            .to_string();
        Ok(Box::new(HttpDestination {
            http: self.http.clone(),
            id,
        }))
    }

    async fn source(&self, context: SourceContext) -> AbiResult<Box<dyn Source>> {
        let v = self
            .http
            .json_post(
                "/v2/source",
                &serde_json::json!({
                    "json": context.json,
                }),
            )
            .await
            .map_err(map_http)?;
        let id = v
            .get("id")
            .and_then(|x| x.as_str())
            .ok_or_else(|| PluginError::internal("source id missing"))?
            .to_string();
        Ok(Box::new(HttpSource {
            http: self.http.clone(),
            id,
        }))
    }

    async fn worker(&self, context: WorkerContext) -> AbiResult<Box<dyn JobHandler>> {
        let v = self
            .http
            .json_post(
                "/v2/worker",
                &serde_json::json!({
                    "jobId": context.job_id,
                    "json": context.json,
                }),
            )
            .await
            .map_err(map_http)?;
        let id = v
            .get("id")
            .and_then(|x| x.as_str())
            .ok_or_else(|| PluginError::internal("handler id missing"))?
            .to_string();
        Ok(Box::new(HttpJobHandler {
            http: self.http.clone(),
            id,
            table: Rc::clone(&self.table),
        }))
    }
}

struct HttpDestination {
    http: BridgeHttp,
    id: String,
}

#[async_trait(?Send)]
impl Destination for HttpDestination {
    async fn head(&self, key: &str) -> AbiResult<Option<ObjectMetadata>> {
        let v = self
            .http
            .json_post(
                &format!("/v2/dest/{}/head", self.id),
                &serde_json::json!({ "key": key }),
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
                &format!("/v2/dest/{}/list", self.id),
                &serde_json::json!({
                    "prefix": options.prefix,
                    "cursor": options.cursor,
                    "limit": options.limit,
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
        let mut path = format!("/v2/dest/{}/get?key={}", self.id, percent_encode(key));
        if let Some(r) = range {
            path.push_str(&format!("&offset={}", r.offset));
            if let Some(len) = r.length {
                path.push_str(&format!("&length={len}"));
            }
        }
        let (meta, body) = self.http.get_stream(&path).await.map_err(map_http)?;
        Ok(ReadResult { meta, body })
    }

    async fn put(
        &self,
        key: &str,
        body: Pin<Box<dyn AsyncRead + Send>>,
        options: WriteOptions,
    ) -> AbiResult<PutResult> {
        let path = format!("/v2/dest/{}/put?key={}", self.id, percent_encode(key));
        self.http
            .put_stream(
                &path,
                body,
                options.content_type.as_deref(),
                options.content_length,
            )
            .await
            .map_err(map_http)
    }

    async fn copy(&self, from: &str, to: &str) -> AbiResult<CopyResult> {
        let v = self
            .http
            .json_post(
                &format!("/v2/dest/{}/copy", self.id),
                &serde_json::json!({ "from": from, "to": to }),
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
                &format!("/v2/dest/{}/delete", self.id),
                &serde_json::json!({ "key": key }),
            )
            .await
            .map_err(map_http)?;
        Ok(())
    }
}

impl Drop for HttpDestination {
    fn drop(&mut self) {
        dispose_best_effort(self.http.clone(), format!("/v2/dest/{}/dispose", self.id));
    }
}

struct HttpSource {
    http: BridgeHttp,
    id: String,
}

#[async_trait(?Send)]
impl Source for HttpSource {
    async fn open(&self, key: &str) -> AbiResult<ReadResult> {
        let path = format!("/v2/source/{}/open?key={}", self.id, percent_encode(key));
        let (meta, body) = self.http.get_stream(&path).await.map_err(map_http)?;
        Ok(ReadResult { meta, body })
    }
}

impl Drop for HttpSource {
    fn drop(&mut self) {
        dispose_best_effort(self.http.clone(), format!("/v2/source/{}/dispose", self.id));
    }
}

struct HttpJobHandler {
    http: BridgeHttp,
    id: String,
    table: GrantedTable,
}

#[async_trait(?Send)]
impl JobHandler for HttpJobHandler {
    async fn handle(
        &self,
        invocation: JobInvocation,
        context: JobHandlerContext,
    ) -> AbiResult<JobOutcome> {
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
            },
        );
        let _revoke = RevokeGrant {
            table: Rc::clone(&self.table),
            grant: grant.clone(),
        };
        let result = self
            .http
            .json_post(
                &format!("/v2/handler/{}/handle", self.id),
                &serde_json::json!({
                    "grantToken": grant,
                    "invocation": invocation,
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

struct RevokeGrant {
    table: GrantedTable,
    grant: String,
}

impl Drop for RevokeGrant {
    fn drop(&mut self) {
        self.table.borrow_mut().remove(&self.grant);
    }
}

fn dispose_best_effort(http: BridgeHttp, path: String) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            let _ = http.json_post(&path, &serde_json::json!({})).await;
        });
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
