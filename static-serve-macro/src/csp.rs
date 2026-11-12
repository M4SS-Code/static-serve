//! Read the Content Security Policy that an HTML document declares in its
//! `<head>` through `<meta http-equiv="Content-Security-Policy">`
//!
//! This is the smallest subset of the HTML tokenizer that finds these elements
//! the way a browser does: it skips comments and the contents of raw text
//! elements, and stops where the parser would close `<head>`. The `<meta>` is
//! left in the document, so a policy missed here is still enforced by the
//! browser, and a policy found where a browser wouldn't look only makes the
//! response stricter.

use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum CspError {
    #[error("Unsupported character reference `&{0};` in the policy")]
    UnsupportedCharacterReference(String),
    #[error("The policy contains a character that isn't allowed in a header: {0:?}")]
    InvalidHeaderCharacter(char),
    #[error("The policy contains a comma, which would split it into separate policies in a header")]
    Comma,
}

/// Elements that can appear in `<head>` without closing it
const HEAD_ELEMENTS: &[&str] = &[
    "base", "basefont", "bgsound", "link", "meta", "noframes", "noscript", "script", "style",
    "template", "title",
];

/// Elements whose contents can't hold tags, or don't belong to `<head>`
const SKIPPED_CONTENTS: &[&str] = &[
    "noframes", "noscript", "script", "style", "template", "title",
];

/// Collect the policies declared in the `<head>` of `html` into the value of a
/// `Content-Security-Policy` header, where a comma separates policies
pub(crate) fn meta_policies(html: &[u8]) -> Result<Option<String>, CspError> {
    let mut policies = Vec::new();
    let mut rest = html;

    while let Some(start) = rest.iter().position(|&byte| byte == b'<') {
        rest = &rest[start + 1..];

        if let Some(comment) = rest.strip_prefix(b"!--") {
            let Some(end) = find(comment, b"-->") else {
                break;
            };
            rest = &comment[end + 3..];
            continue;
        }
        if rest.starts_with(b"!") || rest.starts_with(b"?") {
            // Doctype or bogus comment
            rest = skip_past(rest, b">");
            continue;
        }

        let is_end_tag = rest.starts_with(b"/");
        let name_start = usize::from(is_end_tag);
        if !rest.get(name_start).is_some_and(u8::is_ascii_alphabetic) {
            continue;
        }

        let (name, after_name) = take_while(&rest[name_start..], |byte| {
            !byte.is_ascii_whitespace() && byte != b'/' && byte != b'>'
        });
        let name = name.to_ascii_lowercase();
        let (attributes, after_tag) = parse_attributes(after_name);
        rest = after_tag;

        if is_end_tag {
            if name == b"head" || name == b"html" || name == b"body" || name == b"br" {
                break;
            }
            continue;
        }

        if name == b"html" || name == b"head" {
            continue;
        }
        let Some(&name) = HEAD_ELEMENTS
            .iter()
            .find(|element| element.as_bytes() == name)
        else {
            break;
        };

        if name == "meta" {
            let http_equiv = attribute(&attributes, b"http-equiv");
            if http_equiv
                .is_some_and(|value| value.eq_ignore_ascii_case(b"content-security-policy"))
                && let Some(content) = attribute(&attributes, b"content")
            {
                // CSP splits on ASCII whitespace, so a policy written over
                // several lines means the same with spaces, which fit a header
                let policy =
                    decode_character_references(content)?.replace(['\n', '\r', '\x0c'], " ");
                let policy = policy.trim_matches(|c: char| c.is_ascii_whitespace());
                if !policy.is_empty() {
                    validate_header_value(policy)?;
                    policies.push(policy.to_owned());
                }
            }
        } else if SKIPPED_CONTENTS.contains(&name) {
            rest = skip_to_end_tag(rest, name.as_bytes());
        }
    }

    Ok((!policies.is_empty()).then(|| policies.join(", ")))
}

/// The lowercased name and the raw value of an attribute
type Attribute<'a> = (Vec<u8>, &'a [u8]);

