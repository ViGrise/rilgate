use crate::{
  proof::handlers,
  proxy::gate::{gate_with_client, page_response},
  runtime::state::AppState,
  transport::{
    errors::ApiError,
    response::{Response, json, response},
    stream::DownstreamStream,
  },
  web::{assets, pages::render_page},
};
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use http::{
  Method, StatusCode,
  header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, HOST},
};
use http_body::Body as _;
use http_body_util::BodyExt;
use pingora::{
  Error, ErrorType,
  apps::{HttpServerOptions, ServerApp},
  protocols::{GetSocketDigest, SocketDigest, l4::stream::Stream},
  proxy::{FailToProxy, HttpProxy, ProxyHttp, Session, http_proxy},
  server::configuration::ServerConf,
  upstreams::peer::HttpPeer,
};
use std::{
  sync::Arc,
  time::{Duration, Instant},
};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::TcpListener,
  sync::watch,
  task::JoinSet,
};

#[derive(Clone)]
pub struct Gateway {
  proxy: Arc<HttpProxy<Handler>>,
  state: AppState,
}
struct Handler {
  state: AppState,
}
pub struct RequestContext {
  id: http::HeaderValue,
  started: Instant,
  uploaded: usize,
  connect_attempts: usize,
  addresses: usize,
  upstream: Option<http::Uri>,
  failure: Option<StatusCode>,
  permit: Option<tokio::sync::OwnedSemaphorePermit>,
}
impl Gateway {
  pub fn new(state: AppState) -> Self {
    // Share the existing Tokio timer wheel rather than register HTTP deadlines with a second clock.
    pingora::prelude::fast_timeout::set_fast_timeout_to_tokio_threshold(Some(Duration::ZERO));
    let conf = Arc::new(ServerConf {
      max_retries: 16,
      ..Default::default()
    });
    let mut proxy = http_proxy(
      &conf,
      Handler {
        state: state.clone(),
      },
    );
    let mut options = HttpServerOptions::default();
    options.h2c = true;
    options.keepalive_request_limit = Some(1000);
    options.h2_idle_timeout = Some(Duration::from_secs(60));
    proxy.server_options = Some(options);
    let mut h2 = pingora::protocols::http::v2::server::default_h2_options();
    h2.max_concurrent_streams(state.cfg.server.h2_max_streams);
    proxy.h2_options = Some(h2);
    Self {
      proxy: Arc::new(proxy),
      state,
    }
  }
  pub async fn serve(
    self,
    listener: TcpListener,
    shutdown: impl std::future::Future<Output = ()>,
  ) -> anyhow::Result<()> {
    let (stop, stopped) = watch::channel(false);
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);
    let outcome = loop {
      tokio::select! {
        _ = &mut shutdown => break Ok(()),
        accepted = listener.accept() => {
          let (socket, _) = match accepted { Ok(pair) => pair, Err(error) => break Err(error.into()) };
          let permit = match self.state.connections.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
              self.state.metrics.connection_rejections.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
              drop(socket);
              continue;
            }
          };
          if let Err(error) = socket.set_nodelay(true) { break Err(error.into()); }
          #[cfg(unix)]
          let digest = { use std::os::fd::AsRawFd; SocketDigest::from_raw_fd(socket.as_raw_fd()) };
          #[cfg(windows)]
          let digest = { use std::os::windows::io::AsRawSocket; SocketDigest::from_raw_socket(socket.as_raw_socket()) };
          let mut stream = Stream::from(socket);
          stream.set_socket_digest(digest);
          let stream = DownstreamStream::new(stream);
          let proxy = self.proxy.clone();
          let stopped = stopped.clone();
          connections.spawn(async move { let _permit = permit; proxy.process_new(Box::new(stream), &stopped).await; });
        }
        Some(result) = connections.join_next(), if !connections.is_empty() => {
          if let Err(error) = result { tracing::warn!(%error, "HTTP connection task failed"); }
        }
      }
    };
    drop(listener);
    self.state.shutdown.cancel();
    let _ = stop.send(true);
    self.proxy.cleanup().await;
    if tokio::time::timeout(Duration::from_secs(30), async {
      while connections.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
      connections.abort_all();
      while connections.join_next().await.is_some() {}
    }
    outcome
  }
}

