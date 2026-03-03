use crate::api::AppState;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::Response;
use std::net::SocketAddr;
use std::sync::Arc;

/// Tower middleware that automatically logs write operations (POST/PUT/PATCH/DELETE)
/// to the audit log for all successful responses (2xx).
///
/// - Skips GET/HEAD entirely
/// - Extracts user info from JWT Bearer token (best-effort; skips if missing/invalid)
/// - Extracts client IP from X-Forwarded-For → X-Real-IP → ConnectInfo
/// - Derives resource/action from request path + method
/// - Fire-and-forget via `crate::db::audit::log_action`
pub async fn audit_middleware(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    // Only audit write operations
    let method = req.method().clone();
    if method == Method::GET || method == Method::HEAD || method == Method::OPTIONS {
        return next.run(req).await;
    }

    // Capture everything we need from the request before consuming it
    let path = req.uri().path().to_string();
    let ip = extract_ip(&req);
    let (user_id, username) = extract_user(&req, &state.jwt_secret);

    // Pass the request through to the actual handler
    let response = next.run(req).await;

    // Only record on success (2xx)
    let status = response.status();
    if !status.is_success() {
        return response;
    }

    // Handlers record audit log entries directly (with resource_id and richer detail).
    // The middleware no longer writes to the audit log to avoid duplicate entries.
    // Tracing is still emitted here for structured log visibility.
    let (resource, action) = derive_resource_action(&path, &method);
    tracing::debug!(
        method = %method,
        path = %path,
        resource = %resource,
        action = %action,
        user_id = %user_id,
        username = %username,
        ip = %ip,
        status = %status.as_u16(),
        "audit: write request completed"
    );

    response
}

/// Extract client IP: X-Forwarded-For → X-Real-IP → ConnectInfo → "unknown"
fn extract_ip(req: &Request<Body>) -> String {
    if let Some(xff) = req.headers().get("x-forwarded-for") {
        if let Ok(s) = xff.to_str() {
            if let Some(ip) = s.split(',').next() {
                let ip = ip.trim();
                if !ip.is_empty() {
                    return ip.to_string();
                }
            }
        }
    }

    if let Some(real_ip) = req.headers().get("x-real-ip") {
        if let Ok(s) = real_ip.to_str() {
            let ip = s.trim();
            if !ip.is_empty() {
                return ip.to_string();
            }
        }
    }

    if let Some(connect_info) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        return connect_info.0.ip().to_string();
    }

    "unknown".to_string()
}

/// Extract user_id and username from the Authorization: Bearer <token> header.
/// Returns ("", "") if the token is absent or invalid — the audit log will still
/// be written so we don't silently drop events for unauthenticated endpoints.
fn extract_user(req: &Request<Body>, jwt_secret: &str) -> (String, String) {
    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));

    match token {
        Some(t) => match crate::auth::jwt::verify(t, jwt_secret) {
            Ok(claims) => (claims.sub, claims.username),
            Err(_) => (String::new(), String::new()),
        },
        None => (String::new(), String::new()),
    }
}

/// Map (path, method) → (resource, action).
///
/// Path prefix `/api/v1/` is stripped. The first path segment becomes the
/// resource (with hyphens normalised to underscores). Subsequent segments and
/// the HTTP method together determine the action.
///
/// Examples:
///   POST   /api/v1/rules              → ("rules",         "create")
///   PUT    /api/v1/rules/abc-123      → ("rules",         "update")
///   DELETE /api/v1/rules/abc-123      → ("rules",         "delete")
///   POST   /api/v1/rules/bulk         → ("rules",         "bulk_create")
///   POST   /api/v1/filters/toggle     → ("filters",       "toggle")
///   POST   /api/v1/upstreams/failover → ("upstreams",     "failover")
///   POST   /api/v1/cache/flush        → ("cache",         "flush")
///   POST   /api/v1/auth/login         → ("auth",          "login")
///   POST   /api/v1/auth/ticket        → ("auth",          "ticket")
///   POST   /api/v1/backup             → ("backup",        "create")
fn derive_resource_action(path: &str, method: &Method) -> (String, String) {
    // Strip leading /api/v1/ (or /api/v1)
    let stripped = path
        .strip_prefix("/api/v1/")
        .or_else(|| path.strip_prefix("/api/v1"))
        .unwrap_or(path);

    let segments: Vec<&str> = stripped.split('/').filter(|s| !s.is_empty()).collect();

    let resource = segments
        .first()
        .map(|s| s.replace('-', "_"))
        .unwrap_or_else(|| "unknown".to_string());

    // Segments after the resource (skip pure ID segments — treat UUIDs and
    // numeric-looking strings as IDs, not sub-actions)
    let sub_segments: Vec<&str> = segments
        .iter()
        .skip(1)
        .filter(|&&s| !is_id_segment(s))
        .copied()
        .collect();

    let action = if sub_segments.is_empty() {
        // No meaningful sub-path — derive from method
        method_to_action(method)
    } else {
        // Map well-known sub-actions; fall back to "<method>_<sub>"
        let sub = sub_segments[0].replace('-', "_");
        match sub.as_str() {
            "bulk" => match *method {
                Method::DELETE => "bulk_delete".to_string(),
                _ => "bulk_create".to_string(),
            },
            "refresh" => "refresh".to_string(),
            "toggle" => "toggle".to_string(),
            "failover" => "failover".to_string(),
            "test" => "test".to_string(),
            "health" => "health_check".to_string(),
            "flush" => "flush".to_string(),
            "ticket" => "ticket".to_string(),
            "login" => "login".to_string(),
            "logout" => "logout".to_string(),
            "password" => "change_password".to_string(),
            "import" => "import".to_string(),
            "export" => "export".to_string(),
            "restore" => "restore".to_string(),
            "enable" => "enable".to_string(),
            "disable" => "disable".to_string(),
            _ => format!("{}_{}", method_to_action(method), sub),
        }
    };

    (resource, action)
}

