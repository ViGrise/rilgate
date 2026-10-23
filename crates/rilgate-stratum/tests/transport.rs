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
use rilgate_stratum::{StratumConfig, config::TlsConfig, transport::connect_transport};
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
    host: "127.0.0.1".into(),
    port,
    user: Secret("x+{difficulty}".into()),
    password: Secret("x".into()),
    agent: "test".into(),
    connect_timeout: Duration::from_secs(2),
    submit_timeout: Duration::from_secs(2),
    tls: TlsConfig::default(),
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
  let mut cfg = config(port);
  cfg.tls = TlsConfig {
    enabled: true,
    server_name: Some("pool.test".into()),
    ca: Some(ca_path),
  };
  let mut stream = connect_transport(&cfg).await.unwrap();
  let mut data = [0; 8];
  stream.read_exact(&mut data).await.unwrap();
  assert_eq!(&data, b"verified");
  cfg.tls.server_name = Some("wrong.test".into());
  assert!(connect_transport(&cfg).await.is_err());
  cfg.tls.server_name = Some("pool.test".into());
  cfg.tls.ca = None;
  assert!(connect_transport(&cfg).await.is_err());
  task.await.unwrap();
}
