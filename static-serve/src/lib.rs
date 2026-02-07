#![doc = include_str!("../README.md")]

use std::convert::Infallible;

use axum::{
    Router,
    extract::FromRequestParts,
    http::{
        StatusCode,
        header::{
            ACCEPT_ENCODING, ACCEPT_RANGES, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE, ETAG,
            HeaderValue, IF_NONE_MATCH, VARY,
        },
        request::Parts,
    },
    response::IntoResponse,
    routing::{MethodRouter, get},
};
use bytes::Bytes;
use range_requests::{headers::range::HttpRange, serve_file_with_http_range};

pub use static_serve_macro::{embed_asset, embed_assets};

/// The accept/reject status for gzip and zstd encoding
#[derive(Debug, Copy, Clone)]
struct AcceptEncoding {
    /// Is gzip accepted?
    pub gzip: bool,
    /// Is zstd accepted?
    pub zstd: bool,
}

impl<S> FromRequestParts<S> for AcceptEncoding
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let accept_encoding = parts.headers.get(ACCEPT_ENCODING);
        let accept_encoding = accept_encoding
            .and_then(|accept_encoding| accept_encoding.to_str().ok())
            .unwrap_or_default();

        Ok(Self {
            gzip: accept_encoding.contains("gzip"),
            zstd: accept_encoding.contains("zstd"),
        })
    }
}

/// Check if the  `IfNoneMatch` header is present
#[derive(Debug)]
struct IfNoneMatch(Option<HeaderValue>);

impl IfNoneMatch {
    /// required function for checking if `IfNoneMatch` is present
    fn matches(&self, etag: &str) -> bool {
        self.0
            .as_ref()
            .is_some_and(|if_none_match| if_none_match.as_bytes() == etag.as_bytes())
    }
}

impl<S> FromRequestParts<S> for IfNoneMatch
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let if_none_match = parts.headers.get(IF_NONE_MATCH).cloned();
        Ok(Self(if_none_match))
    }
}

#[doc(hidden)]
#[expect(clippy::too_many_arguments)]
/// The router for adding routes for static assets
pub fn static_route<S>(
    router: Router<S>,
    web_path: &'static str,
    content_type: &'static str,
    etag: &'static str,
    body: &'static [u8],
    body_gz: Option<&'static [u8]>,
    body_zst: Option<&'static [u8]>,
    cache_busted: bool,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.route(
        web_path,
        get(
            move |accept_encoding: AcceptEncoding,
                  if_none_match: IfNoneMatch,
                  http_range: Option<HttpRange>| async move {
                static_inner(StaticInnerData {
                    content_type,
                    etag,
                    body,
                    body_gz,
                    body_zst,
                    cache_busted,
                    accept_encoding,
                    if_none_match,
                    http_range,
                })
            },
        ),
    )
}

#[doc(hidden)]
/// Creates a route for a single static asset.
///
/// Used by the `embed_asset!` macro, so it needs to be `pub`.
pub fn static_method_router<S>(
    content_type: &'static str,
    etag: &'static str,
    body: &'static [u8],
    body_gz: Option<&'static [u8]>,
    body_zst: Option<&'static [u8]>,
    cache_busted: bool,
) -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
{
    MethodRouter::get(
        MethodRouter::new(),
        move |accept_encoding: AcceptEncoding,
              if_none_match: IfNoneMatch,
              http_range: Option<HttpRange>| async move {
            static_inner(StaticInnerData {
                content_type,
                etag,
                body,
                body_gz,
                body_zst,
                cache_busted,
                accept_encoding,
                if_none_match,
                http_range,
            })
        },
    )
}

/// Struct of parameters for `static_inner` (to avoid `clippy::too_many_arguments`)
///
/// This differs from `StaticRouteData` because it
/// includes the `AcceptEncoding` and `IfNoneMatch` fields
/// and excludes the `web_path`
struct StaticInnerData {
    content_type: &'static str,
    etag: &'static str,
    body: &'static [u8],
    body_gz: Option<&'static [u8]>,
    body_zst: Option<&'static [u8]>,
    cache_busted: bool,
    accept_encoding: AcceptEncoding,
    if_none_match: IfNoneMatch,
    http_range: Option<HttpRange>,
}

fn static_inner(static_inner_data: StaticInnerData) -> impl IntoResponse {
    let StaticInnerData {
        content_type,
        etag,
        body,
        body_gz,
        body_zst,
        cache_busted,
        accept_encoding,
        if_none_match,
        http_range,
    } = static_inner_data;

    let optional_cache_control = if cache_busted {
        Some([(
            CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        )])
    } else {
        None
    };

    let resp_base = (
        [
            (CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (ETAG, HeaderValue::from_static(etag)),
            (VARY, HeaderValue::from_static("Accept-Encoding")),
        ],
        optional_cache_control,
    );

    if if_none_match.matches(etag) {
        return (resp_base, StatusCode::NOT_MODIFIED).into_response();
    }

    let resp_base = (
        [(ACCEPT_RANGES, HeaderValue::from_static("bytes"))],
        resp_base,
    );
    let (selected_body, optional_content_encoding) = match (
        (accept_encoding.gzip, body_gz),
        (accept_encoding.zstd, body_zst),
        &http_range,
    ) {
        (_, (true, Some(body_zst)), None) => (
            Bytes::from_static(body_zst),
            Some([(CONTENT_ENCODING, HeaderValue::from_static("zstd"))]),
        ),
        ((true, Some(body_gz)), _, None) => (
            Bytes::from_static(body_gz),
            Some([(CONTENT_ENCODING, HeaderValue::from_static("gzip"))]),
        ),
        _ => (Bytes::from_static(body), None),
    };

    if selected_body.is_empty() {
        // Empty bodies cannot be range-served; return the full (empty) response.
        return (resp_base, optional_content_encoding, selected_body).into_response();
    }

    match serve_file_with_http_range(selected_body, http_range) {
        Ok(body_range) => (resp_base, optional_content_encoding, body_range).into_response(),
        Err(unsatisfiable) => (resp_base, unsatisfiable).into_response(),
    }
}
