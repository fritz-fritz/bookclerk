//! Bidirectional multiplexed byte streams with per-stream credit windows.
//!
//! Used for the native-behind-workerd socket proxy (guest [`Mux::open`],
//! gateway [`Mux::accept`]) and the OAuth callback tunnel (host opens, guest
//! accepts). Either side may initiate; connection ids from a client are odd
//! and from a server are even so they cannot collide.
//!
//! Frame layout (big-endian):
//! ```text
//! u32 length_of_rest | u8 type | u32 conn_id | payload
//! ```
//! Types: `Open=1`, `Data=2`, `Close=3`, `Window=4`. Data payloads are capped
//! at 1 MiB. A [`Window`] frame carries a `u32` credit (bytes the peer may
//! send). Each stream starts with [`INITIAL_WINDOW`] bytes of send credit.

#![allow(clippy::missing_docs_in_private_items)]

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::sync::{mpsc, Mutex};

use crate::{Result, SdkError};

/// Frame type byte for opening a multiplexed connection.
pub const TYPE_OPEN: u8 = 1;
/// Frame type byte for payload bytes on an open connection.
pub const TYPE_DATA: u8 = 2;
/// Frame type byte for closing a multiplexed connection.
pub const TYPE_CLOSE: u8 = 3;
/// Frame type byte for a credit-window update.
pub const TYPE_WINDOW: u8 = 4;
/// Maximum Data-frame payload in bytes (1 MiB); larger writes are split.
pub const MAX_FRAME_PAYLOAD: usize = 1024 * 1024;
/// Initial per-stream send credit, in bytes.
pub const INITIAL_WINDOW: u32 = 64 * 1024;

/// Client ids are odd (`1, 3, 5, …`).
const CLIENT_ID_BASE: u32 = 1;
/// Server ids are even (`2, 4, 6, …`).
const SERVER_ID_BASE: u32 = 2;

/// Outbound frame queued for the background writer task.
enum OutFrame {
    Open(u32),
    Data(u32, Vec<u8>),
    Close(u32),
    Window(u32, u32),
}

/// One multiplexed endpoint. Cheap to clone (shared tasks + id allocator).
#[derive(Clone)]
pub struct Mux {
    out_tx: mpsc::UnboundedSender<OutFrame>,
    next_id: Arc<AtomicU32>,
    inbound: Arc<Mutex<HashMap<u32, Inbound>>>,
    accept_rx: Arc<Mutex<mpsc::UnboundedReceiver<MuxStream>>>,
    _tasks: Arc<Vec<tokio::task::JoinHandle<()>>>,
}

/// Per-connection inbound slot used by the reader task.
struct Inbound {
    data_tx: mpsc::UnboundedSender<Vec<u8>>,
    send_credit: Arc<AtomicU32>,
    send_waker: Arc<Mutex<Option<Waker>>>,
}

/// One multiplexed logical connection implementing `AsyncRead` + `AsyncWrite`.
pub struct MuxStream {
    id: u32,
    out_tx: mpsc::UnboundedSender<OutFrame>,
    data_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    read_buf: Vec<u8>,
    send_credit: Arc<AtomicU32>,
    send_waker: Arc<Mutex<Option<Waker>>>,
    closed: bool,
}

impl MuxStream {
    /// Stable connection id shared with the peer for this logical stream.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.id
    }
}

