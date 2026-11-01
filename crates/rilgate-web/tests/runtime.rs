use rilgate_web::{
  WebConfig, WebManager,
  i18n::locale::{self, SupportedLocales},
  resources::{embedded::EmbeddedWeb, seed},
  runtime::snapshot::WebSnapshot,
  templates::context::{PageInfo, PublicChallengeInfo, TemplateContext},
};
use std::{collections::BTreeMap, fs, path::Path};

fn write(root: &Path, name: &str, value: &str) {
  let file = root.join(name);
  fs::create_dir_all(file.parent().unwrap()).unwrap();
  fs::write(file, value).unwrap();
}
fn config(root: &Path) -> WebConfig {
  let mut config = WebConfig {
    root: Some(root.into()),
    ..Default::default()
  };
  config.seed.empty_directory = false;
  config
}
fn public(snapshot: &WebSnapshot) -> TemplateContext {
  TemplateContext {
    rilgate: snapshot.public.clone(),
    page: PageInfo {
      open_graph: Vec::new(),
      title: "Just a moment".into(),
      description: String::new(),
    },
    challenge: PublicChallengeInfo {
      solver_module_url: snapshot.asset_url("/static/js/solvers/randomx/solver.js"),
      ..Default::default()
    },
  }
}
fn render(snapshot: &WebSnapshot, locale: &str) -> String {
  snapshot
    .render("pages/challenge.html.jinja", &public(snapshot), locale)
    .unwrap()
}

#[test]
fn embedded_bundle_operates_without_a_web_root() {
  let manager = WebManager::new(WebConfig::default()).unwrap();
  let snapshot = manager.current();
  assert!(render(&snapshot, "en").contains("Protected by"));
  assert!(
    snapshot
      .assets
      .contains_key("js/solvers/randomx/randomx_wasm.wasm")
  );
  assert!(!manager.diagnostics().watch_enabled);
}

#[test]
fn seeding_initializes_directory_trees_and_preserves_existing_files() {
  let root = tempfile::tempdir().unwrap();
  fs::create_dir_all(root.path().join("templates/pages")).unwrap();
  fs::write(root.path().join(".DS_Store"), "metadata").unwrap();
  seed::initialize(root.path()).unwrap();
  for (name, bytes) in EmbeddedWeb::entries() {
    assert_eq!(fs::read(root.path().join(name)).unwrap(), bytes);
  }
  assert!(root.path().join(".rilgate-web-version").exists());
  write(
    root.path(),
    "templates/pages/challenge.html.jinja",
    "operator content",
  );
  assert!(seed::initialize(root.path()).is_err());
  let manager = WebManager::new(WebConfig {
    root: Some(root.path().into()),
    ..Default::default()
  })
  .unwrap();
  assert_eq!(render(&manager.current(), "en"), "operator content");
  assert_eq!(
    fs::read_to_string(root.path().join("templates/pages/challenge.html.jinja")).unwrap(),
    "operator content"
  );
}

#[test]
fn hidden_operator_files_prevent_automatic_initialization() {
  let root = tempfile::tempdir().unwrap();
  fs::write(root.path().join(".operator-config"), "owned").unwrap();
  WebManager::new(WebConfig {
    root: Some(root.path().into()),
    ..Default::default()
  })
  .unwrap();
  assert!(!root.path().join("templates").exists());
}

#[test]
fn concurrent_initialization_never_replaces_destinations() {
  let root = tempfile::tempdir().unwrap();
  std::thread::scope(|scope| {
    for _ in 0..2 {
      scope.spawn(|| {
        let _ = seed::initialize(root.path());
      });
    }
  });
  for (name, bytes) in EmbeddedWeb::entries() {
    assert_eq!(fs::read(root.path().join(name)).unwrap(), bytes);
  }
}

