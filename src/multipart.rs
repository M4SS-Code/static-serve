//! `multipart/byteranges` responses ([RFC 9110 Section 14.6]), enabled by
//! the `multipart` feature.
//!
//! [RFC 9110 Section 14.6]: https://www.rfc-editor.org/rfc/rfc9110#section-14.6

use std::{
    convert::Infallible,
    io::Write as _,
    ops::Range,
    pin::Pin,
    task::{Context, Poll},
    vec,
};

use bytes::Bytes;
use http::{HeaderValue, StatusCode};
use http_body::{Frame, SizeHint};
use rand::Rng as _;

use crate::{
    ContentRange, UnsatisfiableRange,
    headers::{content_range::HttpContentRange, range::HttpRange},
    selected_ranges,
};

/// The most parts served in a `multipart/byteranges` response; a `Range`
/// header with more ranges left after coalescing is ignored (RFC 9110
/// Sections 14.2 and 17.15).
const MAX_PARTS: usize = 32;

/// Returns a [`MultipartBodyRange`] if the provided [`HttpRange`] is satisfiable, otherwise it returns [`UnsatisfiableRange`].
///
/// Unlike [`serve_file_with_http_range`], which ignores a `Range` header
/// asking for several disjoint ranges, this serves them as
/// `multipart/byteranges` content ([RFC 9110 Section 14.6]). The ranges are
/// selected as described in [`file_range_multipart`].
///
/// `content_type` is the `Content-Type` of the representation. It is the
/// `Content-Type` of the response when the whole representation or a
/// single part is sent, and of each part of a `multipart/byteranges`
/// response, whose own `Content-Type` carries the boundary of the body
/// ([RFC 9110 Section 15.3.7.2]). Pass it here rather than setting it on
/// the response, where it would replace the `multipart/byteranges` one.
/// Pass `None` only if the media type is unknown, in which case neither
/// the response nor its parts have a `Content-Type` ([RFC 9110 Section
/// 8.3]).
///
/// The body is never copied: every range is sent as a slice of `body`.
///
/// # Panics
///
/// The boundary of a `multipart/byteranges` response is generated with
/// `rand`'s thread-local random number generator, which panics if the
/// operating system's random number generator fails when seeding or
/// reseeding it.
///
/// [`HttpRange`]: crate::headers::range::HttpRange
/// [`serve_file_with_http_range`]: crate::serve_file_with_http_range
/// [RFC 9110 Section 8.3]: https://www.rfc-editor.org/rfc/rfc9110#section-8.3
/// [RFC 9110 Section 14.6]: https://www.rfc-editor.org/rfc/rfc9110#section-14.6
/// [RFC 9110 Section 15.3.7.2]: https://www.rfc-editor.org/rfc/rfc9110#section-15.3.7.2
pub fn serve_file_with_http_range_multipart(
    body: Bytes,
    http_range: Option<HttpRange>,
    content_type: Option<HeaderValue>,
) -> Result<MultipartBodyRange, UnsatisfiableRange> {
    let size = u64::try_from(body.len()).expect("we do not support 128bit usize");

    Ok(match file_range_multipart(size, http_range)? {
        FileRanges::Full => MultipartBodyRange {
            status: StatusCode::OK,
            header: None,
            content_type,
            body: MultipartBody::single(body),
        },
        FileRanges::Single(part) => MultipartBodyRange {
            status: StatusCode::PARTIAL_CONTENT,
            header: part.header,
            content_type,
            body: MultipartBody::single(slice(&body, &part.range)),
        },
        FileRanges::Multipart(parts) => {
            let boundary = Boundary::random();
            let multipart_content_type = HeaderValue::from_maybe_shared(Bytes::from(format!(
                "multipart/byteranges; boundary={}",
                boundary.as_str()
            )))
            .expect("the boundary only contains visible ASCII");

            MultipartBodyRange {
                status: StatusCode::PARTIAL_CONTENT,
                header: None,
                content_type: Some(multipart_content_type),
                body: MultipartBody::multipart(body, parts, content_type, boundary),
            }
        }
    })
}

