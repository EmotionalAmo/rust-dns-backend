# rust-dns

**DNS ad-blocker for your homelab. Written in Rust. 4.6 MB RAM. No GC pauses.**

If Pi-hole v6 broke your setup — the 403 errors, the UI crashes, the config migration that ate your groups — you are not alone. rust-dns is a single binary that just works.

[![GitHub Stars](https://img.shields.io/github/stars/EmotionalAmo/rust-dns-backend?style=flat-square)](https://github.com/EmotionalAmo/rust-dns-backend)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg?style=flat-square)](LICENSE)
[![v1.9.1](https://img.shields.io/badge/version-v1.9.1-orange?style=flat-square)](https://github.com/EmotionalAmo/rust-dns-backend/releases)

---

## Why rust-dns?

| | rust-dns | Pi-hole v6 | AdGuard Home |
|---|---|---|---|
| Language | Rust | C + Python | Go |
| Memory usage | **4.6 MB** | ~50–100 MB | ~39 MB |
| Deployment | Single binary | Multi-component | Single binary |
| GC pauses | None | Yes | Yes |
| Telemetry | None | None | Optional |

Benchmarked on Docker (idle, with blocklists loaded). AdGuard Home measured at 39.2 MB under same conditions — rust-dns uses **8.5x less memory**.

Works great on a Raspberry Pi 4, a $4 VPS, or anything else you have sitting in the rack.

### Coming from Pi-hole v6?

Pi-hole v6 shipped with a rewritten core and broke a lot of setups: groups not migrating, the web UI returning 403s, systemd unit changes. If you spent a weekend debugging and just want something that works, rust-dns is a 5-minute Docker deploy with zero migration headaches. Your old blocklist URLs still work — paste them in and you're done.

---

## Quick Start (Docker)

**One command to start filtering DNS:**

```bash
docker run -d \
  --name rust-dns \
  --restart unless-stopped \
  -p 53:53/udp \
  -p 53:53/tcp \
  -p 8080:8080 \
  -v rust-dns-data:/data/rust-dns \
  -e RUST_DNS__AUTH__JWT_SECRET=$(openssl rand -hex 32) \
  ghcr.io/emotionalamo/rust-dns-backend:latest
```

Open `http://localhost:8080` — default login: `admin / admin` (change this immediately).

Or with Docker Compose:

```bash
git clone https://github.com/EmotionalAmo/rust-dns-backend.git
cd rust-dns-backend
echo "RUST_DNS__AUTH__JWT_SECRET=$(openssl rand -hex 32)" > .env
docker compose up -d
```

Point your router (or Pi's `/etc/resolv.conf`) to this machine's IP on port 53. Done.

---

## What It Does

- **DNS filtering** — block ads, trackers, and malware domains via blocklists
- **DNS over HTTPS (DoH)** — encrypted upstream queries
- **Bulk rule import** — upload a file or paste text directly; supports dual-tab for rules and allowlist
- **Top queried domains** — see what's actually hitting your DNS, not just block counts
- **REST API** — full management API, consumed by the web dashboard
- **Caching** — in-memory cache, zero cold-start latency on warm queries
- **Web UI** — clean dashboard with English and Chinese (i18n) support
- **SQLite storage** — one file, easy to back up, no database server needed

---

## Configuration

All configuration is done via environment variables.

| Variable | Default | Description |
|---|---|---|
| `RUST_DNS__AUTH__JWT_SECRET` | **(required)** | Secret for signing JWT tokens. Use a random 32+ byte string. |
| `RUST_DNS__DATABASE__PATH` | `/data/rust-dns/rust-dns.db` | Path to the SQLite database file. |
| `RUST_DNS__DNS__PORT` | `53` | DNS server port (UDP + TCP). |
| `RUST_DNS__API__PORT` | `8080` | Management API and dashboard port. |
| `RUST_DNS__API__STATIC_DIR` | `/opt/rust-dns/static` | Path to frontend static assets. |
| `RUST_LOG` | `rust_dns=info` | Log level. Use `debug` for troubleshooting. |

**Security note:** Change the default `admin/admin` credentials immediately after first login. Never use the placeholder `CHANGE_ME` JWT secret in production.

---

## Ports

| Port | Protocol | Purpose |
|---|---|---|
| `53` | UDP + TCP | DNS queries |
| `8080` | TCP | REST API + Web UI |

---

## Build from Source

```bash
# Requires Rust 1.93+
git clone https://github.com/EmotionalAmo/rust-dns-backend.git
cd rust-dns-backend

cargo build --release

# Binding port 53 requires elevated privileges on Linux
sudo ./target/release/rust-dns
```

---

## Security

- All API endpoints require JWT authentication
- Non-root container: runs as a dedicated `rust-dns` user
- CI enforces `cargo clippy`, `cargo fmt`, and `cargo audit` on every push
- No telemetry, no phone-home, no cloud dependency

---

## Project Status

v1.9.1 — actively developed. Core DNS filtering and API are stable. Recent releases added bulk rule import, query analytics, and i18n support.

Roadmap items and known issues are tracked in [GitHub Issues](https://github.com/EmotionalAmo/rust-dns-backend/issues).

---

## Contributing

The codebase is small enough to read in an afternoon. If you want to add a feature or fix something, [browse the good first issues](https://github.com/EmotionalAmo/rust-dns-backend/issues?q=is%3Aopen+label%3A%22good+first+issue%22) — they're real missing features, not busywork. PRs welcome; no CLA required.

```bash
# Fork, clone, then:
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

---

## License

Apache 2.0 — use it, fork it, ship it.