impl Mux {
    /// Client endpoint (odd connection ids). Socket-proxy guests and OAuth hosts.
    pub fn client<R, W>(reader: R, writer: W) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        Self::new(reader, writer, CLIENT_ID_BASE)
    }

    /// Server endpoint (even connection ids). Socket-proxy gateway and OAuth guests.
    pub fn server<R, W>(reader: R, writer: W) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        Self::new(reader, writer, SERVER_ID_BASE)
    }

    fn new<R, W>(reader: R, writer: W, id_base: u32) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let (out_tx, out_rx) = mpsc::unbounded_channel::<OutFrame>();
        let (accept_tx, accept_rx) = mpsc::unbounded_channel();
        let inbound: Arc<Mutex<HashMap<u32, Inbound>>> = Arc::new(Mutex::new(HashMap::new()));
        let mut tasks = Vec::new();
        tasks.push(tokio::spawn(writer_task(writer, out_rx)));
        let inbound_task = Arc::clone(&inbound);
        let out_for_accept = out_tx.clone();
        tasks.push(tokio::spawn(async move {
            let mut reader = reader;
            while let Ok(frame) = read_frame(&mut reader).await {
                match frame {
                    Frame::Open { conn_id } => {
                        let (stream, slot) = MuxStream::pair(conn_id, out_for_accept.clone());
                        inbound_task.lock().await.insert(conn_id, slot);
                        if accept_tx.send(stream).is_err() {
                            break;
                        }
                    }
                    Frame::Data { conn_id, payload } => {
                        let map = inbound_task.lock().await;
                        if let Some(slot) = map.get(&conn_id) {
                            let _ = slot.data_tx.send(payload);
                        }
                    }
                    Frame::Close { conn_id } => {
                        inbound_task.lock().await.remove(&conn_id);
                    }
                    Frame::Window { conn_id, credit } => {
                        let map = inbound_task.lock().await;
                        if let Some(slot) = map.get(&conn_id) {
                            slot.send_credit.fetch_add(credit, Ordering::Release);
                            if let Some(waker) = slot.send_waker.lock().await.take() {
                                waker.wake();
                            }
                        }
                    }
                }
            }
            inbound_task.lock().await.clear();
        }));
        Self {
            out_tx,
            next_id: Arc::new(AtomicU32::new(id_base)),
            inbound,
            accept_rx: Arc::new(Mutex::new(accept_rx)),
            _tasks: Arc::new(tasks),
        }
    }

    /// Open a logical stream toward the peer.
    ///
    /// # Errors
    ///
    /// Returns [`SdkError`] when the writer task has exited.
    pub async fn open(&self) -> Result<MuxStream> {
        let id = self.next_id.fetch_add(2, Ordering::Relaxed);
        let (stream, slot) = MuxStream::pair(id, self.out_tx.clone());
        self.inbound.lock().await.insert(id, slot);
        self.out_tx
            .send(OutFrame::Open(id))
            .map_err(|_| SdkError::message("mux writer gone"))?;
        Ok(stream)
    }

    /// Wait for the next logical stream opened by the peer.
    ///
    /// # Errors
    ///
    /// Returns [`SdkError`] when the reader task exits (peer hangup or bad frame).
    pub async fn accept(&self) -> Result<MuxStream> {
        self.accept_rx
            .lock()
            .await
            .recv()
            .await
            .ok_or_else(|| SdkError::message("mux closed"))
    }
}

impl MuxStream {
    fn pair(id: u32, out_tx: mpsc::UnboundedSender<OutFrame>) -> (Self, Inbound) {
        let (data_tx, data_rx) = mpsc::unbounded_channel();
        let send_credit = Arc::new(AtomicU32::new(INITIAL_WINDOW));
        let send_waker = Arc::new(Mutex::new(None));
        let stream = Self {
            id,
            out_tx,
            data_rx,
            read_buf: Vec::new(),
            send_credit: Arc::clone(&send_credit),
            send_waker: Arc::clone(&send_waker),
            closed: false,
        };
        (
            stream,
            Inbound {
                data_tx,
                send_credit,
                send_waker,
            },
        )
    }

    fn grant_window(&self, n: u32) {
        if n == 0 || self.closed {
            return;
        }
        let _ = self.out_tx.send(OutFrame::Window(self.id, n));
    }
}

impl Drop for MuxStream {
    fn drop(&mut self) {
        if !self.closed {
            let _ = self.out_tx.send(OutFrame::Close(self.id));
            self.closed = true;
        }
    }
}

/// Parsed inbound frame after length and type checks.
enum Frame {
    Open { conn_id: u32 },
    Data { conn_id: u32, payload: Vec<u8> },
    Close { conn_id: u32 },
    Window { conn_id: u32, credit: u32 },
}

