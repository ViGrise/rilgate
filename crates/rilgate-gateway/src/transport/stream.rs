use async_trait::async_trait;
use pingora::protocols::{
  GetProxyDigest, GetSocketDigest, GetTimingDigest, Peek, Shutdown, SocketDigest, Ssl,
  TimingDigest, UniqueID, UniqueIDType, l4::stream::Stream, raw_connect::ProxyDigest,
};
use std::{
  io,
  pin::Pin,
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  task::{Context, Poll, ready},
  time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};

// Holds a generated challenge or error page together with its header.
const COALESCE_LIMIT: usize = 8 * 1024;

/// Client connection that can merge the writes of one locally generated response.
///
/// Writes pass straight through unless coalescing is enabled, so proxied responses reach
/// the client as soon as Pingora writes them. While coalescing, writes up to
/// [`COALESCE_LIMIT`] are held until the next flush, which Pingora issues when a response
/// ends; the buffer is released on every flush.
#[derive(Debug)]
pub struct DownstreamStream {
  inner: Stream,
  pending: Vec<u8>,
  written: usize,
  coalesce: AtomicBool,
}

impl DownstreamStream {
  pub fn new(inner: Stream) -> Self {
    Self {
      inner,
      pending: Vec::new(),
      written: 0,
      coalesce: AtomicBool::new(false),
    }
  }
  /// Enables or disables coalescing when `stream` is a `DownstreamStream`.
  pub fn set_coalescing(stream: Option<&pingora::protocols::Stream>, enabled: bool) {
    if let Some(stream) = stream.and_then(|stream| (**stream).as_any().downcast_ref::<Self>()) {
      stream.coalesce.store(enabled, Ordering::Relaxed);
    }
  }
  fn poll_drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    while self.written < self.pending.len() {
      let n = ready!(Pin::new(&mut self.inner).poll_write(cx, &self.pending[self.written..]))?;
      if n == 0 {
        return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
      }
      self.written += n;
    }
    self.pending = Vec::new();
    self.written = 0;
    Poll::Ready(Ok(()))
  }
  fn can_hold(&mut self, len: usize) -> bool {
    if !self.coalesce.load(Ordering::Relaxed) || self.pending.len() + len > COALESCE_LIMIT {
      return false;
    }
    if self.pending.capacity() == 0 {
      self.pending.reserve_exact(COALESCE_LIMIT);
    }
    true
  }
}

impl AsyncRead for DownstreamStream {
  fn poll_read(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buf: &mut ReadBuf<'_>,
  ) -> Poll<io::Result<()>> {
    Pin::new(&mut self.inner).poll_read(cx, buf)
  }
}

impl AsyncWrite for DownstreamStream {
  fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
    let this = self.get_mut();
    if !this.can_hold(buf.len()) {
      ready!(this.poll_drain(cx))?;
      if !this.can_hold(buf.len()) {
        return Pin::new(&mut this.inner).poll_write(cx, buf);
      }
    }
    this.pending.extend_from_slice(buf);
    Poll::Ready(Ok(buf.len()))
  }
  fn poll_write_vectored(
    self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    bufs: &[io::IoSlice<'_>],
  ) -> Poll<io::Result<usize>> {
    let this = self.get_mut();
    let len = bufs.iter().map(|buf| buf.len()).sum();
    if !this.can_hold(len) {
      ready!(this.poll_drain(cx))?;
      if !this.can_hold(len) {
        return Pin::new(&mut this.inner).poll_write_vectored(cx, bufs);
      }
    }
    for buf in bufs {
      this.pending.extend_from_slice(buf);
    }
    Poll::Ready(Ok(len))
  }
  fn is_write_vectored(&self) -> bool {
    self.inner.is_write_vectored()
  }
  fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    let this = self.get_mut();
    ready!(this.poll_drain(cx))?;
    Pin::new(&mut this.inner).poll_flush(cx)
  }
  fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    let this = self.get_mut();
    ready!(this.poll_drain(cx))?;
    Pin::new(&mut this.inner).poll_shutdown(cx)
  }
}

#[async_trait]
impl Shutdown for DownstreamStream {
  async fn shutdown(&mut self) {
    if let Err(error) = AsyncWriteExt::shutdown(self).await {
      tracing::debug!(%error, "Failed to shut down client connection");
    }
  }
}

#[async_trait]
impl Peek for DownstreamStream {
  async fn try_peek(&mut self, buf: &mut [u8]) -> io::Result<bool> {
    self.inner.try_peek(buf).await
  }
}

impl UniqueID for DownstreamStream {
  fn id(&self) -> UniqueIDType {
    self.inner.id()
  }
}

impl Ssl for DownstreamStream {}

impl GetTimingDigest for DownstreamStream {
  fn get_timing_digest(&self) -> Vec<Option<TimingDigest>> {
    self.inner.get_timing_digest()
  }
  fn get_read_pending_time(&self) -> Duration {
    self.inner.get_read_pending_time()
  }
  fn get_write_pending_time(&self) -> Duration {
    self.inner.get_write_pending_time()
  }
}

impl GetProxyDigest for DownstreamStream {
  fn get_proxy_digest(&self) -> Option<Arc<ProxyDigest>> {
    self.inner.get_proxy_digest()
  }
  fn set_proxy_digest(&mut self, digest: ProxyDigest) {
    self.inner.set_proxy_digest(digest);
  }
}

impl GetSocketDigest for DownstreamStream {
  fn get_socket_digest(&self) -> Option<Arc<SocketDigest>> {
    self.inner.get_socket_digest()
  }
  fn set_socket_digest(&mut self, digest: SocketDigest) {
    self.inner.set_socket_digest(digest);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tokio::io::AsyncReadExt;

  async fn read_now(peer: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut buf = vec![0; 64];
    match tokio::time::timeout(Duration::from_millis(100), peer.read(&mut buf)).await {
      Ok(read) => {
        buf.truncate(read.unwrap());
        buf
      }
      Err(_) => Vec::new(),
    }
  }

  #[tokio::test]
  async fn coalesced_writes_are_held_until_flush_and_others_pass_through() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = tokio::net::TcpStream::connect(listener.local_addr().unwrap());
    let (client, (mut peer, _)) = tokio::join!(client, async { listener.accept().await.unwrap() });
    let mut stream: pingora::protocols::Stream =
      Box::new(DownstreamStream::new(Stream::from(client.unwrap())));

    stream.write_all(b"direct").await.unwrap();
    assert_eq!(read_now(&mut peer).await, b"direct");

    DownstreamStream::set_coalescing(Some(&stream), true);
    stream.write_all(b"head").await.unwrap();
    stream.write_all(b"body").await.unwrap();
    assert!(read_now(&mut peer).await.is_empty());
    stream.flush().await.unwrap();
    assert_eq!(read_now(&mut peer).await, b"headbody");

    stream.write_all(b"held").await.unwrap();
    DownstreamStream::set_coalescing(Some(&stream), false);
    stream.write_all(b"next").await.unwrap();
    let mut ordered = [0; 8];
    peer.read_exact(&mut ordered).await.unwrap();
    assert_eq!(&ordered, b"heldnext");
  }
}