/// Returns the [`FileRanges`] to send if the provided [`HttpRange`] is satisfiable, otherwise it returns [`UnsatisfiableRange`].
///
/// The ranges are selected as described in [`file_range`], except that
/// when several disjoint ranges remain after coalescing they are all
/// selected, in the order they were requested ([`FileRanges::Multipart`]),
/// instead of the whole representation. A single requested range is never
/// selected as [`FileRanges::Multipart`], since a server "MUST NOT generate
/// a multipart response to a request for a single range" ([RFC 9110
/// Section 15.3.7.2]).
///
/// If more than 32 ranges remain, the `Range` header is still ignored and
/// the whole representation selected: a server "MAY ignore the Range
/// header field" ([RFC 9110 Section 14.2]) and "ought to ignore, coalesce,
/// or reject egregious range requests, such as [...] many small ranges in
/// a single set" ([RFC 9110 Section 17.15]).
///
/// [`HttpRange`]: crate::headers::range::HttpRange
/// [`file_range`]: crate::file_range
/// [RFC 9110 Section 14.2]: https://www.rfc-editor.org/rfc/rfc9110#section-14.2
/// [RFC 9110 Section 15.3.7.2]: https://www.rfc-editor.org/rfc/rfc9110#section-15.3.7.2
/// [RFC 9110 Section 17.15]: https://www.rfc-editor.org/rfc/rfc9110#section-17.15
pub fn file_range_multipart(
    size: u64,
    http_range: Option<HttpRange>,
) -> Result<FileRanges, UnsatisfiableRange> {
    let mut ranges = selected_ranges(size, http_range)?;
    Ok(match ranges.len() {
        1 => FileRanges::Single(ContentRange::part(ranges.pop().unwrap(), size)),
        2..=MAX_PARTS => FileRanges::Multipart(
            ranges
                .into_iter()
                .map(|range| ContentRange::part(range, size))
                .collect(),
        ),
        _ => FileRanges::Full,
    })
}

/// The ranges of a representation selected by [`file_range_multipart`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileRanges {
    /// Send the whole representation, `0..size`, in a 200 (OK) response.
    Full,
    /// Send a single part in a 206 (Partial Content) response with its
    /// `Content-Range` header ([RFC 9110 Section 15.3.7.1]).
    ///
    /// [RFC 9110 Section 15.3.7.1]: https://www.rfc-editor.org/rfc/rfc9110#section-15.3.7.1
    Single(ContentRange),
    /// Send two to 32 parts, in this order, in a 206 (Partial Content)
    /// response with `multipart/byteranges` content ([RFC 9110 Section
    /// 15.3.7.2]).
    ///
    /// The response has a `Content-Type: multipart/byteranges;
    /// boundary=...` header and no `Content-Range` header. Each body part
    /// has its own `Content-Range` header and, if the representation has
    /// one, its `Content-Type` ([RFC 9110 Section 14.6]). The boundary must
    /// not appear within any part ([RFC 2046 Section 5.1.1]).
    ///
    /// [RFC 2046 Section 5.1.1]: https://www.rfc-editor.org/rfc/rfc2046#section-5.1.1
    /// [RFC 9110 Section 14.6]: https://www.rfc-editor.org/rfc/rfc9110#section-14.6
    /// [RFC 9110 Section 15.3.7.2]: https://www.rfc-editor.org/rfc/rfc9110#section-15.3.7.2
    Multipart(Vec<ContentRange>),
}

/// A response to a range request, which may hold several ranges as
/// `multipart/byteranges` content.
///
/// If the `axum` feature is enabled this struct also implements
/// `IntoResponse`, which sets the [`status`], the `Content-Range`
/// [`header`] and the [`content_type`]. Don't override the `Content-Type`
/// of that response, since a `multipart/byteranges` one carries the
/// boundary of the body.
///
/// [`status`]: MultipartBodyRange::status
/// [`header`]: MultipartBodyRange::header
/// [`content_type`]: MultipartBodyRange::content_type
#[derive(Debug, Clone)]
pub struct MultipartBodyRange {
    status: StatusCode,
    header: Option<HttpContentRange>,
    content_type: Option<HeaderValue>,
    body: MultipartBody,
}

