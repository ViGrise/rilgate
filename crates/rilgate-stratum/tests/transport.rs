use openssl::{
  asn1::Asn1Time,
  bn::BigNum,
  hash::MessageDigest,
  pkey::{PKey, Private},
  rsa::Rsa,
  x509::{
    X509, X509NameBuilder,
    extension::{BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAlternativeName},
  },
};
use rilgate_crypto::Secret;
use rilgate_stratum::{StratumConfig, config::TlsConfig, transport::connect_via};
use std::{sync::Arc, time::Duration};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::TcpListener,
};
fn certificate(
  name: &str,
  serial: u32,
  issuer: Option<(&X509, &PKey<Private>)>,
  client: bool,
) -> (X509, PKey<Private>) {
  let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
  let mut subject = X509NameBuilder::new().unwrap();
  subject.append_entry_by_text("CN", name).unwrap();
  let subject = subject.build();
  let mut cert = X509::builder().unwrap();
  cert.set_version(2).unwrap();
  cert
    .set_serial_number(&BigNum::from_u32(serial).unwrap().to_asn1_integer().unwrap())
    .unwrap();
  cert.set_subject_name(&subject).unwrap();
  cert
    .set_issuer_name(issuer.map_or(subject.as_ref(), |(ca, _)| ca.subject_name()))
    .unwrap();
  cert.set_pubkey(&key).unwrap();
  cert
    .set_not_before(&Asn1Time::days_from_now(0).unwrap())
    .unwrap();
  cert
    .set_not_after(&Asn1Time::days_from_now(1).unwrap())
    .unwrap();
  if issuer.is_none() {
    cert
      .append_extension(BasicConstraints::new().critical().ca().build().unwrap())
      .unwrap();
    cert
      .append_extension(KeyUsage::new().key_cert_sign().crl_sign().build().unwrap())
      .unwrap();
  } else {
    cert
      .append_extension(BasicConstraints::new().critical().build().unwrap())
      .unwrap();
    cert
      .append_extension(
        KeyUsage::new()
          .digital_signature()
          .key_encipherment()
          .build()
          .unwrap(),
      )
      .unwrap();
    let usage = if client {
      ExtendedKeyUsage::new().client_auth().build()
    } else {
      ExtendedKeyUsage::new().server_auth().build()
    };
    cert.append_extension(usage.unwrap()).unwrap();
    let san = SubjectAlternativeName::new()
      .dns(name)
      .build(&cert.x509v3_context(issuer.map(|(ca, _)| ca.as_ref()), None))
      .unwrap();
    cert.append_extension(san).unwrap();
  }
  cert
    .sign(issuer.map_or(&key, |(_, key)| key), MessageDigest::sha256())
    .unwrap();
  (cert.build(), key)
}
pub fn config(port: u16) -> StratumConfig {
  StratumConfig {
    name: "pool".into(),
    difficulty_mismatches: Default::default(),
    host: "127.0.0.1".into(),
    port,
    user: Secret("x+{difficulty}".into()),
    password: Secret("x".into()),
    agent: "test".into(),
    connect_timeout: Duration::from_secs(2),
    submit_timeout: Duration::from_secs(2),
    tls: TlsConfig::default(),
    proxies: None,
  }
}

