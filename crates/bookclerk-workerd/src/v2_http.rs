//! Minimal HTTP/1.1 client for the workerd v2 bridge (JSON + streamed bodies).

#![allow(clippy::missing_docs_in_private_items)]

use std::pin::Pin;

use anyhow::{anyhow, bail, Context, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use bookclerk_plugin_abi::v2::{ObjectMetadata, PutResult};

/// Loopback HTTP client for the isolate bridge.
#[derive(Clone)]
pub struct BridgeHttp {
    /// Bridge TCP port.
    pub port: u16,
    /// Bearer token (`BRIDGE_TOKEN`).
    pub token: String,
}

impl BridgeHttp {
    /// POST JSON and parse a JSON object response.
    ///
    /// # Errors
    ///
    /// Returns transport, HTTP, or JSON failures, including bridge `{error}`.
    pub async fn json_post(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let payload = serde_json::to_vec(body)?;
        let (status, headers, rest, mut stream) = self
            .exchange(
                "POST",
                path,
                &[("content-type", "application/json")],
                Some(&payload),
            )
            .await?;
        let body = read_body(&mut stream, &headers, rest).await?;
        if status == 401 {
            bail!("bridge unauthorized");
        }
        let value: serde_json::Value =
            serde_json::from_slice(&body).context("parse bridge JSON")?;
        if let Some(err) = value.get("error") {
            let code = err
                .get("code")
                .and_then(|c| c.as_str())
                .unwrap_or("internal");
            let message = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("bridge error");
            bail!("{code}: {message}");
        }
        if !(200..300).contains(&status) {
            bail!("bridge HTTP {status}: {}", String::from_utf8_lossy(&body));
        }
        Ok(value)
    }

    /// GET a streamed object body plus metadata headers.
    ///
    /// # Errors
    ///
    /// Returns transport or HTTP failures.
    pub async fn get_stream(
        &self,
        path: &str,
    ) -> Result<(ObjectMetadata, Pin<Box<dyn AsyncRead + Send>>)> {
        let (status, headers, rest, stream) = self.exchange("GET", path, &[], None).await?;
        if status == 401 {
            bail!("bridge unauthorized");
        }
        if !(200..300).contains(&status) {
            let mut stream = stream;
            let body = read_body(&mut stream, &headers, rest).await?;
            bail!("bridge HTTP {status}: {}", String::from_utf8_lossy(&body));
        }
        let meta = ObjectMetadata {
            key: header(&headers, "x-bookclerk-key").unwrap_or_default(),
            size: header(&headers, "x-bookclerk-size")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            content_type: header(&headers, "x-bookclerk-content-type"),
            etag: header(&headers, "x-bookclerk-etag"),
            sha256: None,
        };
        let body = body_reader(stream, headers, rest);
        Ok((meta, body))
    }

    /// PUT a streamed body.
    ///
    /// # Errors
    ///
    /// Returns transport, HTTP, or JSON failures.
    pub async fn put_stream(
        &self,
        path: &str,
        mut body: Pin<Box<dyn AsyncRead + Send>>,
        content_type: Option<&str>,
        content_length: Option<u64>,
    ) -> Result<PutResult> {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).await?;
        let mut req = format!("PUT {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nconnection: close\r\n", self.port, self.token);
        if let Some(ct) = content_type {
            req.push_str(&format!("content-type: {ct}\r\n"));
        }
        if let Some(len) = content_length {
            req.push_str(&format!("content-length: {len}\r\n\r\n"));
            stream.write_all(req.as_bytes()).await?;
            tokio::io::copy(&mut body, &mut stream).await?;
        } else {
            req.push_str("transfer-encoding: chunked\r\n\r\n");
            stream.write_all(req.as_bytes()).await?;
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = body.read(&mut buf).await?;
                if n == 0 {
                    stream.write_all(b"0\r\n\r\n").await?;
                    break;
                }
                let header = format!("{n:x}\r\n");
                stream.write_all(header.as_bytes()).await?;
                stream.write_all(&buf[..n]).await?;
                stream.write_all(b"\r\n").await?;
            }
        }
        stream.flush().await?;
        let (status, headers, rest) = read_response_head(&mut stream).await?;
        let raw = read_body(&mut stream, &headers, rest).await?;
        if !(200..300).contains(&status) {
            bail!("bridge HTTP {status}: {}", String::from_utf8_lossy(&raw));
        }
        let value: serde_json::Value =
            serde_json::from_slice(&raw).unwrap_or(serde_json::Value::Null);
        if let Some(err) = value.get("error") {
            let code = err
                .get("code")
                .and_then(|c| c.as_str())
                .unwrap_or("internal");
            let message = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("bridge error");
            bail!("{code}: {message}");
        }
        Ok(PutResult {
            key: value
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            bytes_written: value
                .get("bytesWritten")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            etag: value
                .get("etag")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            sha256: None,
        })
    }

    async fn exchange(
        &self,
        method: &str,
        path: &str,
        extra: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Result<(u16, Vec<(String, String)>, Vec<u8>, TcpStream)> {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).await?;
        let mut req = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nconnection: close\r\n",
            self.port, self.token
        );
        for (k, v) in extra {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        if let Some(body) = body {
            req.push_str(&format!("content-length: {}\r\n\r\n", body.len()));
            stream.write_all(req.as_bytes()).await?;
            stream.write_all(body).await?;
        } else {
            req.push_str("\r\n");
            stream.write_all(req.as_bytes()).await?;
        }
        stream.flush().await?;
        let (status, headers, rest) = read_response_head(&mut stream).await?;
        Ok((status, headers, rest, stream))
    }
}