impl MultipartBodyRange {
    /// Returns the status code of the response: 200 (OK) if the whole
    /// representation is sent, 206 (Partial Content) otherwise.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Returns the `Content-Range` header of a single part 206 (Partial
    /// Content) response.
    ///
    /// It is `None` if the whole representation is sent, and for a
    /// `multipart/byteranges` response, which "MUST NOT generate a
    /// Content-Range header field in the HTTP header section" ([RFC 9110
    /// Section 15.3.7.2]).
    ///
    /// [RFC 9110 Section 15.3.7.2]: https://www.rfc-editor.org/rfc/rfc9110#section-15.3.7.2
    pub fn header(&self) -> Option<HttpContentRange> {
        self.header
    }

    /// Returns the `Content-Type` header of the response.
    ///
    /// It is `multipart/byteranges` with its boundary for a multipart
    /// response, and the `content_type` given to
    /// [`serve_file_with_http_range_multipart`] otherwise.
    pub fn content_type(&self) -> Option<&HeaderValue> {
        self.content_type.as_ref()
    }

    /// Returns the body of the response.
    pub fn into_body(self) -> MultipartBody {
        self.body
    }
}

/// The body of a [`MultipartBodyRange`], streamed as an
/// [`http_body::Body`].
///
/// It is the whole representation or a single range of it, sent as one
/// frame, or `multipart/byteranges` content. The latter is produced while
/// it is sent: the delimiter and header section of each part are generated
/// when that part is reached, and the parts themselves are slices of the
/// representation, so it is never copied and the number of allocations
/// made upfront doesn't depend on the number of parts. Its exact length is
/// computed arithmetically upfront, so the response isn't chunked.
#[derive(Debug, Clone)]
pub struct MultipartBody {
    /// The frame to send next: the whole body if it is a single frame, or
    /// the content of the part whose header section was just sent.
    next: Option<Bytes>,
    /// The parts that are yet to be sent, or `None` once the
    /// close-delimiter has been sent or if the body is a single frame.
    multipart: Option<Multipart>,
    /// The number of bytes that are yet to be sent.
    remaining: u64,
}

#[derive(Debug, Clone)]
struct Multipart {
    representation: Bytes,
    parts: vec::IntoIter<ContentRange>,
    content_type: Option<HeaderValue>,
    boundary: Boundary,
    first: bool,
}

impl MultipartBody {
    fn single(body: Bytes) -> Self {
        Self {
            remaining: body.len() as u64,
            next: Some(body).filter(|body| !body.is_empty()),
            multipart: None,
        }
    }

    fn multipart(
        representation: Bytes,
        parts: Vec<ContentRange>,
        content_type: Option<HeaderValue>,
        boundary: Boundary,
    ) -> Self {
        let multipart = Multipart {
            representation,
            parts: parts.into_iter(),
            content_type,
            boundary,
            first: true,
        };
        let parts_len = multipart
            .parts
            .as_slice()
            .iter()
            .enumerate()
            .map(|(i, part)| {
                multipart.headers_len(part, i == 0) + (part.range.end - part.range.start)
            })
            .sum::<u64>();

        Self {
            next: None,
            multipart: Some(multipart),
            remaining: parts_len + CLOSE_DELIMITER_LEN,
        }
    }
}

// multipart-body := dash-boundary CRLF body-part *encapsulation
//                   close-delimiter [CRLF epilogue]
// encapsulation  := delimiter CRLF body-part
// delimiter      := CRLF dash-boundary
// dash-boundary  := "--" boundary
// close-delimiter := delimiter "--"
// (RFC 2046 Section 5.1.1)

/// The length of the close-delimiter followed by the CRLF preceding an
/// empty epilogue.
const CLOSE_DELIMITER_LEN: u64 = ("\r\n--".len() + BOUNDARY_LEN + "--\r\n".len()) as u64;

