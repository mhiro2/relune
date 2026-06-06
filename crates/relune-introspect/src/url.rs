//! Connection URL helpers.

/// Masks credentials in a database URL for safe logging.
///
/// Replaces `user:password@` with `***:***@` and redacts secret query
/// parameters so that secrets are never emitted in tracing spans or log lines.
///
/// The authority is located before the query string is split off, so a raw
/// (unencoded) `?` or `#` inside the password cannot smuggle credentials past
/// the masker. The userinfo boundary is also detected fail-closed: an authority
/// that was truncated mid-`user:password` by a raw separator is still masked
/// (see [`userinfo_separator`]).
#[must_use]
pub fn mask_credentials(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        // No authority component (e.g. `sqlite:file.db?...`); only query
        // secrets are maskable.
        return mask_query_in_tail(url);
    };
    let scheme = &url[..scheme_end];
    let rest = &url[scheme_end + 3..];

    let Some(at) = userinfo_separator(rest) else {
        return format!("{scheme}://{}", mask_query_in_tail(rest));
    };

    // The host ends at the first path/query/fragment delimiter after the
    // userinfo separator; everything before that delimiter is `host[:port]`.
    let host_start = at + 1;
    let host_end = rest[host_start..]
        .find(['/', '?', '#'])
        .map_or(rest.len(), |offset| host_start + offset);
    let host = &rest[host_start..host_end];
    let tail = &rest[host_end..];

    format!("{scheme}://***:***@{host}{}", mask_query_in_tail(tail))
}

/// Locates the byte index of the `@` separating userinfo from host when the
/// authority carries credentials.
///
/// The authority normally terminates at the first `/`, `?`, or `#`, and any
/// `@` within it is the userinfo separator. A raw (unencoded) `/`, `?`, or `#`
/// inside the password truncates that span before the real `@`, so a fallback
/// recovers it: when the truncated span looks like the leading half of a
/// `user:password` pair (it contains a `:`) and a later `@` is followed by a
/// host *and* a path delimiter, that `@` is the hidden userinfo terminator.
///
/// Requiring a path delimiter after the candidate `@` is what distinguishes a
/// hidden separator (`user:p/w@host/db` — `@host/db` has a `/`) from a
/// credential-free URL whose path or port merely contains an `@`
/// (`host:5432/db@tag` — nothing structured follows the `@`). The masker errs
/// toward redaction: the residual false positives are rare credential-free URLs
/// with an `@` mid-path, which only lose log fidelity, never leak a secret.
fn userinfo_separator(rest: &str) -> Option<usize> {
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];

    if let Some(at) = authority.rfind('@') {
        return Some(at);
    }

    // The naive authority was truncated before any `@`. Only a `:`-bearing span
    // (the start of `user:password`) can hide credentials this way.
    if !authority.contains(':') {
        return None;
    }

    let at = authority_end + rest[authority_end..].find('@')?;
    rest[at + 1..].contains(['/', '?', '#']).then_some(at)
}

/// Masks secret query parameters in the portion of a URL after the host,
/// leaving the path and fragment untouched.
fn mask_query_in_tail(tail: &str) -> String {
    let (prefix, fragment) = tail
        .split_once('#')
        .map_or((tail, None), |(head, frag)| (head, Some(frag)));
    let (base, query) = prefix
        .split_once('?')
        .map_or((prefix, None), |(head, q)| (head, Some(q)));

    let mut masked = base.to_string();
    if let Some(query) = query {
        masked.push('?');
        masked.push_str(&mask_query_secrets(query));
    }
    if let Some(fragment) = fragment {
        masked.push('#');
        masked.push_str(fragment);
    }
    masked
}

pub(crate) fn sanitize_error_message(database_url: &str, message: &str) -> String {
    let sanitized = candidate_urls(database_url).into_iter().fold(
        message.to_string(),
        |sanitized, candidate| {
            let masked = mask_credentials(&candidate);
            if masked == candidate {
                sanitized
            } else {
                sanitized.replace(&candidate, &masked)
            }
        },
    );
    sanitize_message_database_urls(&sanitized, &candidate_schemes(database_url))
}

