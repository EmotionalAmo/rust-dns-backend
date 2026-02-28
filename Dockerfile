# Stage 1: Build the Rust binary
FROM rust:1.93-slim AS builder

WORKDIR /app

RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

# Stage 2: Build the frontend
FROM node:20-slim AS frontend-builder

WORKDIR /app/frontend
COPY frontend/package*.json ./
RUN npm ci
COPY frontend ./
RUN npm run build

# Stage 3: Minimal runtime image
FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd -r ent-dns && \
    useradd -r -g ent-dns -s /sbin/nologin ent-dns && \
    mkdir -p /data/ent-dns /opt/ent-dns/static && \
    chown -R ent-dns:ent-dns /data/ent-dns

COPY --from=builder /app/target/release/ent-dns /usr/local/bin/ent-dns
COPY --from=frontend-builder /app/frontend/dist /opt/ent-dns/static

# DNS: 53/udp+tcp, API: 8080
EXPOSE 53/udp 53/tcp 8080

VOLUME ["/data/ent-dns"]

USER ent-dns

ENV ENT_DNS__DATABASE__PATH=/data/ent-dns/ent-dns.db \
    ENT_DNS__DNS__PORT=53 \
    ENT_DNS__API__PORT=8080

CMD ["ent-dns"]
