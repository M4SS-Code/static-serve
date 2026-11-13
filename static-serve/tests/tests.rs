//! Integration tests for static-serve and macro
use std::io::Read;

use axum::{
    Router,
    body::{Body, HttpBody},
    http::{
        HeaderValue, Method, Request, Response, StatusCode,
        header::{
            ACCEPT_ENCODING, ACCEPT_RANGES, CONTENT_ENCODING, IF_MATCH, IF_NONE_MATCH, IF_RANGE,
            RANGE,
        },
    },
};
use http_body_util::BodyExt;
use tower::ServiceExt;

use static_serve_macro::{embed_asset, embed_assets};

enum Compression {
    Zstd,
    Gzip,
    Both,
    None,
}

async fn get_response(
    router: Router<()>,
    request: Request<axum::body::Body>,
) -> Response<axum::body::Body> {
    router
        .into_service()
        .oneshot(request)
        .await
        .expect("sending request")
}

fn create_request(route: &str, compression: &Compression) -> Request<axum::body::Body> {
    let accept_encoding_header = match compression {
        Compression::Both => Some(HeaderValue::from_static("zstd, gzip")),
        Compression::Zstd => Some(HeaderValue::from_static("zstd")),
        Compression::Gzip => Some(HeaderValue::from_static("gzip")),
        Compression::None => None,
    };
    match accept_encoding_header {
        Some(v) => Request::builder()
            .uri(route)
            .header(ACCEPT_ENCODING, v)
            .body(Body::empty())
            .unwrap(),
        None => Request::builder().uri(route).body(Body::empty()).unwrap(),
    }
}

fn decompress_zstd(compressed_body: &[u8]) -> Vec<u8> {
    let mut decoder = zstd::Decoder::new(compressed_body).expect("failed to create decoder");
    let mut decompressed_body = Vec::new();
    std::io::copy(&mut decoder, &mut decompressed_body).expect("failed to decompress");
    decompressed_body
}

fn decompress_gzip(compressed_body: &[u8]) -> Vec<u8> {
    let mut decompressed_body = Vec::new();
    let mut decoder = flate2::bufread::GzDecoder::new(compressed_body);
    decoder
        .read_to_end(&mut decompressed_body)
        .expect("can't decode body");
    decompressed_body
}

#[tokio::test]
async fn router_created_with_lit_str() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/small", compress = false);
    assert!(router.has_routes());
    let request = create_request("/app.js", &Compression::None);
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/small/app.js");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn router_created_uncompressed_because_not_worthwhile() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/small", compress = true);
    assert!(router.has_routes());

    let request = create_request("/app.js", &Compression::Zstd);
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    // Response should not be compressed since the benefit is insignificant
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/small/app.js");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn router_created_compressed_zstd_only() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = true);
    assert!(router.has_routes());

    let request = create_request("/app.js", &Compression::Zstd);
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get(CONTENT_ENCODING),
        Some(&HeaderValue::from_str("zstd").unwrap())
    );
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();

    // Decompress the response body
    let decompressed_body = decompress_zstd(&collected_body_bytes);
    assert_eq!(
        decompressed_body,
        include_bytes!("../../test_fixtures/assets/big/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/app.js.zst");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn router_created_compressed_gzip_only() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = true);
    assert!(router.has_routes());

    let request = create_request("/app.js", &Compression::Gzip);
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get(CONTENT_ENCODING),
        Some(&HeaderValue::from_str("gzip").unwrap())
    );
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let decompressed_body = decompress_gzip(&collected_body_bytes);

    assert_eq!(
        decompressed_body,
        include_bytes!("../../test_fixtures/assets/big/app.js"),
        "decompressed body is not as expected"
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/app.js.gz");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn router_created_compressed_zstd_or_gzip_accepted() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = true);
    assert!(router.has_routes());

    let request = create_request("/app.js", &Compression::Both);
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get(CONTENT_ENCODING),
        Some(&HeaderValue::from_str("zstd").unwrap())
    );
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let decompressed_body = decompress_zstd(&collected_body_bytes);
    assert_eq!(
        decompressed_body,
        include_bytes!("../../test_fixtures/assets/big/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/app.js.zst");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn router_created_ignore_paths_one() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets", ignore_paths = ["dist"]);
    assert!(router.has_routes());

    let request = create_request("/small/app.js", &Compression::None);
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/small/app.js");

    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    assert!(parts.headers.contains_key("etag"));
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn router_created_ignore_paths_three() {
    let router: Router<()> = embed_assets!(
        "../test_fixtures/assets",
        ignore_paths = ["big", "small", "dist", "with_html"]
    );
    // all directories ignored, so router has no routes
    assert!(!router.has_routes());

    let request = create_request("/app.js", &Compression::None);
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();

    // Expect a 404 Not Found with empty body
    assert_eq!(parts.status, StatusCode::NOT_FOUND);
    assert!(collected_body_bytes.is_empty());
}

#[tokio::test]
async fn handles_conditional_requests_same_etag() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = true);
    assert!(router.has_routes());

    let request = create_request("/app.js", &Compression::Zstd);
    let response = get_response(router.clone(), request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get(CONTENT_ENCODING),
        Some(&HeaderValue::from_str("zstd").unwrap())
    );
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    let etag = parts
        .headers
        .get("etag")
        .expect("no etag header when there should be one!");

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let decompressed_body = decompress_zstd(&collected_body_bytes);
    assert_eq!(
        decompressed_body,
        include_bytes!("../../test_fixtures/assets/big/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/app.js.zst");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);

    let request = Request::builder()
        .uri("/app.js")
        .header(IF_NONE_MATCH, etag)
        .header(ACCEPT_ENCODING, "zstd")
        .body(Body::empty())
        .unwrap();
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::NOT_MODIFIED);
    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    assert!(collected_body_bytes.is_empty());
}