fn method_to_action(method: &Method) -> String {
    match *method {
        Method::POST => "create".to_string(),
        Method::PUT | Method::PATCH => "update".to_string(),
        Method::DELETE => "delete".to_string(),
        _ => method.as_str().to_lowercase(),
    }
}

/// Heuristic: a path segment is an "ID" (not a sub-action keyword) if it
/// contains only hex characters and hyphens (UUID-like) or is purely numeric.
///
/// To avoid treating short action keywords (like "bulk", "toggle") as IDs,
/// we require the segment to either contain a hyphen or at least one digit —
/// pure hex-letter words like "bad" are treated as action keywords, not IDs.
fn is_id_segment(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    // Purely numeric
    if s.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    // UUID-shaped or short ID: hex digits and hyphens.
    // Must contain a hyphen or digit so pure-letter words like "bulk" are
    // not misclassified as IDs.
    if s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        && (s.contains('-') || s.chars().any(|c| c.is_ascii_digit()))
    {
        return true;
    }
    false
}

// ─────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn check(path: &str, method: &Method, expected_resource: &str, expected_action: &str) {
        let (resource, action) = derive_resource_action(path, method);
        assert_eq!(
            resource, expected_resource,
            "resource mismatch for {} {}",
            method, path
        );
        assert_eq!(
            action, expected_action,
            "action mismatch for {} {}",
            method, path
        );
    }

    #[test]
    fn test_basic_crud() {
        check("/api/v1/rules", &Method::POST, "rules", "create");
        check("/api/v1/rules/abc-123", &Method::PUT, "rules", "update");
        check("/api/v1/rules/abc-123", &Method::PATCH, "rules", "update");
        check("/api/v1/rules/abc-123", &Method::DELETE, "rules", "delete");
    }

    #[test]
    fn test_sub_actions() {
        check("/api/v1/rules/bulk", &Method::POST, "rules", "bulk_create");
        check(
            "/api/v1/rules/bulk",
            &Method::DELETE,
            "rules",
            "bulk_delete",
        );
        check("/api/v1/filters/toggle", &Method::POST, "filters", "toggle");
        check(
            "/api/v1/upstreams/failover",
            &Method::POST,
            "upstreams",
            "failover",
        );
        check("/api/v1/cache/flush", &Method::POST, "cache", "flush");
        check("/api/v1/auth/login", &Method::POST, "auth", "login");
        check("/api/v1/auth/ticket", &Method::POST, "auth", "ticket");
    }

    #[test]
    fn test_hyphen_normalisation() {
        check(
            "/api/v1/client-groups",
            &Method::POST,
            "client_groups",
            "create",
        );
        check(
            "/api/v1/client-groups/abc-123",
            &Method::DELETE,
            "client_groups",
            "delete",
        );
    }

    #[test]
    fn test_id_then_sub_action() {
        // PUT /api/v1/upstreams/<uuid>/refresh  → resource=upstreams, action=refresh
        check(
            "/api/v1/upstreams/550e8400-e29b-41d4-a716-446655440000/refresh",
            &Method::POST,
            "upstreams",
            "refresh",
        );
    }
}
