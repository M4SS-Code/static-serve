//! Integration tests for static-serve and macro
use std::io::Read;

use axum::{
    body::Body,
    http::{
        header::{ACCEPT_ENCODING, CONTENT_ENCODING, IF_NONE_MATCH},
        HeaderValue, Request, Response, StatusCode,
    },
    Router,
};
use http_body_util::BodyExt;
use tower::ServiceExt;

use static_serve_macro::embed_assets;

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
    embed_assets!("../static-serve/test_assets/small", compress = false);
    let router: Router<()> = static_router();
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
    let expected_body_bytes = include_bytes!("../../test_assets/small/app.js");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn router_created_uncompressed_because_not_worthwhile() {
    embed_assets!("../static-serve/test_assets/small", compress = true);
    let router: Router<()> = static_router();
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
    let expected_body_bytes = include_bytes!("../../test_assets/small/app.js");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn router_created_compressed_zstd_only() {
    embed_assets!("../static-serve/test_assets/big", compress = true);
    let router: Router<()> = static_router();
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
        include_bytes!("../../test_assets/big/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_assets/dist/app.js.zst");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn router_created_compressed_gzip_only() {
    embed_assets!("../static-serve/test_assets/big", compress = true);
    let router: Router<()> = static_router();
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
        include_bytes!("../../test_assets/big/app.js"),
        "decompressed body is not as expected"
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_assets/dist/app.js.gz");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn router_created_compressed_zstd_or_gzip_accepted() {
    embed_assets!("../static-serve/test_assets/big", compress = true);
    let router: Router<()> = static_router();
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
        include_bytes!("../../test_assets/big/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_assets/dist/app.js.zst");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn router_created_ignore_dirs_one() {
    embed_assets!("../static-serve/test_assets", ignore_dirs = ["dist"]);
    let router: Router<()> = static_router();
    assert!(router.has_routes());

    let request = create_request("/small/app.js", &Compression::None);
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_assets/small/app.js");

    assert_eq!(
        parts.headers.get("content-type").unwrap(),
        "text/javascript"
    );
    assert!(parts.headers.contains_key("etag"));
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn router_created_ignore_dirs_three() {
    embed_assets!(
        "../static-serve/test_assets",
        ignore_dirs = ["big", "small", "dist", "with_html"]
    );
    let router: Router<()> = static_router();
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
    embed_assets!("../static-serve/test_assets/big", compress = true);
    let router: Router<()> = static_router();
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
        include_bytes!("../../test_assets/big/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_assets/dist/app.js.zst");
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
    assert_eq!(
        parts
            .headers
            .get("content-length")
            .expect("no content-length header!"),
        "0"
    );
    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    assert!(collected_body_bytes.is_empty());
}

#[tokio::test]
async fn handles_conditional_requests_different_etag() {
    embed_assets!("../static-serve/test_assets/big", compress = true);
    let router: Router<()> = static_router();
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
        include_bytes!("../../test_assets/big/app.js")
    );

    // Expect the compressed version
    let expected_body_bytes = include_bytes!("../../test_assets/dist/app.js.zst");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn strips_html_correctly() {
    embed_assets!(
        "../static-serve/test_assets/with_html",
        compress = false,
        strip_html_ext = true
    );
    let router: Router<()> = static_router();
    assert!(router.has_routes());

    // Test `index.html`
    let request = create_request("/", &Compression::None);
    let response = get_response(router.clone(), request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());
    assert_eq!(parts.headers.get("content-type").unwrap(), "text/html");
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_assets/with_html/index.html");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);

    // Test `.htm`
    let request = create_request("/index2", &Compression::None);
    let response = get_response(router, request).await;
    let (parts, body) = response.into_parts();
    assert!(parts.status.is_success());
    assert_eq!(parts.headers.get("content-type").unwrap(), "text/html");
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_assets/with_html/index2.htm");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn doesnt_strip_html_when_strip_html_false() {
    embed_assets!(
        "../static-serve/test_assets/with_html",
        compress = false,
        strip_html_ext = false
    );
    let router: Router<()> = static_router();
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
    assert_eq!(parts.headers.get("content-type").unwrap(), "text/html");
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_assets/with_html/index.html");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}

#[tokio::test]
async fn doesnt_strip_html_when_not_specified() {
    embed_assets!("../static-serve/test_assets/with_html", compress = false);
    let router: Router<()> = static_router();
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
    assert_eq!(parts.headers.get("content-type").unwrap(), "text/html");
    assert!(parts.headers.contains_key("etag"));

    let collected_body_bytes = body.into_data_stream().collect().await.unwrap().to_bytes();
    let expected_body_bytes = include_bytes!("../../test_assets/with_html/index.html");
    assert_eq!(*collected_body_bytes, *expected_body_bytes);
}
