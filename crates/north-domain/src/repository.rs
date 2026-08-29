//! Pure repository metadata validation.
//!
//! Git access, credentials, and inspection stay outside the domain crate.

pub const MAX_NAME_BYTES: usize = 100;
pub const MAX_DESCRIPTION_BYTES: usize = 10_000;
pub const MAX_URL_BYTES: usize = 2_048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryMetadata {
    pub name: String,
    pub name_normalized: String,
    pub url: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryError {
    EmptyName,
    NameTooLong,
    DescriptionTooLong,
    EmptyUrl,
    UrlTooLong,
    InvalidUrl,
}

impl RepositoryError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::EmptyName => "empty_name",
            Self::NameTooLong => "name_too_long",
            Self::DescriptionTooLong => "description_too_long",
            Self::EmptyUrl => "empty_url",
            Self::UrlTooLong => "url_too_long",
            Self::InvalidUrl => "invalid_url",
        }
    }
}

/// Normalize and validate metadata before persistence.
pub fn validate_metadata(
    name: &str,
    url: &str,
    description: &str,
) -> Result<RepositoryMetadata, RepositoryError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(RepositoryError::EmptyName);
    }
    if name.len() > MAX_NAME_BYTES {
        return Err(RepositoryError::NameTooLong);
    }

    let description = description.trim();
    if description.len() > MAX_DESCRIPTION_BYTES {
        return Err(RepositoryError::DescriptionTooLong);
    }

    let url = url.trim();
    if url.is_empty() {
        return Err(RepositoryError::EmptyUrl);
    }
    if url.len() > MAX_URL_BYTES {
        return Err(RepositoryError::UrlTooLong);
    }
    if !supported_git_url(url) {
        return Err(RepositoryError::InvalidUrl);
    }

    Ok(RepositoryMetadata {
        name: name.to_owned(),
        name_normalized: name.to_lowercase(),
        url: url.to_owned(),
        description: description.to_owned(),
    })
}

/// Validate supported credential-free Git location shapes.
pub fn supported_git_url(value: &str) -> bool {
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return false;
    }
    if let Some(rest) = value.strip_prefix("https://") {
        return https_location(rest);
    }
    if let Some(rest) = value.strip_prefix("ssh://") {
        return ssh_location(rest);
    }
    scp_location(value)
}

fn https_location(rest: &str) -> bool {
    let Some((authority, path)) = rest.split_once('/') else {
        return false;
    };
    !authority.is_empty()
        && !authority.contains(['@', '?', '#', '%'])
        && !path.is_empty()
        && !path.starts_with('/')
        && !path.starts_with('?')
        && !path.starts_with('#')
        && !path.contains(['?', '#'])
        && valid_host_port(authority)
}

fn ssh_location(rest: &str) -> bool {
    let Some((authority, path)) = rest.split_once('/') else {
        return false;
    };
    if authority.is_empty() || path.is_empty() || path.starts_with('/') || path.starts_with('?') {
        return false;
    }
    let host = if let Some((user, host)) = authority.split_once('@') {
        if user != "git" || host.is_empty() || host.contains('@') || user.contains(':') {
            return false;
        }
        host
    } else {
        authority
    };
    valid_host_port(host) && !path.contains(['?', '#'])
}

fn scp_location(value: &str) -> bool {
    let Some((user, rest)) = value.split_once('@') else {
        return false;
    };
    if user != "git" || rest.contains('@') || rest.contains("://") {
        return false;
    }
    let (host, path, bracketed) = if let Some(value) = rest.strip_prefix('[') {
        let Some(close) = value.find(']') else {
            return false;
        };
        let suffix = &value[close + 1..];
        let Some(path) = suffix.strip_prefix(':') else {
            return false;
        };
        (&value[..close], path, true)
    } else {
        let Some((host, path)) = rest.split_once(':') else {
            return false;
        };
        (host, path, false)
    };
    (if bracketed {
        valid_ipv6(host)
    } else {
        valid_host(host)
    }) && !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['?', '#'])
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        })
}

fn valid_host_port(authority: &str) -> bool {
    if let Some(value) = authority.strip_prefix('[') {
        let Some(close) = value.find(']') else {
            return false;
        };
        let host = &value[..close];
        let suffix = &value[close + 1..];
        let port = if suffix.is_empty() {
            None
        } else {
            let Some(port) = suffix.strip_prefix(':') else {
                return false;
            };
            if port.is_empty() {
                return false;
            }
            Some(port)
        };
        return valid_ipv6(host) && valid_port(port);
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() => (host, Some(port)),
        _ => (authority, None),
    };
    valid_host(host) && valid_port(port)
}

fn valid_port(port: Option<&str>) -> bool {
    port.is_none_or(|port| {
        port.chars().all(|character| character.is_ascii_digit())
            && port.parse::<u16>().is_ok_and(|value| value > 0)
    })
}

fn valid_ipv6(host: &str) -> bool {
    host.parse::<std::net::Ipv6Addr>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_is_trimmed_and_normalized() {
        let metadata =
            validate_metadata(" North Repo ", "https://example.test/north.git", " desc ")
                .expect("valid metadata");
        assert_eq!(metadata.name, "North Repo");
        assert_eq!(metadata.name_normalized, "north repo");
        assert_eq!(metadata.description, "desc");
    }

    #[test]
    fn credential_free_git_shapes_are_explicit() {
        for url in [
            "https://github.com/org/repo.git",
            "https://[::1]/org/repo.git",
            "ssh://github.com/org/repo.git",
            "ssh://git@[::1]:22/org/repo.git",
            "ssh://git@github.com/org/repo.git",
            "git@[::1]:org/repo.git",
            "git@github.com:org/repo.git",
        ] {
            assert!(supported_git_url(url), "{url} must be accepted");
        }
        for url in [
            "https://user:password@example.com/repo.git",
            "https://token@example.com/org/repo.git",
            "ssh://git:password@example.com/org/repo.git",
            "ssh://deploy@example.com/org/repo.git",
            "deploy@example.com:org/repo.git",
            "file:///tmp/repo",
            "https://example.com",
            "https://<host>/repo.git",
            "https://[::1]junk/repo.git",
            "ssh://git@[::1]junk/repo.git",
            "https://example..com/repo.git",
            "https://example.com/repo?token=secret",
            "https://example.com/repo#fragment",
            "ssh://git@example.com:22/org/repo.git?token=secret",
        ] {
            assert!(!supported_git_url(url), "{url} must be rejected");
        }
    }

    #[test]
    fn limits_are_utf8_byte_limits() {
        assert_eq!(
            validate_metadata(&"é".repeat(50), "https://example.test/repo", ""),
            Ok(RepositoryMetadata {
                name: "é".repeat(50),
                name_normalized: "é".repeat(50),
                url: "https://example.test/repo".into(),
                description: String::new(),
            })
        );
        assert_eq!(
            validate_metadata(&"é".repeat(51), "https://example.test/repo", ""),
            Err(RepositoryError::NameTooLong)
        );
    }
}