#[test]
fn partial_translations_overlay_keys_and_fall_back_through_parent_and_default_locales() {
  let root = tempfile::tempdir().unwrap();

  write(
    root.path(),
    "i18n/en.json",
    r#"{"challenge.state.preparing.description":"Custom English"}"#,
  );

  write(
    root.path(),
    "i18n/qaa.json",
    r#"{"challenge.state.preparing.title":"Parent Title","challenge.details.body":"Parent Description"}"#,
  );

  write(
    root.path(),
    "i18n/qaa-001.json",
    r#"{"challenge.state.preparing.status":"Regional Status"}"#,
  );

  let mut config = config(root.path());
  config.i18n.supported_locales = ["en".into(), "qaa".into(), "qaa-001".into()].to_vec();

  let manager = WebManager::new(config).unwrap();
  let snapshot = manager.current();
  let html = render(&snapshot, "qaa-001");

  // Parent locale fallback
  assert!(html.contains("Parent Title"));
  assert!(html.contains("Parent Description"));

  // Exact regional locale
  assert!(html.contains("Regional Status"));

  // Default English locale fallback
  assert!(html.contains("Custom English"));
  assert!(html.contains("Try again"));

  // Missing keys fall back to the key itself
  assert_eq!(
    snapshot
      .translations
      .translate("qaa-001", "missing.key", &BTreeMap::new())
      .unwrap(),
    "missing.key"
  );
}

#[test]
fn locale_negotiation_respects_priority_weights_normalization_and_exclusion() {
  let mut config = WebConfig::default().i18n;
  config.supported_locales = vec!["en".into(), "vi".into(), "zh-CN".into()];
  // The declared list stands in for the translation files available here.
  let supported = SupportedLocales::resolve(&config, &config.supported_locales.clone()).unwrap();
  assert_eq!(locale::normalize("zh-cn").unwrap(), "zh-CN");
  assert_eq!(locale::normalize("en-latn-us").unwrap(), "en-Latn-US");
  assert!(locale::normalize("../vi").is_err());
  assert_eq!(
    supported.negotiate(Some("zh-cn"), Some("vi"), Some("en")),
    "zh-CN"
  );
  assert_eq!(
    supported.negotiate(None, Some("vi-VN"), Some("zh-CN")),
    "vi"
  );
  assert_eq!(
    supported.negotiate(None, None, Some("en;q=0.2, vi-VN;q=0.9")),
    "vi"
  );
  assert_eq!(
    supported.negotiate(None, None, Some("vi;q=0,en;q=0.5")),
    "en"
  );
  assert_eq!(
    supported.negotiate(Some("invalid"), None, Some("vi;q=bad")),
    "en"
  );
}

#[test]
fn locales_are_derived_from_translation_files_when_none_are_declared() {
  let mut config = WebConfig::default().i18n;
  config.supported_locales = vec![];
  let available = vec!["vi".to_string(), "en".to_string()];
  let supported = SupportedLocales::resolve(&config, &available).unwrap();
  assert_eq!(supported.tags(), ["en".to_string(), "vi".to_string()]);
  assert_eq!(supported.negotiate(None, None, Some("vi")), "vi");
  assert_eq!(supported.negotiate(None, None, Some("de")), "en");

  // A declared list restricts what the files provide.
  config.supported_locales = vec!["en".into()];
  let supported = SupportedLocales::resolve(&config, &available).unwrap();
  assert_eq!(supported.tags(), ["en".to_string()]);
  assert_eq!(supported.negotiate(None, None, Some("vi")), "en");

  // A default locale with no translations is a configuration error.
  config.supported_locales = vec!["vi".into()];
  assert!(SupportedLocales::resolve(&config, &available).is_err());
}

#[test]
fn templates_escape_public_data_translations_and_script_json() {
  let root = tempfile::tempdir().unwrap();
  write(
    root.path(),
    "i18n/en.json",
    r#"{"challenge.state.preparing.title":"<img src=x onerror=alert(1)>", "challenge.state.preparing.status":"</script><script>alert(1)</script>"}"#,
  );
  let mut config = config(root.path());
  config.name = "<Acme & Co> __INTERNAL_PREFIX__".into();
  config.website_url = "https://example.test/?a=1&b=2".into();
  let manager = WebManager::new(config).unwrap();
  let snapshot = manager.current();
  let mut context = public(&snapshot);
  context.challenge.token = "</script><script>alert(2)</script>".into();
  let html = snapshot
    .render("pages/challenge.html.jinja", &context, "en")
    .unwrap();
  assert!(html.contains("&lt;Acme &amp; Co&gt; __INTERNAL_PREFIX__"));
  assert!(html.contains("a=1&amp;b=2"));
  assert!(html.contains("&lt;img"));
  assert!(!html.contains("</script><script>alert("));
}