#[tokio::test]
async fn tls_rejects_untrusted_certificates_and_wrong_names_before_sending_login() {
  let (ca, ca_key) = certificate("ca", 1, None, false);
  let (leaf, key) = certificate("pool.test", 2, Some((&ca, &ca_key)), false);
  let directory = tempfile::tempdir().unwrap();
  let ca_path = directory.path().join("ca.pem");
  std::fs::write(&ca_path, ca.to_pem().unwrap()).unwrap();
  let server = rustls::ServerConfig::builder()
    .with_no_client_auth()
    .with_single_cert(
      vec![rustls::pki_types::CertificateDer::from(
        leaf.to_der().unwrap(),
      )],
      rustls::pki_types::PrivateKeyDer::try_from(key.private_key_to_der().unwrap()).unwrap(),
    )
    .unwrap();
  let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let port = listener.local_addr().unwrap().port();
  let task = tokio::spawn(async move {
    for valid in [true, false, false] {
      let (stream, _) = listener.accept().await.unwrap();
      let result = acceptor.accept(stream).await;
      if valid {
        let mut stream = result.unwrap();
        stream.write_all(b"verified").await.unwrap();
      } else {
        assert!(result.is_err());
      }
    }
  });
  let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let proxy_config = proxy(proxy_listener.local_addr().unwrap().port(), "http");
  let tunnel = tokio::spawn(async move {
    for _ in 0..3 {
      let (mut client, _) = proxy_listener.accept().await.unwrap();
      headers(&mut client).await;
      let mut upstream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
      client.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await.unwrap();
      tokio::spawn(async move {
        let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
      });
    }
  });
  let mut cfg = config(port);
  cfg.tls = TlsConfig {
    enabled: true,
    server_name: Some("pool.test".into()),
    ca: Some(ca_path),
  };
  let transport = rilgate_stratum::transport::Transport::new(&cfg).unwrap();
  std::fs::remove_file(cfg.tls.ca.as_ref().unwrap()).unwrap();
  let mut stream = transport.connect(&cfg, Some(&proxy_config)).await.unwrap();
  std::fs::write(cfg.tls.ca.as_ref().unwrap(), ca.to_pem().unwrap()).unwrap();
  let mut data = [0; 8];
  stream.read_exact(&mut data).await.unwrap();
  assert_eq!(&data, b"verified");
  cfg.tls.server_name = Some("wrong.test".into());
  assert!(connect_via(&cfg, Some(&proxy_config)).await.is_err());
  cfg.tls.server_name = Some("pool.test".into());
  cfg.tls.ca = None;
  assert!(connect_via(&cfg, Some(&proxy_config)).await.is_err());
  task.await.unwrap();
  tunnel.await.unwrap();
}

fn proxy(port: u16, scheme: &str) -> rilgate_stratum::config::Proxy {
  rilgate_stratum::config::Proxy {
    name: "proxy".into(),
    url: Secret(format!("{scheme}://127.0.0.1:{port}")),
    username: Some(Secret("operator".into())),
    password: Some(Secret("password".into())),
  }
}

async fn headers(stream: &mut tokio::net::TcpStream) -> String {
  let mut bytes = Vec::new();
  while !bytes.ends_with(b"\r\n\r\n") {
    bytes.push(stream.read_u8().await.unwrap());
  }
  String::from_utf8(bytes).unwrap()
}

#[tokio::test]
async fn http_connect_bounds_headers_rejects_refusals_and_preserves_tunnel_bytes() {
  for response in [
    b"HTTP/1.1 200 OK\r\n\r\njob".to_vec(),
    b"HTTP/1.0 200 OK\r\n\r\njob".to_vec(),
    b"HTTP/1.1 200\r\n\r\njob".to_vec(),
    b"HTTP/1.0 200\r\n\r\njob".to_vec(),
    b"HTTP/1.1 403 Forbidden\r\n\r\n".to_vec(),
    vec![b'x'; 8193],
    b"HTTP/1.1 200".to_vec(),
  ] {
    let expected = response.ends_with(b"job");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy = proxy(listener.local_addr().unwrap().port(), "http");
    let server = tokio::spawn(async move {
      let (mut stream, _) = listener.accept().await.unwrap();
      let request = headers(&mut stream).await;
      assert!(request.starts_with("CONNECT 127.0.0.1:3333 HTTP/1.1\r\n"));
      assert!(request.contains("Proxy-Authorization: Basic b3BlcmF0b3I6cGFzc3dvcmQ="));
      stream.write_all(&response).await.unwrap();
    });
    let result = rilgate_stratum::transport::connect_via(&config(3333), Some(&proxy)).await;
    if expected {
      let mut stream = result.unwrap();
      let mut bytes = [0; 3];
      stream.read_exact(&mut bytes).await.unwrap();
      assert_eq!(&bytes, b"job");
    } else {
      assert!(result.is_err());
    }
    server.await.unwrap();
  }
}

