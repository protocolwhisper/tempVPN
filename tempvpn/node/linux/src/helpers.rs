use axum::http::HeaderMap;

pub fn endpoint_host(endpoint: &str) -> &str {
    endpoint
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(endpoint)
}

pub fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
}

pub fn registry_token_matches(configured: Option<&str>, supplied: Option<&str>) -> bool {
    configured.is_some() && configured == supplied
}

#[cfg(test)]
mod tests {
    use super::registry_token_matches;

    #[test]
    fn registry_auth_requires_its_own_configured_token() {
        assert!(registry_token_matches(Some("registry"), Some("registry")));
        assert!(!registry_token_matches(Some("registry"), Some("admin")));
        assert!(!registry_token_matches(None, None));
    }
}
