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

    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];

    // If there is no '@' in the authority portion, there are no credentials to mask.
    let Some(at) = authority.rfind('@') else {
        return url.to_string();
    };

    format!(
        "{}://***:***@{}{}",
        &url[..scheme_end],
        &authority[at + 1..],
        &rest[authority_end..]
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_credentials_and_preserves_path() {
        let masked = mask_credentials("postgres://user%40domain:pass@localhost/db");
        assert_eq!(masked, "postgres://***:***@localhost/db");
    }

    #[test]
    fn does_not_mask_urls_without_credentials() {
        let url = "postgres://localhost/db@v1";
        assert_eq!(mask_credentials(url), url);
    }
}