#[tokio::test]
async fn handles_conditional_requests_different_etag() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = true);
    assert!(router.has_routes());

    let request = Request::builder()
        .uri("/app.js")
        .header(IF_NONE_MATCH, "n0t4r34l3t4g")
        .header(ACCEPT_ENCODING, "zstd")
        .body(Body::empty())
        .unwrap();
    let response = get_response(router, request).await;

    let (parts, body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::OK);
    assert_ne!(
        parts
            .headers
            .get("content-length")
            .expect("no content-length header!"),
        "0",
        "content length is unexpectedly zero!"
    );

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    assert!(!collected_body_bytes.is_empty());
    let decompressed_body = decompress_zstd(&collected_body_bytes);
    assert_eq!(
        decompressed_body,
        include_bytes!("../../test_fixtures/assets/big/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/app.js.zst");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn strips_html_correctly() {
    let router: Router<()> = embed_assets!(
        "../test_fixtures/assets/with_html",
        compress = false,
        strip_html_ext = true
    );
    assert!(router.has_routes());

    // Test `index.html`
    let request = create_request("/", &Compression::None);
    let response = get_response(router.clone(), request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/html; charset=utf-8"
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/with_html/index.html");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);

    // Test `.htm`
    let request = create_request("/index2", &Compression::None);
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/html; charset=utf-8"
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/with_html/index2.htm");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn doesnt_strip_html_when_strip_html_false() {
    let router: Router<()> = embed_assets!(
        "../test_fixtures/assets/with_html",
        compress = false,
        strip_html_ext = false
    );
    assert!(router.has_routes());

    // Requesting `/index` with no extension fails
    let request = create_request("/index", &Compression::None);
    let response = get_response(router.clone(), request).await;
    let (parts, _body) = response.into_parts();
    assert!(!parts.status.is_success());

    // Requesting `/index.html` succeeds
    let request = create_request("/index.html", &Compression::None);
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/html; charset=utf-8"
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/with_html/index.html");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn doesnt_strip_html_when_not_specified() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/with_html", compress = false);
    assert!(router.has_routes());

    // Requesting `/index` with no extension fails
    let request = create_request("/index", &Compression::None);
    let response = get_response(router.clone(), request).await;
    let (parts, _body) = response.into_parts();
    assert!(!parts.status.is_success());

    // Requesting `/index.html` succeeds
    let request = create_request("/index.html", &Compression::None);
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/html; charset=utf-8"
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/with_html/index.html");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn handles_one_file_uncompressed() {
    let router: Router<()> = Router::new();
    let handler = embed_asset!(
        "../test_fixtures/assets/dist/ignore_me_plz.txt",
        compress = true
    );
    let router = router.route("/ignore", handler);
    assert!(router.has_routes());

    let request = create_request("/ignore", &Compression::None);

    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();

    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/plain; charset=utf-8"
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/ignore_me_plz.txt");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn handles_one_file_compressed_zstd() {
    let router: Router<()> = Router::new();
    let handler = embed_asset!("../test_fixtures/assets/big/app.js", compress = true);
    let router = router.route("/app.js", handler);
    assert!(router.has_routes());

    let request = create_request("/app.js", &Compression::Zstd);

    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();

    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();

    // Decompress the response body
    let decompressed_body = decompress_zstd(&collected_body_bytes);
    assert_eq!(
        decompressed_body,
        include_bytes!("../../test_fixtures/assets/big/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/app.js.zst");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn handles_one_file_same_etag_with_cache_control() {
    let router: Router<()> = Router::new();
    let handler = embed_asset!(
        "../test_fixtures/assets/big/app.js",
        compress = true,
        cache_bust = true
    );
    let router = router.route("/app.js", handler);
    assert!(router.has_routes());

    let request = create_request("/app.js", &Compression::Zstd);

    let response = get_response(router.clone(), request).await;
    let (parts, body) = response.into_parts();

    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    assert!(parts.headers.contains_key("etag"));
    let etag = parts
        .headers
        .get("etag")
        .expect("no etag header when there should be one!");

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();

    // Decompress the response body
    let decompressed_body = decompress_zstd(&collected_body_bytes);
    assert_eq!(
        decompressed_body,
        include_bytes!("../../test_fixtures/assets/big/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/app.js.zst");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);

    let request = Request::builder()
        .uri("/app.js")
        .header(IF_NONE_MATCH, etag)
        .header(ACCEPT_ENCODING, "zstd")
        .body(Body::empty())
        .unwrap();
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::NOT_MODIFIED);
    assert!(
        parts
            .headers
            .get("cache-control")
            .expect("cache-control header should exist")
            .to_str()
            .expect("should make cache-control header into str")
            .contains("immutable")
    );

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    assert!(collected_body_bytes.is_empty());
}

#[tokio::test]
async fn handles_one_file_different_etag_with_cache_control() {
    let router: Router<()> = Router::new();
    let handler = embed_asset!(
        "../test_fixtures/assets/big/app.js",
        compress = true,
        cache_bust = true
    );
    let router = router.route("/app.js", handler);
    assert!(router.has_routes());

    let request = create_request("/app.js", &Compression::Zstd);

    let response = get_response(router.clone(), request).await;
    let (parts, body) = response.into_parts();

    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    assert!(parts.headers.contains_key("etag"));
    assert!(
        parts
            .headers
            .get("cache-control")
            .expect("cache-control header should exist")
            .to_str()
            .expect("should make cache-control header into str")
            .contains("immutable")
    );

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();

    // Decompress the response body
    let decompressed_body = decompress_zstd(&collected_body_bytes);
    assert_eq!(
        decompressed_body,
        include_bytes!("../../test_fixtures/assets/big/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/app.js.zst");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);

    let request = Request::builder()
        .uri("/app.js")
        .header(IF_NONE_MATCH, "n0t4r34l3t4g")
        .header(ACCEPT_ENCODING, "zstd")
        .body(Body::empty())
        .unwrap();
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::OK);
    assert_ne!(
        parts
            .headers
            .get("content-length")
            .expect("no content-length header!"),
        "0",
        "content length is unexpectedly zero!"
    );

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    assert!(!collected_body_bytes.is_empty());
    let decompressed_body = decompress_zstd(&collected_body_bytes);
    assert_eq!(
        decompressed_body,
        include_bytes!("../../test_fixtures/assets/big/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/app.js.zst");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn handles_one_file_same_etag_no_cache_control() {
    let router: Router<()> = Router::new();
    let handler = embed_asset!(
        "../test_fixtures/assets/big/app.js",
        compress = true,
        cache_bust = false
    );
    let router = router.route("/app.js", handler);
    assert!(router.has_routes());

    let request = create_request("/app.js", &Compression::Zstd);

    let response = get_response(router.clone(), request).await;
    let (parts, body) = response.into_parts();

    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    assert!(parts.headers.contains_key("etag"));
    let etag = parts
        .headers
        .get("etag")
        .expect("no etag header when there should be one!");
    assert!(parts.headers.get("cache-control").is_none());

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();

    // Decompress the response body
    let decompressed_body = decompress_zstd(&collected_body_bytes);
    assert_eq!(
        decompressed_body,
        include_bytes!("../../test_fixtures/assets/big/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/app.js.zst");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);

    let request = Request::builder()
        .uri("/app.js")
        .header(IF_NONE_MATCH, etag)
        .header(ACCEPT_ENCODING, "zstd")
        .body(Body::empty())
        .unwrap();
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::NOT_MODIFIED);
    assert!(parts.headers.get("cache-control").is_none());

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    assert!(collected_body_bytes.is_empty());
}

#[tokio::test]
async fn handles_one_file_different_etag_no_cache_control() {
    let router: Router<()> = Router::new();
    let handler = embed_asset!(
        "../test_fixtures/assets/big/app.js",
        compress = true,
        cache_bust = false
    );
    let router = router.route("/app.js", handler);
    assert!(router.has_routes());

    let request = create_request("/app.js", &Compression::Zstd);

    let response = get_response(router.clone(), request).await;
    let (parts, body) = response.into_parts();

    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    assert!(parts.headers.contains_key("etag"));
    assert!(parts.headers.get("cache-control").is_none());

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();

    // Decompress the response body
    let decompressed_body = decompress_zstd(&collected_body_bytes);
    assert_eq!(
        decompressed_body,
        include_bytes!("../../test_fixtures/assets/big/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/app.js.zst");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);

    let request = Request::builder()
        .uri("/app.js")
        .header(IF_NONE_MATCH, "n0t4r34l3t4g")
        .header(ACCEPT_ENCODING, "zstd")
        .body(Body::empty())
        .unwrap();
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::OK);
    assert_ne!(
        parts
            .headers
            .get("content-length")
            .expect("no content-length header!"),
        "0",
        "content length is unexpectedly zero!"
    );
    assert!(parts.headers.get("cache-control").is_none());

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    assert!(!collected_body_bytes.is_empty());
    let decompressed_body = decompress_zstd(&collected_body_bytes);
    assert_eq!(
        decompressed_body,
        include_bytes!("../../test_fixtures/assets/big/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/app.js.zst");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn using_both_macros_together_works() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = true);
    let handler = embed_asset!("../test_fixtures/assets/dist/ignore_me_plz.txt");
    let router = router.route("/ignore", handler);

    // Get app.js (created by `embed_assets!`)
    let request = create_request("/app.js", &Compression::Zstd);
    let response = get_response(router.clone(), request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get(CONTENT_ENCODING),
        Some(&HeaderValue::from_str("zstd").unwrap())
    );
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();

    // Decompress the response body
    let decompressed_body = decompress_zstd(&collected_body_bytes);
    assert_eq!(
        decompressed_body,
        include_bytes!("../../test_fixtures/assets/big/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/app.js.zst");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);

    // Get `ignore_me_plz.txt` (mapped to `/ignore`, created by `embed_asset!`)
    let request = create_request("/ignore", &Compression::None);

    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();

    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/plain; charset=utf-8"
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/ignore_me_plz.txt");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn handles_one_file_with_cache_control() {
    let router: Router<()> = Router::new();
    let handler = embed_asset!(
        "../test_fixtures/assets/dist/ignore_me_plz.txt",
        compress = true,
        cache_bust = true
    );
    let router = router.route("/ignore", handler);
    assert!(router.has_routes());

    let request = create_request("/ignore", &Compression::None);

    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();

    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/plain; charset=utf-8"
    );
    assert!(
        parts
            .headers
            .get("cache-control")
            .expect("cache-control header should exist")
            .to_str()
            .expect("should make cache-control header into str")
            .contains("immutable")
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/ignore_me_plz.txt");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn handles_one_file_with_cache_control_denied() {
    let router: Router<()> = Router::new();
    let handler = embed_asset!(
        "../test_fixtures/assets/dist/ignore_me_plz.txt",
        compress = true,
        cache_bust = false
    );
    let router = router.route("/ignore", handler);
    assert!(router.has_routes());

    let request = create_request("/ignore", &Compression::None);

    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();

    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/plain; charset=utf-8"
    );
    assert!(parts.headers.get("cache-control").is_none());
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/ignore_me_plz.txt");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn using_both_macros_with_cache_busting_works() {
    let router: Router<()> = embed_assets!(
        "../test_fixtures/assets/big",
        compress = true,
        cache_busted_paths = ["immutable"]
    );
    let handler = embed_asset!(
        "../test_fixtures/assets/dist/ignore_me_plz.txt",
        cache_bust = true
    );
    let router = router.route("/ignore", handler);

    // Get /big/app.js (created by `embed_assets!`, not cache-busted)
    let request = create_request("/app.js", &Compression::Zstd);
    let response = get_response(router.clone(), request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get(CONTENT_ENCODING),
        Some(&HeaderValue::from_str("zstd").unwrap())
    );
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    assert!(parts.headers.contains_key("etag"));
    assert!(parts.headers.get("cache-control").is_none());

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();

    // Decompress the response body
    let decompressed_body = decompress_zstd(&collected_body_bytes);
    assert_eq!(
        decompressed_body,
        include_bytes!("../../test_fixtures/assets/big/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/app.js.zst");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);

    // Get /big/immutable/app.js (created by `embed_assets!`, cache-busted)
    let request = create_request("/immutable/app.js", &Compression::Zstd);
    let response = get_response(router.clone(), request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get(CONTENT_ENCODING),
        Some(&HeaderValue::from_str("zstd").unwrap())
    );
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    assert!(parts.headers.contains_key("etag"));
    assert!(
        parts
            .headers
            .get("cache-control")
            .expect("cache-control header should exist")
            .to_str()
            .expect("should make cache-control header into str")
            .contains("immutable")
    );

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();

    // Decompress the response body
    let decompressed_body = decompress_zstd(&collected_body_bytes);
    assert_eq!(
        decompressed_body,
        include_bytes!("../../test_fixtures/assets/big/immutable/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/app.js.zst");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);

    // Get `ignore_me_plz.txt` (mapped to `/ignore`, created by `embed_asset!`)
    let request = create_request("/ignore", &Compression::None);

    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();

    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/plain; charset=utf-8"
    );
    assert!(
        parts
            .headers
            .get("cache-control")
            .expect("cache-control header should exist")
            .to_str()
            .expect("should make cache-control header into str")
            .contains("immutable")
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/dist/ignore_me_plz.txt");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn handles_dir_with_cache_control_on_single_file() {
    let router: Router<()> = embed_assets!(
        "../test_fixtures/assets/small",
        compress = true,
        cache_busted_paths = ["styles.css"]
    );
    assert!(router.has_routes());

    // styles.css should be immutable / cache-busted
    let request = create_request("/styles.css", &Compression::None);

    let response = get_response(router.clone(), request).await;
    let (parts, body) = response.into_parts();

    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/css; charset=utf-8"
    );
    assert!(
        parts
            .headers
            .get("cache-control")
            .expect("cache-control header should exist")
            .to_str()
            .expect("should make cache-control header into str")
            .contains("immutable")
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/small/styles.css");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);

    // app.js should not be immutable / cache-busted
    let request = create_request("/app.js", &Compression::None);

    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();

    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    assert!(parts.headers.get("cache-control").is_none());
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/small/app.js");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn handles_dir_with_cache_control_on_filename_and_dir() {
    let router: Router<()> = embed_assets!(
        "../test_fixtures/assets/small",
        compress = true,
        cache_busted_paths = [".", "styles.css"]
    );
    assert!(router.has_routes());

    // styles.css should be immutable / cache-busted
    let request = create_request("/styles.css", &Compression::None);

    let response = get_response(router.clone(), request).await;
    let (parts, body) = response.into_parts();

    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/css; charset=utf-8"
    );
    assert!(
        parts
            .headers
            .get("cache-control")
            .expect("cache-control header should exist")
            .to_str()
            .expect("should make cache-control header into str")
            .contains("immutable")
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/small/styles.css");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);

    // app.js should be immutable / cache-busted, too
    let request = create_request("/app.js", &Compression::None);

    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();

    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    assert!(
        parts
            .headers
            .get("cache-control")
            .expect("cache-control header should exist")
            .to_str()
            .expect("should make cache-control header into str")
            .contains("immutable")
    );
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_fixtures/assets/small/app.js");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn router_created_ignore_paths() {
    let router: Router<()> =
        embed_assets!("../test_fixtures/assets/small", ignore_paths = ["app.js"]);

    // app.js should be ignored, but styles.css should be available
    assert!(router.has_routes());

    // Request for styles.css should succeed
    let request = create_request("/styles.css", &Compression::None);
    let response = get_response(router.clone(), request).await;
    let (parts, _) = response.into_parts();
    assert!(parts.status.is_success());

    // Request for app.js should return 404 since it's ignored
    let request = create_request("/app.js", &Compression::None);
    let response = get_response(router, request).await;
    let (parts, _) = response.into_parts();
    assert_eq!(parts.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn router_created_ignore_multiple_files() {
    let router: Router<()> = embed_assets!(
        "../test_fixtures/assets/big",
        ignore_paths = ["app.js", "styles.css"]
    );

    // app.js and styles.css at root should be ignored, but files in /big/immutable should still be available
    assert!(router.has_routes());

    // Request for ignored files should return 404
    let request = create_request("/app.js", &Compression::None);
    let response = get_response(router.clone(), request).await;
    let (parts, _) = response.into_parts();
    assert_eq!(parts.status, StatusCode::NOT_FOUND);

    let request = create_request("/styles.css", &Compression::None);
    let response = get_response(router.clone(), request).await;
    let (parts, _) = response.into_parts();
    assert_eq!(parts.status, StatusCode::NOT_FOUND);

    // Request for files in subdirectory should succeed
    let request = create_request("/immutable/app.js", &Compression::None);
    let response = get_response(router.clone(), request).await;
    let (parts, _) = response.into_parts();
    assert!(parts.status.is_success());

    let request = create_request("/immutable/styles.css", &Compression::None);
    let response = get_response(router, request).await;
    let (parts, _) = response.into_parts();
    assert!(parts.status.is_success());
}

#[tokio::test]
async fn range_request_returns_206_with_partial_body() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/small", compress = false);

    let request = Request::builder()
        .uri("/app.js")
        .header(RANGE, "bytes=0-4")
        .body(Body::empty())
        .unwrap();
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(parts.headers.get("content-range").unwrap(), "bytes 0-4/30");
    assert_eq!(parts.headers.get(ACCEPT_RANGES).unwrap(), "bytes");

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected = &include_bytes!("../../test_fixtures/assets/small/app.js")[..5];
    assert_eq!(*collected_body_bytes, *expected);
}

#[tokio::test]
async fn unsatisfiable_range_returns_416() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/small", compress = false);

    let request = Request::builder()
        .uri("/app.js")
        .header(RANGE, "bytes=100-200")
        .body(Body::empty())
        .unwrap();
    let response = get_response(router, request).await;
    let (parts, _body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::RANGE_NOT_SATISFIABLE);
    assert!(parts.headers.contains_key("content-range"));
    assert_eq!(parts.headers.get(ACCEPT_RANGES).unwrap(), "bytes");
}

