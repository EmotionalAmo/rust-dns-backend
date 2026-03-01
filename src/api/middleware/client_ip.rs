use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::request::Parts,
};
use std::net::SocketAddr;

/// An extractor that resolves the client's real IP address.
/// It checks `X-Forwarded-For` and `X-Real-IP` headers first (for reverse proxies like Nginx/Vite),
/// and falls back to the TCP peer address `ConnectInfo`.
pub struct ClientIp(pub String);

impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // 1. Check X-Forwarded-For (can be a comma-separated list, first element is original client)
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

        // 2. Check X-Real-IP
        if let Some(real_ip) = parts.headers.get("x-real-ip") {
            if let Ok(s) = real_ip.to_str() {
                let ip = s.trim();
                if !ip.is_empty() {
                    return Ok(ClientIp(ip.to_string()));
                }
            }
        }

        // 3. Fallback to ConnectInfo
        if let Ok(ConnectInfo(addr)) =
            ConnectInfo::<SocketAddr>::from_request_parts(parts, state).await
        {
            return Ok(ClientIp(addr.ip().to_string()));
        }

        Ok(ClientIp("unknown".to_string()))
    }
}
