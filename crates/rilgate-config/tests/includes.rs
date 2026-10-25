use rilgate_config::{
  Overrides, Setting, SettingValue,
  loader::{load, parse},
};
use std::{fs, path::Path};

fn write(directory: &Path, name: &str, text: &str) {
  let path = directory.join(name);
  fs::create_dir_all(path.parent().unwrap()).unwrap();
  fs::write(path, text).unwrap();
}

fn read(
  directory: &Path,
  settings: Vec<Setting>,
) -> anyhow::Result<rilgate_config::schema::Document> {
  Ok(
    load(&Overrides {
      config: Some(directory.join("rilgate.yml")),
      settings,
      ..Default::default()
    })?
    .document,
  )
}

const BASE: &str = "server:\n  upstream:\n    url: http://127.0.0.1:3000\nsecurity:\n  secret: 0123456789abcdef0123456789abcdef\n";

#[test]
fn nested_includes_mix_rules_and_cidrs_and_preserve_policy_order() {
  let directory = tempfile::tempdir().unwrap();
  let directory = directory.path();
  write(
    directory,
    "rilgate.yml",
    &format!("{BASE}policy: !include policy/main.yml\n"),
  );
  write(
    directory,
    "policy/main.yml",
    "mode: custom\ndefault_action: deny\nip_sets:\n  bots:\n    cidrs: !include cidrs.yml\nrules:\n  - name: health\n    when: {path: {exact: /health}}\n    action: allow\n  - !include rules/bots.yml\n  - !include rules/catchall.yml\n",
  );
  write(
    directory,
    "policy/cidrs.yml",
    "- 192.0.2.10/32\n- !include more-cidrs.yml\n",
  );
  write(
    directory,
    "policy/more-cidrs.yml",
    "- 198.51.100.0/24\n- 2001:db8::1/128\n",
  );
  write(
    directory,
    "policy/rules/bots.yml",
    "name: bots\nwhen: {client_ip: {in_sets: [bots]}}\naction: allow\n",
  );
  write(
    directory,
    "policy/rules/catchall.yml",
    "- name: catchall\n  when: {path: {prefix: /}}\n  action: deny\n- name: unreachable\n  when: {path: {exact: /health}}\n  action: deny\n",
  );
  let document = read(directory, vec![]).unwrap();
  document.resolve().unwrap();
  let engine = rilgate_policy::PolicyEngine::from_definition(&document.policy).unwrap();
  let headers = http::HeaderMap::new();
  assert_eq!(engine.evaluate(&headers, "/health", 0).rule, "health");
  let decision = engine.evaluate_context(
    &rilgate_policy::conditions::Request {
      host: "example.test",
      headers: &headers,
      path: "/protected",
      method: &http::Method::GET,
      client_ip: Some("198.51.100.10".parse().unwrap()),
    },
    0,
  );
  assert_eq!(decision.rule, "bots");
  assert_eq!(engine.evaluate(&headers, "/protected", 0).rule, "catchall");
  let output = document.display(true).unwrap();
  assert!(!output.contains("!include"));
  assert!(!output.contains("0123456789abcdef"));
  parse(&output).unwrap();
}

#[test]
fn root_scalar_and_repeated_includes_keep_overrides_and_environment_interpolation() {
  let directory = tempfile::tempdir().unwrap();
  let directory = directory.path();
  write(directory, "rilgate.yml", "!include config/main.yml\n");
  write(
    directory,
    "config/main.yml",
    &BASE
      .replace("url: http://127.0.0.1:3000", "url: !include upstream.yml")
      .replace(
        "server:\n",
        "server:\n  listen: !include listen.yml\n  operations:\n    listen: !include listen.yml\n",
      ),
  );
  write(
    directory,
    "config/upstream.yml",
    "'http://127.0.0.1:3000/${RILGATE_INCLUDE_TEST_VALUE}'\n",
  );
  write(directory, "config/listen.yml", "['127.0.0.1:9090']\n");
  let error = read(directory, vec![]).unwrap_err();
  assert!(format!("{error:#}").contains("RILGATE_INCLUDE_TEST_VALUE"));
  write(directory, "config/upstream.yml", "http://127.0.0.1:3000\n");
  let document = read(
    directory,
    vec![Setting {
      path: "server.listen".into(),
      source: "--server-listen".into(),
      value: SettingValue::List(vec!["127.0.0.1:9091".into()]),
    }],
  )
  .unwrap();
  let config = document.resolve().unwrap();
  assert_eq!(config.listen_addresses[0].port(), 9091);
  assert_eq!(config.operations_listen_addresses[0].port(), 9090);
}

