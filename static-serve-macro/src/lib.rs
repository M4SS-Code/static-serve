//! Proc macro crate for compressing and embedding static assets
//! in a web server

use std::{
    convert::Into,
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

#[proc_macro]
/// Embed and optionally compress a single static asset for a web server
pub fn embed_asset(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let parsed = parse_macro_input!(input as EmbedAsset);
    quote! { #parsed }.into()
}

struct EmbedAsset {
    asset_file: AssetFile,
    should_compress: ShouldCompress,
}

struct AssetFile(LitStr);

impl Parse for EmbedAsset {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let asset_file: AssetFile = input.parse()?;

        // Default to no compression
        let mut maybe_should_compress = None;

        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            if matches!(key.to_string().as_str(), "compress") {
                let value = input.parse()?;
                maybe_should_compress = Some(value);
            } else {
                return Err(syn::Error::new(
                    key.span(),
                    format!(
                        "Unknown key in `embed_asset!` macro. Expected `compress` but got {key}"
                    ),
                ));
            }
        }

        let should_compress = maybe_should_compress.unwrap_or_else(|| {
            ShouldCompress(LitBool {
                value: false,
                span: Span::call_site(),
            })
        });

        Ok(Self {
            asset_file,
            should_compress,
        })
    }
}

impl Parse for AssetFile {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let input_span = input.span();
        let asset_file: LitStr = input.parse()?;
        let literal = asset_file.value();
        let path = Path::new(&literal);
        let metadata = match fs::metadata(path) {
            Ok(meta) => meta,
            Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound) => {
                return Err(syn::Error::new(
                    input_span,
                    format!("The specified asset file ({literal}) does not exist."),
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

        let result = generate_static_handler(asset_file, should_compress);

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

struct EmbedAssets {
    assets_dir: AssetsDir,
    validated_ignore_dirs: IgnoreDirs,
    should_compress: ShouldCompress,
    should_strip_html_ext: ShouldStripHtmlExt,
}

impl Parse for EmbedAssets {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let assets_dir: AssetsDir = input.parse()?;

        // Default to no compression
        let mut maybe_should_compress = None;
        let mut maybe_ignore_dirs = None;
        let mut maybe_should_strip_html_ext = None;

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
                "strip_html_ext" => {
                    let value = input.parse()?;
                    maybe_should_strip_html_ext = Some(value);
                }
                _ => {
                    return Err(syn::Error::new(
                        key.span(),
                        "Unknown key in embed_assets! macro. Expected `compress`, `ignore_dirs`, or `strip_html_ext`",
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

        let ignore_dirs_with_span = maybe_ignore_dirs.unwrap_or(IgnoreDirsWithSpan(vec![]));
        let validated_ignore_dirs = validate_ignore_dirs(ignore_dirs_with_span, &assets_dir.0)?;

        Ok(Self {
            assets_dir,
            validated_ignore_dirs,
            should_compress,
            should_strip_html_ext,
        })
    }
}

impl ToTokens for EmbedAssets {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let AssetsDir(assets_dir) = &self.assets_dir;
        let ignore_dirs = &self.validated_ignore_dirs;
        let ShouldCompress(should_compress) = &self.should_compress;
        let ShouldStripHtmlExt(should_strip_html_ext) = &self.should_strip_html_ext;

        let result = generate_static_routes(
            assets_dir,
            ignore_dirs,
            should_compress,
            should_strip_html_ext,
        );

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

struct AssetsDir(LitStr);

impl Parse for AssetsDir {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let input_span = input.span();
        let assets_dir: LitStr = input.parse()?;
        let literal = assets_dir.value();
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
            let path = PathBuf::from(directory_str.value());
            dirs.push((path, directory_span));

            if !inner_content.is_empty() {
                inner_content.parse::<Token![,]>()?;
            }
        }

        Ok(IgnoreDirsWithSpan(dirs))
    }
}

fn validate_ignore_dirs(
    ignore_dirs: IgnoreDirsWithSpan,
    assets_dir: &LitStr,
) -> syn::Result<IgnoreDirs> {
    let mut valid_ignore_dirs = Vec::new();
    for (dir, span) in ignore_dirs.0 {
        let full_path = PathBuf::from(assets_dir.value()).join(&dir);
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
        }
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

struct ShouldStripHtmlExt(LitBool);

impl Parse for ShouldStripHtmlExt {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let lit = input.parse()?;
        Ok(ShouldStripHtmlExt(lit))
    }
}

fn generate_static_routes(
    assets_dir: &LitStr,
    ignore_dirs: &IgnoreDirs,
    should_compress: &LitBool,
    should_strip_html_ext: &LitBool,
) -> Result<TokenStream, error::Error> {
    let assets_dir_abs = Path::new(&assets_dir.value())
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

        let EmbeddedFileInfo {
            entry_path,
            content_type,
            etag_str,
            lit_byte_str_contents,
            maybe_gzip,
            maybe_zstd,
        } = EmbeddedFileInfo::from_path(
            &entry,
            Some(assets_dir_abs_str),
            should_compress,
            should_strip_html_ext,
        )?;

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

fn generate_static_handler(
    asset_file: &LitStr,
    should_compress: &LitBool,
) -> Result<TokenStream, error::Error> {
    let asset_file_abs = Path::new(&asset_file.value())
        .canonicalize()
        .map_err(Error::CannotCanonicalizeFile)?;

    let EmbeddedFileInfo {
        entry_path: _,
        content_type,
        etag_str,
        lit_byte_str_contents,
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
    )?;

    let route = quote! {
        ::static_serve::static_method_router(
            #content_type,
            #etag_str,
            #lit_byte_str_contents,
            #maybe_gzip,
            #maybe_zstd,
        )
    };

    Ok(route)
}

struct OptionBytesSlice(Option<LitByteStr>);
impl ToTokens for OptionBytesSlice {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(if let Some(inner) = &self.0.as_ref() {
            quote! { ::std::option::Option::Some(#inner) }
        } else {
            quote! { ::std::option::Option::None }
        });
    }
}

struct EmbeddedFileInfo<'a> {
    /// When creating a `Router`, we need the API path/route to the
    /// target file. If creating a `Handler`, this is not needed since
    /// the router is responsible for the file's path on the server.
    entry_path: Option<&'a str>,
    content_type: String,
    etag_str: String,
    lit_byte_str_contents: LitByteStr,
    maybe_gzip: OptionBytesSlice,
    maybe_zstd: OptionBytesSlice,
}

impl<'a> EmbeddedFileInfo<'a> {
    fn from_path(
        pathbuf: &'a PathBuf,
        assets_dir_abs_str: Option<&str>,
        should_compress: &LitBool,
        should_strip_html_ext: &LitBool,
    ) -> Result<Self, Error> {
        let contents = fs::read(pathbuf).map_err(Error::CannotReadEntryContents)?;

        // Optionally compress files
        let (maybe_gzip, maybe_zstd) = if should_compress.value {
            let gzip = gzip_compress(&contents)?;
            let zstd = zstd_compress(&contents)?;
            (gzip, zstd)
        } else {
            (None, None)
        };

        let content_type = file_content_type(pathbuf)?;

        // entry_path is only needed for the router (embed_assets!)
        let entry_path = if let Some(dir) = assets_dir_abs_str {
            if should_strip_html_ext.value && content_type == "text/html" {
                Some(
                    strip_html_ext(pathbuf)?
                        .strip_prefix(dir)
                        .unwrap_or_default(),
                )
            } else {
                pathbuf
                    .to_str()
                    .ok_or(Error::InvalidUnicodeInEntryName)?
                    .strip_prefix(dir)
            }
        } else {
            None
        };

        let etag_str = etag(&contents);
        let lit_byte_str_contents = LitByteStr::new(&contents, Span::call_site());
        let maybe_gzip = OptionBytesSlice(maybe_gzip);
        let maybe_zstd = OptionBytesSlice(maybe_zstd);

        Ok(Self {
            entry_path,
            content_type,
            etag_str,
            lit_byte_str_contents,
            maybe_gzip,
            maybe_zstd,
        })
    }
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

fn is_compression_significant(compressed_len: usize, contents_len: usize) -> bool {
    let ninety_pct_original = contents_len / 10 * 9;
    compressed_len < ninety_pct_original
}

fn maybe_get_compressed(compressed: &[u8], contents: &[u8]) -> Option<LitByteStr> {
    is_compression_significant(compressed.len(), contents.len())
        .then(|| LitByteStr::new(compressed, Span::call_site()))
}

/// Use `mime_guess` to get the best guess of the file's MIME type
/// by looking at its extension, or return an error if unable.
///
/// We accept the first guess because [`mime_guess` updates the order
/// according to the latest IETF RTC](https://docs.rs/mime_guess/2.0.5/mime_guess/struct.MimeGuess.html#note-ordering)
fn file_content_type(path: &Path) -> Result<String, error::Error> {
    match path.extension() {
        Some(ext) => {
            let guesses = mime_guess::MimeGuess::from_ext(
                ext.to_str()
                    .ok_or(error::Error::InvalidFileExtension(path.into()))?,
            );

            if let Some(guess) = guesses.first_raw() {
                Ok(guess.to_owned())
            } else {
                Err(error::Error::UnknownFileExtension(
                    path.extension().map(Into::into),
                ))
            }
        }
        None => Err(error::Error::UnknownFileExtension(None)),
    }
}

fn etag(contents: &[u8]) -> String {
    let sha256 = Sha1::digest(contents);
    let hash = u64::from_le_bytes(sha256[..8].try_into().unwrap())
        ^ u64::from_le_bytes(sha256[8..16].try_into().unwrap());
    format!("\"{hash:016x}\"")
}

fn strip_html_ext(entry: &Path) -> Result<&str, Error> {
    let entry_str = entry.to_str().ok_or(Error::InvalidUnicodeInEntryName)?;
    let mut output = entry_str;

    // Strip the extension
    if let Some(prefix) = output.strip_suffix(".html") {
        output = prefix;
    } else if let Some(prefix) = output.strip_suffix(".htm") {
        output = prefix;
    }

    // If it was `/index.html` or `/index.htm`, also remove `index`
    if output.ends_with("/index") {
        output = output.strip_suffix("index").unwrap_or("/");
    }

    Ok(output)
}
