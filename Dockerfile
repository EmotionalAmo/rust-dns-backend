# Stage 1: Build the Rust binary
FROM rust:1.93-slim AS builder

WORKDIR /app

RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

# Stage 2: Minimal runtime image
FROM debian:bookworm-slim

LABEL maintainer="Ent-DNS Project"
LABEL description="High-performance Enterprise DNS Server Backend"

RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd -r ent-dns && \
    useradd -r -g ent-dns -s /usr/sbin/nologin ent-dns && \
    mkdir -p /data/ent-dns /opt/ent-dns/static && \
    chown -R ent-dns:ent-dns /data/ent-dns /opt/ent-dns/static

# Optional: Place a placeholder index.html so health checks or pure-API visits don't crash
RUN echo "<html><body><h1>Ent-DNS API Server</h1></body></html>" > /opt/ent-dns/static/index.html && \
    chown ent-dns:ent-dns /opt/ent-dns/static/index.html

COPY --from=builder /app/target/release/rust-dns /usr/local/bin/rust-dns

# DNS: 53/udp+tcp, API: 8080
EXPOSE 53/udp 53/tcp 8080

VOLUME ["/data/ent-dns"]

USER ent-dns

ENV ENT_DNS__DATABASE__PATH=/data/ent-dns/ent-dns.db \
    ENT_DNS__DNS__PORT=53 \
    ENT_DNS__API__PORT=8080 \
    ENT_DNS__API__STATIC_DIR=/opt/ent-dns/static

CMD ["rust-dns"]
