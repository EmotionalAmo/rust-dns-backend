use super::handler::DnsHandler;
use crate::shutdown::ShutdownSignal;
use anyhow::Result;
use bytes::Bytes;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::Semaphore;
use tokio::time::{timeout, Duration};

/// Maximum number of concurrent TCP DNS connections (P1-5 fix: DoS防护)
const MAX_TCP_CONNECTIONS: usize = 256;

/// TCP 读取超时：防止慢速客户端长期占用连接（DoS 防护）
const TCP_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Start DNS server (UDP + TCP) using the provided shared handler.
/// This function blocks until the shutdown signal is received.
pub async fn run(
    handler: Arc<DnsHandler>,
    bind_addr: String,
    shutdown_signal: ShutdownSignal,
) -> Result<()> {
    // ── Shutdown coordination ─────────────────────────────────────
    let (shutdown_tx, mut _shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);
    let mut signal_rx = shutdown_signal.subscribe();

    // ── UDP server ──────────────────────────────────────────────
    let udp_socket = Arc::new(UdpSocket::bind(&bind_addr).await?);
    tracing::info!("DNS UDP listening on {}", bind_addr);

    // ── UDP concurrency limit via Semaphore (P0-1 fix) ─────────────
    // With DNS caching, most queries complete in microseconds (cache hit).
    // Use a much larger pool so burst traffic is absorbed rather than dropped.
    let max_concurrent = (num_cpus::get_physical().max(4) * 64).max(512);
    let udp_sem = Arc::new(Semaphore::new(max_concurrent));
    tracing::info!(
        "DNS UDP worker concurrency limit: {} (physical cores: {})",
        max_concurrent,
        num_cpus::get_physical()
    );

    // ── TCP server (RFC 1035: required for responses > 512 bytes) ──
    let tcp_listener = TcpListener::bind(&bind_addr).await?;
    tracing::info!("DNS TCP listening on {}", bind_addr);

    let handler_tcp = handler.clone();
    let tcp_sem = Arc::new(Semaphore::new(MAX_TCP_CONNECTIONS));

    // Track in-flight requests for graceful shutdown
    let active_tcp_connections = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    // Clone before moving into the spawn closure so we can use it after spawn.
    let active_tcp_connections_for_shutdown = active_tcp_connections.clone();
    let mut tcp_shutdown_rx = shutdown_tx.subscribe();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                // Normal accept path
                accept_result = tcp_listener.accept() => {
                    match accept_result {
                        Ok((mut stream, peer)) => {
                            // 检查 TCP 并发数，超限则直接关闭连接
                            let permit = match tcp_sem.clone().try_acquire_owned() {
                                Ok(p) => p,
                                Err(_) => {
                                    tracing::warn!(
                                        "TCP connection limit ({}) reached, rejecting {}",
                                        MAX_TCP_CONNECTIONS,
                                        peer
                                    );
                                    continue;
                                }
                            };
                            let h = handler_tcp.clone();
                            let client_ip = peer.ip().to_string();
                            let connections = active_tcp_connections.clone();

                            // Track connection
                            connections.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                            tokio::spawn(async move {
                                let _permit = permit;
                                // DNS/TCP: 2-byte big-endian length prefix before each message
                                let mut len_buf = [0u8; 2];
                                // 读长度前缀，超时则视为慢速/恶意客户端，断开连接
                                if timeout(TCP_READ_TIMEOUT, stream.read_exact(&mut len_buf))
                                    .await
                                    .is_err()
                                {
                                    connections.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                                    return;
                                }
                                let msg_len = u16::from_be_bytes(len_buf) as usize;
                                if msg_len == 0 || msg_len > 65535 {
                                    connections.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                                    return;
                                }
                                let mut data = vec![0u8; msg_len];
                                // 读消息体，超时则断开连接
                                if timeout(TCP_READ_TIMEOUT, stream.read_exact(&mut data))
                                    .await
                                    .is_err()
                                {
                                    connections.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                                    return;
                                }

                                match h.handle(data, client_ip).await {
                                    Ok(response) => {
                                        let len = (response.len() as u16).to_be_bytes();
                                        if let Err(e) = stream.write_all(&len).await {
                                            tracing::debug!(
                                                "DNS TCP write length failed ({}): {}",
                                                peer,
                                                e
                                            );
                                        } else if let Err(e) = stream.write_all(&response).await {
                                            tracing::debug!(
                                                "DNS TCP write response failed ({}): {}",
                                                peer,
                                                e
                                            );
                                        }
                                    }
                                    Err(e) => tracing::warn!("DNS TCP handler error: {}", e),
                                }

                                // Connection done
                                connections.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                            });
                        }
                        Err(e) => tracing::error!("DNS TCP accept error: {}", e),
                    }
                }
                // Shutdown signal
                _ = tcp_shutdown_rx.recv() => {
                    tracing::info!("DNS TCP server shutdown triggered");
                    // Close listener to stop accepting new connections
                    let _ = tcp_listener.set_ttl(0);
                    return;
                }
            }
        }
    });

    // ── UDP receive loop ─────────────────────────────────────────────
    let udp_socket_clone = udp_socket.clone();
    // Retain a reference for the shutdown cleanup below (after the spawn moves udp_socket).
    let udp_socket_for_shutdown = udp_socket.clone();
    let mut udp_shutdown_rx = shutdown_tx.subscribe();
    tokio::spawn(async move {
        // 65535 = DNS over UDP 最大合法消息大小（RFC 1035 + EDNS0）
        let mut buf = vec![0u8; 65535];
        loop {
            tokio::select! {
                // Normal receive path
                recv_result = udp_socket_clone.recv_from(&mut buf) => {
                    match recv_result {
                        Ok((len, peer)) => {
                            let client_ip = peer.ip();
                            let data = Bytes::copy_from_slice(&buf[..len]);

                            // 非阻塞获取并发许可，超限直接丢弃（UDP 最佳努力，客户端会重试）
                            match udp_sem.clone().try_acquire_owned() {
                                Ok(permit) => {
                                    let handler = handler.clone();
                                    let udp_socket = udp_socket.clone();
                                    tokio::spawn(async move {
                                        let _permit = permit;
                                        let client_ip_str = client_ip.to_string();
                                        match handler.handle_bytes(data, &client_ip_str).await {
                                            Ok(response) => {
                                                if let Err(e) = udp_socket.send_to(&response, peer).await {
                                                    tracing::warn!(
                                                        "Failed to send DNS response to {}: {}",
                                                        peer,
                                                        e
                                                    );
                                                }
                                            }
                                            Err(e) => {
                                                tracing::debug!("DNS handler error from {}: {}", peer, e);
                                            }
                                        }
                                    });
                                }
                                Err(_) => {
                                    tracing::warn!(
                                        "DNS UDP concurrency limit ({}) reached, dropping query from {}",
                                        max_concurrent,
                                        peer
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("UDP recv error: {}", e);
                        }
                    }
                }
                // Shutdown signal
                _ = udp_shutdown_rx.recv() => {
                    tracing::info!("DNS UDP server shutdown triggered");
                    return;
                }
            }
        }
    });

    // ── Wait for global shutdown signal ─────────────────────────────
    signal_rx.recv().await.ok();
    tracing::info!("DNS server shutdown signal received");

    // Signal local shutdown to UDP and TCP loops
    let _ = shutdown_tx.send(());

    // Wait for in-flight TCP connections to drain (with a small timeout)
    let start = std::time::Instant::now();
    let active_connections = active_tcp_connections_for_shutdown.clone();
    loop {
        let connections = active_connections.load(std::sync::atomic::Ordering::Relaxed);
        if connections == 0 || start.elapsed().as_secs() >= 5 {
            if connections > 0 {
                tracing::warn!(
                    "Shutdown timeout: {} TCP connections still active",
                    connections
                );
            }
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Close UDP socket
    let _ = udp_socket_for_shutdown.set_ttl(0);

    tracing::info!("DNS server shutdown complete");
    Ok(())
}
