#[cfg(feature = "axum")]
use std::convert::Infallible;
use std::{
    cmp::Ordering,
    fmt::{self, Display},
    num::IntErrorKind,
    str::FromStr,
};

use http::HeaderValue;

use crate::headers::{
    InvalidHttpU64, InvalidOrderedRange, OrderedRange, ParseHttpRangeOrContentRangeError, UNIT,
    u64_unprefixed_parse,
};

/// The maximum number of list elements, empty ones included, accepted in the
/// `range-set` of a `Range` header.
///
/// RFC 9110 Section 5.6.1.2 requires recipients to accept "a reasonable
/// number of empty list elements [...] but not so much that they could be
/// used as a denial-of-service mechanism", and Section 17.15 asks servers to
/// ignore or reject egregious range requests.
pub const MAX_RANGE_SET_ELEMENTS: usize = 256;

/// A typed HTTP `Range` header with the `bytes` range unit.
///
/// It holds every `range-spec` of the `range-set`, in the order they appear
/// in the header ([RFC 9110 Section 14.1.1]). There is always at least one.
///
/// [RFC 9110 Section 14.1.1]: https://www.rfc-editor.org/rfc/rfc9110#section-14.1.1
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRange {
    specs: Vec<RangeSpec>,
}

/// A single byte `range-spec` of a `Range` header ([RFC 9110 Section 14.1.2]).
///
/// [RFC 9110 Section 14.1.2]: https://www.rfc-editor.org/rfc/rfc9110#section-14.1.2
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeSpec {
    /// An `int-range` without `last-pos`: `first-pos "-"`.
    StartingPoint(u64),
    /// An `int-range` with `last-pos`: `first-pos "-" last-pos`.
    Range(OrderedRange),
    /// A `suffix-range`: `"-" suffix-length`.
    Suffix(u64),
}

impl HttpRange {
    /// Creates a `Range` header from its `range-spec`s.
    ///
    /// Returns `None` if `specs` is empty, since a `range-set` holds at least
    /// one `range-spec`, or if it holds more than [`MAX_RANGE_SET_ELEMENTS`],
    /// the limit that also applies to parsed headers.
    pub fn new(specs: Vec<RangeSpec>) -> Option<Self> {
        if specs.is_empty() || specs.len() > MAX_RANGE_SET_ELEMENTS {
            None
        } else {
            Some(Self { specs })
        }
    }

    /// Returns the `range-spec`s, in the order they appear in the header.
    pub fn specs(&self) -> &[RangeSpec] {
        &self.specs
    }
}

impl From<RangeSpec> for HttpRange {
    fn from(spec: RangeSpec) -> Self {
        Self { specs: vec![spec] }
    }
}

/// Optional whitespace (RFC 9110 Section 5.6.3): `OWS = *( SP / HTAB )`.
fn is_ows(c: char) -> bool {
    c == ' ' || c == '\t'
}

impl FromStr for HttpRange {
    type Err = ParseHttpRangeOrContentRangeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim_matches(is_ows);
        if s.is_empty() {
            return Err(ParseHttpRangeOrContentRangeError::Empty);
        }

        let (unit_str, range_set_str) = s
            .split_once('=')
            .ok_or(ParseHttpRangeOrContentRangeError::Malformed)?;
        // Range unit names are case-insensitive (RFC 9110 Section 14.1).
        if !unit_str.eq_ignore_ascii_case(UNIT) {
            return Err(ParseHttpRangeOrContentRangeError::InvalidUnit);
        }

        // `range-set = 1#range-spec`, which recipients parse as
        // `[ range-spec ] *( OWS "," OWS [ range-spec ] )`, ignoring empty
        // elements (RFC 9110 Section 5.6.1.2). Whitespace right after the `=`
        // is accepted too, as in the `bytes= 0-999, 4500-5499, -1000` example
        // of RFC 9110 Section 14.1.2.
        let mut specs = Vec::new();
        for (i, element) in range_set_str.split(',').enumerate() {
            if i == MAX_RANGE_SET_ELEMENTS {
                return Err(ParseHttpRangeOrContentRangeError::TooManyRanges);
            }

            let element = element.trim_matches(is_ows);
            if !element.is_empty() {
                specs.push(element.parse()?);
            }
        }

        Self::new(specs).ok_or(ParseHttpRangeOrContentRangeError::MalformedRange)
    }
}

impl FromStr for RangeSpec {
    type Err = ParseHttpRangeOrContentRangeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (first_str, last_str) = s
            .split_once('-')
            .ok_or(ParseHttpRangeOrContentRangeError::MalformedRange)?;