/// Parse the attributes of a start tag, returning them and what follows `>`
fn parse_attributes(mut rest: &[u8]) -> (Vec<Attribute<'_>>, &[u8]) {
    let mut attributes = Vec::new();

    loop {
        rest = trim_start(rest, |byte| byte.is_ascii_whitespace() || byte == b'/');
        match rest.first() {
            None => return (attributes, rest),
            Some(b'>') => return (attributes, &rest[1..]),
            Some(_) => {}
        }

        // The first character can be `=`, so it's taken unconditionally
        let (name, after_name) = take_while(&rest[1..], |byte| {
            !byte.is_ascii_whitespace() && !matches!(byte, b'/' | b'>' | b'=')
        });
        let name = [&rest[..1], name].concat().to_ascii_lowercase();
        rest = trim_start(after_name, |byte| byte.is_ascii_whitespace());

        let mut value: &[u8] = b"";
        if let Some(after_equals) = rest.strip_prefix(b"=") {
            rest = trim_start(after_equals, |byte| byte.is_ascii_whitespace());
            match rest.first() {
                Some(&quote @ (b'"' | b'\'')) => {
                    let (quoted, after_value) = take_while(&rest[1..], |byte| byte != quote);
                    value = quoted;
                    rest = after_value.get(1..).unwrap_or_default();
                }
                _ => {
                    (value, rest) =
                        take_while(rest, |byte| !byte.is_ascii_whitespace() && byte != b'>');
                }
            }
        }

        // The first of repeated attributes wins
        if attribute(&attributes, &name).is_none() {
            attributes.push((name, value));
        }
    }
}

fn attribute<'a>(attributes: &[Attribute<'a>], name: &[u8]) -> Option<&'a [u8]> {
    attributes
        .iter()
        .find(|(attribute, _)| attribute == name)
        .map(|&(_, value)| value)
}

/// Decode the character references a serializer would use for a policy
///
/// Policies are ASCII, so only the numeric references and the named ones for
/// ASCII characters that need escaping are decoded. Any other named reference
/// fails instead of being kept as text.
fn decode_character_references(value: &[u8]) -> Result<String, CspError> {
    let value = String::from_utf8_lossy(value);
    let mut decoded = String::with_capacity(value.len());
    let mut rest = &*value;

    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        rest = &rest[start + 1..];

        let reference_len = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '#')
            .unwrap_or(rest.len());
        let reference = &rest[..reference_len];
        if reference.is_empty() {
            decoded.push('&');
            continue;
        }
        if !rest[reference_len..].starts_with(';') {
            return Err(CspError::UnsupportedCharacterReference(
                reference.to_owned(),
            ));
        }

        let character = match reference {
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "lt" => Some('<'),
            "gt" => Some('>'),
            _ => reference.strip_prefix('#').and_then(|number| {
                let code_point = match number.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16),
                    None => number.parse(),
                };
                code_point.ok().and_then(char::from_u32)
            }),
        };
        let Some(character) = character else {
            return Err(CspError::UnsupportedCharacterReference(
                reference.to_owned(),
            ));
        };
        decoded.push(character);
        rest = &rest[reference_len + 1..];
    }
    decoded.push_str(rest);

    Ok(decoded)
}

/// Check that `HeaderValue::from_static` accepts the policy, and that a header
/// keeps it a single policy
fn validate_header_value(policy: &str) -> Result<(), CspError> {
    for c in policy.chars() {
        match c {
            ',' => return Err(CspError::Comma),
            '\t' | ' '..='~' => {}
            c => return Err(CspError::InvalidHeaderCharacter(c)),
        }
    }

    Ok(())
}

/// Skip the contents of a raw text element, up to and including its end tag
fn skip_to_end_tag<'a>(mut rest: &'a [u8], name: &[u8]) -> &'a [u8] {
    while let Some(start) = find(rest, b"</") {
        rest = &rest[start + 2..];
        let is_end_tag = rest
            .get(..name.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
            && rest
                .get(name.len())
                .is_none_or(|&byte| byte.is_ascii_whitespace() || byte == b'/' || byte == b'>');
        if is_end_tag {
            return skip_past(rest, b">");
        }
    }

    b""
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn skip_past<'a>(rest: &'a [u8], needle: &[u8]) -> &'a [u8] {
    find(rest, needle).map_or(b"", |position| &rest[position + needle.len()..])
}

