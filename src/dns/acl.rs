#![allow(dead_code)]

use ipnet::IpNet;
use std::net::IpAddr;

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
