use super::auth::AuthUser;
use crate::api::AppState;
use crate::auth::jwt::Claims;
use crate::error::AppError;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use std::sync::Arc;

/// Axum extractor that requires the caller to have `admin` or `super_admin` role.
/// Returns 403 Forbidden if the authenticated user has an insufficient role.
pub struct AdminUser(#[allow(dead_code)] pub Claims);

impl FromRequestParts<Arc<AppState>> for AdminUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let AuthUser(claims) = AuthUser::from_request_parts(parts, state).await?;
        match claims.role.as_str() {
            "admin" | "super_admin" => Ok(AdminUser(claims)),
            _ => Err(AppError::Unauthorized(
                "Admin or super_admin role required".to_string(),
            )),
        }
    }
}