fn header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.clone())
        .filter(|s| !s.is_empty())
}

async fn read_response_head(
    stream: &mut TcpStream,
) -> Result<(u16, Vec<(String, String)>, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(idx) = find_header_end(&buf) {
            let head = std::str::from_utf8(&buf[..idx]).context("response headers utf-8")?;
            let mut lines = head.split("\r\n");
            let status_line = lines.next().unwrap_or("");
            let status = status_line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| anyhow!("bad status line: {status_line}"))?;
            let mut headers = Vec::new();
            for line in lines {
                if line.is_empty() {
                    continue;
                }
                if let Some((k, v)) = line.split_once(':') {
                    headers.push((k.trim().to_string(), v.trim().to_string()));
                }
            }
            return Ok((status, headers, buf[idx..].to_vec()));
        }
        if buf.len() > 64 * 1024 {
            bail!("response headers too large");
        }
    }
    bail!("eof before HTTP headers")
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

async fn read_body(
    stream: &mut TcpStream,
    headers: &[(String, String)],
    prefix: Vec<u8>,
) -> Result<Vec<u8>> {
    let mut reader = body_reader_owned(stream, headers, prefix);
    let mut out = Vec::new();
    reader.read_to_end(&mut out).await?;
    Ok(out)
}

fn body_reader(
    stream: TcpStream,
    headers: Vec<(String, String)>,
    prefix: Vec<u8>,
) -> Pin<Box<dyn AsyncRead + Send>> {
    let chunked = header(&headers, "transfer-encoding")
        .map(|v| v.to_ascii_lowercase().contains("chunked"))
        .unwrap_or(false);
    let len = header(&headers, "content-length").and_then(|s| s.parse::<u64>().ok());
    if chunked {
        Box::pin(ChunkedBody {
            stream,
            prefix,
            state: ChunkState::Size,
            remain: 0,
        })
    } else if let Some(n) = len {
        let take = n.saturating_sub(prefix.len() as u64);
        let prefixed = PrefixRead {
            prefix,
            rest: Box::pin(stream.take(take)),
        };
        Box::pin(prefixed)
    } else {
        Box::pin(PrefixRead {
            prefix,
            rest: Box::pin(stream),
        })
    }
}

fn body_reader_owned<'a>(
    stream: &'a mut TcpStream,
    headers: &[(String, String)],
    prefix: Vec<u8>,
) -> Pin<Box<dyn AsyncRead + Send + 'a>> {
    let chunked = header(headers, "transfer-encoding")
        .map(|v| v.to_ascii_lowercase().contains("chunked"))
        .unwrap_or(false);
    let len = header(headers, "content-length").and_then(|s| s.parse::<u64>().ok());
    if chunked {
        Box::pin(ChunkedBody {
            stream,
            prefix,
            state: ChunkState::Size,
            remain: 0,
        })
    } else if let Some(n) = len {
        let take = n.saturating_sub(prefix.len() as u64);
        Box::pin(PrefixRead {
            prefix,
            rest: Box::pin(stream.take(take)),
        })
    } else {
        Box::pin(PrefixRead {
            prefix,
            rest: Box::pin(stream),
        })
    }
}