#[tokio::test]
async fn socks_dns_modes_and_credentials_reach_the_proxy_with_the_correct_address_type() {
  for (scheme, address_type, host) in [("socks5", 1, "127.0.0.1"), ("socks5h", 3, "pool.invalid")] {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy = proxy(listener.local_addr().unwrap().port(), scheme);
    let task = tokio::spawn(async move {
      let (mut stream, _) = listener.accept().await.unwrap();
      assert_eq!(stream.read_u8().await.unwrap(), 5);
      let count = stream.read_u8().await.unwrap();
      let mut methods = vec![0; count as usize];
      stream.read_exact(&mut methods).await.unwrap();
      assert!(methods.contains(&2));
      stream.write_all(&[5, 2]).await.unwrap();
      assert_eq!(stream.read_u8().await.unwrap(), 1);
      let len = stream.read_u8().await.unwrap();
      let mut user = vec![0; len as usize];
      stream.read_exact(&mut user).await.unwrap();
      assert_eq!(user, b"operator");
      let len = stream.read_u8().await.unwrap();
      let mut pass = vec![0; len as usize];
      stream.read_exact(&mut pass).await.unwrap();
      assert_eq!(pass, b"password");
      stream.write_all(&[1, 0]).await.unwrap();
      let mut request = [0; 4];
      stream.read_exact(&mut request).await.unwrap();
      assert_eq!(request, [5, 1, 0, address_type]);
      let len = if address_type == 3 {
        stream.read_u8().await.unwrap() as usize
      } else {
        4
      };
      let mut address = vec![0; len];
      stream.read_exact(&mut address).await.unwrap();
      if address_type == 3 {
        assert_eq!(address, b"pool.invalid");
      }
      assert_eq!(stream.read_u16().await.unwrap(), 3333);
      stream
        .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 1])
        .await
        .unwrap();
      stream.write_all(b"job").await.unwrap();
    });
    let mut cfg = config(3333);
    cfg.host = host.into();
    let mut stream = rilgate_stratum::transport::connect_via(&cfg, Some(&proxy))
      .await
      .unwrap();
    let mut bytes = [0; 3];
    stream.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"job");
    task.await.unwrap();
  }
}

#[tokio::test]
async fn shares_for_recent_jobs_reach_the_pool_and_older_jobs_are_rejected() {
  use rilgate_stratum::{StratumEvent, StratumSession};
  use serde_json::json;
  use tokio::io::{AsyncBufReadExt, BufReader};
  async fn write_json(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    value: &serde_json::Value,
  ) -> std::io::Result<()> {
    writer.write_all(format!("{value}\n").as_bytes()).await
  }
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let cfg = config(listener.local_addr().unwrap().port());
  let job = |id| {
    json!({
      "blob": "00".repeat(64), "job_id": id, "seed_hash": "ab".repeat(32),
      "target": "ffffffff", "algo": "rx/0"
    })
  };
  let server = tokio::spawn(async move {
    let (socket, _) = listener.accept().await.unwrap();
    let (read, mut write) = socket.into_split();
    let mut reader = BufReader::new(read).lines();
    reader.next_line().await.unwrap().unwrap();
    write_json(
      &mut write,
      &json!({
        "id": 1, "result": {"id": "miner", "job": job("first")}
      }),
    )
    .await
    .unwrap();
    write_json(
      &mut write,
      &json!({
        "method": "job", "params": job("second")
      }),
    )
    .await
    .unwrap();
    let submit: serde_json::Value =
      serde_json::from_str(&reader.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(submit["params"]["job_id"], "first");
    for id in ["third", "fourth"] {
      write_json(
        &mut write,
        &json!({
          "method": "job", "params": job(id)
        }),
      )
      .await
      .unwrap();
    }
    let submit: serde_json::Value =
      serde_json::from_str(&reader.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(submit["params"]["job_id"], "second");
  });
  let mut session = StratumSession::connect(&cfg, 1000).await.unwrap();
  assert!(matches!(
    session.read_event().await.unwrap(),
    StratumEvent::Job(_)
  ));
  session
    .submit_share("first", "01020304", &"00".repeat(32))
    .await
    .unwrap();
  for _ in 0..2 {
    assert!(matches!(
      session.read_event().await.unwrap(),
      StratumEvent::Job(_)
    ));
  }
  let error = session
    .submit_share("first", "01020304", &"00".repeat(32))
    .await
    .unwrap_err();
  assert_eq!(
    error.to_string(),
    "Share refers to a stale Stratum pool job"
  );
  session
    .submit_share("second", "01020304", &"00".repeat(32))
    .await
    .unwrap();
  server.await.unwrap();
}
