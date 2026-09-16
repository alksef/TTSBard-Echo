use crate::config::constants::{MAX_OPACITY, MIN_OPACITY};
use std::result::Result;
use url::Url;

pub fn validate_opacity(opacity: u8) -> u8 {
    opacity.clamp(MIN_OPACITY, MAX_OPACITY)
}

pub fn validate_hex_color(color: &str) -> Result<String, String> {
    let value = color.trim();
    let valid = value.len() == 7
        && value.as_bytes()[0] == b'#'
        && value.as_bytes()[1..]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit());
    if valid {
        Ok(value.to_ascii_uppercase())
    } else {
        Err("Invalid color format. Use #RRGGBB".to_string())
    }
}

pub fn validate_url(url: &str) -> Result<(), String> {
    if url.is_empty() {
        return Err("URL cannot be empty".to_string());
    }

    let parsed = Url::parse(url).map_err(|e| format!("Invalid URL format: {}", e))?;

    // Check if the scheme is http or https
    match parsed.scheme() {
        "http" | "https" => {
            // Check if host is present (even if it's empty, we don't allow that)
            if let Some(host) = parsed.host_str() {
                if host.is_empty() {
                    return Err("URL must have a non-empty host".to_string());
                }
            } else {
                return Err("URL must have a non-empty host".to_string());
            }
            Ok(())
        }
        _ => Err("URL must start with http:// or https://".to_string()),
    }
}

pub fn validate_connection_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("Connection ID cannot be empty".to_string());
    }
    if id.len() > 256 {
        return Err("Connection ID must be less than or equal to 256 characters".to_string());
    }
    Ok(())
}

pub fn validate_connection_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Connection name cannot be empty".to_string());
    }
    if name.len() > 256 {
        return Err("Connection name must be less than or equal to 256 characters".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_url_valid() {
        assert!(validate_url("https://example.com").is_ok());
        assert!(validate_url("http://localhost:8080").is_ok());
        assert!(validate_url("https://192.168.1.1:3000/path").is_ok());
    }

    #[test]
    fn test_validate_url_invalid_scheme() {
        assert!(validate_url("ftp://example.com").is_err());
        assert!(validate_url("file:///path").is_err());
        assert!(validate_url("ws://example.com").is_err());
    }

    #[test]
    fn test_validate_url_empty_host() {
        assert!(validate_url("https://").is_err());
        assert!(validate_url("http://").is_err());
        assert!(validate_url("https://   ").is_err());
    }

    #[test]
    fn test_validate_url_invalid_format() {
        assert!(validate_url("not a url").is_err());
        assert!(validate_url("").is_err());
    }

    #[test]
    fn test_validate_connection_id_valid() {
        assert!(validate_connection_id("valid_id").is_ok());
        assert!(validate_connection_id(&"a".repeat(256)).is_ok()); // max length
    }

    #[test]
    fn test_validate_connection_id_invalid() {
        assert!(validate_connection_id("").is_err());
        assert!(validate_connection_id(&"a".repeat(257)).is_err()); // too long
    }

    #[test]
    fn test_validate_connection_name_valid() {
        assert!(validate_connection_name("valid_name").is_ok());
        assert!(validate_connection_name(&"a".repeat(256)).is_ok()); // max length
    }

    #[test]
    fn test_validate_connection_name_invalid() {
        assert!(validate_connection_name("").is_err());
        assert!(validate_connection_name(&"a".repeat(257)).is_err()); // too long
    }

    #[test]
    fn test_validate_hex_color_normalizes_value() {
        assert_eq!(validate_hex_color(" #aBc123 ").unwrap(), "#ABC123");
        assert!(validate_hex_color("#12345").is_err());
        assert!(validate_hex_color("white").is_err());
    }
}