#[test]
fn schema_and_semantic_errors_identify_spliced_rule_sources() {
  let directory = tempfile::tempdir().unwrap();
  let directory = directory.path();
  write(
    directory,
    "rilgate.yml",
    &format!("{BASE}policy: !include policy.yml\n"),
  );
  write(
    directory,
    "policy.yml",
    "mode: custom\nrules:\n  - name: first\n    when: {path: {exact: /health}}\n    action: allow\n  - !include rules.yml\n",
  );
  write(
    directory,
    "rules.yml",
    "- name: good\n  when: {path: {exact: /ok}}\n  action: allow\n- name: invalid\n  when: {path: {prefix: /}}\n  action: invalid-action\n",
  );
  let error = format!("{:#}", read(directory, vec![]).unwrap_err());
  assert!(error.contains("rules.yml at [1].action"), "{error}");
  assert!(error.contains("policy.rules[2].action"), "{error}");
  write(
    directory,
    "rules.yml",
    "- name: invalid\n  when: {client_ip: {in_sets: [missing]}}\n  action: allow\n",
  );
  let error = format!(
    "{:#}",
    read(directory, vec![]).unwrap().resolve().unwrap_err()
  );
  assert!(
    error.contains("rules.yml at [0].when.client_ip.in_sets"),
    "{error}"
  );
  assert!(error.contains("policy.rules[1]"), "{error}");
}

#[test]
fn verifier_includes_share_validation_sources_and_filtered_output() {
  let directory = tempfile::tempdir().unwrap();
  let directory = directory.path();
  write(
    directory,
    "rilgate.yml",
    "environment: development\npow:\n  algorithms:\n    randomx:\n      serve: !include verifier.yml\n",
  );
  write(
    directory,
    "verifier.yml",
    "listen: 127.0.0.1:9443\nworkers: 2\ntls: {cert: server.pem, key: server.key}\n",
  );
  let error = format!(
    "{:#}",
    read(directory, vec![])
      .unwrap()
      .resolve_verifier()
      .unwrap_err()
  );
  assert!(error.contains("verifier.yml at workers"), "{error}");
  write(
    directory,
    "verifier.yml",
    "listen: 127.0.0.1:9443\ntls: {cert: server.pem, key: server.key}\n",
  );
  let document = read(directory, vec![]).unwrap();
  document.resolve_verifier().unwrap();
  let output = document.display_verifier(true).unwrap();
  assert!(!output.contains("!include"));
  assert!(!output.contains("security:"));
  parse(&output).unwrap().resolve_verifier().unwrap();
}

#[test]
fn invalid_fragments_fail_with_include_context() {
  let directory = tempfile::tempdir().unwrap();
  let directory = directory.path();
  for (fragment, expected) in [
    ("", "must contain a YAML document"),
    ("# comment only\n", "must contain a YAML document"),
    ("\u{feff} # comment only\n", "must contain a YAML document"),
    ("--- # empty\n...\n", "must contain a YAML document"),
    ("listen: [", "configuration YAML"),
    ("listen: 1\n---\nlisten: 2\n", "exactly one YAML document"),
    ("!include rilgate.yml", "include cycle"),
    ("!include missing.yml", "file does not exist"),
    ("!include [one, two]", "literal path string"),
    (
      "!include '${RILGATE_INCLUDE_PATH}'",
      "do not support environment",
    ),
    ("!unknown file.yml", "unsupported"),
  ] {
    write(directory, "rilgate.yml", "server: !include fragment.yml\n");
    write(directory, "fragment.yml", fragment);
    let error = format!("{:#}", read(directory, vec![]).unwrap_err());
    assert!(error.contains(expected), "expected {expected}: {error}");
    assert!(error.contains("fragment.yml"), "{error}");
    assert!(error.contains("rilgate.yml"), "{error}");
  }
  assert!(
    parse("server: !include server.yml")
      .unwrap_err()
      .to_string()
      .contains("file context")
  );
}