#[test]
fn invalid_edits_leave_the_previous_generation_and_held_snapshots_intact() {
  let root = tempfile::tempdir().unwrap();
  write(
    root.path(),
    "templates/pages/challenge.html.jinja",
    "First {{ t('challenge.state.preparing.title') }}",
  );
  let manager = WebManager::new(config(root.path())).unwrap();
  let first = manager.current();
  write(
    root.path(),
    "templates/pages/challenge.html.jinja",
    "{% if broken",
  );
  assert!(manager.reload().is_err());
  assert_eq!(manager.current().generation, first.generation);
  assert!(render(&manager.current(), "en").starts_with("First"));
  write(
    root.path(),
    "templates/pages/challenge.html.jinja",
    "Second {{ t('challenge.state.preparing.title') }}",
  );
  manager.reload().unwrap();
  assert_eq!(manager.current().generation, first.generation + 1);
  assert!(render(&manager.current(), "en").starts_with("Second"));
  assert!(render(&first, "en").starts_with("First"));
  write(root.path(), "i18n/en.json", "{");
  assert!(manager.reload().is_err());
  assert!(render(&manager.current(), "en").starts_with("Second"));
  assert_eq!(manager.diagnostics().reload_failures_total, 2);
}

#[test]
fn startup_errors_fall_back_but_validation_reports_them() {
  let root = tempfile::tempdir().unwrap();
  write(
    root.path(),
    "templates/pages/challenge.html.jinja",
    "{% include 'pages/missing.html.jinja' %}",
  );
  assert!(WebSnapshot::build(&config(root.path()), 1, true).is_err());
  assert!(rilgate_web::management::validate(&config(root.path()), root.path()).is_err());
  let manager = WebManager::new(config(root.path())).unwrap();
  assert!(render(&manager.current(), "en").contains("Just a moment"));
  assert!(
    manager
      .diagnostics()
      .last_reload_error
      .unwrap()
      .contains("missing.html.jinja")
  );
}

#[test]
fn strict_translation_mode_rejects_missing_keys() {
  let root = tempfile::tempdir().unwrap();
  write(
    root.path(),
    "templates/pages/challenge.html.jinja",
    "{{ t('missing.key') }}",
  );
  let mut config = config(root.path());
  config.i18n.strict = true;
  assert!(WebSnapshot::build(&config, 1, true).is_err());
  config.i18n.strict = false;
  assert_eq!(
    render(&WebSnapshot::build(&config, 1, true).unwrap(), "en"),
    "missing.key"
  );
}

#[test]
fn undefined_variables_recursion_and_fuel_are_checked_before_publication() {
  let root = tempfile::tempdir().unwrap();
  write(
    root.path(),
    "templates/pages/challenge.html.jinja",
    "{{ signing_key }}",
  );
  assert!(WebSnapshot::build(&config(root.path()), 1, true).is_err());
  write(
    root.path(),
    "templates/pages/challenge.html.jinja",
    "{% include 'pages/challenge.html.jinja' %}",
  );
  assert!(WebSnapshot::build(&config(root.path()), 1, true).is_err());
  write(
    root.path(),
    "templates/pages/challenge.html.jinja",
    "{% for i in range(1000000) %}{{ i }}{% endfor %}",
  );
  assert!(WebSnapshot::build(&config(root.path()), 1, true).is_err());
}

#[test]
fn asset_revision_follows_asset_bytes() {
  let revision = |value: &str| {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "static/images/logo.svg", value);
    WebManager::new(config(root.path()))
      .unwrap()
      .current()
      .asset_revision
      .clone()
  };
  assert_eq!(revision("first"), revision("first"));
  assert_ne!(revision("first"), revision("second"));
}