impl Multipart {
    /// Returns the length of what [`headers`] returns, without generating
    /// it.
    ///
    /// [`headers`]: Multipart::headers
    fn headers_len(&self, part: &ContentRange, first: bool) -> u64 {
        let delimiter =
            if first { 0 } else { "\r\n".len() } + "--".len() + BOUNDARY_LEN + "\r\n".len();
        let content_type = self.content_type.as_ref().map_or(0, |content_type| {
            "Content-Type: ".len() + content_type.len() + "\r\n".len()
        });
        let content_range = "Content-Range: bytes ".len()
            + digits(part.range.start)
            + "-".len()
            + digits(part.range.end - 1)
            + "/".len()
            + digits(self.representation.len() as u64)
            + "\r\n\r\n".len();
        (delimiter + content_type + content_range) as u64
    }

    /// Returns the delimiter preceding `part` and the header section of
    /// `part`, including the empty line that ends it.
    fn headers(&self, part: &ContentRange, first: bool) -> Bytes {
        let len = self.headers_len(part, first);
        let mut headers = Vec::with_capacity(len as usize);
        if !first {
            headers.extend_from_slice(b"\r\n");
        }
        headers.extend_from_slice(b"--");
        headers.extend_from_slice(self.boundary.as_str().as_bytes());
        headers.extend_from_slice(b"\r\n");

        // "each with its own Content-Type and Content-Range fields"
        // (RFC 9110 Section 14.6)
        if let Some(content_type) = &self.content_type {
            headers.extend_from_slice(b"Content-Type: ");
            headers.extend_from_slice(content_type.as_bytes());
            headers.extend_from_slice(b"\r\n");
        }
        write!(
            headers,
            "Content-Range: bytes {}-{}/{}\r\n\r\n",
            part.range.start,
            part.range.end - 1,
            self.representation.len()
        )
        .expect("writing to a Vec never fails");

        debug_assert_eq!(headers.len() as u64, len);
        Bytes::from(headers)
    }

    /// Returns the close-delimiter, followed by the CRLF preceding an
    /// empty epilogue.
    fn close_delimiter(&self) -> Bytes {
        let mut close = Vec::with_capacity(CLOSE_DELIMITER_LEN as usize);
        close.extend_from_slice(b"\r\n--");
        close.extend_from_slice(self.boundary.as_str().as_bytes());
        close.extend_from_slice(b"--\r\n");
        Bytes::from(close)
    }
}

impl http_body::Body for MultipartBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        let frame = if let Some(next) = this.next.take() {
            next
        } else if let Some(multipart) = &mut this.multipart {
            match multipart.parts.next() {
                Some(part) => {
                    let headers = multipart.headers(&part, multipart.first);
                    multipart.first = false;
                    this.next = Some(slice(&multipart.representation, &part.range));
                    headers
                }
                None => {
                    let close = multipart.close_delimiter();
                    this.multipart = None;
                    close
                }
            }
        } else {
            return Poll::Ready(None);
        };

        this.remaining -= frame.len() as u64;
        Poll::Ready(Some(Ok(Frame::data(frame))))
    }

    fn is_end_stream(&self) -> bool {
        self.next.is_none() && self.multipart.is_none()
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.remaining)
    }
}

/// The length of a boundary: 128 random bits, in hexadecimal.
const BOUNDARY_LEN: usize = 32;

/// A random boundary of `multipart/byteranges` content.
///
/// "Boundary delimiters must not appear within the encapsulated material"
/// (RFC 2046 Section 5.1.1). Rather than scanning every part for the
/// boundary, which would cost time proportional to the size of the parts
/// before anything can be sent, the boundary holds 128 random bits, which
/// RFC 2046 allows: "The boundary parameter value in the example above
/// could have been the result of an algorithm designed to produce boundary
/// delimiters with a very low probability of already existing in the data
/// to be encapsulated without having to prescan the data."
///
/// The bits come from the thread-local CSPRNG of `rand`, which is seeded,
/// and periodically reseeded, from the operating system, so that
/// generating a boundary doesn't cost a system call. It isn't reseeded
/// when the process forks, so processes forked from one that used it
/// generate the same boundaries until they reseed: that doesn't matter,
/// since a boundary only has to be absent from the parts, not unique.
///
/// Hexadecimal digits are `bcharsnospace` characters (RFC 2046 Section
/// 5.1.1) and `tchar`s (RFC 9110 Section 5.6.2), so the boundary is never
/// quoted, since "some existing implementations handle a quoted boundary
/// string incorrectly" (RFC 9110 Section 14.6).
#[derive(Debug, Clone, Copy)]
struct Boundary([u8; BOUNDARY_LEN]);

