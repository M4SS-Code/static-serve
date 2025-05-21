use std::{
    ffi::{OsStr, OsString},
    fmt::{Display, Formatter},
    io,
};

use glob::{GlobError, PatternError};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("{}", UnknownFileExtension(.0.as_deref()))]
    UnknownFileExtension(Option<OsString>),
    #[error("File extension for file {} is not valid unicode", 0.to_string())]
    InvalidFileExtension(OsString),
    #[error("Cannot canonicalize assets directory")]
    CannotCanonicalizeDirectory(#[source] io::Error),
    #[error("Cannot canonicalize asset file")]
    CannotCanonicalizeFile(#[source] io::Error),
    #[error("File path is not utf-8")]
    FilePathIsNotUtf8,
    #[error("Invalid unicode in directory name")]
    InvalidUnicodeInDirectoryName,
    #[error("Cannot canonicalize ignore directory")]
    CannotCanonicalizeIgnoreDir(#[source] io::Error),
    #[error("Invalid unicode in directory name")]
    InvalidUnicodeInEntryName,
    #[error("Error while compressing with gzip")]
    Gzip(#[from] GzipType),
    #[error("Error while compressing with zstd")]
    Zstd(#[from] ZstdType),
    #[error("Error while reading entry contents")]
    CannotReadEntryContents(#[source] io::Error),
    #[error("Error while parsing glob pattern")]
    Pattern(#[source] PatternError),
    #[error("Error reading path for glob")]
    Glob(#[source] GlobError),
    #[error("Cannot get entry metadata")]
    CannotGetMetadata(#[source] io::Error),
    #[error("Cannot canonicalize directory for cache-busting")]
    CannotCanonicalizeCacheBustedDir(#[source] io::Error),
}

struct UnknownFileExtension<'a>(Option<&'a OsStr>);
impl Display for UnknownFileExtension<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Some(ext) => write!(
                f,
                "Unknown file extension in directory of static assets: {}",
                ext.to_string_lossy()
            ),
            None => write!(f, "Missing file extension"),
        }
    }
}

#[derive(Debug, Error)]
pub(crate) enum GzipType {
    #[error("The compressor could not write")]
    CompressorWrite(#[source] io::Error),
    #[error("The encoder could not complete the `finish` procedure")]
    EncoderFinish(#[source] io::Error),
}

#[derive(Debug, Error)]
pub(crate) enum ZstdType {
    #[error("The encoder could not write")]
    EncoderWrite(#[source] io::Error),
    #[error("The encoder could not complete the `finish` procedure")]
    EncoderFinish(#[source] io::Error),
}

#[cfg(test)]
mod test {
    use std::ffi::OsStr;

    use super::UnknownFileExtension;

    #[test]
    fn unknown_file_extension() {
        let missing_extension = UnknownFileExtension(None);
        assert_eq!(missing_extension.to_string(), "Missing file extension");
        let unknown_extension = UnknownFileExtension(Some(OsStr::new("pippo")));
        assert_eq!(
            unknown_extension.to_string(),
            "Unknown file extension in directory of static assets: pippo"
        );
    }
}