#[test]
fn empty_main_files_still_accept_settings() {
  let directory = tempfile::tempdir().unwrap();
  write(directory.path(), "rilgate.yml", "  \n");
  let document = read(
    directory.path(),
    vec![Setting {
      path: "server.upstream.url".into(),
      source: "--server-upstream-url".into(),
      value: SettingValue::Scalar("http://127.0.0.1:3000".into()),
    }],
  )
  .unwrap();
  assert_eq!(document.server.upstream.url, "http://127.0.0.1:3000");
}

#[cfg(unix)]
#[test]
fn include_cycles_through_symlinks_are_rejected() {
  let directory = tempfile::tempdir().unwrap();
  write(directory.path(), "rilgate.yml", "!include alias.yml\n");
  std::os::unix::fs::symlink("rilgate.yml", directory.path().join("alias.yml")).unwrap();
  let error = format!("{:#}", read(directory.path(), vec![]).unwrap_err());
  assert!(error.contains("include cycle"), "{error}");
}

#[test]
fn include_budgets_bound_reads_depth_bytes_and_expanded_nodes() {
  let directory = tempfile::tempdir().unwrap();
  let directory = directory.path();
  write(directory, "rilgate.yml", "server: !include large.yml\n");
  write(
    directory,
    "large.yml",
    &format!("#{}", "x".repeat(1_048_576 - 16)),
  );
  let error = format!("{:#}", read(directory, vec![]).unwrap_err());
  assert!(error.contains("1 MiB across all sources"), "{error}");

  write(directory, "rilgate.yml", "!include chain-0.yml\n");
  for index in 0..20 {
    write(
      directory,
      &format!("chain-{index}.yml"),
      &format!("!include chain-{}.yml\n", index + 1),
    );
  }
  let error = format!("{:#}", read(directory, vec![]).unwrap_err());
  assert!(error.contains("include depth"), "{error}");

  write(
    directory,
    "rilgate.yml",
    &format!(
      "server:\n  listen:\n{}",
      "    - !include address.yml\n".repeat(256)
    ),
  );
  write(directory, "address.yml", "127.0.0.1:8080\n");
  let error = format!("{:#}", read(directory, vec![]).unwrap_err());
  assert!(error.contains("source reads"), "{error}");

  write(directory, "rilgate.yml", "server: !include nodes.yml\n");
  write(
    directory,
    "nodes.yml",
    &format!("[{}null]", "null,".repeat(65_536)),
  );
  let error = format!("{:#}", read(directory, vec![]).unwrap_err());
  assert!(error.contains("expanded configuration exceeds"), "{error}");
}

#[test]
fn ordinary_nested_lists_are_rejected_by_the_schema_and_null_fragments_remain_values() {
  let directory = tempfile::tempdir().unwrap();
  let directory = directory.path();
  write(
    directory,
    "rilgate.yml",
    "server:\n  listen: !include addresses.yml\n",
  );
  write(directory, "addresses.yml", "- [127.0.0.1:8080]\n");
  let error = format!("{:#}", read(directory, vec![]).unwrap_err());
  assert!(error.contains("addresses.yml"), "{error}");
  assert!(error.contains("server.listen"), "{error}");
  write(
    directory,
    "rilgate.yml",
    "pow:\n  stratum_pool: !include null.yml\n",
  );
  write(directory, "null.yml", "null\n");
  assert!(read(directory, vec![]).unwrap().pow.stratum_pool.is_none());
}

