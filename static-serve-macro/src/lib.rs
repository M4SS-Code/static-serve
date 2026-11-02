//! Proc macro crate for compressing and embedding static assets
//! in a web server

use std::{
    collections::HashMap,
    env, fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

use display_full_error::DisplayFullError;
use flate2::write::GzEncoder;
use mime::Mime;
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use sha2::{Digest as _, Sha256};
use syn::{
    Ident, LitBool, LitByteStr, LitStr, Token, bracketed,
    parse::{Parse, ParseStream},
    parse_macro_input,
};

mod error;
use error::{Error, GzipType, ZstdType};

#[proc_macro]
/// Embed and optionally compress a directory of static assets for a web server
///
/// Generates a `pub fn static_router<S>() -> axum::Router<S>` in the current
/// scope, with one route per file. The path is relative to the directory
/// containing `Cargo.toml`.
///
/// The options are `compress`, `ignore_paths`, `strip_html_ext`,
/// `cache_busted_paths` and `allow_unknown_extensions`. See the
/// [`static-serve` documentation](https://docs.rs/static-serve) for details.
pub fn embed_assets(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let parsed = parse_macro_input!(input as EmbedAssets);
    quote! { #parsed }.into()
}

#[proc_macro]
/// Embed and optionally compress a single static asset for a web server
///
/// Evaluates to an `axum::routing::MethodRouter` serving GET and HEAD. The path
/// is relative to the directory containing `Cargo.toml`.
///
/// The options are `compress`, `cache_bust` and `allow_unknown_extensions`. See
/// the [`static-serve` documentation](https://docs.rs/static-serve) for details.
pub fn embed_asset(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let parsed = parse_macro_input!(input as EmbedAsset);
    quote! { #parsed }.into()
}

/// Resolve a path given to a macro against the directory of `Cargo.toml`
///
/// The working directory of rustc is the workspace root for workspace members
/// and the package root otherwise, so it would give the same path different
/// meanings. It's only used if the crate isn't being built by Cargo.
fn resolve_path(path: &str) -> PathBuf {
    env::var_os("CARGO_MANIFEST_DIR").map_or_else(
        || PathBuf::from(path),
        |manifest_dir| Path::new(&manifest_dir).join(path),
    )
}

/// Store a macro option, rejecting a repeated key instead of silently keeping
/// the last value
fn set_once<T>(option: &mut Option<T>, value: T, key: &Ident) -> syn::Result<()> {
    if option.replace(value).is_some() {
        return Err(syn::Error::new(
            key.span(),
            format!("Duplicate key `{key}` in macro options"),
        ));
    }

    Ok(())
}

struct EmbedAsset {
    asset_file: AssetFile,
    should_compress: ShouldCompress,
    cache_busted: IsCacheBusted,
    allow_unknown_extensions: LitBool,
}

struct AssetFile(LitStr);

impl Parse for EmbedAsset {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let asset_file: AssetFile = input.parse()?;

        // Default to no compression, no cache-busting
        let mut maybe_should_compress = None;
        let mut maybe_is_cache_busted = None;
        let mut maybe_allow_unknown_extensions = None;

        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }

            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            match key.to_string().as_str() {
                "compress" => {
                    set_once(&mut maybe_should_compress, input.parse()?, &key)?;
                }
                "cache_bust" => {
                    set_once(&mut maybe_is_cache_busted, input.parse()?, &key)?;
                }
                "allow_unknown_extensions" => {
                    set_once(&mut maybe_allow_unknown_extensions, input.parse()?, &key)?;
                }
                _ => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!(
                            "Unknown key in `embed_asset!` macro. Expected `compress`, `cache_bust`, or `allow_unknown_extensions` but got {key}"
                        ),
                    ));
                }
            }
        }
        let should_compress = maybe_should_compress.unwrap_or_else(|| {
            ShouldCompress(LitBool {
                value: false,
                span: Span::call_site(),
            })
        });
        let cache_busted = maybe_is_cache_busted.unwrap_or_else(|| {
            IsCacheBusted(LitBool {
                value: false,
                span: Span::call_site(),
            })
        });
        let allow_unknown_extensions = maybe_allow_unknown_extensions.unwrap_or(LitBool {
            value: false,
            span: Span::call_site(),
        });

        Ok(Self {
            asset_file,
            should_compress,
            cache_busted,
            allow_unknown_extensions,
        })
    }
}