async fn writer_task<W: AsyncWrite + Unpin>(
    mut writer: W,
    mut out_rx: mpsc::UnboundedReceiver<OutFrame>,
) {
    while let Some(frame) = out_rx.recv().await {
        let (typ, conn_id, payload) = match frame {
            OutFrame::Open(id) => (TYPE_OPEN, id, Vec::new()),
            OutFrame::Data(id, p) => (TYPE_DATA, id, p),
            OutFrame::Close(id) => (TYPE_CLOSE, id, Vec::new()),
            OutFrame::Window(id, credit) => (TYPE_WINDOW, id, credit.to_be_bytes().to_vec()),
        };
        if write_raw(&mut writer, typ, conn_id, &payload)
            .await
            .is_err()
        {
            break;
        }
    }
}

/// Reads one length-prefixed frame; rejects unknown types and oversize payloads.
///
/// # Errors
///
/// Returns [`SdkError::message`] when the frame is invalid or I/O fails.
async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Frame> {
    let len = reader.read_u32().await.map_err(io_err)?;
    if len < 5 || (len as usize) > MAX_FRAME_PAYLOAD + 5 {
        return Err(SdkError::message(format!("mux bad frame length {len}")));
    }
    let mut rest = vec![0u8; len as usize];
    reader.read_exact(&mut rest).await.map_err(io_err)?;
    let typ = rest[0];
    let conn_id = u32::from_be_bytes(
        rest[1..5]
            .try_into()
            .map_err(|_| SdkError::message("mux short conn id"))?,
    );
    let payload = rest[5..].to_vec();
    match typ {
        TYPE_OPEN => Ok(Frame::Open { conn_id }),
        TYPE_DATA => Ok(Frame::Data { conn_id, payload }),
        TYPE_CLOSE => Ok(Frame::Close { conn_id }),
        TYPE_WINDOW => {
            if payload.len() != 4 {
                return Err(SdkError::message("mux window payload must be 4 bytes"));
            }
            let credit = u32::from_be_bytes(
                payload
                    .as_slice()
                    .try_into()
                    .map_err(|_| SdkError::message("mux window credit"))?,
            );
            Ok(Frame::Window { conn_id, credit })
        }
        other => Err(SdkError::message(format!("mux unknown frame type {other}"))),
    }
}

/// Writes one length-prefixed mux frame and flushes it.
///
/// # Errors
///
/// Returns [`SdkError`] when the underlying write or flush fails.
async fn write_raw<W: AsyncWrite + Unpin>(
    writer: &mut W,
    typ: u8,
    conn_id: u32,
    payload: &[u8],
) -> Result<()> {
    let len = (1 + 4 + payload.len()) as u32;
    let mut buf = Vec::with_capacity(4 + len as usize);
    buf.extend_from_slice(&len.to_be_bytes());
    buf.push(typ);
    buf.extend_from_slice(&conn_id.to_be_bytes());
    buf.extend_from_slice(payload);
    writer.write_all(&buf).await.map_err(io_err)?;
    writer.flush().await.map_err(io_err)?;
    Ok(())
}

fn io_err(err: std::io::Error) -> SdkError {
    SdkError::message(err.to_string())
}

