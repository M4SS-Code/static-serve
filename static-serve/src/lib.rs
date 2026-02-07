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
            move |accept_encoding: AcceptEncoding, if_none_match: IfNoneMatch| async move {
                static_inner(StaticInnerData {
                    content_type,
                    etag,
                    body,
                    body_gz,
                    body_zst,
                    cache_busted,
                    accept_encoding,
                    if_none_match,
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
        move |accept_encoding: AcceptEncoding, if_none_match: IfNoneMatch| async move {
            static_inner(StaticInnerData {
                content_type,
                etag,
                body,
                body_gz,
                body_zst,
                cache_busted,
                accept_encoding,
                if_none_match,
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

    match (
        (accept_encoding.gzip, body_gz),
        (accept_encoding.zstd, body_zst),
    ) {
        (_, (true, Some(body_zst))) => (
            resp_base,
            [(CONTENT_ENCODING, HeaderValue::from_static("zstd"))],
            Bytes::from_static(body_zst),
        )
            .into_response(),
        ((true, Some(body_gz)), _) => (
            resp_base,
            [(CONTENT_ENCODING, HeaderValue::from_static("gzip"))],
            Bytes::from_static(body_gz),
        )
            .into_response(),
        _ => (resp_base, Bytes::from_static(body)).into_response(),
    }
}
