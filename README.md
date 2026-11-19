# Static Serve

Embed static assets into your [axum](https://github.com/tokio-rs/axum) server at compile time, with gzip and zstd compression. Responses support content negotiation, `ETag`s, conditional requests and byte ranges.

## Usage

```toml
[dependencies]
static-serve = "0.6"
axum = "0.8"
tokio = { version = "1", features = ["macros", "net", "rt-multi-thread"] }
```

```rust,ignore
use axum::Router;
use static_serve::{embed_asset, embed_assets};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let robots = embed_asset!("other/robots.txt");
    // One route per file in `public`
    let router: Router = embed_assets!("public", compress = true, cache_busted_paths = ["immutable"])
        .route("/robots.txt", robots);

    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, router).await.unwrap();
}
```

Relative paths start at the directory containing the crate's `Cargo.toml`. Each macro call embeds its own copy of the files, so to use the same assets in several places, call the macro once in a function of your own and call that.

## Options

| Option | Macro | Default | Effect |
|---|---|---|---|
| `compress` | both | `false` | Also embed gzip and zstd bodies, when they save more than 10% |
| `allow_unknown_extensions` | both | `false` | Serve unknown or missing extensions as `application/octet-stream` instead of failing the build |
| `utf8_charset` | both | `true` | Add `charset=utf-8` to the `Content-Type` of text files that are valid UTF-8, except for types that already define their encoding: `text/javascript`, `text/calendar`, `text/vtt` and `text/xml` |
| `cache_bust` | `embed_asset!` | `false` | Send `Cache-Control: public, max-age=31536000, immutable` |
| `cache_busted_paths` | `embed_assets!` | `[]` | The same, for these files and directories. Their names must already change with their content, for example by including a hash |
| `ignore_paths` | `embed_assets!` | `[]` | Skip these files and directories |
| `strip_html_ext` | `embed_assets!` | `false` | Serve `foo.html` at `/foo`, `index.html` at `/` and `docs/index.html` at `/docs/`, with no redirect from `/docs` |

Paths in `cache_busted_paths` and `ignore_paths` are relative to the embedded directory, and must exist inside of it.

Set `utf8_charset = false` if a layer above the router sets the charset, or for HTML that declares a legacy encoding with `<meta charset>` while only containing ASCII: browsers let the HTTP charset override `<meta>`, which also changes the encoding of the forms and URLs the page submits.

## Features

- `multipart` (off by default): answer a request for several disjoint ranges of the uncompressed body with a `206` holding them as `multipart/byteranges` content, each part with the file's `Content-Type`. More than 32 ranges left after merging still get the full `200` response.

## Which files are embedded

- Hidden files are embedded like any other. List the ones you don't want in `ignore_paths`.
- Symlinks are followed and served at the path of the link, not of their target. Entries of `ignore_paths` and `cache_busted_paths` match the link's path.
- Names are percent-encoded in routes the way browsers encode them, so `my file.png` is served at `/my%20file.png`. Other spellings of the same path, such as lowercase hex digits, get 404. See the `embed_assets!` documentation for the exact rules.
- A path segment starting with `:` or `*` fails the build, because axum refuses it as a route.

## HTTP behavior

- The body is selected from `Accept-Encoding`, and each body has its own strong `ETag`. A request that refuses every body of a file gets `406 Not Acceptable`.
- `If-None-Match` and `If-Match` are evaluated against that `ETag`. `Last-Modified` isn't sent, so date-based conditions don't apply.
- Byte ranges and `If-Range` are supported on the uncompressed body, which is the only one sent with `Accept-Ranges: bytes`. `Range` is ignored when a compressed body is selected, so send `Accept-Encoding: identity` to use ranges. Overlapping and nearby ranges are merged and unsatisfiable ones dropped, so a request that is left with one range gets a single-part `206`, and one with none gets `416`. Several disjoint ranges get the full `200` response, unless the `multipart` feature is enabled. Malformed `Range` headers get the full `200` response, and `HEAD` always ignores `Range`.
- `If-Match` and `If-Range` only match a strong `ETag`. CDNs and proxies that compress responses themselves turn it into a weak one, so those conditions then fail. To keep them working, serve the precompressed bodies with `compress = true` and turn off recompression in front of the server.

## Build times

Changing or deleting an embedded file triggers a rebuild, but adding one doesn't. To pick up new files, add a `build.rs` next to `Cargo.toml`:

```rust,ignore
fn main() {
    println!("cargo:rerun-if-changed=public");
}
```

With `compress = true`, assets are compressed every time the crate calling the macro is compiled, including `cargo check`. If that gets slow, move the macro call into a separate crate.

## License

Licensed under either of

- Apache License, Version 2.0, (LICENSE-APACHE or [https://www.apache.org/licenses/LICENSE-2.0](https://www.apache.org/licenses/LICENSE-2.0))
- MIT license (LICENSE-MIT or [https://opensource.org/licenses/MIT](https://opensource.org/licenses/MIT))

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

