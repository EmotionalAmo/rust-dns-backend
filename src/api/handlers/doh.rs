/// DNS-over-HTTPS (DoH) endpoint — RFC 8484
///
/// Supports both wire-format transports:
///   GET  /dns-query?dns=<base64url>   — base64url-encoded DNS wire format
///   POST /dns-query                   — body is raw DNS wire format
///
/// Response: `Content-Type: application/dns-message`, body is DNS wire format.
///
/// This endpoint is public (no authentication required) — typical DoH servers are
/// open resolvers.  Rate-limiting at the reverse-proxy layer is recommended in
/// production.  The underlying DnsHandler applies the same filter/cache/rewrite
/// logic as UDP/TCP DNS queries.
use crate::api::AppState;
use axum::{
    body::Bytes,
    extract::{ConnectInfo, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hickory_proto::op::Message;
use hickory_proto::serialize::binary::BinDecodable;
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;

const DNS_MESSAGE_CONTENT_TYPE: &str = "application/dns-message";
/// RFC 8484 §6: maximum wire-format message size for DoH.
const MAX_DNS_MESSAGE_BYTES: usize = 65_535;
/// Maximum base64url length for the GET `dns` parameter.
/// base64 expands 3 bytes → 4 chars, so 65535 bytes → at most 87380 chars (+ padding).
const MAX_DOH_GET_PARAM_LEN: usize = 87_384;

#[derive(Deserialize)]
pub struct DohGetParams {
    dns: String,
}

/// GET /dns-query?dns=<base64url>
pub async fn get_query(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(params): Query<DohGetParams>,
) -> Response {
    // Guard against excessively large base64 strings before decoding (M-3 fix)
    if params.dns.len() > MAX_DOH_GET_PARAM_LEN {
        return (StatusCode::BAD_REQUEST, "DNS message parameter too large").into_response();
    }
    let data = match URL_SAFE_NO_PAD.decode(&params.dns) {
        Ok(d) => d,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Invalid base64url encoding").into_response();
        }
    };

    resolve_doh(state, data, peer.ip().to_string()).await
}

/// POST /dns-query  (Content-Type: application/dns-message)
pub async fn post_query(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Bytes,
) -> Response {
    if body.len() > MAX_DNS_MESSAGE_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "DNS message too large").into_response();
    }

    resolve_doh(state, body.to_vec(), peer.ip().to_string()).await
}

async fn resolve_doh(state: Arc<AppState>, data: Vec<u8>, client_ip: String) -> Response {
    match state.dns_handler.handle(data, client_ip).await {
        Ok(response_bytes) => {
            let mut res = Response::new(axum::body::Body::from(response_bytes.clone()));
            res.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(DNS_MESSAGE_CONTENT_TYPE),
            );
            // RFC 8484 §5.1: Cache-Control max-age is derived from the minimum TTL
            // across answer records.  Fall back to no-store when unparseable or TTL=0.
            let cache_control = Message::from_bytes(&response_bytes)
                .ok()
                .and_then(|msg| msg.answers().iter().map(|r| r.ttl()).min())
                .filter(|&ttl| ttl > 0)
                .map(|ttl| format!("max-age={ttl}"))
                .unwrap_or_else(|| "no-store".to_string());
            if let Ok(val) = HeaderValue::from_str(&cache_control) {
                res.headers_mut().insert(header::CACHE_CONTROL, val);
            }
            res
        }
        Err(e) => {
            tracing::warn!("DoH handler error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "DNS resolution failed").into_response()
        }
    }
}
