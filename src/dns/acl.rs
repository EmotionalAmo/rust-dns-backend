use ipnet::IpNet;
use std::net::IpAddr;
use std::str::FromStr;

pub struct Acl {
    allowed: Vec<IpNet>,
    denied: Vec<IpNet>,
}

impl Default for Acl {
    fn default() -> Self {
        Self::new()
    }
}

impl Acl {
    pub fn new() -> Self {
        Self {
            allowed: vec![],
            denied: vec![],
        }
    }

    /// Build an ACL from CIDR string lists.  Invalid entries are skipped with a warning.
    pub fn from_cidrs(allowed: &[String], denied: &[String]) -> Self {
        let parse = |cidrs: &[String]| -> Vec<IpNet> {
            cidrs
                .iter()
                .filter_map(|s| {
                    let s = s.trim();
                    if s.is_empty() {
                        return None;
                    }
                    // Accept bare IPs (e.g. "192.168.1.1") by appending /32 or /128
                    let cidr = if s.contains('/') {
                        s.to_string()
                    } else if s.contains(':') {
                        format!("{}/128", s) // IPv6
                    } else {
                        format!("{}/32", s) // IPv4
                    };
                    match IpNet::from_str(&cidr) {
                        Ok(net) => Some(net),
                        Err(_) => {
                            tracing::warn!("ACL: invalid CIDR '{}', skipping", s);
                            None
                        }
                    }
                })
                .collect()
        };

        Self {
            allowed: parse(allowed),
            denied: parse(denied),
        }
    }

    /// Validate a list of CIDR strings.  Returns the list of invalid entries.
    pub fn validate_cidrs(cidrs: &[String]) -> Vec<String> {
        cidrs
            .iter()
            .filter(|s| {
                let s = s.trim();
                if s.is_empty() {
                    return false;
                }
                let cidr = if s.contains('/') {
                    s.to_string()
                } else if s.contains(':') {
                    format!("{}/128", s)
                } else {
                    format!("{}/32", s)
                };
                IpNet::from_str(&cidr).is_err()
            })
            .cloned()
            .collect()
    }

    pub fn is_allowed(&self, ip: IpAddr) -> bool {
        // If no rules, allow all
        if self.allowed.is_empty() && self.denied.is_empty() {
            return true;
        }
        // Check deny list first
        for net in &self.denied {
            if net.contains(&ip) {
                return false;
            }
        }
        // Check allow list
        if self.allowed.is_empty() {
            return true;
        }
        self.allowed.iter().any(|net| net.contains(&ip))
    }
}
