use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::request::Parts,
};
use std::net::{IpAddr, SocketAddr};

/// Returns true if this IP should be treated as a trusted reverse-proxy peer.
/// Only loopback and RFC 1918 private addresses are trusted by default.
/// This prevents external clients from spoofing their IP via proxy headers.
fn is_trusted_peer(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private(),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}

/// An extractor that resolves the client's real IP address.
///
/// `X-Forwarded-For` and `X-Real-IP` headers are **only** trusted when the TCP
/// peer address is a loopback or RFC 1918 private address (i.e. a trusted reverse
/// proxy such as Nginx running on the same host or internal network).  Direct
/// connections from untrusted peers fall back to the TCP peer address, preventing
/// IP-spoofing via forged proxy headers.
pub struct ClientIp(pub String);

impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Resolve the actual TCP peer address first.
        let peer_ip = ConnectInfo::<SocketAddr>::from_request_parts(parts, state)
            .await
            .ok()
            .map(|ConnectInfo(addr)| addr.ip());

        // Only honour proxy headers when the connection comes from a trusted peer.
        if peer_ip.as_ref().map(is_trusted_peer).unwrap_or(false) {
            // 1. X-Forwarded-For — comma-separated list, first value is the original client
            if let Some(forwarded_for) = parts.headers.get("x-forwarded-for") {
                if let Ok(s) = forwarded_for.to_str() {
                    if let Some(ip) = s.split(',').next() {
                        let ip = ip.trim();
                        if !ip.is_empty() {
                            return Ok(ClientIp(ip.to_string()));
                        }
                    }
                }
            }

            // 2. X-Real-IP
            if let Some(real_ip) = parts.headers.get("x-real-ip") {
                if let Ok(s) = real_ip.to_str() {
                    let ip = s.trim();
                    if !ip.is_empty() {
                        return Ok(ClientIp(ip.to_string()));
                    }
                }
            }
        }

        // Fallback: use the TCP peer address directly.
        if let Some(ip) = peer_ip {
            return Ok(ClientIp(ip.to_string()));
        }

        Ok(ClientIp("unknown".to_string()))
    }
}
