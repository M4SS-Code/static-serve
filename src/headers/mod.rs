use std::{
    fmt::{self, Display},
    num::ParseIntError,
    ops::RangeInclusive,
};

pub mod content_range;
pub mod if_range;
pub mod range;
#[cfg(test)]
mod tests;

const UNIT: &str = "bytes";

/// The Errors that may occur during [`HttpContentRange`] and [`HttpRange`] parsing.
///
/// [`HttpRange`]: crate::headers::range::HttpRange
/// [`HttpContentRange`]: crate::headers::content_range::HttpContentRange
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseHttpRangeOrContentRangeError {
    #[error("The header value is malformed")]
    Malformed,
    #[error("Contains nonvisible ASCII")]
    ContainsNonVisibleASCII,
    #[error("Empty header value")]
    Empty,
    #[error("Invalid unit")]
    InvalidUnit,
    #[error("Invalid range value")]
    MalformedRange,
    #[error(transparent)]
    UnorderedRange(#[from] InvalidOrderedRange),
    #[error("Invalid range piece")]
    InvalidRangePiece(#[source] InvalidHttpU64),
    #[error("Invalid size value")]
    InvalidSize(#[source] InvalidHttpU64),
    #[error("Too many ranges")]
    TooManyRanges,
}

#[cfg(feature = "axum")]
impl axum_core::response::IntoResponse for ParseHttpRangeOrContentRangeError {
    fn into_response(self) -> axum_core::response::Response {
        http::StatusCode::BAD_REQUEST.into_response()
    }
}

/// An error that may occur when parsing header values that are u64.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidHttpU64 {
    #[error("{0} has a sign as prefix, so it can't be parsed as an unprefixed int")]
    HasSignPrefix(String),
    #[error(transparent)]
    InvalidInt(#[from] ParseIntError),
}

/// An error that may occur when creating an [`OrderedRange`] with `start` > `end`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("The provided `start`: {start} is greater than `end`: {end}")]
pub struct InvalidOrderedRange {
    start: u64,
    end: u64,
}

/// An ordered range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderedRange {
    start: u64,
    end: u64,
}

impl OrderedRange {
    pub fn new(range: RangeInclusive<u64>) -> Result<Self, InvalidOrderedRange> {
        let start = *range.start();
        let end = *range.end();

        if start > end {
            return Err(InvalidOrderedRange { start, end });
        }

        Ok(Self { start, end })
    }

    /// Returns the inclusive starting point of the range.
    pub fn start(&self) -> u64 {
        self.start
    }

    /// Returns the inclusive ending point of the range.
    pub fn end(&self) -> u64 {
        self.end
    }
}

impl Display for OrderedRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.start(), self.end())
    }
}

pub(crate) fn u64_unprefixed_parse(s: &str) -> Result<u64, InvalidHttpU64> {
    if s.starts_with("+") {
        Err(InvalidHttpU64::HasSignPrefix(s.to_owned()))
    } else {
        Ok(s.parse::<u64>()?)
    }
}