struct PrefixRead<R> {
    prefix: Vec<u8>,
    rest: Pin<Box<R>>,
}

impl<R: AsyncRead + Unpin> AsyncRead for PrefixRead<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if !self.prefix.is_empty() {
            let n = self.prefix.len().min(buf.remaining());
            buf.put_slice(&self.prefix[..n]);
            self.prefix.drain(..n);
            return std::task::Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.rest).poll_read(cx, buf)
    }
}

enum ChunkState {
    Size,
    Data,
    CrLfAfterData,
    Done,
}

struct ChunkedBody<S> {
    stream: S,
    prefix: Vec<u8>,
    state: ChunkState,
    remain: usize,
}

impl<S: AsyncRead + Unpin> ChunkedBody<S> {
    fn pull_prefix(&mut self, want: usize) -> Vec<u8> {
        let n = self.prefix.len().min(want);
        self.prefix.drain(..n).collect()
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for ChunkedBody<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::task::Poll;
        loop {
            match self.state {
                ChunkState::Done => return Poll::Ready(Ok(())),
                ChunkState::Size => {
                    if let Some(pos) = self.prefix.windows(2).position(|w| w == b"\r\n") {
                        let line = self.prefix.drain(..pos).collect::<Vec<_>>();
                        self.prefix.drain(..2);
                        let line = std::str::from_utf8(&line).unwrap_or("0");
                        let size = usize::from_str_radix(line.trim(), 16).unwrap_or(0);
                        if size == 0 {
                            self.state = ChunkState::Done;
                            continue;
                        }
                        self.remain = size;
                        self.state = ChunkState::Data;
                        continue;
                    }
                    let mut tmp = [0u8; 64];
                    let mut read_buf = tokio::io::ReadBuf::new(&mut tmp);
                    match Pin::new(&mut self.stream).poll_read(cx, &mut read_buf) {
                        Poll::Ready(Ok(())) => {
                            let n = read_buf.filled().len();
                            if n == 0 {
                                self.state = ChunkState::Done;
                                return Poll::Ready(Ok(()));
                            }
                            self.prefix.extend_from_slice(&tmp[..n]);
                        }
                        other => return other,
                    }
                }
                ChunkState::Data => {
                    if self.remain == 0 {
                        self.state = ChunkState::CrLfAfterData;
                        continue;
                    }
                    let want = self.remain.min(buf.remaining());
                    if !self.prefix.is_empty() {
                        let chunk = self.pull_prefix(want);
                        buf.put_slice(&chunk);
                        self.remain -= chunk.len();
                        return Poll::Ready(Ok(()));
                    }
                    let mut tmp = vec![0u8; want];
                    let mut read_buf = tokio::io::ReadBuf::new(&mut tmp);
                    match Pin::new(&mut self.stream).poll_read(cx, &mut read_buf) {
                        Poll::Ready(Ok(())) => {
                            let n = read_buf.filled().len();
                            if n == 0 {
                                self.state = ChunkState::Done;
                                return Poll::Ready(Ok(()));
                            }
                            buf.put_slice(&tmp[..n]);
                            self.remain -= n;
                            return Poll::Ready(Ok(()));
                        }
                        other => return other,
                    }
                }
                ChunkState::CrLfAfterData => {
                    if self.prefix.len() >= 2 {
                        self.prefix.drain(..2);
                        self.state = ChunkState::Size;
                        continue;
                    }
                    let mut tmp = [0u8; 2];
                    let mut read_buf = tokio::io::ReadBuf::new(&mut tmp);
                    match Pin::new(&mut self.stream).poll_read(cx, &mut read_buf) {
                        Poll::Ready(Ok(())) => {
                            self.prefix.extend_from_slice(read_buf.filled());
                        }
                        other => return other,
                    }
                }
            }
        }
    }
}