fn failure(status: StatusCode, message: &str) -> Box<Error> {
  Error::explain(ErrorType::HTTPStatus(status.as_u16()), message.to_owned())
}
fn new_request_id() -> http::HeaderValue {
  thread_local! {
    static CONTEXT: uuid::ContextV7 = const { uuid::ContextV7::new() };
  }
  // Independent counters preserve UUIDv7 ordering within each worker without a shared lock.
  let id = CONTEXT.with(|context| uuid::Uuid::new_v7(uuid::Timestamp::now(context)));
  let mut buffer = [0; 36];
  http::HeaderValue::from_bytes(id.hyphenated().encode_lower(&mut buffer).as_bytes())
    .expect("valid UUID")
}
async fn send(session: &mut Session, out: Response, ctx: &RequestContext) -> pingora::Result<()> {
  DownstreamStream::set_coalescing(session.stream(), true);
  let result = write_local(session, out, ctx).await;
  DownstreamStream::set_coalescing(session.stream(), false);
  result
}
async fn write_local(
  session: &mut Session,
  out: Response,
  ctx: &RequestContext,
) -> pingora::Result<()> {
  let (mut parts, mut body) = out.into_parts();
  parts.headers.insert("x-request-id", ctx.id.clone());
  if !parts.headers.contains_key(CONTENT_LENGTH)
    && parts.status != StatusCode::NOT_MODIFIED
    && let Some(size) = body.size_hint().exact()
  {
    parts.headers.insert(CONTENT_LENGTH, size.into());
  }
  // Local responses do not consume untrusted uploads merely to reuse a connection.
  if !session.is_body_done() {
    session.set_keepalive(None);
  }
  let header = pingora::http::ResponseHeader::from(parts);
  session
    .write_response_header(Box::new(header), false)
    .await?;
  if session.req_header().method != Method::HEAD {
    while let Some(frame) = body.frame().await {
      let frame = frame.map_err(|error| {
        Error::because(ErrorType::WriteError, "Local response body failed", error)
      })?;
      if let Ok(bytes) = frame.into_data() {
        session.write_response_body(Some(bytes), false).await?;
      }
    }
  }
  session.write_response_body(None, true).await
}
async fn read_json<T: serde::de::DeserializeOwned>(
  session: &mut Session,
) -> Result<T, Box<Response>> {
  let media = session
    .req_header()
    .headers
    .get(CONTENT_TYPE)
    .and_then(|v| v.to_str().ok())
    .unwrap_or_default()
    .split(';')
    .next()
    .unwrap_or_default()
    .trim();
  if media != "application/json" && !(media.starts_with("application/") && media.ends_with("+json"))
  {
    return Err(Box::new(response(
      StatusCode::UNSUPPORTED_MEDIA_TYPE,
      "Expected application/json",
    )));
  }
  if session
    .req_header()
    .headers
    .get(CONTENT_LENGTH)
    .and_then(|v| v.to_str().ok())
    .and_then(|v| v.parse::<usize>().ok())
    .is_some_and(|n| n > 16 * 1024)
  {
    return Err(Box::new(ApiError::PayloadTooLarge.into_response()));
  }
  let mut bytes = BytesMut::new();
  loop {
    match session.read_request_body().await {
      Ok(Some(chunk)) if bytes.len().saturating_add(chunk.len()) <= 16 * 1024 => {
        bytes.extend_from_slice(&chunk)
      }
      Ok(Some(_)) => return Err(Box::new(ApiError::PayloadTooLarge.into_response())),
      Ok(None) => break,
      Err(_) => {
        return Err(Box::new(
          ApiError::InvalidRequest("Unable to read request body".into()).into_response(),
        ));
      }
    }
  }
  serde_json::from_slice(&bytes).map_err(|error| {
    Box::new(response(
      if error.is_data() {
        StatusCode::UNPROCESSABLE_ENTITY
      } else {
        StatusCode::BAD_REQUEST
      },
      error.to_string(),
    ))
  })
}
impl Handler {
  async fn local(
    &self,
    session: &mut Session,
    ctx: &mut RequestContext,
    path: &str,
  ) -> pingora::Result<Option<Response>> {
    let headers = session.req_header().headers.clone();
    let uri = session.req_header().uri.clone();
    let method = session.req_header().method.clone();
    let proof = matches!(
      path,
      "/challenge" | "/api/challenge" | "/api/verify" | "/api/exchange" | "/api/ws"
    );
    let expected = if matches!(path, "/api/challenge" | "/api/verify" | "/api/exchange") {
      Method::POST
    } else {
      Method::GET
    };
    let known = proof
      || matches!(
        path,
        "/api/client-config" | "/api/health" | "/api/live" | "/api/ready"
      )
      || (path == "/api/metrics" && self.state.cfg.server.metrics_enabled)
      || path.starts_with("/static/");
    if proof {
      let peer = session
        .client_addr()
        .and_then(|address| address.as_inet())
        .map(|address| address.ip());
      let peer =
        match crate::transport::client_ip::client_ip(peer, &headers, &self.state.cfg.server) {
          Ok(peer) => peer,
          Err(message) => {
            return Ok(Some(
              ApiError::InvalidRequest(message.into()).into_response(),
            ));
          }
        };
      if let Err(seconds) = self.state.limiter.check(peer) {
        let mut out = ApiError::RateLimited(seconds).into_response();
        out
          .headers_mut()
          .insert(CACHE_CONTROL, "no-store".parse().unwrap());
        return Ok(Some(out));
      }
    }
    let mut out = if !known {
      response(StatusCode::NOT_FOUND, "")
    } else if method != expected
      && !(expected == Method::GET && method == Method::HEAD && path != "/api/ws")
    {
      let mut out = response(StatusCode::METHOD_NOT_ALLOWED, "");
      out.headers_mut().insert(
        http::header::ALLOW,
        (if expected == Method::POST {
          "POST"
        } else {
          "GET, HEAD"
        })
        .parse()
        .unwrap(),
      );
      out
    } else {
      let result: Result<Response, ApiError> = match path {
        "/challenge" => {
          crate::web::pages::challenge_page(&self.state, &headers, &uri, method == Method::HEAD)
        }
        "/api/client-config" => Ok(handlers::client_config(&self.state)),
        "/api/live" => Ok(json(StatusCode::OK, &serde_json::json!({"status": "ok"}))),
        "/api/ready" => Ok(crate::observability::readiness::service::check(&self.state).await),
        "/api/metrics" => Ok(crate::observability::metrics::service::exposition(
          &self.state,
        )),
        "/api/health" => Ok(json(
          StatusCode::OK,
          &crate::observability::health::service::status(
            self.state.web.diagnostics(),
            self.state.pool.failures(),
          ),
        )),
        "/api/challenge" => match read_json(session).await {
          Ok(value) => handlers::challenge(&self.state, &headers, &uri, value).await,
          Err(out) => Ok(*out),
        },
        "/api/verify" => match read_json(session).await {
          Ok(value) => handlers::verify(&self.state, &headers, &uri, value).await,
          Err(out) => Ok(*out),
        },
        "/api/exchange" => match read_json(session).await {
          Ok(value) => handlers::exchange(&self.state, &headers, &uri, value),
          Err(out) => Ok(*out),
        },
        "/api/ws" => match self.websocket(session, ctx).await {
          Ok(()) => return Ok(None),
          Err(error) => Err(error),
        },
        _ => {
          let raw = path.strip_prefix("/static/").unwrap_or_default();
          match percent_encoding::percent_decode_str(raw).decode_utf8() {
            Ok(path) => assets::static_asset(&self.state, &path, &headers, &method),
            Err(_) => Ok(response(StatusCode::BAD_REQUEST, "Invalid asset path")),
          }
        }
      };
      result.unwrap_or_else(ApiError::into_response)
    };
    if path.starts_with("/api/") {
      out
        .headers_mut()
        .insert(CACHE_CONTROL, "no-store".parse().unwrap());
    }
    Ok(Some(out))
  }
  async fn websocket(&self, session: &mut Session, ctx: &RequestContext) -> Result<(), ApiError> {
    let header = session.req_header();
    let mut request = http::Request::builder()
      .method(header.method.clone())
      .uri(header.uri.clone())
      .version(header.version)
      .body(())
      .unwrap();
    *request.headers_mut() = header.headers.clone();
    let handshake = tokio_tungstenite::tungstenite::handshake::server::create_response(&request)
      .map_err(|error| ApiError::InvalidRequest(error.to_string()))?;
    let presentation = handlers::websocket_admission(&self.state, &header.headers, &header.uri)?;
    let _slot = self.state.pool.acquire_slot().map_err(ApiError::proof)?;
    let (mut parts, _) = handshake.into_parts();
    parts.headers.insert("x-request-id", ctx.id.clone());
    parts
      .headers
      .insert(CACHE_CONTROL, "no-store".parse().unwrap());
    session
      .write_response_header(Box::new(parts.into()), false)
      .await
      .map_err(|error| ApiError::Internal(anyhow::anyhow!(error)))?;
    session.set_read_timeout(None);
    session.set_write_timeout(Some(Duration::from_secs(5)));
    let (socket, mut bridge) = tokio::io::duplex(16 * 1024);
    let socket = tokio_tungstenite::WebSocketStream::from_raw_socket(
      socket,
      tokio_tungstenite::tungstenite::protocol::Role::Server,
      Some(
        tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
          .max_message_size(Some(4096))
          .max_frame_size(Some(4096)),
      ),
    )
    .await;
    let handle = crate::proof::websocket::handle(
      socket,
      self.state.clone(),
      presentation.difficulty,
      presentation.host,
    );
    session.set_keepalive(None);
    let relay = async move {
      let mut bytes = [0u8; 8192];
      loop {
        tokio::select! {
          chunk = session.read_request_body() => match chunk {
            Ok(Some(chunk)) => { tokio::time::timeout(Duration::from_secs(5), bridge.write_all(&chunk)).await.map_err(|_| anyhow::anyhow!("WebSocket backpressure timeout"))??; }
            Ok(None) => break,
            Err(error) => return Err(anyhow::anyhow!(error)),
          },
          read = bridge.read(&mut bytes) => {
            let n = read?;
            if n == 0 { break; }
            session.write_response_body(Some(Bytes::copy_from_slice(&bytes[..n])), false).await.map_err(|error| anyhow::anyhow!(error))?;
          }
        }
      }
      Ok::<_, anyhow::Error>(())
    };
    tokio::select! {
      _ = self.state.shutdown.cancelled() => {},
      result = async { tokio::join!(handle, relay).1 } => { if let Err(error) = result { tracing::debug!(%error, "Proof WebSocket transport closed"); } },
    }
    Ok(())
  }
}

