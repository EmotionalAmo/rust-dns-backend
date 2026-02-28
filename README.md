# Rust DNS Backend

High-performance, enterprise-grade DNS server written in Rust, featuring a robust REST API for management and domain filtering.

## Overview

`rust-dns-backend` is the core backend component responsible for:
- Resolving DNS queries securely and fast.
- Managing DNS records, forwarders, and caching.
- Exposing a RESTful API for the frontend management console.
- Supporting DoH (DNS over HTTPS) and standard UDP/TCP DNS.
- Comprehensive ad-blocking and domain filtering logic.

## Getting Started

### Prerequisites
- Rust 1.93 or later
- Cargo
- Optional: Docker

### Running Locally (Development)

To build and run the backend locally:

```bash
# Compile and run
cargo run --release
```

By default, the server expects to bind to port `53` (TCP/UDP) for DNS operations. Running natively might require `sudo` privileges for binding to privileged ports (< 1024).

```bash
sudo ./target/release/ent-dns
```

### Running with Docker

You can easily build and run the application in a Docker container without worrying about system dependencies:

```bash
docker build -t rust-dns-backend .
docker run -d \
  -p 53:53/udp \
  -p 53:53/tcp \
  -p 8080:8080 \
  -v ent-dns-data:/data/ent-dns \
  rust-dns-backend
```

## Configuration and Environment Variables

The server behaves depending on the given environmental variables:

| Variable | Default Value | Description |
|---|---|---|
| `ENT_DNS__DATABASE__PATH` | `/data/ent-dns/ent-dns.db` | Path to the SQLite database file. |
| `ENT_DNS__DNS__PORT` | `53` | Port for the DNS server to listen on. |
| `ENT_DNS__API__PORT` | `8080` | Port for the management API to listen on. |
| `ENT_DNS__API__STATIC_DIR`| `frontend/dist` | Path to frontend static assets (if hosted identically, nullable). |

## Security and Verification
All actions pushed to this repository undergo automated CI tests via GitHub Actions.
Code quality is strictly enforced with `cargo clippy` and `cargo fmt`.
Dependencies are actively audited using `cargo audit` to prevent security vulnerabilities.