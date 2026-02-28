# Upstream Health API Design

## Background
The current system has a background task in `src/main.rs` that periodically checks upstream health and writes the latency and status to the SQLite database (`upstream_latency_log` and `dns_upstreams`).
The new requirement asks to implement an `Upstream Health API` that uses a background probing task started with `AppState` and stores the latency results in memory, so that `GET /api/v1/settings/upstreams/health` can read them without blocking on the database.

## Architecture & Changes

1.  **State Management (`src/api/mod.rs`)**:
    *   Add `pub upstream_health: DashMap<String, UpstreamHealthResult>` to `AppState`.
    *   `UpstreamHealthResult` will contain `status` (e.g., "healthy", "degraded", "error"), `latency_ms`, and `last_check_at`.

2.  **Background Probing Task (`src/api/mod.rs` or `src/main.rs`)**:
    *   Move or refactor the existing upstream health check task.
    *   Requirement says "在 AppState 启动时起后台任务" (start background task when AppState starts). We will move the task from `src/main.rs` into `src/api/mod.rs:serve` (or run it there alongside) so it can directly access `AppState::upstream_health` and update it.
    *   Wait, the existing task in `main.rs` already does the job of probing and DB logging. We could just pass the `DashMap` into `api::serve` and share it with the `main.rs` task, or we can move the entire task to `api::serve`! Moving it to `api::serve` aligns perfectly with "在 AppState 启动时起后台任务".
    *   Oh, there's `check_upstream_connectivity` in `src/main.rs`. It's better to keep it there or move it to `src/api/mod.rs` or a utility module. Or just leave it in `main.rs` and let `api::serve` spawn the task by passing a clone of the `DashMap`? No, "在 AppState 启动时" means inside `api::serve`. So let's extract the task loop into `src/api/mod.rs` and copy the connectivity check there.

3.  **API Endpoint (`src/api/handlers/settings.rs` or `src/api/handlers/upstreams.rs`)**:
    *   Add `pub async fn get_upstream_health(State(state): State<Arc<AppState>>) -> Result<Json<HashMap<String, UpstreamHealthResult>>, AppError>`.
    *   This will simply iterate over `state.upstream_health` and return the map as JSON.
    *   Add route `GET /api/v1/settings/upstreams/health` in `src/api/router.rs`, protected by Auth middleware.

4.  **Database Integration**:
    *   The background task will continue to update `upstream_latency_log` and `dns_upstreams` in the DB as before, but it will *also* insert/update the in-memory cache in `AppState`.

## Next Steps for Fullstack
1. Create `UpstreamHealthResult` struct.
2. Update `AppState`.
3. Move `check_upstream_connectivity` to a shared location (e.g. `src/dns/upstream.rs` or just inside `src/api/mod.rs`). Wait, `check_upstream_connectivity` can be moved to `src/api/mod.rs`.
4. Move the health check task from `src/main.rs` to `src/api/mod.rs:serve`.
5. Implement the endpoint in `src/api/handlers/upstreams.rs`.
6. Add route in `src/api/router.rs`.
