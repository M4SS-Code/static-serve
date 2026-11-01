#![doc = include_str!("../README.md")]

use std::convert::Infallible;

use axum::{
    Router,
    extract::FromRequestParts,
    http::{
        StatusCode,
        header::{
            ACCEPT_ENCODING, ACCEPT_RANGES, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE, ETAG,
            HeaderMap, HeaderValue, IF_MATCH, IF_NONE_MATCH, VARY,
        },
        request::Parts,
    },
    response::IntoResponse,
    routing::MethodRouter,
};
use bytes::Bytes;
use range_requests::{
    headers::{if_range::IfRange, range::HttpRange},
    serve_file_with_http_range,
};

pub use static_serve_macro::{embed_asset, embed_assets};

/// An embedded body and its `ETag`
///
/// The bytes of each encoding differ, so a strong `ETag` must too.
#[doc(hidden)]
#[derive(Debug, Copy, Clone)]
pub struct EmbeddedBody {
    pub bytes: &'static [u8],
    pub etag: &'static str,
}

/// The accept/reject status for gzip and zstd encoding
#[derive(Debug, Copy, Clone)]
struct AcceptEncoding {
    /// Is gzip accepted?
    pub gzip: bool,
    /// Is zstd accepted?
    pub zstd: bool,
}

impl AcceptEncoding {
    fn from_headers(headers: &HeaderMap) -> Self {
        let mut accept_encoding = Self {
            gzip: false,
            zstd: false,
        };

        let codings = headers
            .get_all(ACCEPT_ENCODING)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','));
        for coding in codings {
            let (name, params) = coding.split_once(';').unwrap_or((coding, ""));
            if is_refused(params) {
                continue;
            }

            let name = name.trim();
            if name.eq_ignore_ascii_case("gzip") || name.eq_ignore_ascii_case("x-gzip") {
                accept_encoding.gzip = true;
            } else if name.eq_ignore_ascii_case("zstd") {
                accept_encoding.zstd = true;
            }
        }

        accept_encoding
    }
}

/// Check if the parameters of an `Accept-Encoding` entry set `q` to zero
fn is_refused(params: &str) -> bool {
    params.split(';').any(|param| {
        param.split_once('=').is_some_and(|(name, value)| {
            name.trim().eq_ignore_ascii_case("q")
                && value
                    .trim()
                    .strip_prefix('0')
                    .is_some_and(|rest| rest.trim_start_matches(['.', '0']).is_empty())
        })
    })
}

/// Iterate over the entries of list-based entity-tag header values
///
/// Splitting on commas can't produce a false match against one of our own
/// `ETag`s, even if an entry contains a comma, because a double quote can't
/// appear inside a valid entity-tag.
fn entity_tags(values: &[HeaderValue]) -> impl Iterator<Item = &[u8]> {
    values
        .iter()
        .flat_map(|value| value.as_bytes().split(|&byte| byte == b','))
        .map(<[u8]>::trim_ascii)
}

/// The values of every `If-Match` header in the request
#[derive(Debug)]
struct IfMatch(Vec<HeaderValue>);

impl IfMatch {
    fn from_headers(headers: &HeaderMap) -> Self {
        let if_match = headers.get_all(IF_MATCH);
        Self(if_match.iter().cloned().collect())
    }

    /// Check if the header is absent, or if `etag` is matched by `*` or by an
    /// entry of the list
    ///
    /// `If-Match` uses strong comparison, so `W/` entries never match
    /// (RFC 9110 section 13.1.1).
    fn passes(&self, etag: &str) -> bool {
        self.0.is_empty()
            || entity_tags(&self.0)
                .any(|candidate| candidate == b"*" || candidate == etag.as_bytes())
    }
}

/// The values of every `If-None-Match` header in the request
#[derive(Debug)]
struct IfNoneMatch(Vec<HeaderValue>);

impl IfNoneMatch {
    fn from_headers(headers: &HeaderMap) -> Self {
        let if_none_match = headers.get_all(IF_NONE_MATCH);
        Self(if_none_match.iter().cloned().collect())
    }

    /// Check if `etag` is matched by `*` or by an entry of the list
    ///
    /// `If-None-Match` uses weak comparison, so a `W/` prefix is ignored
    /// (RFC 9110 section 13.1.2).
    fn matches(&self, etag: &str) -> bool {
        entity_tags(&self.0).any(|candidate| {
            candidate == b"*"
                || candidate.strip_prefix(b"W/").unwrap_or(candidate) == etag.as_bytes()
        })
    }
}