#[tokio::test]
async fn accept_ranges_present_on_200() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/small", compress = false);

    let request = create_request("/app.js", &Compression::None);
    let response = get_response(router, request).await;
    let (parts, _body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::OK);
    assert_eq!(parts.headers.get(ACCEPT_RANGES).unwrap(), "bytes");
}

#[tokio::test]
async fn accept_ranges_absent_on_304() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = true);

    // First request to get the etag
    let request = create_request("/app.js", &Compression::None);
    let response = get_response(router.clone(), request).await;
    let etag = response
        .headers()
        .get("etag")
        .expect("should have etag")
        .clone();

    // Second request with matching etag -> 304
    let request = Request::builder()
        .uri("/app.js")
        .header(IF_NONE_MATCH, etag)
        .body(Body::empty())
        .unwrap();
    let response = get_response(router, request).await;
    let (parts, _body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::NOT_MODIFIED);
    assert!(parts.headers.get(ACCEPT_RANGES).is_none());
}

#[tokio::test]
async fn range_is_ignored_when_a_compressed_body_is_selected() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = true);

    let request = create_request("/app.js", &Compression::Zstd);
    let plain = get_response(router.clone(), request).await;
    assert!(!plain.headers().contains_key(ACCEPT_RANGES));
    let etag = plain.headers().get("etag").unwrap().clone();

    let mut request = create_request("/app.js", &Compression::Zstd);
    request
        .headers_mut()
        .insert(RANGE, HeaderValue::from_static("bytes=0-4"));
    let response = get_response(router.clone(), request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get(CONTENT_ENCODING).unwrap(), "zstd");
    assert_eq!(response.headers().get("etag"), Some(&etag));

    // Preconditions see the same representation with and without `Range`
    let mut request = create_request("/app.js", &Compression::Zstd);
    let headers = request.headers_mut();
    headers.insert(RANGE, HeaderValue::from_static("bytes=0-4"));
    headers.insert(IF_NONE_MATCH, etag.clone());
    let response = get_response(router.clone(), request).await;
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);

    let mut request = create_request("/app.js", &Compression::Zstd);
    let headers = request.headers_mut();
    headers.insert(RANGE, HeaderValue::from_static("bytes=0-4"));
    headers.insert(IF_MATCH, etag);
    let response = get_response(router, request).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn if_range_matching_etag_honors_range() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/small", compress = false);

    // First get the etag
    let request = create_request("/app.js", &Compression::None);
    let response = get_response(router.clone(), request).await;
    let etag = response.headers().get("etag").unwrap().clone();

    // Range + If-Range with matching etag -> 206
    let request = Request::builder()
        .uri("/app.js")
        .header(RANGE, "bytes=0-4")
        .header(IF_RANGE, etag)
        .body(Body::empty())
        .unwrap();
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(parts.headers.get("content-range").unwrap(), "bytes 0-4/30");

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected = &include_bytes!("../../test_fixtures/assets/small/app.js")[..5];
    assert_eq!(*collected_body_bytes, *expected);
}

