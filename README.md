# Static Serve

A Rust library for compressing and embedding static assets in a web server using [Axum](https://github.com/tokio-rs/axum). This crate provides efficient asset embedding with optional compression (`gzip` and `zstd`) and conditional requests support.

## Features

- **Embed static assets** at compile-time for efficient serving

- **Automatic compression** with `gzip` and `zstd`

- **ETag support** for conditional requests and caching

- **Seamless Axum integration** with request extraction for encoding and caching headers

## Installation

Add the following to your `Cargo.toml`:

```toml
[dependencies]
static-serve = "0.2"
axum = "0.8"
```

## Usage

### Embedding a directory of static assets

Use the `embed_assets!` macro to create a `static_router()` function in scope which will include your static files, embedding them into your binary:

```rust
use static_serve::embed_assets;

embed_assets!("assets", compress = true);
let router = static_router();
```

This will:

- Include all files from the `assets` directory
- Compress them using `gzip` and `zstd` (if beneficial)
- Generate a `static_router()` function to serve these assets

#### Required parameter

- `path_to_dir` - a valid `&str` string literal of the path to the static files to be included

#### Optional parameters

- `compress = false` - compress static files with zstd and gzip, true or false (defaults to false)

- `ignore_dirs = [my_ignore_dir, other_ignore_dir]` - a bracketed list of `&str`s of the paths/subdirectories inside the target directory, which should be ignored and not included. (If this parameter is missing, no subdirectories will be ignored)

- `strip_html_ext = false` - strips the `.html` or `.htm` from all HTML files included. If the filename is `index.html` or `index.htm`, the `index` part will also be removed, leaving just the root (defaults to false)

### Embedding a single static asset file

Use the `embed_asset!` macro to return a function you can use as a GET handler, which will include your static file, embedded into your binary:

```rust
use static_serve::embed_assets;

let router: Router<()> = Router::new();
let handler = embed_asset!("assets/my_file.png", compress = true);
let router = router.route("/my_file.png", handler);

```

This will:

- Include the file `my_file.png` from the `assets` directory
- Compress it using `gzip` and `zstd` (if beneficial)
- Generate a `MethodRouter` "handler" you can add as a route on your router to serve the file

#### Required parameter

- `path_to_file` - a valid `&str` string literal of the path to the static file to be included

#### Optional parameter

- `compress = false` - compress a static file with zstd and gzip, true or false (defaults to false)

## Conditional Requests & Caching

The crate automatically handles:

- `Accept-Encoding` header to serve compressed versions if available
- `If-None-Match` header for ETag validation, returning `304 Not Modified` if unchanged

## Example

```rust
use axum::{Router, Server};
use static_serve::{embed_assets, embed_asset};

embed_assets!("public", compress = true);

#[tokio::main]
async fn main() {
    let router = static_router();
    let my_file_handler = embed_asset!("other_files/my_file.txt");
    let router = router.route("/other_files/my_file.txt", my_file_handler);

    Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(router.into_make_service())
        .await
        .unwrap();
}
```

## License

Licensed under either of

- Apache License, Version 2.0, (LICENSE-APACHE or [https://www.apache.org/licenses/LICENSE-2.0](https://www.apache.org/licenses/LICENSE-2.0))
- MIT license (LICENSE-MIT or [https://opensource.org/licenses/MIT](https://opensource.org/licenses/MIT))

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
