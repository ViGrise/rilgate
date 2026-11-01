use std::{fs, path::Path};

#[test]
fn test_fixtures_resolve() {
  let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
  let mut checked = 0;
  for entry in fs::read_dir(&directory).unwrap() {
    let path = entry.unwrap().path();
    if path.extension().is_none_or(|extension| extension != "yml") {
      continue;
    }
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let text = fs::read_to_string(&path)
      .unwrap()
      .replace("${FIXTURE_LISTEN}", "127.0.0.1:18080")
      .replace("${FIXTURE_OPERATIONS_LISTEN}", "127.0.0.1:18081")
      .replace("${FIXTURE_UPSTREAM_URL}", "http://127.0.0.1:18082")
      .replace("${RILGATE_SECURITY_SECRET}", &"s".repeat(64));
    rilgate_config::loader::parse(&text)
      .and_then(|document| document.resolve())
      .unwrap_or_else(|error| panic!("Fixture {name} does not resolve: {error:#}"));
    checked += 1;
  }
  assert!(checked > 0, "No fixtures found in {}", directory.display());
}
