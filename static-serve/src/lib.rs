#![doc = include_str!("../README.md")]

use std::convert::Infallible;

use axum::{
    extract::FromRequestParts,
    http::{
        header::{
            HeaderValue, ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE, ETAG,
            IF_NONE_MATCH, VARY,
        },
        request::Parts,
        StatusCode,
    },
    response::IntoResponse,
    routing::{get, MethodRouter},
    Router,
};
use bytes::Bytes;

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
/// The router for adding routes for static assets
pub fn static_route<S>(
    router: Router<S>,
    web_path: &'static str,
    content_type: &'static str,
    etag: &'static str,
    body: &'static [u8],
    body_gz: Option<&'static [u8]>,
    body_zst: Option<&'static [u8]>,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.route(
        web_path,
        get(
            move |accept_encoding: AcceptEncoding, if_none_match: IfNoneMatch| async move {
                static_inner(
                    content_type,
                    etag,
                    body,
                    body_gz,
                    body_zst,
                    accept_encoding,
                    &if_none_match,
                )
            },
        ),
    )
}

#[doc(hidden)]
/// Creates a route for a single static asset
pub fn static_method_router(
    content_type: &'static str,
    etag: &'static str,
    body: &'static [u8],
    body_gz: Option<&'static [u8]>,
    body_zst: Option<&'static [u8]>,
) -> MethodRouter {
    MethodRouter::get(
        MethodRouter::new(),
        move |accept_encoding: AcceptEncoding, if_none_match: IfNoneMatch| async move {
            static_inner(
                content_type,
                etag,
                body,
                body_gz,
                body_zst,
                accept_encoding,
                &if_none_match,
            )
        },
    )
}

fn static_inner(
    content_type: &'static str,
    etag: &'static str,
    body: &'static [u8],
    body_gz: Option<&'static [u8]>,
    body_zst: Option<&'static [u8]>,
    accept_encoding: AcceptEncoding,
    if_none_match: &IfNoneMatch,
) -> impl IntoResponse {
    let headers_base = [
        (CONTENT_TYPE, HeaderValue::from_static(content_type)),
        (ETAG, HeaderValue::from_static(etag)),
        (
            CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        ),
        (VARY, HeaderValue::from_static("Accept-Encoding")),
    ];

    match (
        if_none_match.matches(etag),
        accept_encoding.gzip,
        accept_encoding.zstd,
        body_gz,
        body_zst,
    ) {
        (true, _, _, _, _) => (headers_base, StatusCode::NOT_MODIFIED).into_response(),
        (false, _, true, _, Some(body_zst)) => (
            headers_base,
            [(CONTENT_ENCODING, HeaderValue::from_static("zstd"))],
            Bytes::from_static(body_zst),
        )
            .into_response(),
        (false, true, _, Some(body_gz), _) => (
            headers_base,
            [(CONTENT_ENCODING, HeaderValue::from_static("gzip"))],
            Bytes::from_static(body_gz),
        )
            .into_response(),
        _ => (headers_base, Bytes::from_static(body)).into_response(),
    }
}
