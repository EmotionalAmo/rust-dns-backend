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
use axum::{
    body::Bytes,
    extract::{ConnectInfo, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;
use crate::api::AppState;

const DNS_MESSAGE_CONTENT_TYPE: &str = "application/dns-message";
/// RFC 8484 §6: maximum wire-format message size for DoH.
const MAX_DNS_MESSAGE_BYTES: usize = 65_535;

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
            let mut res = Response::new(axum::body::Body::from(response_bytes));
            res.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(DNS_MESSAGE_CONTENT_TYPE),
            );
            // RFC 8484 §5.1: cache-control is derived from the DNS TTL; we use a
            // conservative default here.  A production implementation could inspect
            // the response TTL and set the header accordingly.
            res.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("no-store"),
            );
            res
        }
        Err(e) => {
            tracing::warn!("DoH handler error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "DNS resolution failed").into_response()
        }
    }
}