#[tokio::test]
async fn if_range_mismatched_etag_returns_full_body() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/small", compress = false);

    // Range + If-Range with non-matching etag -> 200 with full body
    let request = Request::builder()
        .uri("/app.js")
        .header(RANGE, "bytes=0-4")
        .header(IF_RANGE, "\"stale-etag\"")
        .body(Body::empty())
        .unwrap();
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::OK);
    assert!(parts.headers.get("content-range").is_none());

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected = include_bytes!("../../test_fixtures/assets/small/app.js");
    assert_eq!(*collected_body_bytes, *expected);
}

#[tokio::test]
async fn if_range_mismatched_allows_compression() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = true);

    // Range + stale If-Range + Accept-Encoding -> 200 with compression
    let request = Request::builder()
        .uri("/app.js")
        .header(RANGE, "bytes=0-4")
        .header(IF_RANGE, "\"stale-etag\"")
        .header(ACCEPT_ENCODING, "zstd")
        .body(Body::empty())
        .unwrap();
    let response = get_response(router, request).await;
    let (parts, _body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::OK);
    assert!(parts.headers.get("content-range").is_none());
    assert_eq!(parts.headers.get(CONTENT_ENCODING).unwrap(), "zstd");
}

/// The corresponding failing test is `unknown_extensions_fail_the_build` in
/// static-serve-macro.
#[tokio::test]
async fn serves_unknown_extensions() {
    let router: Router<()> = embed_assets!(
        "../test_fixtures/unknown_extensions",
        allow_unknown_extensions = true
    );
    assert!(router.has_routes());

    // Test `.DS_STORE`
    let request = create_request("/.DS_STORE", &Compression::None);
    let response = get_response(router.clone(), request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "application/octet-stream"
    );

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    assert_eq!(*collected_body_bytes, []);

    // Test `example.wtf`
    let request = create_request("/example.wtf", &Compression::None);
    let response = get_response(router.clone(), request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());
    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "application/octet-stream"
    );

    let collected_body_bytes = body
        .into_data_stream()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    assert_eq!(&collected_body_bytes, &b"example.wtf");
}