/// Everything a request can ask of a static asset
#[derive(Debug)]
struct StaticRequest {
    accept_encoding: AcceptEncoding,
    if_match: IfMatch,
    if_none_match: IfNoneMatch,
    http_range: Option<HttpRange>,
    if_range: Option<IfRange>,
}

impl<S> FromRequestParts<S> for StaticRequest
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self {
            accept_encoding: AcceptEncoding::from_headers(&parts.headers),
            if_match: IfMatch::from_headers(&parts.headers),
            if_none_match: IfNoneMatch::from_headers(&parts.headers),
            http_range: Option::from_request_parts(parts, state).await?,
            if_range: Option::from_request_parts(parts, state).await?,
        })
    }
}

#[doc(hidden)]
/// The router for adding routes for static assets
pub fn static_route<S>(
    router: Router<S>,
    web_path: &'static str,
    content_type: &'static str,
    body: EmbeddedBody,
    body_gz: Option<EmbeddedBody>,
    body_zst: Option<EmbeddedBody>,
    cache_busted: bool,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.route(
        web_path,
        static_method_router(content_type, body, body_gz, body_zst, cache_busted),
    )
}

#[doc(hidden)]
/// Creates a route for a single static asset.
///
/// Used by the `embed_asset!` macro, so it needs to be `pub`.
pub fn static_method_router<S>(
    content_type: &'static str,
    body: EmbeddedBody,
    body_gz: Option<EmbeddedBody>,
    body_zst: Option<EmbeddedBody>,
    cache_busted: bool,
) -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
{
    MethodRouter::get(
        MethodRouter::new(),
        move |request: StaticRequest| async move {
            static_inner(content_type, body, body_gz, body_zst, cache_busted, request)
        },
    )
}

fn static_inner(
    content_type: &'static str,
    body: EmbeddedBody,
    body_gz: Option<EmbeddedBody>,
    body_zst: Option<EmbeddedBody>,
    cache_busted: bool,
    request: StaticRequest,
) -> impl IntoResponse {
    let StaticRequest {
        accept_encoding,
        if_match,
        if_none_match,
        http_range,
        if_range,
    } = request;

    let optional_cache_control = if cache_busted {
        Some([(
            CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        )])
    } else {
        None
    };

    // Ranges always apply to the uncompressed body, so an `If-Range` holding
    // the `ETag` of a compressed body must not match.
    let http_range = match (http_range, if_range) {
        (Some(range), Some(if_range)) => {
            let etag_value = HeaderValue::from_static(body.etag);
            if_range.evaluate(range, None, Some(&etag_value))
        }
        (range, _) => range,
    };

    let (selected, optional_content_encoding) = match (
        (accept_encoding.gzip, body_gz),
        (accept_encoding.zstd, body_zst),
        &http_range,
    ) {
        (_, (true, Some(body_zst)), None) => (
            body_zst,
            Some([(CONTENT_ENCODING, HeaderValue::from_static("zstd"))]),
        ),
        ((true, Some(body_gz)), _, None) => (
            body_gz,
            Some([(CONTENT_ENCODING, HeaderValue::from_static("gzip"))]),
        ),
        _ => (body, None),
    };
    let selected_body = Bytes::from_static(selected.bytes);

    let resp_base = (
        [
            (ETAG, HeaderValue::from_static(selected.etag)),
            (VARY, HeaderValue::from_static("Accept-Encoding")),
        ],
        optional_cache_control,
    );

    if !if_match.passes(selected.etag) {
        return (resp_base, StatusCode::PRECONDITION_FAILED).into_response();
    }

    if if_none_match.matches(selected.etag) {
        return (resp_base, StatusCode::NOT_MODIFIED).into_response();
    }

    let resp_base = (
        [
            (CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (ACCEPT_RANGES, HeaderValue::from_static("bytes")),
        ],
        resp_base,
    );

    match serve_file_with_http_range(selected_body, http_range) {
        Ok(body_range) => (resp_base, optional_content_encoding, body_range).into_response(),
        Err(unsatisfiable) => (resp_base, unsatisfiable).into_response(),
    }
}
