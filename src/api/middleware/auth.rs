use crate::api::AppState;
use crate::auth::jwt::Claims;
use crate::error::AppError;
use axum::{extract::FromRequestParts, http::request::Parts};
use std::sync::Arc;

/// Axum extractor that validates a Bearer JWT token.
/// Add this as a handler parameter to require authentication.
pub struct AuthUser(pub Claims);

impl FromRequestParts<Arc<AppState>> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or(AppError::AuthFailed)?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(AppError::AuthFailed)?;

        let claims =
            crate::auth::jwt::verify(token, &state.jwt_secret).map_err(|_| AppError::AuthFailed)?;

        // Reject tokens that have been explicitly invalidated (e.g. via logout).
        if state.token_blacklist.contains_key(&claims.jti) {
            return Err(AppError::AuthFailed);
        }

        Ok(AuthUser(claims))
    }
}
