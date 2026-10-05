use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedWriteHalf;
const MAX_STRATUM_LINE_BYTES: usize = 64 * 1024;

pub(crate) struct JsonLineReader<R> {
  reader: BufReader<R>,
  pending: Vec<u8>,
}

impl<R: AsyncRead + Unpin> JsonLineReader<R> {
  pub(crate) fn new(reader: R) -> Self {
    Self {
      reader: BufReader::new(reader),
      pending: Vec::new(),
    }
  }

  pub(crate) async fn next_json(&mut self) -> Result<Value> {
    loop {
      let available = self.reader.fill_buf().await?;
      if available.is_empty() {
        return Err(anyhow!(
          "P2Pool Stratum connection closed before a complete message"
        ));
      }
      let newline = available.iter().position(|byte| *byte == b'\n');
      let count = newline.map_or(available.len(), |index| index + 1);
      if self.pending.len() + count > MAX_STRATUM_LINE_BYTES {
        return Err(anyhow!("P2Pool Stratum message exceeds 64 KiB"));
      }
      // Partial frames remain buffered when a caller cancels a read.
      self.pending.extend_from_slice(&available[..count]);
      self.reader.consume(count);
      if newline.is_some() {
        let message = serde_json::from_slice(&self.pending).context("Invalid P2Pool Stratum JSON");
        self.pending.clear();
        return message;
      }
    }
  }
}

pub(crate) async fn write_json_line(writer: &mut OwnedWriteHalf, value: &Value) -> Result<()> {
  let mut encoded = serde_json::to_vec(value)?;
  encoded.push(b'\n');
  writer.write_all(&encoded).await?;
  writer.flush().await?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::{future::Future, task::Poll};

  #[tokio::test]
  async fn partial_json_survives_a_canceled_read() {
    let (read, mut write) = tokio::io::duplex(1024);
    let mut reader = JsonLineReader::new(read);
    write.write_all(b"{\"id\":2,\"res").await.unwrap();
    {
      let future = reader.next_json();
      tokio::pin!(future);
      std::future::poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        Poll::Ready(())
      })
      .await;
    }
    assert!(!reader.pending.is_empty());
    write
      .write_all(b"ult\":{\"status\":\"OK\"}}\n")
      .await
      .unwrap();
    let message = reader.next_json().await.unwrap();
    assert_eq!(message["id"], 2);
    assert_eq!(message["result"]["status"], "OK");
  }

  #[tokio::test]
  async fn oversized_json_is_rejected_before_a_newline() {
    let (read, mut write) = tokio::io::duplex(MAX_STRATUM_LINE_BYTES * 2);
    write
      .write_all(&vec![b'x'; MAX_STRATUM_LINE_BYTES + 1])
      .await
      .unwrap();
    let mut reader = JsonLineReader::new(read);
    assert!(
      reader
        .next_json()
        .await
        .unwrap_err()
        .to_string()
        .contains("exceeds")
    );
    assert!(reader.pending.len() <= MAX_STRATUM_LINE_BYTES);
  }

  #[tokio::test]
  async fn truncated_json_is_rejected_when_the_peer_disconnects() {
    let (read, mut write) = tokio::io::duplex(1024);
    write.write_all(b"{\"id\":2}").await.unwrap();
    drop(write);
    assert!(JsonLineReader::new(read).next_json().await.is_err());
  }
}