impl Boundary {
    fn random() -> Self {
        const HEX: &[u8; 16] = b"0123456789abcdef";

        let mut random = [0; BOUNDARY_LEN / 2];
        rand::rng().fill_bytes(&mut random);

        let mut boundary = [0; BOUNDARY_LEN];
        for (hex, byte) in boundary.as_chunks_mut::<2>().0.iter_mut().zip(random) {
            *hex = [HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 0xf)]];
        }
        Self(boundary)
    }

    fn as_str(&self) -> &str {
        str::from_utf8(&self.0).expect("the boundary is hexadecimal")
    }
}

/// Returns the number of decimal digits of `n`.
fn digits(n: u64) -> usize {
    n.checked_ilog10().map_or(1, |log| log as usize + 1)
}

fn slice(body: &Bytes, range: &Range<u64>) -> Bytes {
    let start = usize::try_from(range.start).expect("u64 doesn't fit usize");
    let end = usize::try_from(range.end).expect("u64 doesn't fit usize");
    body.slice(start..end)
}

#[cfg(feature = "axum")]
mod axum {
    use axum_core::{
        body::Body,
        response::{IntoResponse, Response},
    };
    use http::{
        HeaderValue,
        header::{CONTENT_RANGE, CONTENT_TYPE},
    };

    use super::MultipartBodyRange;

    impl IntoResponse for MultipartBodyRange {
        fn into_response(self) -> Response {
            let content_range = self
                .header
                .map(|range| [(CONTENT_RANGE, HeaderValue::from(&range))]);
            let content_type = self
                .content_type
                .map(|content_type| [(CONTENT_TYPE, content_type)]);
            // A `Body` rather than `Bytes`, which would add a
            // `Content-Type: application/octet-stream` when the media type
            // is unknown, unlike the parts of a multipart response.
            let body = Body::new(self.body);
            (self.status, content_range, content_type, body).into_response()
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{
        pin::Pin,
        task::{Context, Poll, Waker},
    };

    use bytes::Bytes;
    use http::HeaderValue;
    use http_body::Body;

    use super::{Boundary, MultipartBody, digits};
    use crate::ContentRange;

    /// Collects the frames of `body`, checking that its exact size is
    /// known upfront and after every frame.
    pub(crate) fn collect<B>(mut body: B) -> (Vec<Bytes>, Vec<u8>)
    where
        B: Body<Data = Bytes> + Unpin,
        B::Error: std::fmt::Debug,
    {
        let mut frames = Vec::new();
        let mut collected = Vec::new();
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            let remaining = body.size_hint().exact().unwrap();
            assert_eq!(body.is_end_stream(), remaining == 0);
            match Pin::new(&mut body).poll_frame(&mut cx) {
                Poll::Ready(Some(frame)) => {
                    let frame = frame.unwrap().into_data().unwrap();
                    assert!(!frame.is_empty());
                    assert_eq!(
                        body.size_hint().exact().unwrap(),
                        remaining - frame.len() as u64
                    );
                    collected.extend_from_slice(&frame);
                    frames.push(frame);
                }
                Poll::Ready(None) => {
                    assert_eq!(remaining, 0);
                    return (frames, collected);
                }
                Poll::Pending => unreachable!("the body never waits"),
            }
        }
    }

    /// `boundary := 0*69<bchars> bcharsnospace` (RFC 2046 Section 5.1.1),
    /// restricted to the characters that are also `tchar`s (RFC 9110
    /// Section 5.6.2).
    pub(crate) fn is_valid_boundary(boundary: &str) -> bool {
        (1..=70).contains(&boundary.len())
            && boundary
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"'+_-.".contains(&b))
    }

    fn parts(ranges: &[(u64, u64)], size: u64) -> Vec<ContentRange> {
        ranges
            .iter()
            .map(|&(start, end)| ContentRange::part(start..end + 1, size))
            .collect()
    }