impl Parse for AssetFile {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let input_span = input.span();
        let asset_file: LitStr = input.parse()?;
        let literal = asset_file.value();
        let path = resolve_path(&literal);
        let metadata = match fs::metadata(&path) {
            Ok(meta) => meta,
            Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound) => {
                return Err(syn::Error::new(
                    input_span,
                    format!(
                        "The specified asset file ({}) does not exist. Relative paths start at the directory of Cargo.toml.",
                        path.display()
                    ),
                ));
            }
            Err(e) => {
                return Err(syn::Error::new(
                    input_span,
                    format!("Error reading file {literal}: {}", DisplayFullError(&e)),
                ));
            }
        };

        if metadata.is_dir() {
            return Err(syn::Error::new(
                input_span,
                "The specified asset is a directory, not a file. Did you mean to call `embed_assets!` instead?",
            ));
        }

        Ok(AssetFile(asset_file))
    }
}

impl ToTokens for EmbedAsset {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let AssetFile(asset_file) = &self.asset_file;
        let ShouldCompress(should_compress) = &self.should_compress;
        let IsCacheBusted(cache_busted) = &self.cache_busted;
        let allow_unknown_extensions = &self.allow_unknown_extensions;

        let result = generate_static_handler(
            asset_file,
            should_compress,
            cache_busted,
            allow_unknown_extensions,
        );

        match result {
            Ok(value) => {
                tokens.extend(quote! {
                    #value
                });
            }
            Err(err) => {
                let error = syn::Error::new(Span::call_site(), DisplayFullError(&err));
                tokens.extend(error.to_compile_error());
            }
        }
    }
}

struct EmbedAssets {
    assets_dir: AssetsDir,
    validated_ignore_paths: IgnorePaths,
    should_compress: ShouldCompress,
    should_strip_html_ext: ShouldStripHtmlExt,
    cache_busted_paths: CacheBustedPaths,
    allow_unknown_extensions: LitBool,
}

impl Parse for EmbedAssets {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let assets_dir: AssetsDir = input.parse()?;

        // Default to no compression
        let mut maybe_should_compress = None;
        let mut maybe_ignore_paths = None;
        let mut maybe_should_strip_html_ext = None;
        let mut maybe_cache_busted_paths = None;
        let mut maybe_allow_unknown_extensions = None;

        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }

            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            match key.to_string().as_str() {
                "compress" => {
                    set_once(&mut maybe_should_compress, input.parse()?, &key)?;
                }
                "ignore_paths" => {
                    set_once(&mut maybe_ignore_paths, input.parse()?, &key)?;
                }
                "strip_html_ext" => {
                    set_once(&mut maybe_should_strip_html_ext, input.parse()?, &key)?;
                }
                "cache_busted_paths" => {
                    set_once(&mut maybe_cache_busted_paths, input.parse()?, &key)?;
                }
                "allow_unknown_extensions" => {
                    set_once(&mut maybe_allow_unknown_extensions, input.parse()?, &key)?;
                }
                _ => {
                    return Err(syn::Error::new(
                        key.span(),
                        "Unknown key in embed_assets! macro. Expected `compress`, `ignore_paths`, `strip_html_ext`, `cache_busted_paths`, or `allow_unknown_extensions`",
                    ));
                }
            }
        }

        let should_compress = maybe_should_compress.unwrap_or_else(|| {
            ShouldCompress(LitBool {
                value: false,
                span: Span::call_site(),
            })
        });

        let should_strip_html_ext = maybe_should_strip_html_ext.unwrap_or_else(|| {
            ShouldStripHtmlExt(LitBool {
                value: false,
                span: Span::call_site(),
            })
        });

        let ignore_paths_with_span = maybe_ignore_paths.unwrap_or(IgnorePathsWithSpan(vec![]));
        let validated_ignore_paths = validate_ignore_paths(ignore_paths_with_span, &assets_dir.0)?;

        let maybe_cache_busted_paths =
            maybe_cache_busted_paths.unwrap_or(CacheBustedPathsWithSpan(vec![]));
        let cache_busted_paths =
            validate_cache_busted_paths(maybe_cache_busted_paths, &assets_dir.0)?;

        let allow_unknown_extensions = maybe_allow_unknown_extensions.unwrap_or(LitBool {
            value: false,
            span: Span::call_site(),
        });

        Ok(Self {
            assets_dir,
            validated_ignore_paths,
            should_compress,
            should_strip_html_ext,
            cache_busted_paths,
            allow_unknown_extensions,
        })
    }
}

