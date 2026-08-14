use std::{
    ffi::{OsStr, OsString},
    fmt::{Display, Formatter},
    io,
    path::PathBuf,
};

use glob::{GlobError, PatternError};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("{}", UnknownFileExtension(.0.as_deref()))]
    UnknownFileExtension(Option<OsString>),
    #[error("File extension for file {} is not valid unicode", 0.to_string())]
    InvalidFileExtension(OsString),
    #[error("Cannot canonicalize assets directory {dir}: {error}")]
    CannotCanonicalizeDirectory {
        dir: String,
        #[source]
        error: io::Error,
    },
    #[error("Cannot canonicalize asset file {entry}: {error}")]
    CannotCanonicalizeFile {
        entry: PathBuf,
        #[source]
        error: io::Error,
    },
    #[error("Cannot make file path {0} relative to directory")]
    CannotMakeFileRelative(PathBuf),
    #[error("File path {0} is not utf-8")]
    FilePathIsNotUtf8(PathBuf),
    #[error("Invalid unicode in directory name {0}")]
    InvalidUnicodeInDirectoryName(PathBuf),
    #[error("Cannot canonicalize ignore path {path}: {error}")]
    CannotCanonicalizeIgnorePath {
        path: PathBuf,
        #[source]
        error: io::Error,
    },
    #[error("Error while compressing entry {entry} with gzip: {error}")]
    Gzip {
        entry: PathBuf,
        #[source]
        error: GzipType,
    },
    #[error("Error while compressing entry {entry} with zstd: {error}")]
    Zstd {
        entry: PathBuf,
        #[source]
        error: ZstdType,
    },
    #[error("Error while reading entry {entry} contents: {error}")]
    CannotReadEntryContents {
        entry: PathBuf,
        #[source]
        error: io::Error,
    },
    #[error("Error while parsing glob pattern")]
    Pattern(#[source] PatternError),
    #[error("Error reading path for glob: {0}")]
    Glob(#[source] GlobError),
    #[error("Cannot get entry {entry} metadata: {error}")]
    CannotGetMetadata {
        entry: PathBuf,
        #[source]
        error: io::Error,
    },
    #[error("Cannot canonicalize directory {dir} for cache-busting: {error}")]
    CannotCanonicalizeCacheBustedDir {
        dir: PathBuf,
        #[source]
        error: io::Error,
    },
    #[error("Multiple files map to the same web path {web_path}: {first_file} and {second_file}")]
    DuplicateWebPath {
        web_path: String,
        first_file: String,
        second_file: String,
    },
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
