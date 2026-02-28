use crate::api::middleware::auth::AuthUser;
use crate::api::AppState;
use crate::error::AppResult;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use uuid::Uuid;

/// Ticket TTL: 30 seconds is enough time to open the WebSocket connection.
const WS_TICKET_TTL: Duration = Duration::from_secs(30);

/// Issue a one-time WebSocket ticket.
/// The client calls this authenticated REST endpoint, then immediately opens the WS
/// connection using ?ticket=<uuid> instead of embedding the long-lived JWT in the URL.
/// This prevents the JWT from appearing in server logs, browser history, and Referer headers.
pub async fn issue_ticket(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    // Evict expired tickets (lazy cleanup)
    state
        .ws_tickets
        .retain(|_, issued_at: &mut Instant| issued_at.elapsed() < WS_TICKET_TTL);

    let ticket = Uuid::new_v4().to_string();
    state.ws_tickets.insert(ticket.clone(), Instant::now());

    Ok(Json(
        json!({ "ticket": ticket, "expires_in": WS_TICKET_TTL.as_secs() }),
    ))
}

#[derive(Deserialize)]
pub struct WsParams {
    ticket: String,
}

/// WebSocket endpoint for real-time query log streaming.
/// Authenticated via one-time ticket (see `issue_ticket`); ticket is consumed on use.
pub async fn query_log_ws(
    State(state): State<Arc<AppState>>,
    Query(params): Query<WsParams>,
    ws: WebSocketUpgrade,
) -> Response {
    // Validate and consume the ticket atomically
    match state.ws_tickets.remove(&params.ticket) {
        Some((_, issued_at)) if issued_at.elapsed() < WS_TICKET_TTL => {
            // Ticket valid — proceed with upgrade
        }
        Some(_) => {
            // Ticket found but expired — already removed above
            return (StatusCode::UNAUTHORIZED, "WebSocket ticket expired").into_response();
        }
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                "Invalid or already-used WebSocket ticket",
            )
                .into_response();
        }
    }

    let tx = state.query_log_tx.clone();
    ws.on_upgrade(move |socket| handle_socket(socket, tx))
        .into_response()
}

async fn handle_socket(mut socket: WebSocket, tx: broadcast::Sender<serde_json::Value>) {
    let mut rx = tx.subscribe();

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        if let Ok(text) = serde_json::to_string(&event) {
                            if socket.send(Message::Text(text.into())).await.is_err() {
                                break; // client disconnected
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue, // skip missed events
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(_)) => {} // ignore client messages (ping/pong handled by axum)
                    _ => break,       // client disconnected or error
                }
            }
        }
    }
}