impl ToTokens for EmbedAssets {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let AssetsDir(assets_dir) = &self.assets_dir;
        let ignore_paths = &self.validated_ignore_paths;
        let ShouldCompress(should_compress) = &self.should_compress;
        let ShouldStripHtmlExt(should_strip_html_ext) = &self.should_strip_html_ext;
        let cache_busted_paths = &self.cache_busted_paths;
        let allow_unknown_extensions = &self.allow_unknown_extensions;

        let result = generate_static_routes(
            assets_dir,
            ignore_paths,
            should_compress,
            should_strip_html_ext,
            cache_busted_paths,
            allow_unknown_extensions.value,
        );

        match result {
            Ok(value) => {
                tokens.extend(quote! {
                    #value
                });
            }
            Err(err) => {
                let error = syn::Error::new(Span::call_site(), DisplayFullError(&err));
                tokens.extend(error.to_compile_error());
            }
        }
    }
}

struct AssetsDir(LitStr);

impl Parse for AssetsDir {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let input_span = input.span();
        let assets_dir: LitStr = input.parse()?;
        let literal = assets_dir.value();
        let path = resolve_path(&literal);
        let metadata = match fs::metadata(&path) {
            Ok(meta) => meta,
            Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound) => {
                return Err(syn::Error::new(
                    input_span,
                    format!(
                        "The specified assets directory ({}) does not exist. Relative paths start at the directory of Cargo.toml.",
                        path.display()
                    ),
                ));
            }
            Err(e) => {
                return Err(syn::Error::new(
                    input_span,
                    format!(
                        "Error reading directory {literal}: {}",
                        DisplayFullError(&e)
                    ),
                ));
            }
        };

        if !metadata.is_dir() {
            return Err(syn::Error::new(
                input_span,
                "The specified assets directory is not a directory",
            ));
        }

        Ok(AssetsDir(assets_dir))
    }
}

struct IgnorePaths(Vec<PathBuf>);

struct IgnorePathsWithSpan(Vec<(PathBuf, Span)>);

impl Parse for IgnorePathsWithSpan {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let dirs = parse_dirs(input)?;

        Ok(IgnorePathsWithSpan(dirs))
    }
}

/// Check that the entries of a path list exist, without following a final
/// symlink so that a dangling link can be ignored
fn validate_paths(
    paths: Vec<(PathBuf, Span)>,
    assets_dir: &LitStr,
    not_found_message: &str,
) -> syn::Result<Vec<PathBuf>> {
    let assets_dir = resolve_path(&assets_dir.value());
    paths
        .into_iter()
        .map(
            |(path, span)| match fs::symlink_metadata(assets_dir.join(&path)) {
                Ok(_) => Ok(path),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound) => {
                    Err(syn::Error::new(span, not_found_message))
                }
                Err(e) => Err(syn::Error::new(
                    span,
                    format!(
                        "Error reading path {}: {}",
                        path.to_string_lossy(),
                        DisplayFullError(&e)
                    ),
                )),
            },
        )
        .collect()
}

fn validate_ignore_paths(
    ignore_paths: IgnorePathsWithSpan,
    assets_dir: &LitStr,
) -> syn::Result<IgnorePaths> {
    validate_paths(
        ignore_paths.0,
        assets_dir,
        "The specified ignored path does not exist",
    )
    .map(IgnorePaths)
}

struct ShouldCompress(LitBool);

impl Parse for ShouldCompress {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let lit = input.parse()?;
        Ok(ShouldCompress(lit))
    }
}

struct ShouldStripHtmlExt(LitBool);

impl Parse for ShouldStripHtmlExt {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let lit = input.parse()?;
        Ok(ShouldStripHtmlExt(lit))
    }
}

struct IsCacheBusted(LitBool);

impl Parse for IsCacheBusted {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let lit = input.parse()?;
        Ok(IsCacheBusted(lit))
    }
}

struct CacheBustedPaths(Vec<PathBuf>);
struct CacheBustedPathsWithSpan(Vec<(PathBuf, Span)>);