#[test]
fn static_overrides_have_immutable_bytes_and_matching_cache_metadata() {
  use http::{HeaderMap, StatusCode};
  use rilgate_web::assets::AssetCache;
  let root = tempfile::tempdir().unwrap();
  write(root.path(), "static/images/logo.svg", "first");
  let manager = WebManager::new(config(root.path())).unwrap();
  let first = manager.current();
  let etag = first.assets["images/logo.svg"].etag.clone();
  write(root.path(), "static/images/logo.svg", "second");
  manager.reload().unwrap();
  let second = manager.current();
  assert_eq!(first.assets["images/logo.svg"].bytes().unwrap(), b"first");
  assert_eq!(second.assets["images/logo.svg"].bytes().unwrap(), b"second");
  assert_ne!(etag, second.assets["images/logo.svg"].etag);
  let mut headers = HeaderMap::new();
  headers.insert("if-none-match", etag.parse().unwrap());
  assert_eq!(
    first.assets["images/logo.svg"]
      .response(&headers, false, AssetCache::Revalidate)
      .unwrap()
      .status(),
    StatusCode::NOT_MODIFIED
  );
  assert_eq!(
    second.assets["images/logo.svg"]
      .response(&headers, false, AssetCache::Revalidate)
      .unwrap()
      .status(),
    StatusCode::OK
  );
  assert_eq!(
    second.assets["images/logo.svg"]
      .response(&HeaderMap::new(), true, AssetCache::Revalidate)
      .unwrap()
      .headers()["content-length"],
    "6"
  );
  fs::remove_dir_all(root.path()).unwrap();
  assert_eq!(first.assets["images/logo.svg"].bytes().unwrap(), b"first");
  assert!(render(&second, "en").contains("Just a moment"));
}

#[test]
fn stylesheet_and_public_branding_can_remove_product_names_from_rendered_pages() {
  let root = tempfile::tempdir().unwrap();
  let mut config = config(root.path());
  config.name = "Example Shield".into();
  config.website_url = "https://example.test".into();
  config.base_path = "/.well-known/shield".into();
  config.stylesheet_path = "css/shield.css".into();
  let snapshot = WebSnapshot::build(&config, 1, true).unwrap();
  assert!(!render(&snapshot, "en").to_lowercase().contains("rilgate"));
  assert!(snapshot.assets.contains_key("css/shield.css"));
}

#[test]
fn concurrent_reloads_are_serialized() {
  let root = tempfile::tempdir().unwrap();
  let manager = WebManager::new(config(root.path())).unwrap();
  std::thread::scope(|scope| {
    for _ in 0..4 {
      let manager = manager.clone();
      scope.spawn(move || manager.reload().unwrap());
    }
  });
  assert_eq!(manager.current().generation, 5);
}

#[cfg(unix)]
#[test]
fn symlink_escapes_and_invalid_logical_paths_are_rejected() {
  let root = tempfile::tempdir().unwrap();
  let outside = tempfile::tempdir().unwrap();
  write(outside.path(), "secret.html.jinja", "secret");
  fs::create_dir_all(root.path().join("templates/pages")).unwrap();
  std::os::unix::fs::symlink(
    outside.path().join("secret.html.jinja"),
    root.path().join("templates/pages/challenge.html.jinja"),
  )
  .unwrap();
  assert!(WebSnapshot::build(&config(root.path()), 1, true).is_err());
  assert!(seed::initialize(root.path()).is_err());
  assert!(
    rilgate_web::resources::loader::read_checked(
      root.path(),
      &root.path().join("templates/pages/challenge.html.jinja")
    )
    .is_err()
  );
  for name in [
    "../secret",
    "/etc/passwd",
    "C:\\secret",
    "pages/../secret",
    "pages/\0secret",
    "pages//secret",
  ] {
    assert!(rilgate_web::resources::loader::logical_path(name).is_err());
  }
}

#[tokio::test]
async fn watcher_debounces_updates_retains_invalid_edits_and_recovers() {
  use std::time::Duration;
  let root = tempfile::tempdir().unwrap();
  write(root.path(), "templates/pages/challenge.html.jinja", "First");
  let mut config = config(root.path());
  config.watch.enabled = true;
  config.watch.debounce = Duration::from_millis(120);
  let manager = WebManager::new(config).unwrap();
  let _watch = rilgate_web::runtime::watcher::start(&manager)
    .unwrap()
    .unwrap();
  for value in ["One", "Two", "Three"] {
    write(root.path(), "templates/pages/challenge.html.jinja", value);
  }
  wait_until(|| manager.current().generation == 2).await;
  assert_eq!(render(&manager.current(), "en"), "Three");
  write(
    root.path(),
    "templates/pages/challenge.html.jinja",
    "{% if broken",
  );
  wait_until(|| manager.diagnostics().reload_failures_total > 0).await;
  assert_eq!(manager.current().generation, 2);
  write(
    root.path(),
    "templates/pages/challenge.html.jinja",
    "Recovered",
  );
  wait_until(|| manager.current().generation == 3).await;
  assert_eq!(render(&manager.current(), "en"), "Recovered");
  async fn wait_until(predicate: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(8), async {
      while !predicate() {
        tokio::time::sleep(Duration::from_millis(20)).await;
      }
    })
    .await
    .unwrap();
  }
}