#[tokio::test]
async fn allow_unknown_extensions_must_retain_content_type_for_known_extensions() {
    // Given a router embedding assets with `allow_unknown_extensions = true` and a file with known
    // extensions (in this case, `app.js`)
    let router: Router<()> = embed_assets!(
        "../test_fixtures/assets/small",
        allow_unknown_extensions = true
    );

    // When requesting `.app.js` from the router
    let request = create_request("/app.js", &Compression::None);
    let response = get_response(router.clone(), request).await;

    // Then the content type must be `application/javascript`
    assert_eq!(
        "text/javascript",
        response.headers().get("content-type").unwrap()
    );
}

#[tokio::test]
async fn not_modified_keeps_cache_headers_and_drops_content_type() {
    let router: Router<()> = embed_assets!(
        "../test_fixtures/assets/big",
        compress = true,
        cache_busted_paths = ["app.js"]
    );

    let request = create_request("/app.js", &Compression::None);
    let response = get_response(router.clone(), request).await;
    let etag = response.headers().get("etag").unwrap().clone();

    let request = Request::builder()
        .uri("/app.js")
        .header(IF_NONE_MATCH, &etag)
        .body(Body::empty())
        .unwrap();
    let response = get_response(router, request).await;
    let (parts, _body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::NOT_MODIFIED);
    assert_eq!(parts.headers.get("etag"), Some(&etag));
    assert_eq!(parts.headers.get("vary").unwrap(), "Accept-Encoding");
    assert!(parts.headers.get("cache-control").is_some());
    assert!(parts.headers.get("content-type").is_none());
}