#[test]
fn override_errors_keep_the_override_source_without_file_attribution() {
  let directory = tempfile::tempdir().unwrap();
  write(directory.path(), "rilgate.yml", BASE);
  let error = format!(
    "{:#}",
    read(
      directory.path(),
      vec![Setting {
        path: "server.listen".into(),
        source: "--server-listen".into(),
        value: SettingValue::List(vec!["invalid-address".into()]),
      }]
    )
    .unwrap()
    .resolve()
    .unwrap_err()
  );
  assert!(error.contains("--server-listen"), "{error}");
  assert!(!error.contains("rilgate.yml"), "{error}");
}

#[test]
fn empty_sequence_includes_retain_their_sources_after_nested_insertion() {
  let directory = tempfile::tempdir().unwrap();
  let directory = directory.path();
  write(directory, "empty.yml", "[]\n");
  write(directory, "group.yml", "- !include empty.yml\n");
  for fragment in ["empty.yml", "group.yml"] {
    write(
      directory,
      "rilgate.yml",
      &BASE.replace(
        "server:\n",
        &format!("server:\n  listen: [!include {fragment}]\n"),
      ),
    );
    let error = format!(
      "{:#}",
      read(directory, vec![]).unwrap().resolve().unwrap_err()
    );
    assert!(error.contains("server.listen"), "{error}");
    assert!(error.contains("empty.yml"), "{error}");
    assert!(error.contains(fragment), "{error}");
    assert!(error.contains("include chain:"), "{error}");

    write(
      directory,
      "rilgate.yml",
      &format!("server:\n  listen: [!include {fragment}, {{invalid: address}}]\n"),
    );
    let error = format!("{:#}", read(directory, vec![]).unwrap_err());
    assert!(
      error.contains(&format!(
        "{} at server.listen",
        directory.join("rilgate.yml").display()
      )),
      "{error}"
    );
    assert!(error.contains("empty.yml"), "{error}");
    assert!(error.contains("include chain:"), "{error}");
  }
}

#[test]
fn cross_field_conflicts_report_both_file_and_override_sources() {
  let directory = tempfile::tempdir().unwrap();
  let directory = directory.path();
  let challenge_key = "include-test-challenge-signing-key-with-32-bytes";
  let auth_key = "include-test-auth-signing-key-with-32-bytes";
  let secret = "include-test-master-secret-with-at-least-32-bytes";
  write(
    directory,
    "rilgate.yml",
    "server:\n  upstream: {url: http://127.0.0.1:3000}\nsecurity: !include security.yml\n",
  );
  write(
    directory,
    "security.yml",
    &format!("keys:\n  challenge: {challenge_key}\n  auth: {auth_key}\n"),
  );
  let error = format!(
    "{:#}",
    read(
      directory,
      vec![Setting {
        path: "security.secret".into(),
        source: "RILGATE_SECURITY_SECRET".into(),
        value: SettingValue::Scalar(secret.into()),
      }]
    )
    .unwrap()
    .resolve()
    .unwrap_err()
  );
  assert!(
    error.contains("secret and keys are mutually exclusive"),
    "{error}"
  );
  assert!(error.contains("RILGATE_SECURITY_SECRET"), "{error}");
  assert!(error.contains("security.yml"), "{error}");
  assert!(error.contains("include chain:"), "{error}");
  for value in [challenge_key, auth_key, secret] {
    assert!(!error.contains(value), "{error}");
  }
}

