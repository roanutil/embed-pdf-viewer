//! `packages/framework/web/src/external-uri.ts`: URI hygiene for links a PDF
//! supplies. A document's `/URI` can carry anything (`javascript:`, `file:`,
//! `data:`, custom app schemes); only the allowlisted schemes may be opened.
//!
//! The TypeScript applies this in its default UI adapter. Rust ships no
//! default adapter, so the dispatcher applies it before
//! [`ActionUiAdapter::open_uri`](crate::ActionUiAdapter) and an adapter only
//! ever receives an allowed URI.

/// The schemes a document link legitimately opens.
pub const DEFAULT_ALLOWED_URI_SCHEMES: [&str; 4] = ["http:", "https:", "mailto:", "tel:"];

/// `sanitizeExternalUri(uri, allowedSchemes)`: the trimmed URI when its
/// scheme is allowed (scheme-relative and bare URIs resolve as https), else
/// `None`. A scheme may be given with or without its trailing colon.
///
/// The scheme is read as the WHATWG URL parser reads it: leading and trailing
/// C0 controls and spaces are ignored, tabs and newlines anywhere are
/// removed, and a scheme is a letter followed by letters, digits, `+`, `-`
/// or `.` up to the first colon. Anything else is relative to an https base.
/// Unlike `new URL`, it does not validate the rest of the URI.
pub fn sanitize_external_uri<'a>(uri: &'a str, allowed_schemes: &[&str]) -> Option<&'a str> {
    let trimmed = uri.trim();
    if trimmed.is_empty() {
        return None;
    }
    let scheme = scheme_of(trimmed).unwrap_or_else(|| "https".to_string());
    let allowed = allowed_schemes.iter().any(|allowed| {
        let allowed = allowed.strip_suffix(':').unwrap_or(allowed);
        allowed.eq_ignore_ascii_case(&scheme)
    });
    allowed.then_some(trimmed)
}

/// The lowercased scheme, or `None` for a relative reference.
fn scheme_of(uri: &str) -> Option<String> {
    let mut chars =
        uri.trim_matches(|c: char| c <= ' ').chars().filter(|c| !matches!(c, '\t' | '\n' | '\r'));
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let mut scheme = String::from(first.to_ascii_lowercase());
    for c in chars {
        match c {
            ':' => return Some(scheme),
            c if c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.') => {
                scheme.push(c.to_ascii_lowercase());
            }
            _ => return None,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sanitize(uri: &str) -> Option<&str> {
        sanitize_external_uri(uri, &DEFAULT_ALLOWED_URI_SCHEMES)
    }

    #[test]
    fn allowed_schemes_pass_trimmed() {
        assert_eq!(sanitize("https://a.test/x"), Some("https://a.test/x"));
        assert_eq!(sanitize("  HTTP://a.test "), Some("HTTP://a.test"));
        assert_eq!(sanitize("mailto:a@b.test"), Some("mailto:a@b.test"));
        assert_eq!(sanitize("tel:+15550100"), Some("tel:+15550100"));
    }

    #[test]
    fn relative_references_resolve_as_https() {
        assert_eq!(sanitize("//host/x"), Some("//host/x"));
        assert_eq!(sanitize("example.com/x"), Some("example.com/x"));
        assert_eq!(sanitize("/path?q=1"), Some("/path?q=1"));
    }

    #[test]
    fn other_schemes_are_refused() {
        for uri in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "java\tscript:alert(1)",
            "java\nscript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,<b>x</b>",
            "vbscript:msgbox(1)",
            "myapp://open",
            "localhost:3000",
            "",
            "   ",
        ] {
            assert_eq!(sanitize(uri), None, "{uri:?}");
        }
    }

    #[test]
    fn a_custom_allowlist_takes_schemes_with_or_without_the_colon() {
        assert_eq!(sanitize_external_uri("myapp://open", &["myapp"]), Some("myapp://open"));
        assert_eq!(sanitize_external_uri("https://a.test", &["myapp:"]), None);
    }
}