#[test]
fn validation_checks_both_fast_and_randomx_template_branches() {
  let root = tempfile::tempdir().unwrap();
  write(
    root.path(),
    "templates/pages/challenge.html.jinja",
    "{% if challenge.algorithm == 'randomx' %}{% include 'pages/missing.html.jinja' %}{% endif %}",
  );
  assert!(WebSnapshot::build(&config(root.path()), 1, true).is_err());
  write(
    root.path(),
    "templates/pages/challenge.html.jinja",
    "{% if challenge.difficulty >= 1000 %}{{ t('refresh.waiting', seconds=challenge.expires_in) }}{% endif %}",
  );
  assert!(WebSnapshot::build(&config(root.path()), 1, true).is_ok());
}

#[test]
fn legacy_asset_overrides_are_shared_by_aliases_and_canonical_paths() {
  let root = tempfile::tempdir().unwrap();
  write(root.path(), "static/js/hash-worker.js", "legacy worker");
  let snapshot = WebSnapshot::build(&config(root.path()), 1, true).unwrap();
  let canonical = rilgate_web::assets::canonical_asset("js/hash-worker.js");
  assert_eq!(
    snapshot.assets[canonical].bytes().unwrap(),
    b"legacy worker"
  );
  assert_eq!(
    rilgate_web::assets::canonical_asset("hash-worker.js"),
    canonical
  );

  write(
    root.path(),
    "static/js/solvers/fast/worker.js",
    "canonical worker",
  );
  let snapshot = WebSnapshot::build(&config(root.path()), 2, true).unwrap();
  assert_eq!(
    snapshot.assets[canonical].bytes().unwrap(),
    b"canonical worker"
  );
}

#[test]
fn cached_challenge_shell_preserves_locales_and_isolates_bootstrap_data() {
  let root = tempfile::tempdir().unwrap();
  write(
    root.path(),
    "i18n/vi.json",
    r#"{"challenge.state.preparing.title":"Kiểm tra"}"#,
  );
  let snapshot = WebSnapshot::build(&config(root.path()), 1, true).unwrap();
  for locale in ["en", "vi"] {
    let mut context = public(&snapshot);
    context.page.title = snapshot
      .translations
      .translate(locale, "challenge.title", &BTreeMap::new())
      .unwrap();
    for (token, algorithm) in [
      ("first", "fast"),
      ("</script><script>alert(1)</script>&\u{2028}", "randomx"),
    ] {
      context.challenge.token = token.into();
      context.challenge.algorithm = algorithm.into();
      let cached = snapshot
        .render_challenge(&context.challenge, locale)
        .unwrap();
      assert_eq!(
        cached,
        snapshot
          .render("pages/challenge.html.jinja", &context, locale)
          .unwrap()
      );
      assert!(!cached.contains("</script><script>alert("));
      assert!(!cached.contains("data-rilgate-bootstrap"));
      let json = cached
        .split("id=\"pow-bootstrap\">")
        .nth(1)
        .unwrap()
        .split("</script>")
        .next()
        .unwrap();
      assert_eq!(
        serde_json::from_str::<serde_json::Value>(json).unwrap()["presentation_token"],
        token
      );
    }
    let fresh = snapshot
      .render_challenge(
        &PublicChallengeInfo {
          token: "fresh".into(),
          ..context.challenge
        },
        locale,
      )
      .unwrap();
    assert!(!fresh.contains("alert(1)"));
  }
}

#[test]
fn customized_templates_keep_dynamic_challenge_branches() {
  let root = tempfile::tempdir().unwrap();
  write(
    root.path(),
    "templates/partials/operator-info.html.jinja",
    "{% if challenge.difficulty >= 1000 %}RX {{ challenge.expires_in }}{% else %}FAST{% endif %}",
  );
  write(
    root.path(),
    "templates/pages/challenge.html.jinja",
    "{% include 'partials/operator-info.html.jinja' %}",
  );
  let snapshot = WebSnapshot::build(&config(root.path()), 1, true).unwrap();
  let mut challenge = public(&snapshot).challenge;
  challenge.difficulty = Some(1);
  assert!(
    snapshot
      .render_challenge(&challenge, "en")
      .unwrap()
      .contains("FAST")
  );
  challenge.difficulty = Some(1000);
  challenge.expires_in = Some(45);
  assert!(
    snapshot
      .render_challenge(&challenge, "en")
      .unwrap()
      .contains("RX 45")
  );
}