impl AsyncRead for MuxStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if !self.read_buf.is_empty() {
            let n = buf.remaining().min(self.read_buf.len());
            buf.put_slice(&self.read_buf[..n]);
            self.read_buf.drain(..n);
            self.grant_window(n as u32);
            return Poll::Ready(Ok(()));
        }
        if self.closed {
            return Poll::Ready(Ok(()));
        }
        match Pin::new(&mut self.data_rx).poll_recv(cx) {
            Poll::Ready(Some(chunk)) => {
                let n = buf.remaining().min(chunk.len());
                buf.put_slice(&chunk[..n]);
                if n < chunk.len() {
                    self.read_buf.extend_from_slice(&chunk[n..]);
                }
                self.grant_window(n as u32);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(None) => {
                self.closed = true;
                Poll::Ready(Ok(()))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWrite for MuxStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.closed {
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "mux closed",
            )));
        }
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let credit = self.send_credit.load(Ordering::Acquire);
        if credit == 0 {
            let mut slot = match self.send_waker.try_lock() {
                Ok(slot) => slot,
                Err(_) => {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
            };
            *slot = Some(cx.waker().clone());
            // Re-check after installing the waker to avoid a lost Window.
            if self.send_credit.load(Ordering::Acquire) == 0 {
                return Poll::Pending;
            }
        }
        let credit = self.send_credit.load(Ordering::Acquire);
        if credit == 0 {
            return Poll::Pending;
        }
        let n = buf.len().min(MAX_FRAME_PAYLOAD).min(credit as usize);
        match self.out_tx.send(OutFrame::Data(self.id, buf[..n].to_vec())) {
            Ok(()) => {
                self.send_credit.fetch_sub(n as u32, Ordering::AcqRel);
                Poll::Ready(Ok(n))
            }
            Err(_) => Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "mux writer gone",
            ))),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        if !self.closed {
            let _ = self.out_tx.send(OutFrame::Close(self.id));
            self.closed = true;
        }
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
#[allow(clippy::missing_panics_doc)]
mod tests {
    use super::*;
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};

    fn pair() -> (Mux, Mux) {
        let (a, b) = duplex(64 * 1024);
        let (ar, aw) = tokio::io::split(a);
        let (br, bw) = tokio::io::split(b);
        (Mux::client(ar, aw), Mux::server(br, bw))
    }

    #[tokio::test]
    async fn client_open_server_accept_echo() {
        let (client, server) = pair();
        let mut c = client.open().await.expect("open");
        let mut s = server.accept().await.expect("accept");
        c.write_all(b"hello").await.expect("write");
        let mut buf = [0u8; 5];
        s.read_exact(&mut buf).await.expect("read");
        assert_eq!(&buf, b"hello");
        s.write_all(b"world").await.expect("write");
        let mut buf2 = [0u8; 5];
        c.read_exact(&mut buf2).await.expect("read");
        assert_eq!(&buf2, b"world");
    }

    #[tokio::test]
    async fn server_can_open_toward_the_client() {
        let (client, server) = pair();
        let mut s = server.open().await.expect("server open");
        let mut c = client.accept().await.expect("client accept");
        s.write_all(b"up").await.expect("write");
        let mut buf = [0u8; 2];
        c.read_exact(&mut buf).await.expect("read");
        assert_eq!(&buf, b"up");
    }

    #[tokio::test]
    async fn two_streams_backpressure_does_not_stall_the_other() {
        let (client, server) = pair();
        let mut a = client.open().await.expect("open a");
        let mut b = client.open().await.expect("open b");
        let mut sa = server.accept().await.expect("accept a");
        let mut sb = server.accept().await.expect("accept b");

        let payload = vec![0xABu8; INITIAL_WINDOW as usize + 8 * 1024];
        let payload_b = vec![0xCDu8; 4096];

        let writer_a = tokio::spawn(async move {
            a.write_all(&payload).await.expect("write a");
            a.shutdown().await.expect("shutdown a");
            payload.len()
        });
        let writer_b = tokio::spawn(async move {
            b.write_all(&payload_b).await.expect("write b");
            b.shutdown().await.expect("shutdown b");
            payload_b.len()
        });

        // Drain B first while A is still credit-blocked.
        let mut got_b = Vec::new();
        sb.read_to_end(&mut got_b).await.expect("read b");
        assert_eq!(got_b.len(), 4096);
        assert!(got_b.iter().all(|x| *x == 0xCD));
        writer_b.await.expect("join b");

        let mut got_a = Vec::new();
        sa.read_to_end(&mut got_a).await.expect("read a");
        assert_eq!(got_a.len(), INITIAL_WINDOW as usize + 8 * 1024);
        writer_a.await.expect("join a");
    }
}
