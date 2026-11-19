#![cfg_attr(docsrs, feature(doc_cfg))]

use std::ops::Range;

use bytes::Bytes;

pub mod headers;

use crate::headers::{
    content_range::{Bound, HttpContentRange, Unsatisfiable},
    range::{HttpRange, RangeSpec},
};

/// Returns a [`BodyRange`] of [`Bytes`] if the provided [`HttpRange`] is satisfiable, otherwise it returns [`UnsatisfiableRange`].
///
/// The range is selected as described in [`file_range`].
///
/// [`HttpRange`]: crate::headers::range::HttpRange
pub fn serve_file_with_http_range(
    body: Bytes,
    http_range: Option<HttpRange>,
) -> Result<BodyRange<Bytes>, UnsatisfiableRange> {
    let size = u64::try_from(body.len()).expect("we do not support 128bit usize");

    let content_range = file_range(size, http_range)?;

    let start = usize::try_from(content_range.range.start).expect("u64 doesn't fit usize");
    let end = usize::try_from(content_range.range.end).expect("u64 doesn't fit usize");

    Ok(BodyRange {
        body: body.slice(start..end),
        header: content_range.header,
    })
}

/// Returns a [`ContentRange`] if the provided [`HttpRange`] is satisfiable, otherwise it returns [`UnsatisfiableRange`].
///
/// Following [RFC 9110 Section 14.2]:
///
/// - without a [`HttpRange`] the whole representation is selected;
/// - if none of the requested ranges is satisfiable ([RFC 9110 Section
///   14.1.2]), [`UnsatisfiableRange`] is returned, to be sent as a 416
///   (Range Not Satisfiable) response;
/// - satisfiable ranges that overlap, or that are separated by a gap
///   smaller than the overhead of sending them as separate parts, are
///   coalesced ([RFC 9110 Section 15.3.7.2]); if a single range remains,
///   it is selected, to be sent as a 206 (Partial Content) response;
/// - if several disjoint ranges remain, the `Range` header is ignored and
///   the whole representation is selected, to be sent as a 200 (OK)
///   response, since this crate can't generate the
///   `multipart/byteranges` content they would need;
/// - if the representation is empty and the ranges are satisfiable, the
///   `Range` header is ignored and the whole (empty) representation is
///   selected, since the `Content-Range` of a 206 can't describe an empty
///   range.
///
/// [`HttpRange`]: crate::headers::range::HttpRange
/// [RFC 9110 Section 14.1.2]: https://www.rfc-editor.org/rfc/rfc9110#section-14.1.2
/// [RFC 9110 Section 14.2]: https://www.rfc-editor.org/rfc/rfc9110#section-14.2
/// [RFC 9110 Section 15.3.7.2]: https://www.rfc-editor.org/rfc/rfc9110#section-15.3.7.2
pub fn file_range(
    size: u64,
    http_range: Option<HttpRange>,
) -> Result<ContentRange, UnsatisfiableRange> {
    let full = ContentRange {
        header: None,
        range: 0..size,
    };
    let Some(http_range) = http_range else {
        return Ok(full);
    };

    let ranges = satisfiable_ranges(size, &http_range);
    if ranges.is_empty() {
        let content_range = HttpContentRange::Unsatisfiable(Unsatisfiable::new(size));
        return Err(UnsatisfiableRange(content_range));
    }

    // A non-zero suffix-range is satisfiable even when the representation
    // is empty (RFC 9110 Section 14.1.2), but the `Content-Range` of a 206
    // cannot be expressed for an empty body. Ignore the range and serve the
    // full (empty) representation instead, as permitted by RFC 9110
    // Section 14.2.
    if size == 0 {
        return Ok(full);
    }

    match coalesce(ranges).as_slice() {
        [range] => {
            let content_range = HttpContentRange::Bound(
                Bound::new(range.start..=range.end - 1, Some(size)).unwrap(),
            );
            Ok(ContentRange {
                header: Some(content_range),
                range: range.clone(),
            })
        }
        _ => Ok(full),
    }
}

/// Resolves the satisfiable `range-spec`s of `http_range` against a
/// representation of `size` bytes, in request order, dropping the
/// unsatisfiable ones (RFC 9110 Section 14.1.2).
///
/// A returned range is only empty when `size` is zero.
fn satisfiable_ranges(size: u64, http_range: &HttpRange) -> Vec<Range<u64>> {
    http_range
        .specs()
        .iter()
        .filter_map(|spec| match *spec {
            RangeSpec::StartingPoint(start) if start < size => Some(start..size),
            RangeSpec::Range(range) if range.start() < size => {
                Some(range.start()..range.end().saturating_add(1).min(size))
            }
            RangeSpec::Suffix(suffix) if suffix > 0 => Some(size.saturating_sub(suffix)..size),
            _ => None,
        })
        .collect()
}