fn candidate_urls(database_url: &str) -> Vec<String> {
    let trimmed = database_url.trim();
    if trimmed.is_empty() {
        return vec![];
    }

    let mut candidates = vec![trimmed.to_string()];
    let lowered = lowercase_scheme(trimmed);
    push_candidate(&mut candidates, lowered.clone());
    add_scheme_aliases(&mut candidates, &lowered);
    candidates
}

fn candidate_schemes(database_url: &str) -> Vec<&'static str> {
    let trimmed = database_url.trim();
    if trimmed.is_empty() {
        return vec![];
    }

    let lowered = lowercase_scheme(trimmed);
    if lowered.starts_with("postgres://") || lowered.starts_with("postgresql://") {
        vec!["postgres://", "postgresql://"]
    } else if lowered.starts_with("mysql://") || lowered.starts_with("mariadb://") {
        vec!["mysql://", "mariadb://"]
    } else if lowered.starts_with("sqlite:") {
        vec!["sqlite:"]
    } else {
        vec![]
    }
}

fn push_candidate(candidates: &mut Vec<String>, candidate: String) {
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

fn add_scheme_aliases(candidates: &mut Vec<String>, url: &str) {
    if let Some(rest) = url.strip_prefix("mariadb://") {
        push_candidate(candidates, format!("mysql://{rest}"));
    } else if let Some(rest) = url.strip_prefix("mysql://") {
        push_candidate(candidates, format!("mariadb://{rest}"));
    }

    if let Some(rest) = url.strip_prefix("postgres://") {
        push_candidate(candidates, format!("postgresql://{rest}"));
    } else if let Some(rest) = url.strip_prefix("postgresql://") {
        push_candidate(candidates, format!("postgres://{rest}"));
    }
}

fn sanitize_message_database_urls(message: &str, schemes: &[&str]) -> String {
    if schemes.is_empty() {
        return message.to_string();
    }

    let mut sanitized = String::with_capacity(message.len());
    let mut cursor = 0;

    while cursor < message.len() {
        let Some((start, scheme)) = next_scheme_match(message, cursor, schemes) else {
            sanitized.push_str(&message[cursor..]);
            break;
        };

        sanitized.push_str(&message[cursor..start]);
        let end = url_token_end(message, start + scheme.len());
        let (url, trailing) = split_trailing_url_punctuation(&message[start..end]);
        sanitized.push_str(&mask_credentials(url));
        sanitized.push_str(trailing);
        cursor = end;
    }

    sanitized
}

fn next_scheme_match<'a>(
    message: &'a str,
    cursor: usize,
    schemes: &[&'a str],
) -> Option<(usize, &'a str)> {
    schemes
        .iter()
        .filter_map(|scheme| {
            message[cursor..]
                .find(scheme)
                .map(|offset| (cursor + offset, *scheme))
        })
        .min_by_key(|(offset, _scheme)| *offset)
}

fn url_token_end(message: &str, start: usize) -> usize {
    let tail = &message[start..];
    let offset = tail
        .find(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | '<' | '>' | '`'))
        .unwrap_or(tail.len());
    start + offset
}

fn split_trailing_url_punctuation(token: &str) -> (&str, &str) {
    let trimmed_len = token
        .trim_end_matches([',', '.', ';', '!', '?', ')', ']', '}'])
        .len();
    token.split_at(trimmed_len)
}

fn mask_query_secrets(query: &str) -> String {
    query
        .split('&')
        .map(mask_query_parameter)
        .collect::<Vec<_>>()
        .join("&")
}