#[test]
fn challenge_shells_and_i18n_json_follow_snapshot_generations() {
  let root = tempfile::tempdir().unwrap();
  let manager = WebManager::new(config(root.path())).unwrap();
  let first = manager.current();
  write(
    root.path(),
    "i18n/en.json",
    r#"{"challenge.state.preparing.title":"New title", "challenge.state.preparing.status":"New status </script>"}"#,
  );
  manager.reload().unwrap();
  let second = manager.current();
  let html = second
    .render_challenge(&public(&second).challenge, "en")
    .unwrap();
  assert!(html.contains("New title"));
  assert!(html.contains("New status \\u003c/script\\u003e"));
  assert!(
    !first
      .render_challenge(&public(&first).challenge, "en")
      .unwrap()
      .contains("New title")
  );
}

#[test]
fn invalid_json_templates_keep_the_previous_resource_snapshot() {
  let root = tempfile::tempdir().unwrap();
  let manager = WebManager::new(config(root.path())).unwrap();
  let generation = manager.current().generation;
  write(
    root.path(),
    "templates/pages/challenge.json.jinja",
    r#"{"error": {{ challenge.token }}}"#,
  );
  assert!(manager.reload().is_err());
  assert_eq!(manager.current().generation, generation);
  write(
    root.path(),
    "templates/pages/challenge.json.jinja",
    r#"{"error":{"token":{{ challenge.token | tojson }}}}"#,
  );
  manager.reload().unwrap();
  assert!(manager.current().generation > generation);
}

#[test]
fn generated_manifest_matches_sources_and_declares_startup_order() {
  use sha2::{Digest, Sha256};
  let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("generated");
  let manifest: serde_json::Value =
    serde_json::from_slice(&fs::read(directory.join("manifest.json")).unwrap()).unwrap();
  let members = manifest["startup"].as_array().unwrap();
  let paths = members
    .iter()
    .map(|member| member["source"].as_str().unwrap())
    .collect::<Vec<_>>();
  assert_eq!(
    paths,
    [
      "static/js/challenge/boot.js",
      "static/js/challenge/promise.js",
      "static/js/challenge/runtime.js",
      "static/js/challenge/registry.js",
      "static/js/challenge/stratum-pool.js",
      "static/js/solvers/fast/worker.js",
      "static/js/solvers/fast/solver.js",
      "static/js/solvers/fast/adapter.js",
      "static/js/solvers/randomx/adapter.js",
      "static/js/solvers/hashwx/adapter.js",
      "static/js/challenge.js",
    ]
  );
  let mut expected = Vec::new();
  for member in members {
    let source = EmbeddedWeb::get(member["source"].as_str().unwrap()).unwrap();
    assert_eq!(
      hex::encode(Sha256::digest(source)),
      member["sha256"].as_str().unwrap()
    );
    expected.extend(fs::read(directory.join(member["output"].as_str().unwrap())).unwrap());
    expected.extend_from_slice(b"\n;\n");
  }
  let snapshot = WebSnapshot::build(&WebConfig::default(), 1, false).unwrap();
  assert_eq!(
    snapshot.assets["js/challenge.bundle.js"].bytes().unwrap(),
    expected
  );
  let html = render(&snapshot, "en");
  assert!(html.contains("<script defer src="));
  assert!(html.contains("/js/challenge.bundle.js"));
  assert!(!html.contains("type=\"module\""));
  assert!(
    html.contains(
      "<noscript><style>.bar,#status{display:none}.load-error{display:none!important}</style><p>Please enable JavaScript"
    )
  );
}