impl Parse for CacheBustedPathsWithSpan {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let dirs = parse_dirs(input)?;
        Ok(CacheBustedPathsWithSpan(dirs))
    }
}

fn validate_cache_busted_paths(
    tuples: CacheBustedPathsWithSpan,
    assets_dir: &LitStr,
) -> syn::Result<CacheBustedPaths> {
    validate_paths(
        tuples.0,
        assets_dir,
        "The specified path for cache busting does not exist",
    )
    .map(CacheBustedPaths)
}

/// Helper function for turning an array of strs representing paths into
/// a `Vec` containing tuples of each `PathBuf` and its `Span` in the `ParseStream`
fn parse_dirs(input: ParseStream) -> syn::Result<Vec<(PathBuf, Span)>> {
    let inner_content;
    bracketed!(inner_content in input);

    let mut dirs = Vec::new();
    while !inner_content.is_empty() {
        let directory_span = inner_content.span();
        let directory_str = inner_content.parse::<LitStr>()?;
        let path = PathBuf::from(directory_str.value());
        dirs.push((path, directory_span));

        if !inner_content.is_empty() {
            inner_content.parse::<Token![,]>()?;
        }
    }
    Ok(dirs)
}

fn generate_static_routes(
    assets_dir: &LitStr,
    ignore_paths: &IgnorePaths,
    should_compress: &LitBool,
    should_strip_html_ext: &LitBool,
    cache_busted_paths: &CacheBustedPaths,
    allow_unknown_extensions: bool,
) -> Result<TokenStream, error::Error> {
    let assets_dir_abs = resolve_path(&assets_dir.value())
        .canonicalize()
        .map_err(|error| Error::CannotCanonicalizeDirectory {
            dir: assets_dir.value(),
            error,
        })?;
    let option_paths = |paths: &[PathBuf]| {
        paths
            .iter()
            .map(|path| path_inside_assets_dir(&assets_dir_abs, path))
            .collect::<Result<Vec<_>, _>>()
    };
    let ignore_paths = option_paths(&ignore_paths.0)?;
    let cache_busted_paths = option_paths(&cache_busted_paths.0)?;

    let mut entries = Vec::new();
    collect_files(&assets_dir_abs, &ignore_paths, &mut entries)?;

    let mut routes = Vec::new();
    let mut seen_web_paths = HashMap::new();
    for entry in entries {
        let is_entry_cache_busted = cache_busted_paths
            .iter()
            .any(|path| entry.starts_with(path));

        let entry_str = entry
            .to_str()
            .ok_or_else(|| Error::FilePathIsNotUtf8(entry.clone()))?;
        let EmbeddedFileInfo {
            entry_path,
            content_type,
            body,
            maybe_gzip,
            maybe_zstd,
        } = EmbeddedFileInfo::from_path(
            &entry,
            Some(&assets_dir_abs),
            should_compress,
            should_strip_html_ext,
            allow_unknown_extensions,
        )?;

        let mut web_paths = Vec::new();
        if let Some(web_path) = &entry_path {
            check_route_syntax(web_path, entry_str)?;

            let encoded = encode_web_path(web_path, false);
            check_duplicate_web_path(&mut seen_web_paths, &encoded, entry_str)?;
            let lowercase_encoded = encode_web_path(web_path, true);
            web_paths.push(encoded);
            if !web_paths.contains(&lowercase_encoded) {
                web_paths.push(lowercase_encoded);
            }
        }

        routes.push(quote! {
            router = ::static_serve::static_route(
                router,
                &[#(#web_paths),*],
                #content_type,
                #body,
                #maybe_gzip,
                #maybe_zstd,
                #is_entry_cache_busted
            );
        });
    }

    Ok(quote! {
    pub fn static_router<S>() -> ::axum::Router<S>
        where S: ::std::clone::Clone + ::std::marker::Send + ::std::marker::Sync + 'static {
            let mut router = ::axum::Router::<S>::new();
            #(#routes)*
            router
        }
    })
}

/// Turn an entry of a path list into an absolute path below the assets directory
///
/// `.` and `..` are resolved lexically because entries are compared to the
/// paths of the directory walk, which keep the names of symlinks.
fn path_inside_assets_dir(assets_dir_abs: &Path, path: &Path) -> Result<PathBuf, Error> {
    let mut normalized = PathBuf::new();
    for component in assets_dir_abs.join(path).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component),
        }
    }

    if normalized.starts_with(assets_dir_abs) {
        Ok(normalized)
    } else {
        Err(Error::PathOutsideAssetsDir(path.to_path_buf()))
    }
}

