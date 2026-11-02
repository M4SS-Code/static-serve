# Static Serve

A Rust library for compressing and embedding static assets in a web server using [Axum](https://github.com/tokio-rs/axum). This crate provides efficient asset embedding with optional compression (`gzip` and `zstd`) and conditional requests support.

## Features

- **Embed static assets** at compile-time for efficient serving

- **Automatic compression** with `gzip` and `zstd`

- **ETag support** for conditional requests and caching

- **HTTP Range Requests** for partial content responses (RFC 9110)

- **Seamless Axum integration** with request extraction for encoding and caching headers

## Installation

Add the following to your `Cargo.toml`:

```toml
[dependencies]
static-serve = "0.6"
axum = "0.8"
```

## Usage

### Embedding a directory of static assets

Use the `embed_assets!` macro to create a `static_router()` function in scope which will include your static files, embedding them into your binary:

```rust,ignore
use axum::Router;
use static_serve::embed_assets;

embed_assets!("assets", compress = true, ignore_paths = ["temp.txt","temp"], cache_busted_paths = ["immutable"]);

let router: Router<()> = static_router();
```

This will:

- Include all files from the `assets` directory except `temp.txt` and the `temp` directory
- Compress them using `gzip` and `zstd` (if beneficial)
- For only files in `assets/immutable`, add a `Cache-Control` header with `public, max-age=31536000, immutable` (since these are marked as cache-busted paths)
- Generate a `static_router()` function to serve these assets

#### Required parameter

- `path_to_dir` - a valid `&str` string literal of the path to the static files to be included. Relative paths start at the directory containing the crate's `Cargo.toml`

#### Optional parameters

- `compress = false` - compress static files with zstd and gzip, true or false (defaults to false)

- `ignore_paths = ["my_ignore_dir", "other_ignore_dir", "my_ignore_file.txt"]` - a bracketed list of `&str`s of paths/subdirectories/files inside the target directory, which should be ignored and not included. (If this parameter is missing, no paths/subdirectories/files will be ignored)

- `strip_html_ext = false` - strips the `.html` or `.htm` from all HTML files included. If the filename is `index.html` or `index.htm`, the `index` part will also be removed, so `index.html` is served at `/` and `docs/index.html` at `/docs/`, with no redirect from `/docs` (defaults to false)

- `cache_busted_paths = ["my_immutables_dir", "my_immutable_file"]` - a bracketed list of `&str`s of the subdirectories and/or single files which should gain the `Cache-Control` header with `public, max-age=31536000, immutable` for cache-busted paths. If this parameter is missing, the default is that no embedded files will have the `Cache-Control` header. Note: the files in `cache_busted_paths` need to already be compatible with cache-busting by having hashes in their file paths (for example). All `static-serve` does is set the appropriate header. 

- `allow_unknown_extensions = false` - serve files with unknown extensions as `application/octet-stream` content-type; when not set to `true`, compilation fails if a content type cannot be guessed from the extension, or if the file has no extension

#### Which files are embedded

- Hidden files are embedded like any other file. List the ones you don't want in `ignore_paths`
- Symlinks are followed and served at the path of the link, not of their target. Entries of `ignore_paths` and `cache_busted_paths` match the link's path, and must be inside the target directory
- File names are percent-encoded in routes, so `my file.png` is served at `/my%20file.png`. A path segment starting with `:` or `*` is a compile error, because axum refuses it as a route

#### Rebuilding after adding files

Cargo tracks the embedded files, but not the directory: changing or deleting a file triggers a rebuild, while adding one doesn't. To pick up new files, add a `build.rs` to the crate calling the macro:

```rust,ignore
fn main() {
    println!("cargo:rerun-if-changed=assets");
}
```

#### Compile times

With `compress = true` every asset is compressed each time the crate calling the macro is compiled, including `cargo check`. If that gets slow, put the macro call in a separate crate containing only the assets, which is rarely recompiled.

### Embedding a single static asset file

Use the `embed_asset!` macro to return a `MethodRouter` serving GET and HEAD, which will include your static file, embedded into your binary:

```rust,ignore
use axum::Router;
use static_serve::embed_asset;

let router: Router<()> = Router::new();
let handler = embed_asset!("assets/my_file.png", compress = true, cache_bust = true);
let router = router.route("/my_file.png", handler);

```

This will:

- Include the file `my_file.png` from the `assets` directory
- Compress it using `gzip` and `zstd` (if beneficial)
- Add a `Cache-Control` header with the value `public, max-age=31536000, immutable` for cache-busted paths. Note: the file in `embed_asset!` needs to already be compatible with cache-busting by having a hash in its file path (for example). All `static-serve` does is set the appropriate header. 
- Generate a `MethodRouter` "handler" you can add as a route on your router to serve the file

#### Required parameter

- `path_to_file` - a valid `&str` string literal of the path to the static file to be included. Relative paths start at the directory containing the crate's `Cargo.toml`

#### Optional parameters

- `compress = false` - compress a static file with zstd and gzip, true or false (defaults to false)
- `cache_bust = false` - add a `Cache-Control` header with the value `public, max-age=31536000, immutable` for a cache-busted asset (defaults to false)
- `allow_unknown_extensions = false` - serve files with unknown extensions as `application/octet-stream` content-type; when not set to `true`, compilation   fails if a content type cannot be guessed from the extension, or if the file has no extension

## Conditional Requests & Caching

The crate automatically handles:

- `Accept-Encoding` header to serve compressed versions if available
- `If-None-Match` header for ETag validation, returning `304 Not Modified` if unchanged. The uncompressed, gzip and zstd bodies of a file each have their own `ETag`
- `If-Match` header, returning `412 Precondition Failed` if no entry matches the `ETag`

- With the optional cache-bust headers feature, each embedded file in the `cache_busted_paths` array (or single file in the case of `embed_asset!` with `cache_bust = true`) will be returned with a `Cache-Control` header with the value `public, max-age=31536000, immutable`. Note: the files involved need to already be compatible with cache-busting by having hashes in their file paths (for example). All `static-serve` does is set the appropriate header.

- `Accept-Ranges: bytes` is advertised on uncompressed responses. When a `Range` header is present, the server responds with `206 Partial Content` and the requested byte range, or `416 Range Not Satisfiable` if the range can't be satisfied. Malformed and multi-range values are ignored. Ranges only apply to the uncompressed body, so `Range` is ignored and the full body is served when the request selects a compressed one. Send `Accept-Encoding: identity` to use ranges.

## Example

```rust,ignore
use axum::Router;
use static_serve::{embed_asset, embed_assets};
use tokio::net::TcpListener;

embed_assets!("public", compress = true);

#[tokio::main]
async fn main() {
    let router: Router<()> = static_router();
    let my_file_handler = embed_asset!("other_files/my_file.txt");
    let router = router.route("/other_files/my_file.txt", my_file_handler);

    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, router).await.unwrap();
}
```

## License

Licensed under either of

- Apache License, Version 2.0, (LICENSE-APACHE or [https://www.apache.org/licenses/LICENSE-2.0](https://www.apache.org/licenses/LICENSE-2.0))
- MIT license (LICENSE-MIT or [https://opensource.org/licenses/MIT](https://opensource.org/licenses/MIT))

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

