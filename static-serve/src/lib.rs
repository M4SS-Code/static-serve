#![doc = include_str!("../README.md")]

use std::convert::Infallible;

use axum::{
    Router,
    extract::FromRequestParts,
    http::{
        StatusCode,
        header::{
            ACCEPT_ENCODING, ACCEPT_RANGES, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_LENGTH,
            CONTENT_TYPE, ETAG, HeaderMap, HeaderValue, IF_MATCH, IF_NONE_MATCH, IF_RANGE, VARY,
        },
        request::Parts,
    },
    response::IntoResponse,
    routing::MethodRouter,
};
use bytes::Bytes;
use range_requests::{headers::range::HttpRange, serve_file_with_http_range};

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

/// The weights, in thousandths, listed in `Accept-Encoding`
#[derive(Debug, Copy, Clone, Default)]
struct AcceptEncoding {
    gzip: Option<u16>,
    zstd: Option<u16>,
    identity: Option<u16>,
    wildcard: Option<u16>,
}

impl AcceptEncoding {
    /// Select the most preferred of the compressed bodies, if any is acceptable
    /// and not less preferred than an explicitly listed `identity`
    ///
    /// Ties prefer zstd over gzip, and either over `identity`. An unlisted
    /// `identity` doesn't compete, otherwise `gzip;q=0.5` would be refused.
    fn select(
        self,
        body_gz: Option<EmbeddedBody>,
        body_zst: Option<EmbeddedBody>,
    ) -> Option<(EmbeddedBody, &'static str)> {
        let weight = |coding: Option<u16>| coding.or(self.wildcard).unwrap_or(0);
        [
            (weight(self.gzip), body_gz, "gzip"),
            (weight(self.zstd), body_zst, "zstd"),
        ]
        .into_iter()
        .filter(|&(weight, _, _)| weight > 0 && weight >= self.identity.unwrap_or(0))
        .filter_map(|(weight, body, coding)| Some((weight, body?, coding)))
        .max_by_key(|&(weight, _, _)| weight)
        .map(|(_, body, coding)| (body, coding))
    }
}

impl AcceptEncoding {
    fn from_headers(headers: &HeaderMap) -> Self {
        let mut accept_encoding = Self::default();

        let codings = headers
            .get_all(ACCEPT_ENCODING)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','));
        for coding in codings {
            let (name, params) = coding.split_once(';').unwrap_or((coding, ""));
            let name = name.trim();

            let field = if name.eq_ignore_ascii_case("gzip") || name.eq_ignore_ascii_case("x-gzip")
            {
                &mut accept_encoding.gzip
            } else if name.eq_ignore_ascii_case("zstd") {
                &mut accept_encoding.zstd
            } else if name.eq_ignore_ascii_case("identity") {
                &mut accept_encoding.identity
            } else if name == "*" {
                &mut accept_encoding.wildcard
            } else {
                continue;
            };
            *field = (*field).max(Some(weight(params)));
        }

        accept_encoding
    }
}

/// Read the weight of an `Accept-Encoding` entry in thousandths
///
/// An entry without a valid `q` parameter counts as fully accepted.
fn weight(params: &str) -> u16 {
    params
        .split(';')
        .filter_map(|param| param.split_once('='))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("q"))
        .and_then(|(_, value)| parse_qvalue(value.trim()))
        .unwrap_or(1000)
}

