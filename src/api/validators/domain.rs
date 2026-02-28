use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub code: String,
    pub message: String,
    pub field: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

pub trait Validator {
    #[allow(clippy::result_large_err)]
    fn validate(&self, input: &str) -> Result<(), ValidationError>;
}

pub struct DomainValidator;

impl Default for DomainValidator {
    fn default() -> Self {
        Self
    }
}

impl DomainValidator {
    pub fn new() -> Self {
        Self
    }

    fn is_valid_label_char(c: char) -> bool {
        c.is_alphanumeric() || c == '-'
    }
}

impl Validator for DomainValidator {
    fn validate(&self, input: &str) -> Result<(), ValidationError> {
        let s = input.trim();

        // E001: Empty domain
        if s.is_empty() {
            return Err(ValidationError {
                code: "E001".to_string(),
                message: "Domain cannot be empty".to_string(),
                field: "domain".to_string(),
                line: None,
                column: None,
                suggestion: Some("Provide a valid domain like example.com".to_string()),
            });
        }

        // E002: Domain too long (RFC 1035)
        if s.len() > 253 {
            return Err(ValidationError {
                code: "E002".to_string(),
                message: format!("Domain exceeds 253 characters (got {})", s.len()),
                field: "domain".to_string(),
                line: None,
                column: None,
                suggestion: Some("Use a shorter domain name".to_string()),
            });
        }

        // Remove trailing dot for validation
        let domain = s.trim_end_matches('.');

        // Handle wildcard domains (*.example.com)
        let labels: Vec<&str> = if let Some(rest) = domain.strip_prefix("*.") {
            if rest.is_empty() {
                return Err(ValidationError {
                    code: "E003".to_string(),
                    message: "Wildcard domain missing target domain".to_string(),
                    field: "domain".to_string(),
                    line: None,
                    column: Some(1),
                    suggestion: Some("Use *.example.com format".to_string()),
                });
            }
            let mut labels = vec!["*"];
            labels.extend(rest.split('.'));
            labels
        } else {
            domain.split('.').collect()
        };

        // E003: Invalid label length
        for (i, label) in labels.iter().enumerate() {
            if label.is_empty() {
                return Err(ValidationError {
                    code: "E003".to_string(),
                    message: "Domain contains empty label".to_string(),
                    field: "domain".to_string(),
                    line: None,
                    column: Some(domain.len() - i), // Approximate position
                    suggestion: Some("Ensure domain has no consecutive dots".to_string()),
                });
            }

            if label.len() > 63 {
                return Err(ValidationError {
                    code: "E003".to_string(),
                    message: format!("Label '{}' exceeds 63 characters", label),
                    field: "domain".to_string(),
                    line: None,
                    column: None,
                    suggestion: Some("Use shorter labels in the domain".to_string()),
                });
            }

            // Skip wildcard label validation
            if *label == "*" {
                continue;
            }

            // E004: Invalid characters in label
            if !label.chars().all(Self::is_valid_label_char) {
                return Err(ValidationError {
                    code: "E004".to_string(),
                    message: format!("Label '{}' contains invalid characters", label),
                    field: "domain".to_string(),
                    line: None,
                    column: None,
                    suggestion: Some(
                        "Labels can only contain letters, digits, and hyphens".to_string(),
                    ),
                });
            }

            // E005: Label cannot start or end with hyphen
            if label.starts_with('-') || label.ends_with('-') {
                return Err(ValidationError {
                    code: "E005".to_string(),
                    message: format!("Label '{}' cannot start or end with hyphen", label),
                    field: "domain".to_string(),
                    line: None,
                    column: None,
                    suggestion: Some("Remove leading/trailing hyphens".to_string()),
                });
            }
        }

        // At least 2 labels (e.g., example.com)
        if labels.len() < 2 {
            return Err(ValidationError {
                code: "E004".to_string(),
                message: "Domain must have at least 2 labels (e.g., example.com)".to_string(),
                field: "domain".to_string(),
                line: None,
                column: None,
                suggestion: Some("Use a fully qualified domain name".to_string()),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_domains() {
        let validator = DomainValidator::new();
        assert!(validator.validate("example.com").is_ok());
        assert!(validator.validate("sub.example.com").is_ok());
        assert!(validator.validate("my-sub.example.org").is_ok());
        assert!(validator.validate("*.example.com").is_ok());
        assert!(validator.validate("example.com.").is_ok()); // Trailing dot OK
    }

    #[test]
    fn test_empty_domain() {
        let validator = DomainValidator::new();
        let err = validator.validate("").unwrap_err();
        assert_eq!(err.code, "E001");
    }

    #[test]
    fn test_domain_too_long() {
        let validator = DomainValidator::new();
        let long_domain = "a".repeat(300);
        let err = validator.validate(&long_domain).unwrap_err();
        assert_eq!(err.code, "E002");
    }

    #[test]
    fn test_empty_label() {
        let validator = DomainValidator::new();
        let err = validator.validate("example..com").unwrap_err();
        assert_eq!(err.code, "E003");
    }

    #[test]
    fn test_label_too_long() {
        let validator = DomainValidator::new();
        let long_label = format!("{}.com", "a".repeat(100));
        let err = validator.validate(&long_label).unwrap_err();
        assert_eq!(err.code, "E003");
    }

    #[test]
    fn test_invalid_characters() {
        let validator = DomainValidator::new();
        let err = validator.validate("ex@mple.com").unwrap_err();
        assert_eq!(err.code, "E004");
    }

    #[test]
    fn test_invalid_hyphen_position() {
        let validator = DomainValidator::new();
        assert!(validator.validate("-example.com").is_err());
        assert!(validator.validate("example-.com").is_err());
    }

    #[test]
    fn test_wildcard_domains() {
        let validator = DomainValidator::new();
        assert!(validator.validate("*.example.com").is_ok());
        assert!(validator.validate("**.example.com").is_err());
        assert!(validator.validate("*.").is_err());
    }
}
