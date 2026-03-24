//! Connection URL helpers.

/// Lowercases only the URL scheme portion before the first `://`.
pub(crate) fn lowercase_scheme(url: &str) -> String {
    let t = url.trim();
    let Some(idx) = t.find("://") else {
        return t.to_string();
    };
    let (scheme, rest) = t.split_at(idx);
    format!("{}{}", scheme.to_ascii_lowercase(), rest)
}
