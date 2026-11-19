# Range requests

Various utilities to deal with `Range Requests`, including requests for
multiple ranges, which are served when they coalesce into a single one, or
as `multipart/byteranges` with the `multipart` feature.

## Features

- `axum`: extract the `Range` header and turn the responses into axum
  responses.
- `multipart`: serve several disjoint ranges as `multipart/byteranges`
  content, streamed without copying the body, delimited by a boundary
  generated with `rand`.

## License

Licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