fn take_while(bytes: &[u8], predicate: impl Fn(u8) -> bool) -> (&[u8], &[u8]) {
    let end = bytes
        .iter()
        .position(|&byte| !predicate(byte))
        .unwrap_or(bytes.len());
    bytes.split_at(end)
}

fn trim_start(bytes: &[u8], predicate: impl Fn(u8) -> bool) -> &[u8] {
    take_while(bytes, predicate).1
}

#[cfg(test)]
mod tests {
    use super::{CspError, meta_policies};

    fn policies(html: &str) -> Option<String> {
        meta_policies(html.as_bytes()).unwrap()
    }

    #[test]
    fn reads_the_sveltekit_meta() {
        let html = r#"<!doctype html>
<html lang="en">
	<head>
		<meta charset="utf-8" />
		<meta http-equiv="content-security-policy" content="script-src 'self' 'sha256-abc='; object-src &#39;none&#39;">
		<link rel="modulepreload" href="/_app/immutable/entry/start.js">
	</head>
	<body></body>
</html>"#;
        assert_eq!(
            policies(html).as_deref(),
            Some("script-src 'self' 'sha256-abc='; object-src 'none'")
        );
    }

    #[test]
    fn matches_attributes_like_a_browser() {
        assert_eq!(
            policies("<META HTTP-EQUIV=Content-Security-Policy CONTENT=default-src>").as_deref(),
            Some("default-src")
        );
        assert_eq!(
            policies(r#"<meta content='a &quot;b&quot;' http-equiv="CONTENT-security-POLICY"/>"#)
                .as_deref(),
            Some(r#"a "b""#)
        );
        assert_eq!(policies(r#"<meta http-equiv="refresh" content="5">"#), None);
        assert_eq!(
            policies(r#"<meta http-equiv="content-security-policy" content="  ">"#),
            None
        );
    }

    #[test]
    fn joins_multiple_policies() {
        let html = r#"<meta http-equiv="content-security-policy" content="a">
<meta http-equiv="content-security-policy" content="b">"#;
        assert_eq!(policies(html).as_deref(), Some("a, b"));
    }

    #[test]
    fn ignores_metas_browsers_ignore() {
        let meta = r#"<meta http-equiv="content-security-policy" content="a">"#;
        for html in [
            format!("<!-- {meta} -->"),
            format!("<script>let s = '{meta}';</script>"),
            format!("<title>{meta}</title>"),
            format!("<template>{meta}</template>"),
            format!("<head></head>{meta}"),
            format!("<body>{meta}"),
            format!("<div></div>{meta}"),
        ] {
            assert_eq!(policies(&html), None, "{html}");
        }

        // The end tag of a raw text element must match its whole name
        assert_eq!(
            policies(&format!("<script></scripts>{meta}</script>{meta}")).as_deref(),
            Some("a")
        );
    }

    #[test]
    fn rejects_policies_that_dont_fit_a_header() {
        for (content, expected) in [
            ("a, b", "Comma"),
            ("a&nbsp;b", "UnsupportedCharacterReference"),
            ("a&ampb", "UnsupportedCharacterReference"),
            ("caf\u{e9}", "InvalidHeaderCharacter"),
            ("a\u{7f}b", "InvalidHeaderCharacter"),
        ] {
            let html =
                format!(r#"<meta http-equiv="content-security-policy" content="{content}">"#);
            let error = meta_policies(html.as_bytes()).unwrap_err();
            let name = match error {
                CspError::Comma => "Comma",
                CspError::UnsupportedCharacterReference(_) => "UnsupportedCharacterReference",
                CspError::InvalidHeaderCharacter(_) => "InvalidHeaderCharacter",
            };
            assert_eq!(name, expected, "{content}");
        }

        // A lone ampersand is text
        assert_eq!(
            policies(r#"<meta http-equiv="content-security-policy" content="a & b">"#).as_deref(),
            Some("a & b")
        );
    }

    #[test]
    fn turns_line_breaks_into_spaces() {
        let html = "<meta http-equiv=\"content-security-policy\" content=\"\n  default-src 'self';\r\n  img-src *\x0c\n\">";
        assert_eq!(
            policies(html).as_deref(),
            Some("default-src 'self';    img-src *")
        );
        assert_eq!(
            policies(r#"<meta http-equiv="content-security-policy" content="a&#10;b">"#).as_deref(),
            Some("a b")
        );
    }
}
