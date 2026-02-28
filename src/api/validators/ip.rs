use crate::api::validators::domain::{ValidationError, Validator};

pub struct IpValidator;

impl Default for IpValidator {
    fn default() -> Self {
        Self
    }
}

impl IpValidator {
    pub fn new() -> Self {
        Self
    }
}

impl Validator for IpValidator {
    fn validate(&self, input: &str) -> Result<(), ValidationError> {
        let s = input.trim();

        // E006: Empty IP
        if s.is_empty() {
            return Err(ValidationError {
                code: "E006".to_string(),
                message: "IP address cannot be empty".to_string(),
                field: "ip".to_string(),
                line: None,
                column: None,
                suggestion: Some("Provide a valid IPv4 or IPv6 address".to_string()),
            });
        }

        // Try parsing as IPv4
        if s.parse::<std::net::Ipv4Addr>().is_ok() {
            return Ok(());
        }

        // Try parsing as IPv6
        if s.parse::<std::net::Ipv6Addr>().is_ok() {
            return Ok(());
        }

        // E007: Invalid IP format
        Err(ValidationError {
            code: "E007".to_string(),
            message: format!("Invalid IP address format: {}", s),
            field: "ip".to_string(),
            line: None,
            column: None,
            suggestion: Some(
                "Use IPv4 (e.g., 192.168.1.1) or IPv6 (e.g., 2001:db8::1)".to_string(),
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_ipv4() {
        let validator = IpValidator::new();
        assert!(validator.validate("192.168.1.1").is_ok());
        assert!(validator.validate("0.0.0.0").is_ok());
        assert!(validator.validate("255.255.255.255").is_ok());
        assert!(validator.validate("10.0.0.1").is_ok());
    }

    #[test]
    fn test_valid_ipv6() {
        let validator = IpValidator::new();
        assert!(validator.validate("::1").is_ok());
        assert!(validator.validate("2001:db8::1").is_ok());
        assert!(validator.validate("fe80::1").is_ok());
    }

    #[test]
    fn test_empty_ip() {
        let validator = IpValidator::new();
        let err = validator.validate("").unwrap_err();
        assert_eq!(err.code, "E006");
    }

    #[test]
    fn test_invalid_ipv4() {
        let validator = IpValidator::new();
        let err = validator.validate("192.168.1.256").unwrap_err();
        assert_eq!(err.code, "E007");
    }

    #[test]
    fn test_invalid_format() {
        let validator = IpValidator::new();
        let err = validator.validate("not-an-ip").unwrap_err();
        assert_eq!(err.code, "E007");
    }

    #[test]
    fn test_invalid_ipv6_format() {
        let validator = IpValidator::new();
        assert!(validator.validate("2001:db8::gggg").is_err());
    }
}
