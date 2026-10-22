use http::{HeaderMap, StatusCode, header};
use http_body_util::BodyExt;
use rilgate_web::assets::AssetMetadata;

#[tokio::test]
async fn asset_responses_preserve_bytes_and_cache_semantics() {
  let root = tempfile::tempdir().unwrap();
  for size in [0, 1, 4096, 65535, 65536, 65537, 143360, 1048576] {
    let bytes: Vec<u8> = (0..size).map(|index| (index % 251) as u8).collect();
    let path = root.path().join("asset.wasm");
    std::fs::write(&path, &bytes).unwrap();
    let memory = AssetMetadata::memory("asset.wasm", bytes.clone());
    let file = AssetMetadata::file(root.path(), "asset.wasm", &path).unwrap();
    assert_eq!(memory.etag, file.etag);
    for asset in [memory, file] {
      assert_eq!(asset.bytes().unwrap(), bytes);
      let head = asset.response(&HeaderMap::new(), true).unwrap();
      assert_eq!(head.status(), StatusCode::OK);
      assert_eq!(head.headers()[header::CONTENT_LENGTH], size.to_string());
      assert_eq!(head.headers()[header::CONTENT_TYPE], "application/wasm");
      assert!(
        head
          .into_body()
          .collect()
          .await
          .unwrap()
          .to_bytes()
          .is_empty()
      );

      for value in [
        asset.etag.clone(),
        format!("\"other\", W/{}", asset.etag),
        "*".into(),
      ] {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, value.parse().unwrap());
        let response = asset.response(&headers, false).unwrap();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(response.headers()[header::ETAG], asset.etag);
        assert!(!response.headers().contains_key(header::CONTENT_LENGTH));
        assert!(
          response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
        );
      }

      let first = asset
        .response(&HeaderMap::new(), false)
        .unwrap()
        .into_body();
      let second = asset
        .response(&HeaderMap::new(), false)
        .unwrap()
        .into_body();
      std::fs::write(&path, b"replacement").unwrap();
      drop(asset);
      let (first, second) = tokio::join!(first.collect(), second.collect());
      assert_eq!(first.unwrap().to_bytes().as_ref(), bytes);
      assert_eq!(second.unwrap().to_bytes().as_ref(), bytes);
    }
  }
}

#[tokio::test]
async fn challenge_assets_revalidate_the_bundle_and_individual_scripts() {
  let snapshot =
    rilgate_web::runtime::snapshot::WebSnapshot::build(&Default::default(), 1, false).unwrap();
  for name in [
    "js/challenge.bundle.js",
    "js/challenge.js",
    "js/challenge/runtime.js",
    "js/challenge/registry.js",
    "js/challenge/p2pool.js",
    "js/solvers/fast/worker.js",
    "js/solvers/fast/solver.js",
    "js/solvers/fast/adapter.js",
    "js/solvers/randomx/adapter.js",
    "js/solvers/randomx/loader.js",
    "js/solvers/randomx/solver.js",
    "challenge.js",
    "hash-worker.js",
  ] {
    let asset = &snapshot.assets[rilgate_web::assets::canonical_asset(name)];
    let response = asset.response(&HeaderMap::new(), false).unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{name}");
    assert_eq!(response.headers()[header::ETAG], asset.etag);
    assert_eq!(
      response.headers()[header::CACHE_CONTROL],
      "public, max-age=0, must-revalidate"
    );
    assert_eq!(
      response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .as_ref(),
      asset.bytes().unwrap()
    );
    let mut headers = HeaderMap::new();
    headers.insert(header::IF_NONE_MATCH, asset.etag.parse().unwrap());
    let response = asset.response(&headers, false).unwrap();
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED, "{name}");
    assert!(
      response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .is_empty()
    );
  }
}