/// Recursively collect the files of `dir` in a deterministic order, following
/// symlinks and skipping `ignore_paths` without looking inside of them
fn collect_files(
    dir: &Path,
    ignore_paths: &[PathBuf],
    files: &mut Vec<PathBuf>,
) -> Result<(), Error> {
    let read_dir_error = |error| Error::CannotReadDirectory {
        dir: dir.to_path_buf(),
        error,
    };
    let mut entries = fs::read_dir(dir)
        .map_err(read_dir_error)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<_>>>()
        .map_err(read_dir_error)?;
    entries.sort();

    for entry in entries {
        if ignore_paths.iter().any(|path| entry.starts_with(path)) {
            continue;
        }

        let metadata = fs::metadata(&entry).map_err(|error| Error::CannotGetMetadata {
            entry: entry.clone(),
            error,
        })?;
        if metadata.is_dir() {
            collect_files(&entry, ignore_paths, files)?;
        } else if metadata.is_file() {
            files.push(entry);
        } else {
            return Err(Error::NotARegularFile(entry));
        }
    }

    Ok(())
}

/// Record the web path claimed by `entry_str`, erroring if another file
/// already claimed it.
///
/// Registering the same path twice would panic at runtime when the `Router`
/// is built. This can happen with `strip_html_ext = true`, where files like
/// `foo.html` and `foo.htm` both map to `/foo`.
fn check_duplicate_web_path(
    seen_web_paths: &mut HashMap<String, String>,
    web_path: &str,
    entry_str: &str,
) -> Result<(), Error> {
    match seen_web_paths.insert(web_path.to_owned(), entry_str.to_owned()) {
        Some(first_file) => Err(Error::DuplicateWebPath {
            web_path: web_path.to_owned(),
            first_file,
            second_file: entry_str.to_owned(),
        }),
        None => Ok(()),
    }
}

fn generate_static_handler(
    asset_file: &LitStr,
    should_compress: &LitBool,
    cache_busted: &LitBool,
    allow_unknown_extensions: &LitBool,
) -> Result<TokenStream, error::Error> {
    let asset_file_abs = resolve_path(&asset_file.value())
        .canonicalize()
        .map_err(|error| Error::CannotCanonicalizeFile {
            entry: resolve_path(&asset_file.value()),
            error,
        })?;
    let EmbeddedFileInfo {
        entry_path: _,
        content_type,
        body,
        maybe_gzip,
        maybe_zstd,
    } = EmbeddedFileInfo::from_path(
        &asset_file_abs,
        None,
        should_compress,
        &LitBool {
            value: false,
            span: Span::call_site(),
        },
        allow_unknown_extensions.value(),
    )?;

    let route = quote! {
        ::static_serve::static_method_router(
            #content_type,
            #body,
            #maybe_gzip,
            #maybe_zstd,
            #cache_busted
        )
    };

    Ok(route)
}

/// A compressed body, if worth serving, and the `ETag` of its bytes
struct OptionCompressedBody(Option<(LitByteStr, String)>);
impl ToTokens for OptionCompressedBody {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(if let Some((body, etag)) = &self.0 {
            quote! {
                ::std::option::Option::Some(::static_serve::EmbeddedBody {
                    bytes: #body,
                    etag: #etag,
                })
            }
        } else {
            quote! { ::std::option::Option::None }
        });
    }
}

struct EmbeddedFileInfo {
    /// When creating a `Router`, we need the API path/route to the
    /// target file. If creating a `Handler`, this is not needed since
    /// the router is responsible for the file's path on the server.
    entry_path: Option<String>,
    content_type: String,
    body: TokenStream,
    maybe_gzip: OptionCompressedBody,
    maybe_zstd: OptionCompressedBody,
}

