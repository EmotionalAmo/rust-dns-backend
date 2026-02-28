use crate::api::validators::domain::DomainValidator;
use crate::api::validators::domain::{ValidationError, Validator};
use crate::api::validators::ip::IpValidator;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleValidationRequest {
    #[serde(rename = "type")]
    pub rule_type: String,
    pub rule: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleValidationResponse {
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ValidationError>,
}

pub struct RuleValidator;

impl Default for RuleValidator {
    fn default() -> Self {
        Self
    }
}

impl RuleValidator {
    pub fn new() -> Self {
        Self
    }

    #[allow(clippy::result_large_err)]
    pub fn validate_rule(&self, rule_type: &str, rule: &str) -> Result<(), ValidationError> {
        let rule = rule.trim();

        // E008: Empty rule
        if rule.is_empty() {
            return Err(ValidationError {
                code: "E008".to_string(),
                message: "Rule cannot be empty".to_string(),
                field: "rule".to_string(),
                line: None,
                column: None,
                suggestion: Some("Provide a valid rule".to_string()),
            });
        }

        match rule_type {
            "filter" => self.validate_filter_rule(rule),
            "rewrite" => self.validate_rewrite_rule(rule),
            _ => Err(ValidationError {
                code: "E011".to_string(),
                message: format!("Unknown rule type: {}", rule_type),
                field: "type".to_string(),
                line: None,
                column: None,
                suggestion: Some("Use 'filter' or 'rewrite' as rule type".to_string()),
            }),
        }
    }

    #[allow(clippy::result_large_err)]
    fn validate_filter_rule(&self, rule: &str) -> Result<(), ValidationError> {
        // Support AdGuard format: ||domain^
        // Support hosts format: 0.0.0.0 domain
        // Support plain domain

        let domain_validator = DomainValidator::new();

        // AdGuard format: ||domain^
        if rule.starts_with("||") {
            let domain_part = rule.trim_start_matches("||").trim_end_matches('^');
            return domain_validator.validate(domain_part).map_err(|mut e| {
                e.field = "rule".to_string();
                e
            });
        }

        // White list format: @@||domain^
        if rule.starts_with("@@||") {
            let domain_part = rule.trim_start_matches("@@||").trim_end_matches('^');
            return domain_validator.validate(domain_part).map_err(|mut e| {
                e.field = "rule".to_string();
                e
            });
        }

        // Hosts format: IP domain
        if rule.contains(char::is_whitespace) {
            let parts: Vec<&str> = rule.split_whitespace().collect();
            if parts.len() >= 2 {
                let ip = parts[0];
                let domain = parts[1];

                // Validate IP first
                IpValidator::new().validate(ip)?;

                // Validate domain
                return domain_validator.validate(domain).map_err(|mut e| {
                    e.field = "rule".to_string();
                    e
                });
            }
        }

        // Plain domain (fallback)
        DomainValidator::new().validate(rule).map_err(|mut e| {
            e.field = "rule".to_string();
            e
        })
    }

    #[allow(clippy::result_large_err)]
    fn validate_rewrite_rule(&self, rule: &str) -> Result<(), ValidationError> {
        // Format: domain -> IP
        // Example: myapp.local -> 192.168.1.100

        let parts: Vec<&str> = rule.split("->").collect();

        // E009: Invalid rewrite format
        if parts.len() != 2 {
            return Err(ValidationError {
                code: "E009".to_string(),
                message: "Rewrite rule must be in format: domain -> IP".to_string(),
                field: "rule".to_string(),
                line: None,
                column: Some(rule.find("->").map_or(rule.len(), |pos| pos)),
                suggestion: Some("Example: myapp.local -> 192.168.1.100".to_string()),
            });
        }

        let domain = parts[0].trim();
        let ip = parts[1].trim();

        // E010: Empty domain or IP in rewrite
        if domain.is_empty() || ip.is_empty() {
            return Err(ValidationError {
                code: "E010".to_string(),
                message: "Domain and IP cannot be empty in rewrite rule".to_string(),
                field: "rule".to_string(),
                line: None,
                column: Some(rule.find("->").map_or(0, |pos| pos + 2)),
                suggestion: Some("Example: myapp.local -> 192.168.1.100".to_string()),
            });
        }

        // Validate domain
        DomainValidator::new().validate(domain).map_err(|mut e| {
            e.field = "rule".to_string();
            e
        })?;

        // Validate IP
        IpValidator::new().validate(ip).map_err(|mut e| {
            e.field = "rule".to_string();
            e
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_filter_rules() {
        let validator = RuleValidator::new();

        // AdGuard format
        assert!(validator.validate_rule("filter", "||example.com^").is_ok());
        assert!(validator
            .validate_rule("filter", "||ads.example.com^")
            .is_ok());

        // White list format
        assert!(validator
            .validate_rule("filter", "@@||example.com^")
            .is_ok());

        // Hosts format
        assert!(validator
            .validate_rule("filter", "0.0.0.0 example.com")
            .is_ok());
        assert!(validator
            .validate_rule("filter", "127.0.0.1 ads.example.com")
            .is_ok());

        // Plain domain
        assert!(validator.validate_rule("filter", "example.com").is_ok());
    }

    #[test]
    fn test_valid_rewrite_rules() {
        let validator = RuleValidator::new();

        assert!(validator
            .validate_rule("rewrite", "myapp.local -> 192.168.1.100")
            .is_ok());
        assert!(validator
            .validate_rule("rewrite", "example.com -> ::1")
            .is_ok());
        assert!(validator
            .validate_rule("rewrite", "  myapp.local  ->  192.168.1.100  ")
            .is_ok());
    }

    #[test]
    fn test_empty_rule() {
        let validator = RuleValidator::new();
        let err = validator.validate_rule("filter", "").unwrap_err();
        assert_eq!(err.code, "E008");
    }

    #[test]
    fn test_invalid_rewrite_format() {
        let validator = RuleValidator::new();

        // Missing arrow
        let err = validator
            .validate_rule("rewrite", "myapp.local 192.168.1.100")
            .unwrap_err();
        assert_eq!(err.code, "E009");

        // Multiple arrows
        let err = validator
            .validate_rule("rewrite", "myapp.local -> IP -> 192.168.1.100")
            .unwrap_err();
        assert_eq!(err.code, "E009");
    }

    #[test]
    fn test_invalid_rewrite_parts() {
        let validator = RuleValidator::new();

        // Empty domain
        let err = validator
            .validate_rule("rewrite", " -> 192.168.1.100")
            .unwrap_err();
        assert_eq!(err.code, "E010");

        // Empty IP
        let err = validator
            .validate_rule("rewrite", "myapp.local -> ")
            .unwrap_err();
        assert_eq!(err.code, "E010");
    }

    #[test]
    fn test_invalid_rule_type() {
        let validator = RuleValidator::new();
        let err = validator
            .validate_rule("unknown", "example.com")
            .unwrap_err();
        assert_eq!(err.code, "E011");
    }

    #[test]
    fn test_invalid_domain_in_filter() {
        let validator = RuleValidator::new();
        assert!(validator.validate_rule("filter", "||ex@mple.com^").is_err());
    }

    #[test]
    fn test_invalid_ip_in_hosts_format() {
        let validator = RuleValidator::new();
        assert!(validator
            .validate_rule("filter", "999.999.999.999 example.com")
            .is_err());
    }
}
