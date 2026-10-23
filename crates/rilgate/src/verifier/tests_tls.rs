use super::*;
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

#[tokio::test]
async fn mtls_requires_a_trusted_client_certificate() {
  let directory = tempfile::tempdir().unwrap();
  let (ca, key) = certificate("ca", 1, None, false);
  let (server, server_key) = certificate("verifier.test", 2, Some((&ca, &key)), false);
  let (client, client_key) = certificate("gateway.test", 3, Some((&ca, &key)), true);
  let (foreign_ca, foreign_key) = certificate("foreign", 4, None, false);
  let (foreign, foreign_client_key) = certificate(
    "foreign gateway",
    5,
    Some((&foreign_ca, &foreign_key)),
    true,
  );
  for (name, bytes) in [
    ("ca", ca.to_pem().unwrap()),
    ("cert", server.to_pem().unwrap()),
    ("key", server_key.private_key_to_pem_pkcs8().unwrap()),
  ] {
    std::fs::write(directory.path().join(name), bytes).unwrap();
  }
  let mut document = rilgate_config::schema::Document::default();
  document.pow.algorithms.randomx.serve = Some(rilgate_config::schema::pow::Serve {
    listen: "127.0.0.1:9443".into(),
    tls: rilgate_config::schema::pow::ServeTls {
      cert: Some(directory.path().join("cert")),
      key: Some(directory.path().join("key")),
      client_ca: Some(directory.path().join("ca")),
    },
    ..Default::default()
  });
  let config = document.resolve_verifier().unwrap();
  let acceptor = tls(&config).unwrap();
  for identity in [
    Some((&client, &client_key)),
    Some((&foreign, &foreign_client_key)),
    None,
  ] {
    let authenticated =
      identity.is_some_and(|(cert, _)| cert.to_der().unwrap() == client.to_der().unwrap());
    let mut roots = rustls::RootCertStore::empty();
    roots
      .add(CertificateDer::from(ca.to_der().unwrap()))
      .unwrap();
    let builder = rustls::ClientConfig::builder_with_provider(Arc::new(
      rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots);
    let client_config = match identity {
      Some((cert, key)) => builder
        .with_client_auth_cert(
          vec![CertificateDer::from(cert.to_der().unwrap())],
          PrivateKeyDer::from_pem_slice(&key.private_key_to_pem_pkcs8().unwrap()).unwrap(),
        )
        .unwrap(),
      None => builder.with_no_client_auth(),
    };
    let connector = tokio_rustls::TlsConnector::from(Arc::new(client_config));
    let (client_io, server_io) = tokio::io::duplex(16384);
    let acceptor = acceptor.clone();
    let state = State::new(Arc::new(super::tests::Fake), 4, true);
    let server_task = tokio::spawn(async move {
      let Ok(stream) = acceptor.accept(server_io).await else {
        return false;
      };
      let service = service_fn(move |request: Request<Incoming>| {
        let state = state.clone();
        async { Ok::<_, Infallible>(state.handle(request, Some("client")).await) }
      });
      let _ = http1::Builder::new()
        .serve_connection(TokioIo::new(stream), service)
        .await;
      true
    });
    let outcome = async {
      let stream = connector
        .connect(
          rustls::pki_types::ServerName::try_from("verifier.test").unwrap(),
          client_io,
        )
        .await?;
      let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
      tokio::spawn(async move {
        let _ = connection.await;
      });
      let response = sender
        .send_request(
          Request::post("/v1/randomx/hash")
            .header("host", "verifier.test")
            .header("content-type", "application/json")
            .header("connection", "close")
            .body(Full::new(Bytes::from(serde_json::json!({"seed":"ab".repeat(32),"input":"01","deadline_unix_ms":unix_ms()+5000}).to_string())))
            .unwrap(),
        )
        .await?;
      let status = response.status();
      let body = response.into_body().collect().await?.to_bytes();
      Ok::<_, anyhow::Error>((status,body))
    }
    .await;
    let accepted = server_task.await.unwrap();
    assert_eq!(accepted, authenticated);
    if authenticated {
      let (status, body) = outcome.unwrap();
      assert_eq!(status, StatusCode::OK);
      assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["hash"],
        "07".repeat(32)
      );
    }
  }
}