#[test]
fn seeded_sources_select_generated_output_and_edits_are_served_as_authored() {
  let root = tempfile::tempdir().unwrap();
  seed::initialize(root.path()).unwrap();
  let embedded = WebSnapshot::build(&WebConfig::default(), 1, false).unwrap();
  let seeded = WebSnapshot::build(&config(root.path()), 1, true).unwrap();
  for name in ["js/challenge.bundle.js", "js/solvers/fast/worker.js"] {
    assert_eq!(embedded.assets[name].etag, seeded.assets[name].etag);
    assert_eq!(
      embedded.assets[name].bytes().unwrap(),
      seeded.assets[name].bytes().unwrap()
    );
  }
  let authored = "const operatorMessage = 'authored bytes';\n";
  write(root.path(), "static/js/challenge/registry.js", authored);
  let worker = "self.onmessage = function () { self.postMessage('operator'); };\n";
  write(root.path(), "static/js/solvers/fast/worker.js", worker);
  let edited = WebSnapshot::build(&config(root.path()), 2, true).unwrap();
  let bundle = String::from_utf8(edited.assets["js/challenge.bundle.js"].bytes().unwrap()).unwrap();
  assert!(bundle.contains(authored));
  assert!(bundle.contains(worker));
  assert!(!bundle.contains("Duplicate solver adapter"));
  assert_eq!(
    edited.assets["js/solvers/fast/worker.js"].bytes().unwrap(),
    worker.as_bytes()
  );
  assert_eq!(
    edited.assets["js/challenge/registry.js"].bytes().unwrap(),
    authored.as_bytes()
  );
  assert_ne!(
    seeded.assets["js/challenge.bundle.js"].etag,
    edited.assets["js/challenge.bundle.js"].etag
  );
}

#[test]
fn wasm_revision_is_shared_by_module_and_bundle_overrides() {
  let root = tempfile::tempdir().unwrap();
  write(
    root.path(),
    "static/js/challenge.js",
    "window.operatorRevision = '__WASM_REVISION__';",
  );
  write(
    root.path(),
    "static/js/solvers/randomx/randomx_wasm.wasm",
    "operator wasm",
  );
  let snapshot = WebSnapshot::build(&config(root.path()), 1, true).unwrap();
  let revision = snapshot.assets["js/solvers/randomx/randomx_wasm.wasm"]
    .etag
    .trim_matches('"');
  for name in ["js/challenge.bundle.js", "js/solvers/randomx/solver.js"] {
    let text = String::from_utf8(snapshot.assets[name].bytes().unwrap()).unwrap();
    assert!(text.contains(revision));
    assert!(!text.contains("__WASM_REVISION__"));
  }
}

#[test]
fn optional_legal_translations_share_fallback_and_escape_dialog_paragraphs() {
  let mut embedded_config = WebConfig::default();
  embedded_config.i18n.strict = true;
  let embedded = WebSnapshot::build(&embedded_config, 1, true).unwrap();
  let html = render(&embedded, "en");
  assert!(!html.contains("Privacy"));
  assert!(!html.contains("Terms"));
  assert!(!html.contains("data-dialog=\"privacy\""));
  assert!(!html.contains("data-dialog=\"terms\""));
  for name in ["challenge", "refresh", "denied", "error"] {
    embedded
      .render(
        &format!("pages/{name}.html.jinja"),
        &public(&embedded),
        "en",
      )
      .unwrap();
  }
  let root = tempfile::tempdir().unwrap();
  write(
    root.path(),
    "i18n/en.json",
    r#"{
    "legal.privacy.label":"Privacy",
    "legal.privacy.title":"Privacy information",
    "legal.privacy.body":"First paragraph.\n\n<script>alert(1)</script>",
    "legal.terms.label":"Terms",
    "legal.terms.url":"https://example.com/terms"
  }"#,
  );
  write(
    root.path(),
    "templates/pages/lookup.html.jinja",
    "{{ has_t('common.close') }} {{ has_t('missing.key') }} {{ has_t('legal.privacy.label') }}",
  );
  let mut config = config(root.path());
  config.i18n.strict = true;
  let snapshot = WebSnapshot::build(&config, 1, true).unwrap();
  assert_eq!(
    snapshot
      .render("pages/lookup.html.jinja", &public(&snapshot), "en-US")
      .unwrap(),
    "True False True"
  );
  let html = render(&snapshot, "en");
  assert!(html.contains("<dialog id=\"privacy\""));
  assert!(
    html.contains("<p>First paragraph.</p><p>&lt;script&gt;alert(1)&lt;&#x2f;script&gt;</p>")
  );
  assert!(html.contains(
    "<a href=\"https:&#x2f;&#x2f;example.com&#x2f;terms\" rel=\"noopener noreferrer\">Terms</a>"
  ));
  assert!(!html.contains("<script>alert(1)</script>"));
}
