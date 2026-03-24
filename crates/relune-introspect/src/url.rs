//! Connection URL helpers.

/// Masks credentials in a database URL for safe logging.
///
/// Replaces `user:password@` with `***:***@` so that secrets
/// are never emitted in tracing spans or log lines.
pub(crate) fn mask_credentials(url: &str) -> String {
    // scheme://userinfo@host...  →  scheme://***:***@host...
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let after_scheme = scheme_end + 3; // skip past "://"
    let rest = &url[after_scheme..];

    // If there is no '@', there are no credentials to mask.
    let Some(at) = rest.find('@') else {
        return url.to_string();
    };

    format!("{}://***:***@{}", &url[..scheme_end], &rest[at + 1..])
}

/// Lowercases only the URL scheme portion before the first `://`.
pub(crate) fn lowercase_scheme(url: &str) -> String {
    let t = url.trim();
    let Some(idx) = t.find("://") else {
        return t.to_string();
    };
    let (scheme, rest) = t.split_at(idx);
    format!("{}{}", scheme.to_ascii_lowercase(), rest)
}
