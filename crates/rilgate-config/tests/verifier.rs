use rilgate_config::loader::parse;

#[test]
fn verifier_resolves_without_gateway_dependencies() {
  let document = parse("environment: development\npow:\n  algorithms:\n    randomx:\n      serve:\n        listen: 127.0.0.1:9443\n        tls: {cert: server.pem, key: server.key}\n").unwrap();
  let config = document.resolve_verifier().unwrap();
  assert_eq!(config.serve.queue, 64);
  assert_eq!(config.serve.workers, 1);
  assert!(config.warnings().is_empty());
  assert!(document.resolve().is_err());
}

#[test]
fn verifier_checks_authentication_workers_and_listener_overlap() {
  let yaml = "pow:\n  algorithms:\n    randomx:\n      serve:\n        listen: 8.8.8.8:9443\n        tls: {cert: server.pem, key: server.key}\n";
  let document = parse(yaml).unwrap();
  assert!(
    document
      .resolve_verifier()
      .unwrap_err()
      .to_string()
      .contains("verifier-serve-unauthenticated")
  );
  let mut document = parse(&format!("environment: development\n{yaml}")).unwrap();
  document
    .pow
    .algorithms
    .randomx
    .serve
    .as_mut()
    .unwrap()
    .workers = 2;
  assert!(
    document
      .resolve_verifier()
      .unwrap_err()
      .to_string()
      .contains("must be 1")
  );
  document
    .pow
    .algorithms
    .randomx
    .serve
    .as_mut()
    .unwrap()
    .workers = 1;
  document.server.operations.listen.0 = vec!["8.8.8.8:9443".into()];
  assert!(document.resolve_verifier().is_err());
  document.server.operations.listen.0.clear();
  document.environment = rilgate_config::Environment::Production;
  document
    .pow
    .algorithms
    .randomx
    .serve
    .as_mut()
    .unwrap()
    .tls
    .client_ca = Some("ca.pem".into());
  let config = document.resolve_verifier().unwrap();
  assert!(
    config
      .warnings()
      .iter()
      .any(|f| f.id == "verifier-serve-public")
  );
}

#[test]
fn verifier_display_contains_only_its_settings() {
  let document = parse("environment: development\npow:\n  algorithms:\n    randomx:\n      serve:\n        listen: 127.0.0.1:9443\n        tls: {cert: server.pem, key: server.key}\n").unwrap();
  for defaults in [true, false] {
    let text = document.display_verifier(defaults).unwrap();
    assert!(!text.contains("upstream"));
    assert!(!text.contains("security"));
    assert!(!text.contains("verify:"));
    assert!(!text.contains("stratum_pool"));
    assert!(!text.contains("site:"));
    parse(&text).unwrap().resolve_verifier().unwrap();
  }
}