#[tokio::test]
async fn if_none_match_handles_wildcard_weak_tags_and_lists() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = true);

    let request = create_request("/app.js", &Compression::None);
    let response = get_response(router.clone(), request).await;
    let etag = response.headers().get("etag").unwrap().to_str().unwrap();

    let matching: [&[String]; 5] = [
        &["*".to_owned()],
        &[format!("W/{etag}")],
        &[format!("\"other\", {etag}")],
        &[format!("\"other\",W/{etag}")],
        &["\"other\"".to_owned(), etag.to_owned()],
    ];
    for values in matching {
        let mut request = Request::builder().uri("/app.js");
        for value in values {
            request = request.header(IF_NONE_MATCH, value);
        }
        let request = request.body(Body::empty()).unwrap();
        let response = get_response(router.clone(), request).await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED, "{values:?}");
    }

    let not_matching = ["\"other\", W/\"another\"", "\"other, *\"", "W/*"];
    for value in not_matching {
        let request = Request::builder()
            .uri("/app.js")
            .header(IF_NONE_MATCH, value)
            .body(Body::empty())
            .unwrap();
        let response = get_response(router.clone(), request).await;
        assert_eq!(response.status(), StatusCode::OK, "{value:?}");
    }
}

#[tokio::test]
async fn accept_encoding_honors_refusals_and_multiple_fields() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = true);

    let cases: [(&[&str], Option<&str>); 19] = [
        (&["gzip;q=0"], None),
        (&["zstd;q=0"], None),
        (&["gzip ; Q=0.000, zstd;q=0."], None),
        (&["zstd;q=0, gzip"], Some("gzip")),
        (&["gzip;q=0.5"], Some("gzip")),
        (&["x-gzip"], Some("gzip")),
        (&["GZIP, ZSTD"], Some("zstd")),
        (&["identity", "gzip"], Some("gzip")),
        (&["notgzip, zstd-ish"], None),
        (&["gzip;q=1, zstd;q=0.1"], Some("gzip")),
        (&["gzip;q=0.1, zstd;q=0.1"], Some("zstd")),
        (&["gzip;q=0.5, identity;q=1"], None),
        (&["gzip, identity"], Some("gzip")),
        (&["*"], Some("zstd")),
        (&["*;q=0"], None),
        (&["gzip;q=0, *"], Some("zstd")),
        (&["*, zstd;q=0"], Some("gzip")),
        (&["*, identity;q=0"], Some("zstd")),
        (&["identity;q=1, *;q=0"], None),
    ];
    for (values, expected) in cases {
        let mut request = Request::builder().uri("/app.js");
        for value in values {
            request = request.header(ACCEPT_ENCODING, *value);
        }
        let request = request.body(Body::empty()).unwrap();
        let response = get_response(router.clone(), request).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(CONTENT_ENCODING)
                .map(|value| value.to_str().unwrap()),
            expected,
            "{values:?}"
        );
    }
}

#[tokio::test]
async fn each_encoding_has_its_own_etag() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = true);

    let mut etags = Vec::new();
    for compression in [Compression::None, Compression::Gzip, Compression::Zstd] {
        let request = create_request("/app.js", &compression);
        let response = get_response(router.clone(), request).await;
        let etag = response.headers().get("etag").unwrap().clone();

        // A revalidation of the same encoding gets the same `ETag` back
        let mut request = create_request("/app.js", &compression);
        request.headers_mut().insert(IF_NONE_MATCH, etag.clone());
        let response = get_response(router.clone(), request).await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(response.headers().get("etag"), Some(&etag));

        // Another encoding is a different representation
        let mut request = create_request("/app.js", &Compression::None);
        if !matches!(compression, Compression::None) {
            request.headers_mut().insert(IF_NONE_MATCH, etag.clone());
            let response = get_response(router.clone(), request).await;
            assert_eq!(response.status(), StatusCode::OK);
        }

        etags.push(etag);
    }

    assert_ne!(etags[0], etags[1]);
    assert_ne!(etags[0], etags[2]);
    assert_ne!(etags[1], etags[2]);
}

#[tokio::test]
async fn if_range_with_compressed_etag_serves_the_full_body() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = true);

    let request = create_request("/app.js", &Compression::Gzip);
    let response = get_response(router.clone(), request).await;
    let etag = response.headers().get("etag").unwrap().clone();

    let mut request = create_request("/app.js", &Compression::Gzip);
    request
        .headers_mut()
        .insert(RANGE, HeaderValue::from_static("bytes=0-1"));
    request.headers_mut().insert(IF_RANGE, etag.clone());
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::OK);
    assert_eq!(parts.headers.get(CONTENT_ENCODING).unwrap(), "gzip");
    assert_eq!(parts.headers.get("etag"), Some(&etag));
    let body = body.into_data_stream().collect().await.unwrap().to_bytes();
    assert_eq!(
        *body,
        *include_bytes!("../../test_fixtures/assets/dist/app.js.gz")
    );
}

#[tokio::test]
async fn if_match_requires_a_strong_match() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = true);

    let request = create_request("/app.js", &Compression::None);
    let response = get_response(router.clone(), request).await;
    let etag = response.headers().get("etag").unwrap().to_str().unwrap();

    let cases = [
        ("*".to_owned(), StatusCode::OK),
        (etag.to_owned(), StatusCode::OK),
        (format!("\"other\", {etag}"), StatusCode::OK),
        ("\"other\"".to_owned(), StatusCode::PRECONDITION_FAILED),
        (format!("W/{etag}"), StatusCode::PRECONDITION_FAILED),
    ];
    for (value, expected) in cases {
        let request = Request::builder()
            .uri("/app.js")
            .header("if-match", &value)
            .body(Body::empty())
            .unwrap();
        let response = get_response(router.clone(), request).await;
        assert_eq!(response.status(), expected, "{value:?}");
    }

    // A failed `If-Match` takes precedence over `If-None-Match`
    let request = Request::builder()
        .uri("/app.js")
        .header("if-match", "\"other\"")
        .header(IF_NONE_MATCH, etag)
        .body(Body::empty())
        .unwrap();
    let response = get_response(router, request).await;
    assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
}

