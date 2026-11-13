use std::{io, path::PathBuf};

use thiserror::Error;

use crate::csp::CspError;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("Unknown file extension of {0}")]
    UnknownFileExtension(PathBuf),
    #[error("Missing file extension of {0}")]
    MissingFileExtension(PathBuf),
    #[error("Cannot canonicalize assets directory {dir}")]
    CannotCanonicalizeDirectory {
        dir: String,
        #[source]
        error: io::Error,
    },
    #[error("Cannot canonicalize asset file {entry}")]
    CannotCanonicalizeFile {
        entry: PathBuf,
        #[source]
        error: io::Error,
    },
    #[error("Cannot make file path {0} relative to directory")]
    CannotMakeFileRelative(PathBuf),
    #[error("File path {0} is not utf-8")]
    FilePathIsNotUtf8(PathBuf),
    #[error(
        "Web path {web_path} of {file} can't be a route: axum rejects segments starting with `:` or `*`. Rename the file or add it to `ignore_paths`"
    )]
    UnsupportedRouteSegment { web_path: String, file: String },
    #[error("Path {0} is outside of the assets directory")]
    PathOutsideAssetsDir(PathBuf),
    #[error("{0} is not a regular file")]
    NotARegularFile(PathBuf),
    #[error("Error while compressing entry {entry} with gzip")]
    Gzip {
        entry: PathBuf,
        #[source]
        error: GzipType,
    },
    #[error("Error while compressing entry {entry} with zstd")]
    Zstd {
        entry: PathBuf,
        #[source]
        error: ZstdType,
    },
    #[error("Error while reading entry {entry} contents")]
    CannotReadEntryContents {
        entry: PathBuf,
        #[source]
        error: io::Error,
    },
    #[error("Cannot read directory {dir}")]
    CannotReadDirectory {
        dir: PathBuf,
        #[source]
        error: io::Error,
    },
    #[error("Cannot get entry {entry} metadata")]
    CannotGetMetadata {
        entry: PathBuf,
        #[source]
        error: io::Error,
    },
    #[error("Cannot move the Content Security Policy of {entry} to a header")]
    ContentSecurityPolicy {
        entry: PathBuf,
        #[source]
        error: CspError,
    },
    #[error("Multiple files map to the same web path {web_path}: {first_file} and {second_file}")]
    DuplicateWebPath {
        web_path: String,
        first_file: String,
        second_file: String,
    },
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
