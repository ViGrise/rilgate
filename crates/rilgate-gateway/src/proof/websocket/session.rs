use super::messages::{self, ClientWsMessage, ServerWsMessage};
use crate::runtime::state::AppState;
use anyhow::{Result, anyhow};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};
type WebSocket = WebSocketStream<tokio::io::DuplexStream>;
use futures_util::{SinkExt, StreamExt};
use rilgate_pool::StratumEvent;
use rilgate_pow::target::hash_meets_difficulty;
use rilgate_protocol::{PowAlgorithm, PowVerification};
use std::time::Duration;
use tokio::time::{Instant, sleep_until};

pub async fn handle(socket: WebSocket, state: AppState, difficulty: u64, host: String) {
  if let Err(error) = run(socket, state, difficulty, host).await {
    tracing::warn!(?error, "PoW WebSocket session ended with an error");
  }
}

async fn run(socket: WebSocket, state: AppState, difficulty: u64, host: String) -> Result<()> {
  if state.pow.verification(PowAlgorithm::RandomX) != PowVerification::PoolShare {
    return Err(anyhow!("WebSocket verification is disabled"));
  }

  let (mut ws_tx, mut ws_rx) = socket.split();
  let mut stratum = match state.pool.connect(difficulty).await {
    Ok(session) => session,
    Err(error) => {
      send_ws(
        &mut ws_tx,
        ServerWsMessage::Error {
          message: "Stratum pool connection unavailable".into(),
          code: error
            .is::<rilgate_pool::provider::AllPoolsUnavailable>()
            .then(|| "work_unavailable".into()),
        },
      )
      .await?;
      return Err(error);
    }
  };
  let session_deadline = Instant::now() + state.cfg.pow.stratum_pool.session_ttl;

  send_ws(
    &mut ws_tx,
    messages::job(
      stratum.current_job.clone(),
      difficulty,
      state.cfg.pow.nonce_offset,
    ),
  )
  .await?;

  let mut pending_submit: Option<(u64, Instant)> = None;

  loop {
    tokio::select! {
      _ = sleep_until(session_deadline) => {
        send_ws(&mut ws_tx, ServerWsMessage::Error { code: None, message: "Proof-of-work session expired".into() }).await?;
        return Ok(());
      }
      _ = async {
        if let Some((_, deadline)) = pending_submit {
          sleep_until(deadline).await;
        } else {
          std::future::pending::<()>().await;
        }
      } => {
        send_ws(&mut ws_tx, ServerWsMessage::Error { code: None, message: "Stratum pool share acceptance timed out".into() }).await?;
        return Ok(());
      }
      browser = ws_rx.next() => {
        let Some(browser) = browser else {
          return Ok(());
        };

        let browser = browser?;
        if matches!(browser, Message::Close(_)) { return Ok(()); }
        let Message::Text(text) = browser else {
          continue;
        };

        let message: ClientWsMessage = serde_json::from_str(&text)?;

        match message {
          ClientWsMessage::Share { job_id, nonce, result } => {
            if pending_submit.is_some() {
              send_ws(
                &mut ws_tx,
                ServerWsMessage::Rejected {
                  reason: "A share submission is already pending".to_string(),
                },
              ).await?;
              continue;
            }

            let valid_result = hex::decode(&result)
              .is_ok_and(|hash| hash_meets_difficulty(&hash, difficulty));
            if !stratum.is_recent_job(&job_id) || !valid_result {
              send_ws(&mut ws_tx, ServerWsMessage::Rejected {
                reason: "Share must match the current job and required difficulty".into(),
              }).await?;
              send_ws(
                &mut ws_tx,
                messages::job(stratum.current_job.clone(), difficulty, state.cfg.pow.nonce_offset),
              ).await?;
              continue;
            }
            let request_id = stratum.submit_share(&job_id, &nonce, &result).await?;
            pending_submit = Some((request_id, Instant::now() + state.cfg.pow.stratum_pool.submit_timeout));
          }
        }
      }

      event = stratum.read_event() => {
        let event = event?;

        match event {
          StratumEvent::Job(job) => {
            // Submissions remain pending until the pool responds, even during job rotation.
            send_ws(
              &mut ws_tx,
              messages::job(job,difficulty,state.cfg.pow.nonce_offset),
            ).await?;
          }

          StratumEvent::Response { id, accepted, error } => {
            if !pending_submit.is_some_and(|(pending_id, _)| pending_id == id) {
              continue;
            }

            pending_submit = None;

            if accepted {
              let receipt = state.pow.issue_stratum_pool_receipt(difficulty, host.clone())?;
              send_ws(&mut ws_tx, ServerWsMessage::Accepted { receipt }).await?;
              return Ok(());
            }

            send_ws(
              &mut ws_tx,
              ServerWsMessage::Rejected {
                reason: error
                  .unwrap_or_else(|| "Stratum pool rejected the share".to_string()),
              },
            )
            .await?;

            send_ws(
              &mut ws_tx,
              messages::job(stratum.current_job.clone(),difficulty,state.cfg.pow.nonce_offset),
            )
            .await?;
          }
        }
      }
    }
  }
}

async fn send_ws(
  sink: &mut futures_util::stream::SplitSink<WebSocket, Message>,
  message: ServerWsMessage,
) -> Result<()> {
  tokio::time::timeout(
    Duration::from_secs(5),
    sink.send(Message::Text(serde_json::to_string(&message)?.into())),
  )
  .await
  .map_err(|_| anyhow!("Browser WebSocket write timed out"))??;
  Ok(())
}
