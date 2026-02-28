use tokio::signal;
/// Graceful shutdown utilities.
///
/// Provides shutdown signal handling and coordination for clean server termination.
use tokio::sync::broadcast;
use tracing::{info, warn};

/// Shutdown signal broadcast channel.
/// The shutdown initiator sends a signal; all listeners stop their work.
#[derive(Debug, Clone)]
pub struct ShutdownSignal {
    tx: broadcast::Sender<()>,
}

impl ShutdownSignal {
    /// Create a new shutdown signal channel.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1);
        Self { tx }
    }

    /// Subscribe to the shutdown signal.
    /// Returns a receiver that will receive `()` when shutdown is requested.
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.tx.subscribe()
    }

    /// Initiate shutdown by broadcasting to all listeners.
    pub fn initiate(&self) {
        // Ignore error if there are no receivers
        let _ = self.tx.send(());
    }

    /// Wait for the shutdown signal.
    /// This is a convenience method that returns when shutdown is triggered.
    pub async fn recv(&self) {
        let mut rx = self.subscribe();
        // First call always succeeds; we ignore errors if channel is closed
        let _ = rx.recv().await;
    }
}

impl Default for ShutdownSignal {
    fn default() -> Self {
        Self::new()
    }
}

/// Wait for OS termination signals (SIGTERM, SIGINT).
///
/// This is typically used in `select!` loops to coordinate graceful shutdown
/// across multiple server tasks (DNS UDP/TCP, HTTP API, background workers).
///
/// Returns when either SIGTERM or SIGINT is received.
pub async fn wait_for_termination_signal() {
    #[cfg(unix)]
    {
        let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to setup SIGTERM handler");
        let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())
            .expect("Failed to setup SIGINT handler");

        tokio::select! {
            _ = sigterm.recv() => {
                info!("Received SIGTERM, initiating graceful shutdown...");
            }
            _ = sigint.recv() => {
                info!("Received SIGINT, initiating graceful shutdown...");
            }
        }
    }

    #[cfg(windows)]
    {
        let mut ctrl_c = signal::ctrl_c().expect("Failed to setup Ctrl-C handler");
        ctrl_c.await.expect("Failed to wait for Ctrl-C");
        info!("Received Ctrl-C, initiating graceful shutdown...");
    }
}

/// Graceful shutdown timeout.
///
/// When a shutdown signal is received, servers should stop accepting new connections
/// and wait for in-flight requests to complete. This timeout prevents indefinite waiting.
///
/// Default: 30 seconds (reasonable for DNS/HTTP workloads).
pub const DEFAULT_SHUTDOWN_TIMEOUT_SECS: u64 = 30;

/// Execute a graceful shutdown with a timeout.
///
/// This is a utility for wrapping server shutdown operations.
/// If the shutdown does not complete within the timeout, it will be forcibly terminated.
///
/// # Arguments
/// * `timeout_secs` - Maximum time to wait for graceful shutdown
/// * `shutdown_future` - The async operation to perform (e.g., closing a server)
///
/// # Returns
/// * `Ok(())` if shutdown completes gracefully
/// * `Err` if shutdown times out or fails
pub async fn shutdown_with_timeout<F, Fut, E>(
    timeout_secs: u64,
    shutdown_future: F,
) -> Result<(), String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), E>>,
    E: std::fmt::Display,
{
    let result = tokio::time::timeout(
        tokio::time::Duration::from_secs(timeout_secs),
        shutdown_future(),
    )
    .await;

    match result {
        Ok(Ok(())) => {
            info!("Graceful shutdown completed successfully");
            Ok(())
        }
        Ok(Err(e)) => {
            warn!("Shutdown completed with error: {}", e);
            Err(format!("Shutdown error: {}", e))
        }
        Err(_) => {
            warn!(
                "Shutdown timeout ({}s) exceeded, forcing exit",
                timeout_secs
            );
            Err(format!("Shutdown timeout after {}s", timeout_secs))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_shutdown_signal_broadcast() {
        let signal = ShutdownSignal::new();

        // Multiple listeners
        let mut rx1 = signal.subscribe();
        let mut rx2 = signal.subscribe();

        // Broadcast shutdown
        signal.initiate();

        // Both should receive the signal
        tokio::select! {
            _ = rx1.recv() => {}
            _ = sleep(Duration::from_millis(100)) => panic!("rx1 did not receive signal"),
        }

        tokio::select! {
            _ = rx2.recv() => {}
            _ = sleep(Duration::from_millis(100)) => panic!("rx2 did not receive signal"),
        }
    }

    #[tokio::test]
    async fn test_shutdown_signal_recv() {
        let signal = ShutdownSignal::new();

        // Initiate from another task
        let signal_clone = signal.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(50)).await;
            signal_clone.initiate();
        });

        // This should return when signal is received
        signal.recv().await;
    }

    #[tokio::test]
    async fn test_shutdown_with_timeout_success() {
        let result = shutdown_with_timeout(1, || async { Ok::<(), String>(()) }).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_shutdown_with_timeout_timeout() {
        let result = shutdown_with_timeout(1, || async {
            sleep(Duration::from_secs(2)).await;
            Ok::<(), String>(())
        })
        .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("timeout"));
    }

    #[tokio::test]
    async fn test_shutdown_with_timeout_error() {
        let result =
            shutdown_with_timeout(1, || async { Err::<(), String>("test error".to_string()) })
                .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("test error"));
    }
}
