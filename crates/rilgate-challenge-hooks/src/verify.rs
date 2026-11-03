use crate::{
  HookSet,
  accept::accepts,
  availability::{self, Availability, HookMetrics},
  definition::{Encoding, VerifyMethod},
  template::TemplateValue,
};
use anyhow::Result;
use std::{
  collections::BTreeMap,
  net::IpAddr,
  sync::{Arc, atomic::Ordering},
  time::{Duration, Instant},
};
use uuid::Uuid;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
  Pass,
  Fail,
  Unavailable,
}
pub struct VerifyValues<'a> {
  pub challenge_id: &'a str,
  pub host: &'a str,
  pub locale: &'a str,
  pub client_ip: Option<IpAddr>,
  pub user_agent: &'a str,
  pub result: &'a str,
}
pub struct ChallengeHooks {
  pub set: Arc<HookSet>,
  client: reqwest::Client,
  states: BTreeMap<String, Availability>,
}
impl ChallengeHooks {
  pub fn new(set: Arc<HookSet>) -> Result<Self> {
    let client = reqwest::Client::builder()
      .use_rustls_tls()
      .redirect(reqwest::redirect::Policy::none())
      .user_agent("Rilgate-ChallengeHook/1.0")
      .build()?;
    let states = set
      .hooks
      .keys()
      .map(|name| (name.clone(), Availability::default()))
      .collect();
    Ok(Self {
      set,
      client,
      states,
    })
  }
  pub fn is_unavailable(&self, name: &str) -> bool {
    self
      .states
      .get(name)
      .is_some_and(Availability::is_unavailable)
  }
  pub fn record(&self, name: &str, outcome: Outcome) {
    if let Some(state) = self.states.get(name) {
      state.record(name, outcome, &self.set);
    }
  }
  pub fn fallback(&self, name: &str) {
    if let Some(state) = self.states.get(name) {
      state.fallbacks.fetch_add(1, Ordering::Relaxed);
    }
  }
  pub fn metrics(&self) -> BTreeMap<String, HookMetrics> {
    availability::snapshot(&self.states)
  }
  pub async fn verify(&self, name: &str, input: &VerifyValues<'_>) -> Outcome {
    let outcome = self.verify_inner(name, input).await;
    self.record(name, outcome);
    outcome
  }
  async fn verify_inner(&self, name: &str, input: &VerifyValues<'_>) -> Outcome {
    let Some(hook) = self.set.hooks.get(name) else {
      return Outcome::Unavailable;
    };
    let verify = &hook.verify;
    let mut values = hook.values();
    let verification_id = Uuid::new_v4().to_string();
    let client_ip = input.client_ip.map(|ip| ip.to_string()).unwrap_or_default();
    for (key, value) in [
      ("challenge_id", input.challenge_id),
      ("host", input.host),
      ("locale", input.locale),
      ("client_ip", client_ip.as_str()),
      ("user_agent", input.user_agent),
      ("result", input.result),
      ("verification_id", verification_id.as_str()),
    ] {
      values
        .values
        .insert(key.into(), TemplateValue::String(value.into()));
    }
    let Ok(url) = verify.request.url.render_url(&values) else {
      return Outcome::Unavailable;
    };
    let mut headers = http::HeaderMap::new();
    for (key, template) in &verify.request.headers {
      let (Ok(key), Ok(value)) = (
        http::HeaderName::try_from(key.as_str()),
        http::HeaderValue::try_from(template.render(&values).to_string()),
      ) else {
        return Outcome::Fail;
      };
      headers.insert(key, value);
    }
    let fields: BTreeMap<_, _> = verify
      .request
      .fields
      .iter()
      .filter_map(|(key, value)| {
        let value = value.render(&values);
        if matches!(&value,TemplateValue::String(s) if s.is_empty()) {
          None
        } else {
          Some((key.clone(), value))
        }
      })
      .collect();
    let deadline = Instant::now() + verify.timeout;
    for attempt in 0..verify.retry.attempts {
      let Some(remaining) = deadline
        .checked_duration_since(Instant::now())
        .filter(|r| !r.is_zero())
      else {
        break;
      };
      let mut request = self
        .client
        .request(
          match verify.request.method {
            VerifyMethod::Get => reqwest::Method::GET,
            VerifyMethod::Post => reqwest::Method::POST,
          },
          url.clone(),
        )
        .headers(headers.clone());
      let strings: BTreeMap<_, _> = fields.iter().map(|(k, v)| (k, v.to_string())).collect();
      request = match verify.request.method {
        VerifyMethod::Get => request.query(&strings),
        VerifyMethod::Post => match verify.request.encoding {
          Encoding::Form => request.form(&strings),
          Encoding::Json => request.json(&fields),
        },
      };
      let attempt_result = tokio::time::timeout(remaining.min(verify.attempt_timeout), async {
        let mut response = request.send().await.map_err(AttemptError::Request)?;
        let status = response.status();
        let retry_after = response
          .headers()
          .get(http::header::RETRY_AFTER)
          .and_then(|s| s.to_str().ok())
          .filter(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
          .and_then(|s| s.parse::<u64>().ok())
          .map(Duration::from_secs);
        if status.is_client_error() && status.as_u16() != 429 {
          return Ok((Some(Outcome::Fail), None));
        }
        if matches!(status.as_u16(), 429 | 502 | 503 | 504) {
          return Ok((None, retry_after));
        }
        if !status.is_success() {
          return Ok((Some(Outcome::Unavailable), None));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| AttemptError::Body)? {
          if bytes.len() + chunk.len() > 16 * 1024 {
            return Err(AttemptError::Body);
          }
          bytes.extend_from_slice(&chunk);
        }
        let json: serde_json::Value =
          serde_json::from_slice(&bytes).map_err(|_| AttemptError::Body)?;
        if depth(&json) > 32 {
          return Err(AttemptError::Body);
        }
        Ok((
          Some(if accepts(&verify.accept, &json, &values) {
            Outcome::Pass
          } else {
            Outcome::Fail
          }),
          None,
        ))
      })
      .await;
      let retry_after = match attempt_result {
        Ok(Ok((Some(outcome), _))) => return outcome,
        Ok(Ok((None, retry_after))) => retry_after,
        Err(_) if verify.retry.on_timeout => None,
        Ok(Err(AttemptError::Request(error)))
          if (error.is_connect() && !error.is_timeout())
            || (error.is_timeout() && verify.retry.on_timeout) =>
        {
          None
        }
        _ => return Outcome::Unavailable,
      };
      if attempt + 1 == verify.retry.attempts {
        break;
      }
      let ceiling = verify.retry.backoff.saturating_mul(1 << attempt).as_nanos();
      // Rejection sampling keeps every delay in the inclusive range equally likely.
      let wait = retry_after.unwrap_or_else(|| {
        let range = ceiling + 1;
        let bits = loop {
          let bits = u128::from_le_bytes(*Uuid::new_v4().as_bytes());
          // UUID version and variant bits are fixed; pack the remaining random bits.
          let bytes = bits.to_le_bytes();
          let mut packed = 0u128;
          for (i, byte) in bytes.into_iter().enumerate() {
            let (value, width) = match i {
              6 => (byte & 0x0f, 4),
              8 => (byte & 0x3f, 6),
              _ => (byte, 8),
            };
            packed = (packed << width) | u128::from(value);
          }
          let sample_limit = (1u128 << 122) - (1u128 << 122) % range;
          if packed < sample_limit {
            break packed;
          }
        };
        Duration::from_nanos((bits % range) as u64)
      });
      if deadline
        .checked_duration_since(Instant::now())
        .is_none_or(|remaining| wait >= remaining)
      {
        break;
      }
      self.states[name].retries.fetch_add(1, Ordering::Relaxed);
      tokio::time::sleep(wait).await;
    }
    Outcome::Unavailable
  }
}
enum AttemptError {
  Request(reqwest::Error),
  Body,
}
fn depth(value: &serde_json::Value) -> usize {
  match value {
    serde_json::Value::Array(v) => 1 + v.iter().map(depth).max().unwrap_or(0),
    serde_json::Value::Object(v) => 1 + v.values().map(depth).max().unwrap_or(0),
    _ => 0,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    definition::*,
    template::{Context, Template},
  };
  use std::sync::{Mutex, atomic::AtomicUsize};
  use tokio::io::{AsyncReadExt, AsyncWriteExt};
  fn set(url: &str, on_timeout: bool) -> Arc<HookSet> {
    let template = |s| Template::parse(s, Context::Verify).unwrap();
    Arc::new(HookSet {
      hooks: BTreeMap::from([(
        "captcha".into(),
        HookDefinition {
          params: Default::default(),
          secrets: Default::default(),
          grant_ttl: Duration::from_secs(60),
          presentation: Presentation::Redirect {
            url: Template::parse("https://example.com/", Context::BrowserUrl).unwrap(),
            result_param: "result".into(),
          },
          verify: Verify {
            request: VerifyRequest {
              method: VerifyMethod::Post,
              url: template(url),
              encoding: Encoding::Form,
              headers: Default::default(),
              fields: BTreeMap::from([
                (
                  "id".into(),
                  Operand::Template(template("{verification_id}")),
                ),
                ("ip".into(), Operand::Template(template("{client_ip}"))),
              ]),
            },
            accept: vec![Accept {
              field: "/success".into(),
              operator: Operator::Equals(Operand::Literal(TemplateValue::Bool(true))),
            }],
            timeout: Duration::from_secs(2),
            attempt_timeout: Duration::from_millis(100),
            retry: Retry {
              attempts: 3,
              backoff: Duration::from_millis(10),
              on_timeout,
            },
          },
          unavailable: Unavailable {
            action: UnavailableAction::Deny,
            fallback: None,
            grant: FallbackGrant::WhileUnavailable,
            after: 2,
            recheck: Duration::from_secs(30),
          },
        },
      )]),
    })
  }
  fn values() -> VerifyValues<'static> {
    VerifyValues {
      challenge_id: "id",
      host: "example.com",
      locale: "en",
      client_ip: None,
      user_agent: "test",
      result: "result",
    }
  }
  struct Server {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
  }
  impl Drop for Server {
    fn drop(&mut self) {
      self.task.abort();
    }
  }
  async fn server(statuses: Vec<u16>, delay_first: bool) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/verify", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let count = Arc::new(AtomicUsize::new(0));
    let task = tokio::spawn(async move {
      loop {
        let (mut socket, _) = listener.accept().await.unwrap();
        let requests = captured.clone();
        let count = count.clone();
        let statuses = statuses.clone();
        tokio::spawn(async move {
          let mut data = Vec::new();
          let mut buffer = [0u8; 1024];
          loop {
            let n = socket.read(&mut buffer).await.unwrap();
            if n == 0 {
              return;
            }
            data.extend_from_slice(&buffer[..n]);
            if let Some(end) = data.windows(4).position(|v| v == b"\r\n\r\n") {
              let headers = String::from_utf8_lossy(&data[..end]).to_ascii_lowercase();
              let length = headers
                .lines()
                .find_map(|line| {
                  line
                    .strip_prefix("content-length:")
                    .and_then(|s| s.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
              if data.len() >= end + 4 + length {
                break;
              }
            }
          }
          let index = count.fetch_add(1, Ordering::Relaxed);
          requests
            .lock()
            .unwrap()
            .push(String::from_utf8(data).unwrap());
          if delay_first && index == 0 {
            tokio::time::sleep(Duration::from_millis(250)).await;
          }
          let status = statuses[index.min(statuses.len() - 1)];
          let body = "{\"success\":true}";
          let _ = socket
            .write_all(
              format!(
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
              )
              .as_bytes(),
            )
            .await;
        });
      }
    });
    Server {
      url,
      requests,
      task,
    }
  }
  #[tokio::test]
  async fn transient_status_retries_and_passes() {
    let server = server(vec![503, 200], false).await;
    let hooks = ChallengeHooks::new(set(&server.url, false)).unwrap();
    assert_eq!(hooks.verify("captcha", &values()).await, Outcome::Pass);
    assert_eq!(server.requests.lock().unwrap().len(), 2);
    assert_eq!(hooks.metrics()["captcha"].retries, 1);
  }
  #[tokio::test]
  async fn exhausted_status_retries_are_unavailable() {
    let server = server(vec![503], false).await;
    let hooks = ChallengeHooks::new(set(&server.url, false)).unwrap();
    assert_eq!(
      hooks.verify("captcha", &values()).await,
      Outcome::Unavailable
    );
    assert_eq!(server.requests.lock().unwrap().len(), 3);
  }
  #[tokio::test]
  async fn timeout_keeps_single_use_result_to_one_request() {
    let server = server(vec![200], true).await;
    let hooks = ChallengeHooks::new(set(&server.url, false)).unwrap();
    assert_eq!(
      hooks.verify("captcha", &values()).await,
      Outcome::Unavailable
    );
    assert_eq!(server.requests.lock().unwrap().len(), 1);
  }
  #[tokio::test]
  async fn timeout_retry_keeps_verification_id() {
    let server = server(vec![200], true).await;
    let hooks = ChallengeHooks::new(set(&server.url, true)).unwrap();
    assert_eq!(hooks.verify("captcha", &values()).await, Outcome::Pass);
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let body = |i: usize| requests[i].split("\r\n\r\n").nth(1).unwrap();
    assert_eq!(body(0), body(1));
    assert!(body(0).starts_with("id="));
  }
  #[tokio::test]
  async fn client_rejection_is_not_retried() {
    let server = server(vec![400], false).await;
    let hooks = ChallengeHooks::new(set(&server.url, false)).unwrap();
    assert_eq!(hooks.verify("captcha", &values()).await, Outcome::Fail);
    assert_eq!(server.requests.lock().unwrap().len(), 1);
  }
  #[tokio::test]
  async fn unknown_client_ip_is_omitted_from_fields() {
    let server = server(vec![200], false).await;
    let hooks = ChallengeHooks::new(set(&server.url, false)).unwrap();
    assert_eq!(hooks.verify("captcha", &values()).await, Outcome::Pass);
    assert!(
      !server.requests.lock().unwrap()[0]
        .split("\r\n\r\n")
        .nth(1)
        .unwrap()
        .contains("ip=")
    );
  }
  #[test]
  fn unavailable_threshold_and_pass_control_availability() {
    let hooks = ChallengeHooks::new(set("https://example.com/", false)).unwrap();
    hooks.record("captcha", Outcome::Unavailable);
    assert!(!hooks.is_unavailable("captcha"));
    hooks.record("captcha", Outcome::Unavailable);
    assert!(hooks.is_unavailable("captcha"));
    hooks.record("captcha", Outcome::Pass);
    assert!(!hooks.is_unavailable("captcha"));
  }
}
