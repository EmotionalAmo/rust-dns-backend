use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Alert {
    pub id: String,
    pub alert_type: String,
    pub client_id: Option<String>,
    pub message: String,
    pub is_read: i32,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateAlertRequest {
    pub alert_type: String,
    pub client_id: Option<String>,
    pub message: String,
}