#[tokio::test]
async fn not_modified_describes_the_200_and_has_an_empty_body() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = true);

    let request = create_request("/app.js", &Compression::Gzip);
    let response = get_response(router.clone(), request).await;
    let etag = response.headers().get("etag").unwrap().clone();
    let content_length = response.headers().get("content-length").unwrap().clone();

    for method in [Method::GET, Method::HEAD] {
        let request = Request::builder()
            .method(method)
            .uri("/app.js")
            .header(ACCEPT_ENCODING, "gzip")
            .header(IF_NONE_MATCH, &etag)
            .body(Body::empty())
            .unwrap();
        let response = get_response(router.clone(), request).await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(
            response.headers().get("content-length"),
            Some(&content_length)
        );
        // Compression middleware skips bodies of an exact, small size. A body
        // of unknown size made it compress the 304, which breaks HTTP/2.
        assert_eq!(response.body().size_hint().exact(), Some(0));
    }
}

#[tokio::test]
async fn error_responses_are_not_cacheable_and_have_no_content_type() {
    let router: Router<()> = embed_assets!(
        "../test_fixtures/assets/big",
        cache_busted_paths = ["immutable"]
    );

    for (header, value, status) in [
        (IF_MATCH, "\"stale\"", StatusCode::PRECONDITION_FAILED),
        (RANGE, "bytes=999999-", StatusCode::RANGE_NOT_SATISFIABLE),
    ] {
        let request = Request::builder()
            .uri("/immutable/app.js")
            .header(header, value)
            .body(Body::empty())
            .unwrap();
        let response = get_response(router.clone(), request).await;
        assert_eq!(response.status(), status);
        assert!(!response.headers().contains_key("cache-control"));
        assert!(!response.headers().contains_key("content-type"));
    }
}

#[tokio::test]
async fn entity_tag_containing_a_star_is_not_a_wildcard() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = false);

    for (header, status) in [
        (IF_NONE_MATCH, StatusCode::OK),
        (IF_MATCH, StatusCode::PRECONDITION_FAILED),
    ] {
        let request = Request::builder()
            .uri("/app.js")
            .header(header, "\"a,*,b\"")
            .body(Body::empty())
            .unwrap();
        let response = get_response(router.clone(), request).await;
        assert_eq!(response.status(), status);
    }
}

#[tokio::test]
async fn if_range_that_cant_match_ignores_the_range() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = false);

    let response = get_response(
        router.clone(),
        create_request("/app.js", &Compression::None),
    )
    .await;
    let etag = response.headers().get("etag").unwrap().clone();

    let obs_text = HeaderValue::from_bytes(b"\"caf\xc3\xa9\"").unwrap();
    let empty = HeaderValue::from_static("");
    let cases: [&[&HeaderValue]; 3] = [&[&obs_text], &[&empty], &[&etag, &empty]];
    for values in cases {
        let mut builder = Request::builder().uri("/app.js").header(RANGE, "bytes=0-1");
        for value in values {
            builder = builder.header(IF_RANGE, *value);
        }
        let request = builder.body(Body::empty()).unwrap();
        let response = get_response(router.clone(), request).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn vary_is_only_sent_when_a_compressed_body_exists() {
    {
        let router: Router<()> = embed_assets!("../test_fixtures/assets/small", compress = true);
        let request = create_request("/app.js", &Compression::Gzip);
        let response = get_response(router, request).await;
        assert!(!response.headers().contains_key("vary"));
    }

    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = true);
    let response = get_response(router, create_request("/app.js", &Compression::None)).await;
    assert_eq!(response.headers().get("vary").unwrap(), "Accept-Encoding");
}

#[tokio::test]
async fn symlinks_are_served_at_their_own_path() {
    let router: Router<()> = embed_assets!("../test_fixtures/symlinks");

    for route in ["/real/a.txt", "/alias.txt", "/linked_dir/a.txt"] {
        let response =
            get_response(router.clone(), create_request(route, &Compression::None)).await;
        assert_eq!(response.status(), StatusCode::OK, "{route}");
    }
}

#[tokio::test]
async fn ignoring_a_symlink_only_ignores_the_link() {
    let router: Router<()> = embed_assets!(
        "../test_fixtures/symlinks",
        ignore_paths = ["alias.txt", "linked_dir"]
    );

    for (route, status) in [
        ("/real/a.txt", StatusCode::OK),
        ("/alias.txt", StatusCode::NOT_FOUND),
        ("/linked_dir/a.txt", StatusCode::NOT_FOUND),
    ] {
        let response =
            get_response(router.clone(), create_request(route, &Compression::None)).await;
        assert_eq!(response.status(), status, "{route}");
    }
}

#[tokio::test]
async fn cache_busted_paths_match_symlinks_by_name() {
    let router: Router<()> = embed_assets!(
        "../test_fixtures/symlinks",
        cache_busted_paths = ["linked_dir"]
    );

    let request = create_request("/linked_dir/a.txt", &Compression::None);
    let response = get_response(router.clone(), request).await;
    assert!(response.headers().contains_key("cache-control"));

    let request = create_request("/real/a.txt", &Compression::None);
    let response = get_response(router, request).await;
    assert!(!response.headers().contains_key("cache-control"));
}