/// Parse a `qvalue` (RFC 9110 section 12.4.2) into thousandths
fn parse_qvalue(value: &str) -> Option<u16> {
    let (integer, fraction) = value.split_once('.').unwrap_or((value, ""));
    if fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    match integer {
        "0" => format!("{fraction:0<3}").parse().ok(),
        "1" if fraction.bytes().all(|byte| byte == b'0') => Some(1000),
        _ => None,
    }
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

/// Check if a field is the `*` wildcard
///
/// `*` is only valid as a whole field value. Looking for it among the entries
/// of [`entity_tags`] would also find it in a tag like `"a,*,b"`.
fn is_wildcard(values: &[HeaderValue]) -> bool {
    values
        .iter()
        .any(|value| value.as_bytes().trim_ascii() == b"*")
}

/// The values of the conditional headers of a request
#[derive(Debug)]
struct Conditions {
    matching: Vec<HeaderValue>,
    not_matching: Vec<HeaderValue>,
    range_validator: Vec<HeaderValue>,
}

impl Conditions {
    fn from_headers(headers: &HeaderMap) -> Self {
        let values = |name| headers.get_all(name).iter().cloned().collect();
        Self {
            matching: values(IF_MATCH),
            not_matching: values(IF_NONE_MATCH),
            range_validator: values(IF_RANGE),
        }
    }

    /// Check if `If-Match` is absent, or matches `etag`
    ///
    /// `If-Match` uses strong comparison, so `W/` entries never match
    /// (RFC 9110 section 13.1.1).
    fn if_match_passes(&self, etag: &str) -> bool {
        self.matching.is_empty()
            || is_wildcard(&self.matching)
            || entity_tags(&self.matching).any(|candidate| candidate == etag.as_bytes())
    }

    /// Check if `If-None-Match` matches `etag`
    ///
    /// `If-None-Match` uses weak comparison, so a `W/` prefix is ignored
    /// (RFC 9110 section 13.1.2).
    fn if_none_match_matches(&self, etag: &str) -> bool {
        is_wildcard(&self.not_matching)
            || entity_tags(&self.not_matching).any(|candidate| {
                candidate.strip_prefix(b"W/").unwrap_or(candidate) == etag.as_bytes()
            })
    }

    /// Check if `If-Range` is absent, or holds exactly `etag`
    ///
    /// Bytes are compared directly since any other value is false: the
    /// comparison is strong, and we don't send `Last-Modified` for a date to
    /// match. A value we can't interpret must fail too, not count as absent,
    /// because RFC 9110 section 13.1.5 only allows a range if it matches.
    fn if_range_passes(&self, etag: &str) -> bool {
        match self.range_validator.as_slice() {
            [] => true,
            [value] => value.as_bytes().trim_ascii() == etag.as_bytes(),
            _ => false,
        }
    }
}

/// Everything a request can ask of a static asset
#[derive(Debug)]
struct StaticRequest {
    accept_encoding: AcceptEncoding,
    conditions: Conditions,
    http_range: Option<HttpRange>,
}

impl<S> FromRequestParts<S> for StaticRequest
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self {
            accept_encoding: AcceptEncoding::from_headers(&parts.headers),
            conditions: Conditions::from_headers(&parts.headers),
            http_range: Option::from_request_parts(parts, state).await?,
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
        conditions,
        http_range,
    } = request;

    let optional_cache_control = if cache_busted {
        Some([(
            CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        )])
    } else {
        None
    };

    let has_compressed_body = body_gz.is_some() || body_zst.is_some();
    let (selected, optional_content_encoding) = match accept_encoding.select(body_gz, body_zst) {
        Some((compressed, coding)) => (
            compressed,
            Some([(CONTENT_ENCODING, HeaderValue::from_static(coding))]),
        ),
        None => (body, None),
    };
    let selected_body = Bytes::from_static(selected.bytes);

    // Without a compressed body the response doesn't depend on `Accept-Encoding`,
    // and listing it would only split the entries of shared caches.
    let validators = (
        [(ETAG, HeaderValue::from_static(selected.etag))],
        has_compressed_body.then_some([(VARY, HeaderValue::from_static("Accept-Encoding"))]),
    );

    // Cache-Control stays off 412 and 416: without it they can't be stored,
    // while with it a cache could answer later requests with the empty error.
    if !conditions.if_match_passes(selected.etag) {
        return (validators, StatusCode::PRECONDITION_FAILED).into_response();
    }

    if conditions.if_none_match_matches(selected.etag) {
        // The explicit length keeps axum from deriving `Content-Length: 0`
        // from the empty body, which would misdescribe the 200 response
        // (RFC 9110 section 8.6). Middleware reads the exact body size of 0,
        // so it won't try to compress the 304.
        let content_length = HeaderValue::from(selected_body.len());
        return (
            StatusCode::NOT_MODIFIED,
            [(CONTENT_LENGTH, content_length)],
            (validators, optional_cache_control),
            (),
        )
            .into_response();
    }

    // Only the uncompressed body can be ranged: a slice of a compressed stream
    // can't be decoded on its own, and answering with uncompressed bytes under
    // a different `ETag` would make the outcome of the preconditions above
    // depend on `Range`. RFC 9110 section 14.2 lets us ignore `Range` instead.
    let is_identity = optional_content_encoding.is_none();
    let http_range =
        http_range.filter(|_| is_identity && conditions.if_range_passes(selected.etag));
    let accept_ranges = is_identity.then_some([(ACCEPT_RANGES, HeaderValue::from_static("bytes"))]);

    match serve_file_with_http_range(selected_body, http_range) {
        Ok(body_range) => (
            (
                validators,
                optional_cache_control,
                accept_ranges,
                [(CONTENT_TYPE, HeaderValue::from_static(content_type))],
                optional_content_encoding,
            ),
            body_range,
        )
            .into_response(),
        Err(unsatisfiable) => (validators, accept_ranges, unsatisfiable).into_response(),
    }
}
