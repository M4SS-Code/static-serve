//! Proc macro crate for compressing and embedding static assets
//! in a web server
//! Macro invocation: `embed_assets!('path/to/assets', compress = true);`

use std::{
    convert::Into,
    fmt::Display,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use display_full_error::DisplayFullError;
use flate2::write::GzEncoder;
use glob::glob;
use proc_macro2::{Span, TokenStream};
use quote::{quote, ToTokens};
use sha1::{Digest as _, Sha1};
use syn::{
    bracketed,
    parse::{Parse, ParseStream},
    parse_macro_input, Ident, LitBool, LitByteStr, LitStr, Token,
};

mod error;
use error::{Error, GzipType, ZstdType};

#[proc_macro]
/// Embed and optionally compress static assets for a web server
pub fn embed_assets(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let parsed = parse_macro_input!(input as EmbedAssets);
    quote! { #parsed }.into()
}

struct EmbedAssets {
    assets_dir: AssetsDir,
    validated_ignore_dirs: IgnoreDirs,
    should_compress: ShouldCompress,
}

impl Parse for EmbedAssets {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let assets_dir: AssetsDir = input.parse()?;

        // Default to no compression
        let mut maybe_should_compress = None;
        let mut maybe_ignore_dirs = None;

        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            match key.to_string().as_str() {
                "compress" => {
                    let value = input.parse()?;
                    maybe_should_compress = Some(value);
                }
                "ignore_dirs" => {
                    let value = input.parse()?;
                    maybe_ignore_dirs = Some(value);
                }
                _ => {
                    return Err(syn::Error::new(
                        key.span(),
                        "Unknown key in embed_assets! macro. Expected `compress` or `ignore_dirs`",
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

        let ignore_dirs_with_span = maybe_ignore_dirs.unwrap_or(IgnoreDirsWithSpan(vec![]));
        let validated_ignore_dirs = validate_ignore_dirs(ignore_dirs_with_span, &assets_dir.0)?;

        Ok(Self {
            assets_dir,
            validated_ignore_dirs,
            should_compress,
        })
    }
}

impl ToTokens for EmbedAssets {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let AssetsDir(assets_dir) = &self.assets_dir;
        let ignore_dirs = &self.validated_ignore_dirs;
        let ShouldCompress(should_compress) = &self.should_compress;

        let result = generate_static_routes(assets_dir, ignore_dirs, should_compress);

        match result {
            Ok(value) => {
                tokens.extend(quote! {
                    #value
                });
            }
            Err(err_message) => {
                let error = syn::Error::new(Span::call_site(), err_message);
                tokens.extend(error.to_compile_error());
            }
        }
    }
}

struct AssetsDir(ValidAssetsDirTypes);

impl Parse for AssetsDir {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let input_span = input.span();
        let assets_dir: ValidAssetsDirTypes = input.parse()?;
        let literal = assets_dir.to_string();
        let path = Path::new(&literal);
        let metadata = match fs::metadata(path) {
            Ok(meta) => meta,
            Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound) => {
                return Err(syn::Error::new(
                    input_span,
                    "The specified assets directory does not exist",
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

enum ValidAssetsDirTypes {
    LiteralStr(LitStr),
    Ident(Ident),
}

impl Display for ValidAssetsDirTypes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LiteralStr(inner) => write!(f, "{}", inner.value()),
            Self::Ident(inner) => write!(f, "{inner}"),
        }
    }
}

impl Parse for ValidAssetsDirTypes {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if let Ok(inner) = input.parse::<LitStr>() {
            Ok(ValidAssetsDirTypes::LiteralStr(inner))
        } else {
            let inner = input.parse::<Ident>().map_err(|_| {
                syn::Error::new(
                    input.span(),
                    "Assets directory must be a literal string or valid identifier",
                )
            })?;
            Ok(ValidAssetsDirTypes::Ident(inner))
        }
    }
}

struct IgnoreDirs(Vec<PathBuf>);

struct IgnoreDirsWithSpan(Vec<(PathBuf, Span)>);

impl Parse for IgnoreDirsWithSpan {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let inner_content;
        bracketed!(inner_content in input);

        let mut dirs = Vec::new();
        while !inner_content.is_empty() {
            let directory_span = inner_content.span();
            let directory_str = inner_content.parse::<LitStr>()?;
            if !inner_content.is_empty() {
                inner_content.parse::<Token![,]>()?;
            }
            let path = PathBuf::from(directory_str.value());
            dirs.push((path, directory_span));
        }

        Ok(IgnoreDirsWithSpan(dirs))
    }
}

fn validate_ignore_dirs(
    ignore_dirs: IgnoreDirsWithSpan,
    assets_dir: &ValidAssetsDirTypes,
) -> syn::Result<IgnoreDirs> {
    let mut valid_ignore_dirs = Vec::new();
    for (dir, span) in ignore_dirs.0 {
        let full_path = PathBuf::from(assets_dir.to_string()).join(&dir);
        match fs::metadata(&full_path) {
            Ok(meta) if !meta.is_dir() => {
                return Err(syn::Error::new(
                    span,
                    "The specified ignored directory is not a directory",
                ));
            }
            Ok(_) => valid_ignore_dirs.push(full_path),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound) => {
                return Err(syn::Error::new(
                    span,
                    "The specified ignored directory does not exist",
                ))
            }
            Err(e) => {
                return Err(syn::Error::new(
                    span,
                    format!(
                        "Error reading ignored directory {}: {}",
                        dir.to_string_lossy(),
                        DisplayFullError(&e)
                    ),
                ))
            }
        };
    }
    Ok(IgnoreDirs(valid_ignore_dirs))
}

struct ShouldCompress(LitBool);

impl Parse for ShouldCompress {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let lit = input.parse()?;
        Ok(ShouldCompress(lit))
    }
}

fn generate_static_routes(
    assets_dir: &ValidAssetsDirTypes,
    ignore_dirs: &IgnoreDirs,
    should_compress: &LitBool,
) -> Result<TokenStream, error::Error> {
    let assets_dir_abs = Path::new(&assets_dir.to_string())
        .canonicalize()
        .map_err(Error::CannotCanonicalizeDirectory)?;
    let assets_dir_abs_str = assets_dir_abs
        .to_str()
        .ok_or(Error::InvalidUnicodeInDirectoryName)?;
    let canon_ignore_dirs = ignore_dirs
        .0
        .iter()
        .map(|d| d.canonicalize().map_err(Error::CannotCanonicalizeIgnoreDir))
        .collect::<Result<Vec<_>, _>>()?;

    let mut routes = Vec::new();
    for entry in glob(&format!("{assets_dir_abs_str}/**/*")).map_err(Error::Pattern)? {
        let entry = entry.map_err(Error::Glob)?;
        let metadata = entry.metadata().map_err(Error::CannotGetMetadata)?;
        if metadata.is_dir() {
            continue;
        }

        // Skip `entry`s which are located in ignored subdirectories
        if canon_ignore_dirs
            .iter()
            .any(|ignore_dir| entry.starts_with(ignore_dir))
        {
            continue;
        }

        let contents = fs::read(&entry).map_err(Error::CannotReadEntryContents)?;

        // Optionally compress files
        let (maybe_gzip, maybe_zstd) = if should_compress.value {
            let gzip = gzip_compress(&contents)?;
            let zstd = zstd_compress(&contents)?;
            (gzip, zstd)
        } else {
            (None, None)
        };

        // Create parameters for `::static_serve::static_route()`
        let entry_path = entry
            .to_str()
            .ok_or(Error::InvalidUnicodeInEntryName)?
            .strip_prefix(assets_dir_abs_str)
            .unwrap_or_default();
        let content_type = file_content_type(&entry)?;
        let etag_str = etag(&contents);
        let lit_byte_str_contents = LitByteStr::new(&contents, Span::call_site());
        let maybe_gzip = option_to_token_stream_option(maybe_gzip.as_ref());
        let maybe_zstd = option_to_token_stream_option(maybe_zstd.as_ref());

        routes.push(quote! {
            router = ::static_serve::static_route(
                router,
                #entry_path,
                #content_type,
                #etag_str,
                #lit_byte_str_contents,
                #maybe_gzip,
                #maybe_zstd,
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

fn gzip_compress(contents: &[u8]) -> Result<Option<LitByteStr>, Error> {
    let mut compressor = GzEncoder::new(Vec::new(), flate2::Compression::best());
    compressor
        .write_all(contents)
        .map_err(|e| Error::Gzip(GzipType::CompressorWrite(e)))?;
    let compressed = compressor
        .finish()
        .map_err(|e| Error::Gzip(GzipType::EncoderFinish(e)))?;

    Ok(maybe_get_compressed(&compressed, contents))
}

fn zstd_compress(contents: &[u8]) -> Result<Option<LitByteStr>, Error> {
    let level = *zstd::compression_level_range().end();
    let mut encoder = zstd::Encoder::new(Vec::new(), level).unwrap();
    write_to_zstd_encoder(&mut encoder, contents)
        .map_err(|e| Error::Zstd(ZstdType::EncoderWrite(e)))?;

    let compressed = encoder
        .finish()
        .map_err(|e| Error::Zstd(ZstdType::EncoderFinish(e)))?;

    Ok(maybe_get_compressed(&compressed, contents))
}

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
    encoder.window_log(23)?;
    encoder.include_checksum(false)?;
    encoder.include_contentsize(false)?;
    encoder.long_distance_matching(false)?;
    encoder.write_all(contents)?;

    Ok(())
}

fn option_to_token_stream_option<T: ToTokens>(opt: Option<&T>) -> TokenStream {
    if let Some(inner) = opt {
        quote! { ::std::option::Option::Some(#inner) }
    } else {
        quote! { ::std::option::Option::None }
    }
}

fn is_compression_significant(compressed_len: usize, contents_len: usize) -> bool {
    let ninety_pct_original = contents_len / 10 * 9;
    compressed_len < ninety_pct_original
}

fn maybe_get_compressed(compressed: &[u8], contents: &[u8]) -> Option<LitByteStr> {
    is_compression_significant(compressed.len(), contents.len())
        .then(|| LitByteStr::new(compressed, Span::call_site()))
}

fn file_content_type(path: &Path) -> Result<&'static str, error::Error> {
    match path.extension() {
        Some(ext) if ext.eq_ignore_ascii_case("css") => Ok("text/css"),
        Some(ext) if ext.eq_ignore_ascii_case("js") => Ok("text/javascript"),
        Some(ext) if ext.eq_ignore_ascii_case("txt") => Ok("text/plain"),
        Some(ext) if ext.eq_ignore_ascii_case("woff") => Ok("font/woff"),
        Some(ext) if ext.eq_ignore_ascii_case("woff2") => Ok("font/woff2"),
        Some(ext) if ext.eq_ignore_ascii_case("svg") => Ok("image/svg+xml"),
        ext => Err(error::Error::UnknownFileExtension(ext.map(Into::into))),
    }
}

fn etag(contents: &[u8]) -> String {
    let sha256 = Sha1::digest(contents);
    let hash = u64::from_le_bytes(sha256[..8].try_into().unwrap())
        ^ u64::from_le_bytes(sha256[8..16].try_into().unwrap());
    format!("\"{hash:016x}\"")
}