#[tokio::test]
async fn dangling_symlinks_can_be_ignored() {
    let router: Router<()> =
        embed_assets!("../test_fixtures/dangling", ignore_paths = ["dangling.txt"]);
    let response = get_response(router, create_request("/ok.txt", &Compression::None)).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn glob_characters_in_the_path_are_literal() {
    let router: Router<()> = embed_assets!("../test_fixtures/glob[1]/assets");
    let response = get_response(router, create_request("/a.txt", &Compression::None)).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn file_names_are_reachable_at_their_encoded_url() {
    let router: Router<()> = embed_assets!("../test_fixtures/url_names");

    for (route, status) in [
        ("/with%20space.txt", StatusCode::OK),
        ("/caf%C3%A9.txt", StatusCode::OK),
        ("/caf%c3%a9.txt", StatusCode::OK),
        ("/100%25.txt", StatusCode::OK),
        ("/%7Bbraces%7D.txt", StatusCode::OK),
        ("/br[1].txt", StatusCode::OK),
        // Must not act as a capture
        ("/anything.txt", StatusCode::NOT_FOUND),
    ] {
        let response =
            get_response(router.clone(), create_request(route, &Compression::None)).await;
        assert_eq!(response.status(), status, "{route}");
    }
}

#[tokio::test]
async fn macro_options_accept_a_trailing_comma() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/small", compress = true,);
    assert!(router.has_routes());
}

#[tokio::test]
async fn head_matches_get_without_a_body() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/big", compress = true);

    let get = get_response(
        router.clone(),
        create_request("/app.js", &Compression::Gzip),
    )
    .await;
    let request = Request::builder()
        .method(Method::HEAD)
        .uri("/app.js")
        .header(ACCEPT_ENCODING, "gzip")
        .header(RANGE, "bytes=0-1")
        .body(Body::empty())
        .unwrap();
    let head = get_response(router, request).await;

    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(head.headers(), get.headers());
    assert!(
        head.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
    );
}

#[tokio::test]
async fn embed_asset_handles_ranges_and_preconditions() {
    let handler = embed_asset!(
        "../test_fixtures/assets/big/app.js",
        compress = true,
        cache_bust = true
    );
    let router: Router<()> = Router::new().route("/app.js", handler);
    let asset = include_bytes!("../../test_fixtures/assets/big/app.js");

    let request = Request::builder()
        .uri("/app.js")
        .header(RANGE, "bytes=-3")
        .body(Body::empty())
        .unwrap();
    let response = get_response(router.clone(), request).await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response.headers().get("cache-control").unwrap(),
        "public, max-age=31536000, immutable"
    );
    assert_eq!(
        response
            .headers()
            .get("content-range")
            .unwrap()
            .to_str()
            .unwrap(),
        format!(
            "bytes {}-{}/{}",
            asset.len() - 3,
            asset.len() - 1,
            asset.len()
        )
    );
    let etag = response.headers().get("etag").unwrap().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(*body, asset[asset.len() - 3..]);

    let request = Request::builder()
        .uri("/app.js")
        .header(IF_MATCH, etag)
        .header(RANGE, "bytes=1-")
        .body(Body::empty())
        .unwrap();
    let response = get_response(router, request).await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
}

#[tokio::test]
async fn unsatisfiable_and_ignored_ranges() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/small");
    let length = include_bytes!("../../test_fixtures/assets/small/app.js").len();

    for (range, status) in [
        ("bytes=-0", StatusCode::RANGE_NOT_SATISFIABLE),
        ("bytes=999-", StatusCode::RANGE_NOT_SATISFIABLE),
        ("bytes=2-1", StatusCode::OK),
        ("bytes=0-1,3-4", StatusCode::OK),
        ("items=0-1", StatusCode::OK),
    ] {
        let request = Request::builder()
            .uri("/app.js")
            .header(RANGE, range)
            .body(Body::empty())
            .unwrap();
        let response = get_response(router.clone(), request).await;
        assert_eq!(response.status(), status, "{range}");
        if status == StatusCode::RANGE_NOT_SATISFIABLE {
            assert_eq!(
                response
                    .headers()
                    .get("content-range")
                    .unwrap()
                    .to_str()
                    .unwrap(),
                format!("bytes */{length}")
            );
        }
    }
}

#[tokio::test]
async fn only_get_and_head_are_allowed() {
    let router: Router<()> = embed_assets!("../test_fixtures/assets/small");
    let request = Request::builder()
        .method(Method::POST)
        .uri("/app.js")
        .body(Body::empty())
        .unwrap();
    let response = get_response(router, request).await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers().get("allow").unwrap(), "GET,HEAD");
}

#[tokio::test]
async fn embed_assets_is_an_expression() {
    let small: Router<()> = embed_assets!("../test_fixtures/assets/small");
    let html: Router<()> = embed_assets!("../test_fixtures/assets/with_html");
    let router = Router::new().nest("/small", small).merge(html);

    for route in ["/small/app.js", "/index.html"] {
        let response =
            get_response(router.clone(), create_request(route, &Compression::None)).await;
        assert_eq!(response.status(), StatusCode::OK, "{route}");
    }
}

#[tokio::test]
async fn utf8_charset_can_be_disabled() {
    let file = embed_asset!(
        "../test_fixtures/assets/dist/ignore_me_plz.txt",
        utf8_charset = false
    );
    let dir: Router<()> = embed_assets!("../test_fixtures/assets/small", utf8_charset = false);
    let router = dir.route("/ignore", file);

    for (route, content_type) in [("/ignore", "text/plain"), ("/styles.css", "text/css")] {
        let response =
            get_response(router.clone(), create_request(route, &Compression::None)).await;
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            content_type,
            "{route}"
        );
    }
}

#[tokio::test]
async fn csp_header_is_read_from_the_meta() {
    let policy = "script-src 'self' 'sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU='; object-src 'none'";
    let file = embed_asset!("../test_fixtures/csp/index.html", csp_header = true);
    let dir: Router<()> = embed_assets!("../test_fixtures/csp", csp_header = true, compress = true);
    let router = dir.route("/single", file);

    for (route, expected) in [
        ("/index.html", Some(policy)),
        ("/single", Some(policy)),
        ("/plain.html", None),
    ] {
        for compression in [Compression::None, Compression::Both] {
            let response = get_response(router.clone(), create_request(route, &compression)).await;
            assert_eq!(response.status(), StatusCode::OK, "{route}");
            assert_eq!(
                response
                    .headers()
                    .get("content-security-policy")
                    .map(|value| value.to_str().unwrap()),
                expected,
                "{route}"
            );
        }
    }
}

#[tokio::test]
async fn csp_header_is_off_by_default() {
    let router: Router<()> = embed_assets!("../test_fixtures/csp");
    let response = get_response(router, create_request("/index.html", &Compression::None)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key("content-security-policy"));
}
