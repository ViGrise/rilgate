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
      title: "Checking your browser".into(),
      description: String::new(),
    },
    challenge: PublicChallengeInfo {
      solver_module_url: format!(
        "{}/static/js/solvers/randomx/solver.js",
        snapshot.config.base_path
      ),
      ..Default::default()
    },
  }
}
fn render(snapshot: &WebSnapshot, locale: &str) -> String {
  snapshot
    .render("pages/challenge.html", &public(snapshot), locale)
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
    "templates/pages/challenge.html",
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
    fs::read_to_string(root.path().join("templates/pages/challenge.html")).unwrap(),
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
    r#"{"challenge.description":"Custom English"}"#,
  );
  write(
    root.path(),
    "i18n/vi.json",
    r#"{"challenge.title":"Kiểm tra", "challenge.explanation":"Xin chào {name}"}"#,
  );
  write(
    root.path(),
    "i18n/vi-VN.json",
    r#"{"challenge.status.preparing":"Đang chuẩn bị"}"#,
  );
  let mut config = config(root.path());
  config.i18n.supported_locales = ["en".into(), "vi".into(), "vi-VN".into()].to_vec();
  let manager = WebManager::new(config).unwrap();
  let snapshot = manager.current();
  let html = render(&snapshot, "vi-VN");
  assert!(html.contains("Kiểm tra"));
  assert!(html.contains("Custom English"));
  assert!(html.contains("Đang chuẩn bị"));
  assert!(html.contains("Xin chào Rilgate"));
  assert!(html.contains("Try again"));
  assert_eq!(
    snapshot
      .translations
      .translate("vi-VN", "missing.key", &BTreeMap::new())
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
    r#"{"challenge.title":"<img src=x onerror=alert(1)>", "challenge.status.preparing":"</script><script>alert(1)</script>"}"#,
  );
  let mut config = config(root.path());
  config.name = "<Acme & Co> __INTERNAL_PREFIX__".into();
  config.website_url = "https://example.test/?a=1&b=2".into();
  let manager = WebManager::new(config).unwrap();
  let snapshot = manager.current();
  let mut context = public(&snapshot);
  context.challenge.token = "</script><script>alert(2)</script>".into();
  let html = snapshot
    .render("pages/challenge.html", &context, "en")
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
    "templates/pages/challenge.html",
    "First {{ t('challenge.title') }}",
  );
  let manager = WebManager::new(config(root.path())).unwrap();
  let first = manager.current();
  write(
    root.path(),
    "templates/pages/challenge.html",
    "{% if broken",
  );
  assert!(manager.reload().is_err());
  assert_eq!(manager.current().generation, first.generation);
  assert!(render(&manager.current(), "en").starts_with("First"));
  write(
    root.path(),
    "templates/pages/challenge.html",
    "Second {{ t('challenge.title') }}",
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
    "templates/pages/challenge.html",
    "{% include 'pages/missing.html' %}",
  );
  assert!(WebSnapshot::build(&config(root.path()), 1, true).is_err());
  assert!(rilgate_web::management::validate(&config(root.path()), root.path()).is_err());
  let manager = WebManager::new(config(root.path())).unwrap();
  assert!(render(&manager.current(), "en").contains("Checking your browser"));
  assert!(
    manager
      .diagnostics()
      .last_reload_error
      .unwrap()
      .contains("missing.html")
  );
}

#[test]
fn strict_translation_mode_rejects_missing_keys() {
  let root = tempfile::tempdir().unwrap();
  write(
    root.path(),
    "templates/pages/challenge.html",
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
    "templates/pages/challenge.html",
    "{{ signing_key }}",
  );
  assert!(WebSnapshot::build(&config(root.path()), 1, true).is_err());
  write(
    root.path(),
    "templates/pages/challenge.html",
    "{% include 'pages/challenge.html' %}",
  );
  assert!(WebSnapshot::build(&config(root.path()), 1, true).is_err());
  write(
    root.path(),
    "templates/pages/challenge.html",
    "{% for i in range(1000000) %}{{ i }}{% endfor %}",
  );
  assert!(WebSnapshot::build(&config(root.path()), 1, true).is_err());
}

#[test]
fn static_overrides_have_immutable_bytes_and_matching_cache_metadata() {
  use axum::http::{HeaderMap, StatusCode};
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
      .response(&headers, false)
      .unwrap()
      .status(),
    StatusCode::NOT_MODIFIED
  );
  assert_eq!(
    second.assets["images/logo.svg"]
      .response(&headers, false)
      .unwrap()
      .status(),
    StatusCode::OK
  );
  assert_eq!(
    second.assets["images/logo.svg"]
      .response(&HeaderMap::new(), true)
      .unwrap()
      .headers()["content-length"],
    "6"
  );
  fs::remove_dir_all(root.path()).unwrap();
  assert_eq!(first.assets["images/logo.svg"].bytes().unwrap(), b"first");
  assert!(render(&second, "en").contains("Checking your browser"));
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
  write(outside.path(), "secret.html", "secret");
  fs::create_dir_all(root.path().join("templates/pages")).unwrap();
  std::os::unix::fs::symlink(
    outside.path().join("secret.html"),
    root.path().join("templates/pages/challenge.html"),
  )
  .unwrap();
  assert!(WebSnapshot::build(&config(root.path()), 1, true).is_err());
  assert!(seed::initialize(root.path()).is_err());
  assert!(
    rilgate_web::resources::loader::read_checked(
      root.path(),
      &root.path().join("templates/pages/challenge.html")
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
  write(root.path(), "templates/pages/challenge.html", "First");
  let mut config = config(root.path());
  config.watch.enabled = true;
  config.watch.debounce = Duration::from_millis(120);
  let manager = WebManager::new(config).unwrap();
  let _watch = rilgate_web::runtime::watcher::start(&manager)
    .unwrap()
    .unwrap();
  for value in ["One", "Two", "Three"] {
    write(root.path(), "templates/pages/challenge.html", value);
  }
  wait_until(|| manager.current().generation == 2).await;
  assert_eq!(render(&manager.current(), "en"), "Three");
  write(
    root.path(),
    "templates/pages/challenge.html",
    "{% if broken",
  );
  wait_until(|| manager.diagnostics().reload_failures_total > 0).await;
  assert_eq!(manager.current().generation, 2);
  write(root.path(), "templates/pages/challenge.html", "Recovered");
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
    "templates/pages/challenge.html",
    "{% if challenge.algorithm == 'randomx' %}{% include 'pages/missing.html' %}{% endif %}",
  );
  assert!(WebSnapshot::build(&config(root.path()), 1, true).is_err());
  write(
    root.path(),
    "templates/pages/challenge.html",
    "{% if challenge.difficulty >= 1000 %}{{ t('challenge.expires_in', seconds=challenge.expires_in) }}{% endif %}",
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