        match (first_str.is_empty(), last_str.is_empty()) {
            (false, false) => {
                let start = parse_position(first_str)?;
                let end = parse_position(last_str)?;

                // Positions past `u64::MAX` saturate, so only compare the
                // digits when both saturated.
                let unordered = start > end
                    || (start == u64::MAX
                        && end == u64::MAX
                        && cmp_decimal(first_str, last_str) == Ordering::Greater);
                if unordered {
                    return Err(InvalidOrderedRange { start, end }.into());
                }

                Ok(Self::Range(OrderedRange { start, end }))
            }
            (false, true) => Ok(Self::StartingPoint(parse_position(first_str)?)),
            (true, false) => Ok(Self::Suffix(parse_position(last_str)?)),
            (true, true) => Err(ParseHttpRangeOrContentRangeError::Malformed),
        }
    }
}

/// Parses a `first-pos`, `last-pos` or `suffix-length` (`1*DIGIT`).
///
/// RFC 9110 Section 14.1.2 requires recipients to "anticipate potentially
/// large decimal numerals and prevent parsing errors due to integer
/// conversion overflows". A value that doesn't fit is saturated to
/// `u64::MAX`, which keeps its meaning since no representation can be
/// longer than `u64::MAX` bytes: as a `first-pos` it is unsatisfiable, as
/// a `last-pos` it gets clamped, and as a `suffix-length` it selects the
/// whole representation.
///
/// Only a value made of digits alone saturates: the integer parser reports
/// an overflow as soon as the digits read so far don't fit, before reaching
/// any invalid character that follows them.
fn parse_position(s: &str) -> Result<u64, ParseHttpRangeOrContentRangeError> {
    match u64_unprefixed_parse(s) {
        Ok(n) => Ok(n),
        Err(InvalidHttpU64::InvalidInt(err))
            if *err.kind() == IntErrorKind::PosOverflow
                && s.bytes().all(|b| b.is_ascii_digit()) =>
        {
            Ok(u64::MAX)
        }
        Err(err) => Err(ParseHttpRangeOrContentRangeError::InvalidRangePiece(err)),
    }
}

/// Compares two strings of ASCII digits by the number they represent.
fn cmp_decimal(a: &str, b: &str) -> Ordering {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

impl From<&HttpRange> for HeaderValue {
    fn from(value: &HttpRange) -> Self {
        HeaderValue::from_maybe_shared(value.to_string())
            .expect("`HttpRange` Display produced non-visible ASCII characters")
    }
}

impl TryFrom<&HeaderValue> for HttpRange {
    type Error = ParseHttpRangeOrContentRangeError;
    fn try_from(value: &HeaderValue) -> Result<Self, Self::Error> {
        value
            .to_str()
            .map_err(|_| ParseHttpRangeOrContentRangeError::ContainsNonVisibleASCII)?
            .parse::<Self>()
    }
}

#[cfg(feature = "axum")]
impl<S> axum_core::extract::OptionalFromRequestParts<S> for HttpRange
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    /// Extracts an optional [`HttpRange`] from the request's `Range` header.
    ///
    /// Per [RFC 9110 Section 14.2], a server must ignore the `Range` header
    /// on methods other than GET and on range units it does not
    /// understand, and may ignore an invalid ranges-specifier. This
    /// extractor returns `Ok(None)` in all such cases, as well as for a
    /// `range-set` with more than [`MAX_RANGE_SET_ELEMENTS`] elements,
    /// instead of rejecting the request, so that the full representation is
    /// served.
    ///
    /// [RFC 9110 Section 14.2]: https://www.rfc-editor.org/rfc/rfc9110#section-14.2
    async fn from_request_parts(
        parts: &mut http::request::Parts,
        _state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        if parts.method != http::Method::GET {
            return Ok(None);
        }

        let range = parts
            .headers
            .get(http::header::RANGE)
            .and_then(|range| HttpRange::try_from(range).ok());
        Ok(range)
    }
}

impl Display for HttpRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{UNIT}=")?;
        for (i, spec) in self.specs.iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            write!(f, "{spec}")?;
        }
        Ok(())
    }
}

impl Display for RangeSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RangeSpec::StartingPoint(start) => write!(f, "{start}-"),
            RangeSpec::Range(range) => write!(f, "{range}"),
            RangeSpec::Suffix(suffix) => write!(f, "-{suffix}"),
        }
    }
}