#[async_trait]
impl ProxyHttp for Handler {
  type CTX = RequestContext;
  fn new_ctx(&self) -> RequestContext {
    RequestContext {
      id: http::HeaderValue::from_static(""),
      started: Instant::now(),
      uploaded: 0,
      connect_attempts: 0,
      addresses: 0,
      upstream: None,
      failure: None,
      permit: None,
    }
  }
  async fn request_filter(
    &self,
    session: &mut Session,
    ctx: &mut RequestContext,
  ) -> pingora::Result<bool> {
    ctx.id = session
      .req_header()
      .headers
      .get("x-request-id")
      .filter(|value| value.to_str().is_ok())
      .cloned()
      .unwrap_or_else(new_request_id);
    session
      .req_header_mut()
      .insert_header("x-request-id", ctx.id.clone())?;
    match self.state.requests.clone().try_acquire_owned() {
      Ok(permit) => ctx.permit = Some(permit),
      Err(_) => {
        self
          .state
          .metrics
          .request_rejections
          .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut out = response(
          StatusCode::SERVICE_UNAVAILABLE,
          "Gateway capacity exhausted",
        );
        out
          .headers_mut()
          .insert("retry-after", "1".parse().unwrap());
        out
          .headers_mut()
          .insert(CACHE_CONTROL, "no-store".parse().unwrap());
        send(session, out, ctx).await?;
        return Ok(true);
      }
    }
    session.set_read_timeout(Some(Duration::from_secs(30)));
    session.set_write_timeout(Some(Duration::from_secs(30)));
    if session
      .req_header()
      .headers
      .contains_key(http::header::TRANSFER_ENCODING)
    {
      // The parser reconciles framing fields, so ambiguity must be checked against the original headers.
      let raw = session.as_ref().to_h1_raw();
      let has_length = raw.split(|byte| *byte == b'\n').skip(1).any(|line| {
        line
          .split(|byte| *byte == b':')
          .next()
          .is_some_and(|name| name.eq_ignore_ascii_case(b"content-length"))
      });
      if has_length {
        session.set_keepalive(None);
        send(
          session,
          ApiError::InvalidRequest("Conflicting request framing".into()).into_response(),
          ctx,
        )
        .await?;
        return Ok(true);
      }
    }
    let prefix = &self.state.cfg.web.base_path;
    if let Some(path) = session
      .req_header()
      .uri
      .path()
      .strip_prefix(prefix)
      .filter(|suffix| suffix.is_empty() || suffix.starts_with('/'))
      .map(str::to_owned)
    {
      let out = if path == "/api/ws" {
        self.local(session, ctx, &path).await?
      } else {
        match tokio::time::timeout(Duration::from_secs(30), self.local(session, ctx, &path)).await {
          Ok(result) => result?,
          Err(_) => Some(response(StatusCode::REQUEST_TIMEOUT, "")),
        }
      };
      if let Some(out) = out {
        send(session, out, ctx).await?;
      }
      return Ok(true);
    }

    ctx.upstream = match self.state.proxy.request_target(&session.req_header().uri) {
      Ok(url) => Some(url),
      Err(error) => {
        send(session, error.into_response(), ctx).await?;
        return Ok(true);
      }
    };
    let header = session.req_header();
    let peer = session
      .client_addr()
      .and_then(|address| address.as_inet())
      .map(|address| address.ip());
    let client_ip =
      match crate::transport::client_ip::client_ip(peer, &header.headers, &self.state.cfg.server) {
        Ok(ip) => ip,
        Err(message) => {
          send(
            session,
            ApiError::InvalidRequest(message.into()).into_response(),
            ctx,
          )
          .await?;
          return Ok(true);
        }
      };
    match gate_with_client(
      &self.state,
      &header.headers,
      &header.uri,
      &header.method,
      client_ip,
    ) {
      Ok(Some(out)) => {
        send(session, out, ctx).await?;
        Ok(true)
      }
      Err(error) => {
        send(session, error.into_response(), ctx).await?;
        Ok(true)
      }
      Ok(None) => {
        if session
          .req_header()
          .headers
          .get(CONTENT_LENGTH)
          .and_then(|v| v.to_str().ok())
          .and_then(|v| v.parse::<usize>().ok())
          .is_some_and(|n| n > self.state.cfg.upstream.max_body_bytes)
        {
          send(session, ApiError::PayloadTooLarge.into_response(), ctx).await?;
          return Ok(true);
        }
        Ok(false)
      }
    }
  }
  async fn upstream_peer(
    &self,
    _: &mut Session,
    ctx: &mut RequestContext,
  ) -> pingora::Result<Box<HttpPeer>> {
    let remaining = Duration::from_secs(30)
      .checked_sub(ctx.started.elapsed())
      .ok_or_else(|| failure(StatusCode::REQUEST_TIMEOUT, "Request timed out"))?;
    let (peer, addresses) = self
      .state
      .proxy
      .peer(ctx.connect_attempts, remaining)
      .await?;
    ctx.connect_attempts += 1;
    ctx.addresses = addresses;
    Ok(peer)
  }
  fn fail_to_connect(
    &self,
    _: &mut Session,
    _: &HttpPeer,
    ctx: &mut RequestContext,
    mut error: Box<Error>,
  ) -> Box<Error> {
    // An alternate address is safe only before any HTTP request has been sent.
    error.set_retry(
      ctx.connect_attempts < ctx.addresses && ctx.started.elapsed() < Duration::from_secs(30),
    );
    error
  }
  fn error_while_proxy(
    &self,
    _: &HttpPeer,
    _: &mut Session,
    mut error: Box<Error>,
    _: &mut RequestContext,
    _: bool,
  ) -> Box<Error> {
    error.set_retry(false);
    error
  }
  async fn upstream_request_filter(
    &self,
    _: &mut Session,
    request: &mut pingora::http::RequestHeader,
    ctx: &mut RequestContext,
  ) -> pingora::Result<()> {
    request.set_uri(
      ctx
        .upstream
        .as_ref()
        .expect("validated upstream URI")
        .clone(),
    );
    if !self.state.cfg.upstream.preserve_host || !request.headers.contains_key(HOST) {
      request.insert_header(HOST, self.state.proxy.host())?;
    }
    Ok(())
  }
  async fn request_body_filter(
    &self,
    session: &mut Session,
    body: &mut Option<Bytes>,
    _: bool,
    ctx: &mut RequestContext,
  ) -> pingora::Result<()> {
    if session.as_ref().was_upgraded() {
      return Ok(());
    }
    if ctx.started.elapsed() >= Duration::from_secs(30) {
      ctx.failure = Some(StatusCode::REQUEST_TIMEOUT);
      *body = None;
      return Err(failure(StatusCode::REQUEST_TIMEOUT, "Request timed out"));
    }
    ctx.uploaded = ctx
      .uploaded
      .saturating_add(body.as_ref().map_or(0, Bytes::len));
    if ctx.uploaded > self.state.cfg.upstream.max_body_bytes {
      ctx.failure = Some(StatusCode::PAYLOAD_TOO_LARGE);
      *body = None;
      return Err(failure(
        StatusCode::PAYLOAD_TOO_LARGE,
        "Request body is too large",
      ));
    }
    Ok(())
  }
  async fn response_filter(
    &self,
    _: &mut Session,
    header: &mut pingora::http::ResponseHeader,
    ctx: &mut RequestContext,
  ) -> pingora::Result<()> {
    let upgraded = header.status == StatusCode::SWITCHING_PROTOCOLS;
    let nominated: Vec<http::HeaderName> = header
      .headers
      .get_all(http::header::CONNECTION)
      .iter()
      .filter_map(|value| value.to_str().ok())
      .flat_map(|value| value.split(','))
      .filter_map(|name| name.trim().parse().ok())
      .collect();
    for name in nominated {
      if !(upgraded && name == http::header::UPGRADE) {
        header.remove_header(&name);
      }
    }
    for name in [
      "connection",
      "keep-alive",
      "proxy-connection",
      "proxy-authenticate",
      "proxy-authorization",
      "te",
      "trailer",
      "upgrade",
    ] {
      if !(upgraded && matches!(name, "connection" | "upgrade")) {
        header.remove_header(name);
      }
    }
    if upgraded {
      header.insert_header(http::header::CONNECTION, "Upgrade")?;
    }
    header.insert_header("x-request-id", ctx.id.clone())?;
    Ok(())
  }
  async fn fail_to_proxy(
    &self,
    session: &mut Session,
    error: &Error,
    ctx: &mut RequestContext,
  ) -> FailToProxy {
    let status = ctx.failure.unwrap_or_else(|| match error.etype() {
      ErrorType::HTTPStatus(code) => StatusCode::from_u16(*code).unwrap_or(StatusCode::BAD_GATEWAY),
      ErrorType::ReadTimedout | ErrorType::WriteTimedout
        if error.esource() == &pingora::ErrorSource::Downstream =>
      {
        StatusCode::REQUEST_TIMEOUT
      }
      ErrorType::ConnectTimedout | ErrorType::ReadTimedout | ErrorType::WriteTimedout => {
        StatusCode::GATEWAY_TIMEOUT
      }
      _ if error.esource() == &pingora::ErrorSource::Downstream => StatusCode::BAD_REQUEST,
      _ => StatusCode::BAD_GATEWAY,
    });
    if session.response_written().is_none() {
      let out = if matches!(session.req_header().method, Method::GET | Method::HEAD)
        && matches!(
          status,
          StatusCode::BAD_GATEWAY | StatusCode::GATEWAY_TIMEOUT
        ) {
        let (html, locale) = render_page(
          &self.state,
          "pages/error.html",
          "",
          "fast",
          &session.req_header().headers,
          &session.req_header().uri,
        );
        page_response(
          status,
          html,
          locale,
          session.req_header().method == Method::HEAD,
        )
      } else if status == StatusCode::REQUEST_TIMEOUT {
        response(status, "")
      } else {
        match status {
          StatusCode::PAYLOAD_TOO_LARGE => ApiError::PayloadTooLarge,
          StatusCode::GATEWAY_TIMEOUT => ApiError::GatewayTimeout,
          StatusCode::BAD_REQUEST => ApiError::InvalidRequest("Unable to read request body".into()),
          _ => ApiError::BadGateway,
        }
        .into_response()
      };
      session.set_keepalive(None);
      let _ = send(session, out, ctx).await;
    }
    FailToProxy {
      error_code: status.as_u16(),
      can_reuse_downstream: false,
    }
  }
  fn suppress_error_log(&self, _: &Session, _: &RequestContext, _: &Error) -> bool {
    true
  }
  async fn logging(&self, session: &mut Session, error: Option<&Error>, ctx: &mut RequestContext) {
    let status = session
      .response_written()
      .map_or(0, |header| header.status.as_u16());
    self.state.metrics.observe(
      status,
      ctx.started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
    );
    ctx.permit.take();
    tracing::debug!(request_id = %ctx.id.to_str().unwrap_or_default(), method = %session.req_header().method, path = %session.req_header().uri.path(), status = session.response_written().map(|header| header.status.as_u16()), elapsed_us = ctx.started.elapsed().as_micros(), error = ?error, "HTTP request completed");
  }
}

#[cfg(test)]
mod tests {
  #[test]
  fn generated_request_ids_are_uuid_v7_and_unique_across_workers() {
    let workers: Vec<_> = (0..8)
      .map(|_| {
        std::thread::spawn(|| {
          (0..1024)
            .map(|_| {
              let value = super::new_request_id();
              let id = uuid::Uuid::parse_str(value.to_str().unwrap()).unwrap();
              assert_eq!(id.get_version(), Some(uuid::Version::SortRand));
              id
            })
            .collect::<Vec<_>>()
        })
      })
      .collect();
    let mut unique = std::collections::HashSet::new();
    for worker in workers {
      let ids = worker.join().unwrap();
      assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
      for id in ids {
        assert!(unique.insert(id));
      }
    }
  }
}