fn mask_query_parameter(parameter: &str) -> String {
    let Some((name, _value)) = parameter.split_once('=') else {
        return if is_secret_query_parameter(parameter) {
            format!("{parameter}=***")
        } else {
            parameter.to_string()
        };
    };

    if is_secret_query_parameter(name) {
        format!("{name}=***")
    } else {
        parameter.to_string()
    }
}

fn is_secret_query_parameter(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "password"
            | "pass"
            | "pwd"
            | "token"
            | "access_token"
            | "auth_token"
            | "api_key"
            | "apikey"
            | "secret"
            | "sslkey"
    ) || normalized.ends_with("_password")
        || normalized.ends_with("_token")
        || normalized.ends_with("_secret")
        || normalized.ends_with("_key")
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

    #[test]
    fn does_not_mask_authority_with_at_sign_only_in_path() {
        // An `@` that appears in the path with nothing structured after it
        // carries no credentials, so neither a port nor an IPv6 host may be
        // mistaken for userinfo.
        for url in [
            "postgres://localhost:5432/db@v1",
            "postgres://[::1]/db@tag",
            "postgres://[::1]:5432/db@tag",
        ] {
            assert_eq!(mask_credentials(url), url, "should not mask {url}");
        }
    }

    #[test]
    fn masks_credentials_with_unencoded_password_separators() {
        // Raw `/`, `?`, and `#` inside a password truncate the naive authority
        // before the real `@`; the masker must still redact the credentials,
        // even when the password is numeric (and would parse as a port).
        for url in [
            "postgres://user:p/w@host/db",
            "postgres://user:p?w@host/db",
            "postgres://user:p#w@host/db",
            "postgres://user:123/w@host/db",
        ] {
            assert_eq!(
                mask_credentials(url),
                "postgres://***:***@host/db",
                "failed to mask {url}"
            );
        }
    }

    #[test]
    fn masks_credentials_for_bracketed_ipv6_hosts() {
        let masked = mask_credentials("postgres://user:pass@[::1]:5432/db");
        assert_eq!(masked, "postgres://***:***@[::1]:5432/db");
    }

    #[test]
    fn masks_credentials_when_password_separator_precedes_query() {
        // The query string is split only after the authority is located, so a
        // raw `?` in the password does not leak the credentials or the secret.
        let masked = mask_credentials("postgres://user:p?w@host/db?password=hunter2");
        assert_eq!(masked, "postgres://***:***@host/db?password=***");
    }

    #[test]
    fn masks_secret_query_parameters() {
        let masked = mask_credentials(
            "postgres://user:pass@localhost/db?password=hunter2&token=abc&sslmode=require",
        );
        assert_eq!(
            masked,
            "postgres://***:***@localhost/db?password=***&token=***&sslmode=require"
        );
    }

    #[test]
    fn sanitize_error_message_handles_mariadb_scheme_rewrites() {
        let message = "failed to connect to mysql://user:pass@localhost/db?token=abc";
        let sanitized =
            sanitize_error_message("mariadb://user:pass@localhost/db?token=abc", message);
        assert_eq!(
            sanitized,
            "failed to connect to mysql://***:***@localhost/db?token=***"
        );
    }

    #[test]
    fn sanitize_error_message_handles_postgres_scheme_rewrites() {
        let message = "failed to connect to postgresql://user:pass@localhost/db?password=hunter2";
        let sanitized = sanitize_error_message(
            "postgres://user:pass@localhost/db?password=hunter2",
            message,
        );
        assert_eq!(
            sanitized,
            "failed to connect to postgresql://***:***@localhost/db?password=***"
        );
    }

    #[test]
    fn sanitize_error_message_masks_query_secrets_after_upstream_credential_masking() {
        let message =
            "failed to connect to postgresql://***:***@localhost/db?sslmode=require&token=abc";
        let sanitized = sanitize_error_message(
            "postgres://user:pass@localhost/db?sslmode=require&token=abc",
            message,
        );
        assert_eq!(
            sanitized,
            "failed to connect to postgresql://***:***@localhost/db?sslmode=require&token=***"
        );
    }
}
