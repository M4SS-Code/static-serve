# Static Serve

Embed static assets into your [axum](https://github.com/tokio-rs/axum) server at compile time, with gzip and zstd compression. Responses support content negotiation, `ETag`s, conditional requests and byte ranges.

## Usage

```toml
[dependencies]
static-serve = "0.6"
axum = "0.8"
```

```rust,ignore
use axum::Router;
use static_serve::{embed_asset, embed_assets};
use tokio::net::TcpListener;

// Generates `static_router()`, with one route per file in `public`
embed_assets!("public", compress = true, cache_busted_paths = ["immutable"]);

#[tokio::main]
async fn main() {
    let robots = embed_asset!("other/robots.txt");
    let router: Router = static_router().route("/robots.txt", robots);

    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, router).await.unwrap();
}
```

Relative paths start at the directory containing the crate's `Cargo.toml`.

## Options

| Option | Macro | Default | Effect |
|---|---|---|---|
| `compress` | both | `false` | Also embed gzip and zstd bodies, when they save at least 10% |
| `allow_unknown_extensions` | both | `false` | Serve unknown or missing extensions as `application/octet-stream` instead of failing the build |
| `cache_bust` | `embed_asset!` | `false` | Send `Cache-Control: public, max-age=31536000, immutable` |
| `cache_busted_paths` | `embed_assets!` | `[]` | The same, for these files and directories. Their names must already change with their content, for example by including a hash |
| `ignore_paths` | `embed_assets!` | `[]` | Skip these files and directories |
| `strip_html_ext` | `embed_assets!` | `false` | Serve `foo.html` at `/foo`, `index.html` at `/` and `docs/index.html` at `/docs/`, with no redirect from `/docs` |

Paths in `cache_busted_paths` and `ignore_paths` are relative to the embedded directory, and must exist inside of it.

## Which files are embedded

- Hidden files are embedded like any other. List the ones you don't want in `ignore_paths`.
- Symlinks are followed and served at the path of the link.
- Names are percent-encoded in routes, so `my file.png` is served at `/my%20file.png`.
- A path segment starting with `:` or `*` fails the build, because axum refuses it as a route.

## HTTP behavior

- The body is selected from `Accept-Encoding`, and each body has its own strong `ETag`.
- `If-None-Match` and `If-Match` are evaluated against that `ETag`. `Last-Modified` isn't sent, so date-based conditions don't apply.
- Single byte ranges and `If-Range` are supported on the uncompressed body. `Range` is ignored when a compressed body is selected, so send `Accept-Encoding: identity` to use ranges.

## Build times

Changing or deleting an embedded file triggers a rebuild, but adding one doesn't. To pick up new files, add a `build.rs` next to `Cargo.toml`:

```rust,ignore
fn main() {
    println!("cargo:rerun-if-changed=assets");
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

