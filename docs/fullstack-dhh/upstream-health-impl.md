# Upstream Health API Implementation

## Implementation Steps
1. Add `UpstreamHealthResult` struct to `src/api/mod.rs` or `src/api/handlers/upstreams.rs` and `pub upstream_health: DashMap<String, UpstreamHealthResult>` to `AppState`.
2. Move background task loop from `src/main.rs` to `src/api/mod.rs` `serve` function, passing `state.clone()` so it can update `upstream_health`.
3. Move `check_upstream_connectivity` to `src/api/mod.rs`.
4. Create the GET handler `get_upstream_health` in `src/api/handlers/upstreams.rs`.
5. Register `Router::route("/health", get(get_upstream_health))` inside `src/api/router.rs` under `/api/v1/settings/upstreams`.

All code will be verified by running `cargo build`.
