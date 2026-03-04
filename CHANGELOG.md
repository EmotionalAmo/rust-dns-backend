# Changelog

All notable changes to rust-dns-backend will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.5.0] - 2026-03-04

### Fixed

- **上游趋势数据时间排序错乱** — `get_upstream_trend` handler 中的数据聚合容器由 `HashMap` 改为 `BTreeMap`，确保按时间 key 自然升序输出，修复图表时间轴乱序问题。

## [1.2.0] - 2026-03-03

### Added

- **TCP upstream support** — configure upstream DNS servers over TCP by prefixing addresses with `tcp://` (e.g., `tcp://8.8.8.8`, `tcp://8.8.8.8:53`). Useful when UDP is blocked by firewalls or unreliable on a given network path.

### Fixed

- **DoH/DoT upstream health checks were silently skipped** — the background health monitor was skipping all `https://` and `tls://` upstreams with a hard-coded `continue`, causing `health_status` to remain `"unknown"` indefinitely and disabling failover for these upstream types. Health checks now run correctly for all protocols (UDP, DoH, DoT).
- **DoT upstream test connectivity always failed** — the on-demand connectivity test (`POST /api/upstreams/:id/test`) was trying to parse `tls://...` addresses as raw IP addresses, causing immediate parse errors. DoT connectivity is now verified with a real TLS handshake via hickory-resolver.

## [1.1.0] - 2026-03-03

### Added

- **DNS-over-TLS (DoT) upstream support** — resolvers can now forward queries to upstream DoT servers, improving privacy and security in transit.
- **DNS-over-HTTPS (DoH) upstream support** — forward queries to upstream DoH servers as an alternative secure transport.
- **Audit middleware** — automatic logging of all write operations (POST/PUT/PATCH/DELETE) without requiring manual `log_action` calls in each handler.

### Fixed

- `Cache-Control: max-age` in DoH responses is now derived from the actual DNS response TTL instead of a hard-coded value.
- DnsCache capacity reported in cache stats now reflects the correct configured value.
- Audit middleware correctly identifies short path-segment IDs such as `abc-123`.
- Removed duplicate `log_action` calls that were emitted by both the middleware and individual handlers.

### Changed

- Docker Compose now uses the pre-built GHCR image by default instead of building locally, reducing setup time for new deployments.
- README updated with actual benchmark numbers and GHCR installation instructions.

## [1.0.1] - 2026-03-03

### Added

- GHCR release workflow — Docker images are automatically built and pushed to GitHub Container Registry on every versioned tag.
- Benchmark infrastructure with a dedicated Cargo workspace and CI integration.

### Fixed

- `Cargo.lock` is now tracked in version control for reproducible Docker builds.
- Benchmark script bash compatibility and container configuration issues.
- `oisd_performance` benchmark is marked `#[ignore]` so it is skipped in CI but still runnable locally.
- GHCR image name is lowercased in the release workflow to satisfy registry requirements.

## [1.0.0] - 2026-03-03

### Added

- Initial public release of rust-dns-backend.
- High-performance DNS server written in Rust.
- REST API for managing DNS zones, records, upstreams, ACLs, and client groups.
- DNS caching layer with configurable capacity and TTL.
- Audit log for tracking configuration changes.
- Docker and Docker Compose support.

[Unreleased]: https://github.com/EmotionalAmo/rust-dns-backend/compare/v1.5.0...HEAD
[1.5.0]: https://github.com/EmotionalAmo/rust-dns-backend/compare/v1.2.0...v1.5.0
[1.2.0]: https://github.com/EmotionalAmo/rust-dns-backend/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/EmotionalAmo/rust-dns-backend/compare/v1.0.1...v1.1.0
[1.0.1]: https://github.com/EmotionalAmo/rust-dns-backend/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/EmotionalAmo/rust-dns-backend/releases/tag/v1.0.0