#[test]
fn merge_bases_apply_in_order_before_inline_keys_and_replace_lists() {
  let directory = tempfile::tempdir().unwrap();
  let directory = directory.path();
  write(
    directory,
    "rilgate.yml",
    "server:\n  max_requests: 30\n  <<: [!include bases.yml, {max_connections: 40}]\n  listen: [127.0.0.1:9090]\n",
  );
  write(
    directory,
    "bases.yml",
    "- <<: !include first.yml\n- max_requests: 20\n  max_connections: 20\n  trusted_proxies: [198.51.100.0/24]\n  upstream: {preserve_host: false}\n",
  );
  write(
    directory,
    "first.yml",
    "max_requests: 10\nmax_connections: 10\nlisten: [127.0.0.1:8080, 127.0.0.1:8081]\ntrusted_proxies: [192.0.2.0/24, 203.0.113.0/24]\nupstream: {url: 'http://127.0.0.1:3000'}\n",
  );
  let document = read(directory, vec![]).unwrap();
  assert_eq!(document.server.max_requests, 30);
  assert_eq!(document.server.max_connections, 40);
  assert_eq!(document.server.listen.0, ["127.0.0.1:9090"]);
  assert_eq!(document.server.trusted_proxies, ["198.51.100.0/24"]);
  assert_eq!(document.server.upstream.url, "http://127.0.0.1:3000");
  assert!(!document.server.upstream.preserve_host);
  let output = document.display(true).unwrap();
  assert!(!output.contains("<<"));
  assert!(!output.contains("!include"));
  parse(&output).unwrap();
}

#[test]
fn root_merges_and_aliases_deep_merge_maps_and_allow_null_replacement() {
  let document = parse(
    "<<:\n  server: &server\n    upstream: {url: 'http://127.0.0.1:3000', preserve_host: false}\n  pow:\n    stratum_pool: {url: 'stratum+tcp://127.0.0.1:3333'}\nserver:\n  <<: *server\n  upstream: {preserve_host: true}\npow:\n  stratum_pool: null\n",
  )
  .unwrap();
  assert_eq!(document.server.upstream.url, "http://127.0.0.1:3000");
  assert!(document.server.upstream.preserve_host);
  assert!(document.pow.stratum_pool.is_none());
}

#[test]
fn invalid_merge_values_report_the_source_and_local_path() {
  let directory = tempfile::tempdir().unwrap();
  let directory = directory.path();
  write(directory, "rilgate.yml", "server: !include server.yml\n");
  for value in ["42", "null", "[{}, 42]", "[[{}]]", "!include scalar.yml"] {
    write(
      directory,
      "server.yml",
      &format!("upstream:\n  <<: {value}\n"),
    );
    write(directory, "scalar.yml", "42\n");
    let error = format!("{:#}", read(directory, vec![]).unwrap_err());
    assert!(
      error.contains("`<<` must be a mapping or a list of mappings"),
      "{error}"
    );
    assert!(error.contains("server.yml"), "{error}");
    assert!(error.contains("upstream.<<"), "{error}");
    assert!(error.contains("include chain:"), "{error}");
  }
}

#[test]
fn merged_schema_and_semantic_errors_retain_the_winning_origin() {
  let directory = tempfile::tempdir().unwrap();
  let directory = directory.path();
  write(directory, "rilgate.yml", "<<: !include base.yml\n");
  write(directory, "base.yml", "server: {max_requests: invalid}\n");
  let error = format!("{:#}", read(directory, vec![]).unwrap_err());
  assert!(error.contains("base.yml at server.max_requests"), "{error}");
  assert!(error.contains("include chain:"), "{error}");

  write(directory, "base.yml", "server: {listen: invalid-address}\n");
  write(
    directory,
    "rilgate.yml",
    &format!("<<: !include base.yml\n{BASE}"),
  );
  let error = format!(
    "{:#}",
    read(directory, vec![]).unwrap().resolve().unwrap_err()
  );
  assert!(error.contains("base.yml at server.listen"), "{error}");
  write(directory, "base.yml", BASE);
  write(
    directory,
    "rilgate.yml",
    "<<: !include base.yml\nserver: {max_requests: invalid}\n",
  );
  let error = format!("{:#}", read(directory, vec![]).unwrap_err());
  assert!(
    error.contains("rilgate.yml at server.max_requests"),
    "{error}"
  );
  assert!(!error.contains("base.yml at"), "{error}");
}