impl EmbeddedFileInfo {
    fn from_path(
        pathbuf: &Path,
        assets_dir_abs: Option<&Path>,
        should_compress: &LitBool,
        should_strip_html_ext: &LitBool,
        allow_unknown_extensions: bool,
    ) -> Result<Self, Error> {
        let contents = fs::read(pathbuf).map_err(|error| Error::CannotReadEntryContents {
            entry: pathbuf.to_path_buf(),
            error,
        })?;
        // Checked first since compressing is by far the slowest step
        let content_type = file_content_type(pathbuf, allow_unknown_extensions)?;

        let path_str = pathbuf
            .to_str()
            .ok_or_else(|| Error::FilePathIsNotUtf8(pathbuf.to_path_buf()))?;
        // `include_bytes!` also tracks the file, and is much cheaper than
        // passing the bytes through the token stream
        let body = quote! { ::core::include_bytes!(#path_str) };

        // Optionally compress files
        let (maybe_gzip, maybe_zstd) = if should_compress.value {
            let gzip = gzip_compress(&contents, pathbuf)?;
            let zstd = zstd_compress(&contents, pathbuf)?;
            (gzip, zstd)
        } else {
            (None, None)
        };

        // entry_path is only needed for the router (embed_assets!)
        let entry_path = if let Some(dir) = assets_dir_abs {
            let relative_entry = pathbuf
                .strip_prefix(dir)
                .map_err(|_| Error::CannotMakeFileRelative(pathbuf.to_path_buf()))?;
            let mut web_path = normalize_web_path(relative_entry);
            if should_strip_html_ext.value && content_type == mime::TEXT_HTML {
                strip_html_ext(&mut web_path);
            }

            Some(web_path)
        } else {
            None
        };

        let content_type = if takes_charset(&content_type) && str::from_utf8(&contents).is_ok() {
            with_utf8_charset(&content_type)
        } else {
            content_type
        };
        let content_type = content_type.to_string();

        let etag_str = etag(&contents);
        let body = quote! {
            ::static_serve::EmbeddedBody {
                bytes: #body,
                etag: #etag_str,
            }
        };
        let maybe_gzip = OptionCompressedBody(maybe_gzip);
        let maybe_zstd = OptionCompressedBody(maybe_zstd);

        Ok(Self {
            entry_path,
            content_type,
            body,
            maybe_gzip,
            maybe_zstd,
        })
    }
}

fn gzip_compress(contents: &[u8], entry: &Path) -> Result<Option<(LitByteStr, String)>, Error> {
    let mut compressor = GzEncoder::new(Vec::new(), flate2::Compression::best());
    compressor.write_all(contents).map_err(|e| Error::Gzip {
        entry: entry.to_path_buf(),
        error: GzipType::CompressorWrite(e),
    })?;
    let compressed = compressor.finish().map_err(|e| Error::Gzip {
        entry: entry.to_path_buf(),
        error: GzipType::EncoderFinish(e),
    })?;

    Ok(maybe_get_compressed(&compressed, contents))
}

fn zstd_compress(contents: &[u8], entry: &Path) -> Result<Option<(LitByteStr, String)>, Error> {
    let level = *zstd::compression_level_range().end();
    let mut encoder = zstd::Encoder::new(Vec::new(), level).unwrap();
    write_to_zstd_encoder(&mut encoder, contents).map_err(|e| Error::Zstd {
        entry: entry.to_path_buf(),
        error: ZstdType::EncoderWrite(e),
    })?;

    let compressed = encoder.finish().map_err(|e| Error::Zstd {
        entry: entry.to_path_buf(),
        error: ZstdType::EncoderFinish(e),
    })?;

    Ok(maybe_get_compressed(&compressed, contents))
}

/// RFC 9659 limits the zstd content coding to a window of 8 MiB
const MAX_ZSTD_WINDOW_LOG: u32 = 23;

fn write_to_zstd_encoder(
    encoder: &mut zstd::Encoder<'static, Vec<u8>>,
    contents: &[u8],
) -> io::Result<()> {
    encoder.set_pledged_src_size(Some(
        contents
            .len()
            .try_into()
            .expect("contents size should fit into u64"),
    ))?;
    encoder.window_log(MAX_ZSTD_WINDOW_LOG)?;
    encoder.include_checksum(false)?;
    encoder.include_contentsize(false)?;
    encoder.long_distance_matching(false)?;
    encoder.write_all(contents)?;

    Ok(())
}

fn is_compression_significant(compressed_len: usize, contents_len: usize) -> bool {
    // `compressed_len < contents_len * 0.9`, computed exactly in integers
    compressed_len * 10 < contents_len * 9
}

fn maybe_get_compressed(compressed: &[u8], contents: &[u8]) -> Option<(LitByteStr, String)> {
    is_compression_significant(compressed.len(), contents.len()).then(|| {
        (
            LitByteStr::new(compressed, Span::call_site()),
            etag(compressed),
        )
    })
}

/// Check if a media type benefits from a `charset` parameter
///
/// Without one browsers guess the encoding of most text, and `text/markdown`
/// requires it. These don't need it: `text/javascript` and `text/calendar`
/// default to UTF-8, `text/vtt` is always UTF-8 and has no such parameter, and
/// XML declares its own encoding, which being valid UTF-8 can't rule out.
fn takes_charset(media_type: &Mime) -> bool {
    let subtype = media_type.subtype();
    media_type.type_() == mime::TEXT
        && media_type.get_param(mime::CHARSET).is_none()
        && subtype != mime::JAVASCRIPT
        && subtype != "calendar"
        && subtype != "vtt"
        && subtype != mime::XML
}

/// Add `charset=utf-8` to a media type, keeping its other parameters
fn with_utf8_charset(media_type: &Mime) -> Mime {
    format!("{media_type}; {}={}", mime::CHARSET, mime::UTF_8)
        .parse()
        .expect("adding a parameter to a valid media type should keep it valid")
}

/// Use `mime_guess` to get the best guess of the file's MIME type
/// by looking at its extension, or return an error if unable.
///
/// If the `allow_unknown_extensions` parameter is true, an unknown ext
/// will not produce an error, but application/octet-stream.
///
/// We accept the first guess because [`mime_guess` updates the order
/// according to the latest IETF RFC](https://docs.rs/mime_guess/2.0.5/mime_guess/struct.MimeGuess.html#note-ordering)
fn file_content_type(path: &Path, allow_unknown_extensions: bool) -> Result<Mime, error::Error> {
    let Some(ext) = path.extension() else {
        return if allow_unknown_extensions {
            Ok(mime::APPLICATION_OCTET_STREAM)
        } else {
            Err(Error::MissingFileExtension(path.to_path_buf()))
        };
    };

    let guess = ext.to_str().map_or(
        mime_guess::MimeGuess::from_ext(""),
        mime_guess::MimeGuess::from_ext,
    );

    if allow_unknown_extensions {
        return Ok(guess.first_or_octet_stream());
    }

    guess
        .first()
        .ok_or_else(|| Error::UnknownFileExtension(path.to_path_buf()))
}

fn etag(contents: &[u8]) -> String {
    let sha256 = Sha256::digest(contents);
    let hash = u64::from_le_bytes(sha256[..8].try_into().unwrap())
        ^ u64::from_le_bytes(sha256[8..16].try_into().unwrap())
        ^ u64::from_le_bytes(sha256[16..24].try_into().unwrap())
        ^ u64::from_le_bytes(sha256[24..32].try_into().unwrap());
    format!("\"{hash:016x}\"")
}

/// Convert a relative filesystem-style path into a rooted web route.
///
/// Path segments are normalized via [`Path::components`] so separator
/// style differences across platforms do not affect route generation.
/// The returned route is always absolute (starts with `/`).
fn normalize_web_path(relative_path: &Path) -> String {
    let normalized = relative_path
        .components()
        .filter_map(|component| match component {
            Component::Normal(segment) => segment.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    format!("/{normalized}")
}

/// Reject path segments that axum refuses as routes, which would otherwise
/// panic when the `Router` is built
fn check_route_syntax(web_path: &str, entry_str: &str) -> Result<(), Error> {
    if web_path
        .split('/')
        .any(|segment| segment.starts_with([':', '*']))
    {
        return Err(Error::UnsupportedRouteSegment {
            web_path: web_path.to_owned(),
            file: entry_str.to_owned(),
        });
    }

    Ok(())
}

/// Percent-encode a web path into the form requested by HTTP clients
///
/// axum matches routes against the undecoded request path, and treats braces
/// as captures, so a file's name must be encoded to be reachable. Clients
/// differ in the case of hex digits, so both spellings are registered. `[`,
/// `]` and `|` stay as they are since URL parsers don't encode them either.
fn encode_web_path(web_path: &str, lowercase_hex: bool) -> String {
    let hex = if lowercase_hex {
        b"0123456789abcdef"
    } else {
        b"0123456789ABCDEF"
    };

    let mut encoded = String::with_capacity(web_path.len());
    for byte in web_path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~!$&'()*+,;=:@[]|".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(hex[usize::from(byte >> 4)]));
            encoded.push(char::from(hex[usize::from(byte & 0xf)]));
        }
    }
    encoded
}

/// Strip `.html`/`.htm` from an already-normalized web path in-place,
/// and map `/index` to its parent directory route.
fn strip_html_ext(path: &mut String) {
    let ext = path.rsplit_once('.').map(|(_, ext)| ext);
    if ext.is_some_and(|ext| ext.eq_ignore_ascii_case("html")) {
        path.truncate(path.len() - ".html".len());
    } else if ext.is_some_and(|ext| ext.eq_ignore_ascii_case("htm")) {
        path.truncate(path.len() - ".htm".len());
    }

    if path.ends_with("/index") {
        path.truncate(path.len() - "index".len());
    }
}

#[cfg(test)]
mod tests {
    use proc_macro2::{TokenStream, TokenTree};
    use quote::{ToTokens, quote};
    use syn::LitStr;

    use super::{EmbedAssets, is_compression_significant};

    /// Expand `embed_assets!` and return the message of the error that fails
    /// the build, if any
    ///
    /// Unlike a `compile_fail` doctest, this tells which error the expansion
    /// fails with.
    fn embed_assets_error(input: TokenStream) -> Option<String> {
        let expanded = match syn::parse2::<EmbedAssets>(input) {
            Ok(embed_assets) => embed_assets.into_token_stream(),
            Err(error) => error.to_compile_error(),
        };

        let mut tokens = expanded.into_iter();
        tokens
            .find(|token| matches!(token, TokenTree::Ident(ident) if ident == "compile_error"))?;
        tokens.find_map(|token| match token {
            TokenTree::Group(group) => syn::parse2::<LitStr>(group.stream())
                .ok()
                .map(|message| message.value()),
            _ => None,
        })
    }

    #[test]
    fn duplicate_web_paths_fail_the_build() {
        // Registering a path twice would panic when the `Router` is built
        assert_eq!(
            embed_assets_error(quote!("../test_fixtures/collision")),
            None
        );
        let error = embed_assets_error(quote!("../test_fixtures/collision", strip_html_ext = true))
            .unwrap();
        assert!(
            error.starts_with("Multiple files map to the same web path /foo"),
            "{error}"
        );
    }

    #[test]
    fn unknown_extensions_fail_the_build() {
        // The fixture holds `example.wtf` and the extensionless `.DS_STORE`
        let error = embed_assets_error(quote!("../test_fixtures/unknown_extensions")).unwrap();
        assert!(error.starts_with("Missing file extension"), "{error}");
        let error = embed_assets_error(quote!(
            "../test_fixtures/unknown_extensions",
            ignore_paths = [".DS_STORE"]
        ))
        .unwrap();
        assert!(error.starts_with("Unknown file extension"), "{error}");
        assert_eq!(
            embed_assets_error(quote!(
                "../test_fixtures/unknown_extensions",
                allow_unknown_extensions = true
            )),
            None
        );
    }

    #[test]
    fn unsupported_route_segments_fail_the_build() {
        // axum would refuse the route and panic when the `Router` is built
        let error = embed_assets_error(quote!("../test_fixtures/route_syntax")).unwrap();
        assert!(error.contains("can't be a route"), "{error}");
        assert_eq!(
            embed_assets_error(quote!(
                "../test_fixtures/route_syntax",
                ignore_paths = [":colon.txt"]
            )),
            None
        );
    }

    #[test]
    fn repeated_options_fail_the_build() {
        let error = embed_assets_error(quote!(
            "../test_fixtures/assets/small",
            compress = true,
            compress = false
        ))
        .unwrap();
        assert_eq!(error, "Duplicate key `compress` in macro options");
        assert_eq!(
            embed_assets_error(quote!("../test_fixtures/assets/small", compress = true,)),
            None
        );
    }

    #[test]
    fn compression_significance_threshold() {
        // Exactly 90% of the original size is not significant
        assert!(!is_compression_significant(90, 100));
        assert!(is_compression_significant(89, 100));

        // Sizes not divisible by 10 must not floor the threshold down:
        // 90% of 19 is 17.1, so 17 is significant and 18 is not
        assert!(is_compression_significant(17, 19));
        assert!(!is_compression_significant(18, 19));
    }
}