    #[test]
    fn layout() {
        let body = Bytes::from_static(b"a\r\n--other\r\nb.................................c");
        let boundary = Boundary::random();
        let multipart = MultipartBody::multipart(
            body,
            parts(&[(0, 12), (46, 46)], 47),
            Some(HeaderValue::from_static("text/plain")),
            boundary,
        );

        let boundary = boundary.as_str();
        assert_eq!(
            collect(multipart).1,
            format!(
                "--{boundary}\r\n\
                 Content-Type: text/plain\r\n\
                 Content-Range: bytes 0-12/47\r\n\
                 \r\n\
                 a\r\n--other\r\nb\r\n\
                 --{boundary}\r\n\
                 Content-Type: text/plain\r\n\
                 Content-Range: bytes 46-46/47\r\n\
                 \r\n\
                 c\r\n\
                 --{boundary}--\r\n"
            )
            .as_bytes()
        );
    }

    #[test]
    fn parts_are_streamed_without_copying_the_body() {
        let size = 1 << 20;
        let body = Bytes::from(vec![b'a'; size]);
        let size = size as u64;
        let multipart = MultipartBody::multipart(
            body.clone(),
            parts(&[(0, 0), (100, size - 1)], size),
            None,
            Boundary::random(),
        );

        let (frames, _) = collect(multipart);
        assert_eq!(frames.len(), 5);
        assert_eq!(frames[1].as_ptr(), body.as_ptr());
        assert_eq!(frames[3].as_ptr(), body[100..].as_ptr());
        assert_eq!(frames[3].len(), body.len() - 100);
        assert!(frames[0].len() + frames[2].len() + frames[4].len() < 1024);
    }

    #[test]
    fn size_hint_is_exact_for_any_number_of_parts() {
        for content_type in [None, Some(HeaderValue::from_static("text/plain"))] {
            for size in [1, 2, 9, 10, 11, 99, 100, 1000, 12_345, 1 << 20] {
                for count in 1..=64 {
                    // Ranges of different widths and positions, some
                    // crossing powers of ten, in a scrambled order.
                    let ranges = (0..count)
                        .map(|i| (i * 7919) % count)
                        .filter_map(|i| {
                            let start = size * i / count;
                            let end = (size * (i + 1) / count).min(start + i % 3 + 1);
                            if start < end {
                                Some((start, end - 1))
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>();
                    let multipart = MultipartBody::multipart(
                        Bytes::from(vec![b'x'; size as usize]),
                        parts(&ranges, size),
                        content_type.clone(),
                        Boundary::random(),
                    );
                    let expected = multipart.size_hint().exact().unwrap();
                    let (frames, collected) = collect(multipart);
                    assert_eq!(collected.len() as u64, expected);
                    assert_eq!(frames.len(), ranges.len() * 2 + 1);
                }
            }
        }
    }

    #[test]
    fn single_frame() {
        let body = Bytes::from_static(b"hello");
        let (frames, collected) = collect(MultipartBody::single(body.clone()));
        assert_eq!(frames, [body]);
        assert_eq!(collected, b"hello");

        let (frames, _) = collect(MultipartBody::single(Bytes::new()));
        assert!(frames.is_empty());
    }

    #[test]
    fn random_boundaries_are_valid_and_differ() {
        let boundary = Boundary::random();
        assert!(is_valid_boundary(boundary.as_str()));
        assert_eq!(boundary.as_str().len(), 32);
        assert_ne!(boundary.as_str(), Boundary::random().as_str());
    }

    #[test]
    fn boundary_validation() {
        assert!(is_valid_boundary("a"));
        assert!(is_valid_boundary(&"a".repeat(70)));
        assert!(!is_valid_boundary(""));
        assert!(!is_valid_boundary(&"a".repeat(71)));
        assert!(!is_valid_boundary("a b"));
        assert!(!is_valid_boundary("a:b"));
    }

    #[test]
    fn digit_count() {
        for n in [0, 1, 9, 10, 99, 100, 12_345, u64::MAX] {
            assert_eq!(digits(n), n.to_string().len());
        }
    }
}
