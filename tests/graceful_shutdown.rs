/// Integration tests for graceful shutdown functionality.
use ent_dns::shutdown;
use std::time::Duration;
use tokio::time::sleep;

#[test]
fn test_shutdown_signal_creation() {
    let signal = shutdown::ShutdownSignal::new();
    // Signal should be cloneable
    let _signal2 = signal.clone();
    // Should implement Default
    let _signal3: shutdown::ShutdownSignal = Default::default();
}

#[tokio::test]
async fn test_shutdown_signal_single_listener() {
    let signal = shutdown::ShutdownSignal::new();
    let mut rx = signal.subscribe();

    // Signal should be ready to receive
    let recv_task = tokio::spawn(async move { rx.recv().await });

    sleep(Duration::from_millis(50)).await;

    // Broadcast shutdown
    signal.initiate();

    // Listener should receive the signal
    let result = recv_task.await.unwrap();
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_shutdown_signal_multiple_listeners() {
    let signal = shutdown::ShutdownSignal::new();
    let mut rx1 = signal.subscribe();
    let mut rx2 = signal.subscribe();
    let mut rx3 = signal.subscribe();

    let task1 = tokio::spawn(async move { rx1.recv().await });
    let task2 = tokio::spawn(async move { rx2.recv().await });
    let task3 = tokio::spawn(async move { rx3.recv().await });

    sleep(Duration::from_millis(50)).await;

    // Broadcast shutdown once
    signal.initiate();

    // All listeners should receive
    assert!(task1.await.unwrap().is_ok());
    assert!(task2.await.unwrap().is_ok());
    assert!(task3.await.unwrap().is_ok());
}

#[tokio::test]
async fn test_shutdown_signal_recv_method() {
    let signal = shutdown::ShutdownSignal::new();

    let signal_clone = signal.clone();
    let task = tokio::spawn(async move {
        // Use recv() convenience method
        signal_clone.recv().await;
        "received"
    });

    sleep(Duration::from_millis(50)).await;

    signal.initiate();

    let result = task.await.unwrap();
    assert_eq!(result, "received");
}

#[tokio::test]
async fn test_shutdown_with_timeout_success() {
    let result = shutdown::shutdown_with_timeout(1, || async {
        sleep(Duration::from_millis(50)).await;
        Ok::<(), String>(())
    })
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_shutdown_with_timeout_actual_timeout() {
    let result = shutdown::shutdown_with_timeout(1, || async {
        sleep(Duration::from_secs(2)).await;
        Ok::<(), String>(())
    })
    .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("timeout"));
}

#[tokio::test]
async fn test_shutdown_with_timeout_error() {
    let result = shutdown::shutdown_with_timeout(1, || async {
        Err::<(), String>("simulated error".to_string())
    })
    .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("simulated error"));
}

#[tokio::test]
async fn test_shutdown_signal_after_channel_closed() {
    let signal = shutdown::ShutdownSignal::new();
    let mut rx = signal.subscribe();

    // Drop signal to close the channel
    drop(signal);

    // recv() should return error when channel is closed
    let result = rx.recv().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_multiple_shutdown_initiates() {
    let signal = shutdown::ShutdownSignal::new();
    let mut rx = signal.subscribe();

    // Initiate multiple times (should be safe)
    signal.initiate();
    signal.initiate();
    signal.initiate();

    // With capacity 1, doing multiple initiates without reading causes Lagged error.
    // We should safely handle this Lagged error and verify we still get the signal.
    match rx.recv().await {
        Ok(_) => {}
        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
            rx.recv().await.unwrap();
        }
        Err(e) => panic!("Unexpected error: {:?}", e),
    }

    // Next recv should timeout (no more messages) or return lag if many were sent
    let result = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_shutdown_signal_listener_dropped() {
    let signal = shutdown::ShutdownSignal::new();
    let rx = signal.subscribe();

    // Drop the listener
    drop(rx);

    // Initiate should not panic even with no listeners
    signal.initiate();
}

#[tokio::test]
async fn test_shutdown_with_custom_timeout() {
    // Test with 0.1 second timeout
    let result = shutdown::shutdown_with_timeout(0, || async {
        sleep(Duration::from_millis(200)).await;
        Ok::<(), String>(())
    })
    .await;

    // Should timeout immediately
    assert!(result.is_err());
}

#[tokio::test]
async fn test_shutdown_signal_concurrent_broadcast() {
    let signal = shutdown::ShutdownSignal::new();

    // Create many listeners
    let mut receivers = vec![];
    for _ in 0..10 {
        receivers.push(signal.subscribe());
    }

    let tasks: Vec<_> = receivers
        .into_iter()
        .map(|mut rx| tokio::spawn(async move { rx.recv().await }))
        .collect();

    sleep(Duration::from_millis(50)).await;

    signal.initiate();

    // All tasks should complete
    for task in tasks {
        assert!(task.await.unwrap().is_ok());
    }
}