/// The fewest bytes that sending a range as one more part of a
/// `multipart/byteranges` body can cost: the `CRLF "--" boundary CRLF`
/// delimiter with a one character boundary, a `Content-Range` header
/// field with single digit positions and complete length, and the empty
/// line ending the part's header section (RFC 9110 Section 14.6, RFC 2046
/// Section 5.1.1).
const MIN_PART_OVERHEAD: u64 =
    ("\r\n--".len() + 1 + "\r\n".len() + "Content-Range: bytes 0-0/1\r\n".len() + "\r\n".len())
        as u64;

/// Coalesces the ranges that overlap, or that are separated by a gap
/// smaller than the overhead of sending multiple parts, as allowed by RFC
/// 9110 Section 15.3.7.2.
///
/// Every coalesced range takes the place of the first of its ranges in
/// request order, since a server "SHOULD send the parts in the same order
/// that the corresponding range-spec appeared in the received Range header
/// field, excluding those ranges that were deemed unsatisfiable or that
/// were coalesced into other ranges".
fn coalesce(ranges: Vec<Range<u64>>) -> Vec<Range<u64>> {
    let mut by_start = ranges.into_iter().enumerate().collect::<Vec<_>>();
    by_start.sort_by_key(|(i, range)| (range.start, *i));

    let mut coalesced: Vec<(usize, Range<u64>)> = Vec::with_capacity(by_start.len());
    for (i, range) in by_start {
        match coalesced.last_mut() {
            Some((first, last)) if range.start < last.end.saturating_add(MIN_PART_OVERHEAD) => {
                *first = (*first).min(i);
                last.end = last.end.max(range.end);
            }
            _ => coalesced.push((i, range)),
        }
    }

    coalesced.sort_by_key(|(first, _)| *first);
    coalesced.into_iter().map(|(_, range)| range).collect()
}

/// A container for the payload slice and the optional `Content-Range` header.
///
/// The header is `None` only if the body was not sliced.
///
/// If the `axum` feature is enabled this struct also implements `IntoResponse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyRange<T> {
    body: T,
    header: Option<HttpContentRange>,
}

impl<T> BodyRange<T> {
    /// Returns the sliced body.
    pub fn body(&self) -> &T {
        &self.body
    }

    pub fn into_body(self) -> T {
        self.body
    }

    /// Returns an option of [`HttpContentRange`].
    /// It is `None` if no range was applied to the body: either no
    /// [`HttpRange`] was provided, or it was ignored as described in
    /// [`file_range`].
    pub fn header(&self) -> Option<HttpContentRange> {
        self.header
    }
}

/// A container for the payload range and the optional `Content-Range` header.
///
/// The header is `None` only if the body was not sliced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentRange {
    header: Option<HttpContentRange>,
    range: Range<u64>,
}

impl ContentRange {
    /// Returns an option of [`HttpContentRange`].
    /// It is `None` if no range was applied to the body: either no
    /// [`HttpRange`] was provided, or it was ignored as described in
    /// [`file_range`].
    pub fn header(&self) -> Option<HttpContentRange> {
        self.header
    }

    /// Returns a [`Range`] of `u64` useful to manually slice the response body.
    pub fn range(&self) -> &Range<u64> {
        &self.range
    }
}

/// An unsatisfiable range request.
///
/// If the `axum` feature is enabled this struct also implements `IntoResponse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsatisfiableRange(HttpContentRange);

impl UnsatisfiableRange {
    /// Returns the [`HttpContentRange`] header.
    pub fn header(&self) -> HttpContentRange {
        self.0
    }
}

#[cfg(feature = "axum")]
mod axum {
    use crate::{BodyRange, UnsatisfiableRange};

    use axum_core::response::{IntoResponse, Response};
    use bytes::Bytes;
    use http::{HeaderValue, StatusCode, header::CONTENT_RANGE};

    impl IntoResponse for BodyRange<Bytes> {
        fn into_response(self) -> Response {
            match self.header {
                Some(range) => (
                    StatusCode::PARTIAL_CONTENT,
                    [(CONTENT_RANGE, HeaderValue::from(&range))],
                    self.body,
                )
                    .into_response(),
                None => (StatusCode::OK, self.body).into_response(),
            }
        }
    }

    impl IntoResponse for UnsatisfiableRange {
        fn into_response(self) -> Response {
            (
                StatusCode::RANGE_NOT_SATISFIABLE,
                [(CONTENT_RANGE, HeaderValue::from(&self.0))],
            )
                .into_response()
        }
    }
}
