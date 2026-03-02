/// Async batch writer for query log entries.
///
/// DnsHandler sends log entries via an UnboundedSender (non-blocking, zero latency
/// on the DNS hot path). This background task drains the channel every second or
/// when a batch of 100 entries accumulates, then writes them in a single SQLite
/// transaction — dramatically reducing write amplification.
use crate::db::DbPool;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

/// A single query log entry to be persisted.
#[derive(Debug, Clone)]
pub struct QueryLogEntry {
    pub time: String,
    pub client_ip: String,
    pub question: String,
    pub qtype: String,
    pub status: String,
    pub reason: Option<String>,
    pub answer: Option<String>,
    pub elapsed_ns: i64,
    pub upstream_ns: Option<i64>,
    pub app_id: Option<i64>,
}

/// How many entries to accumulate before forcing a flush.
const BATCH_SIZE: usize = 500; // Increased from 100 to 500 for better throughput
/// Maximum time between flushes even when batch is not full.
const FLUSH_INTERVAL: Duration = Duration::from_secs(2); // Increased from 1s to 2s

/// Spawn the background writer task.  Returns the sender end of the channel
/// which DnsHandler uses to enqueue entries (non-blocking).
pub fn spawn(
    db: DbPool,
    alert_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
) -> mpsc::UnboundedSender<QueryLogEntry> {
    let (tx, rx) = mpsc::unbounded_channel::<QueryLogEntry>();
    tokio::spawn(run(db, rx, alert_tx));
    tx
}

async fn run(
    db: DbPool,
    mut rx: mpsc::UnboundedReceiver<QueryLogEntry>,
    alert_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
) {
    let mut ticker = interval(FLUSH_INTERVAL);
    let mut batch: Vec<QueryLogEntry> = Vec::with_capacity(BATCH_SIZE);

    // Anomaly detection state: client_ip -> (block_count, first_block_time)
    let mut block_counts: std::collections::HashMap<String, (usize, tokio::time::Instant)> =
        std::collections::HashMap::new();

    loop {
        tokio::select! {
            // New entry arrived
            maybe_entry = rx.recv() => {
                match maybe_entry {
                    Some(entry) => {
                        // Anomaly detection
                        if entry.status == "blocked" {
                            let now = tokio::time::Instant::now();
                            let entry_tracker = block_counts.entry(entry.client_ip.clone()).or_insert((0, now));

                            // Reset tracker if older than 60s
                            if now.duration_since(entry_tracker.1).as_secs() > 60 {
                                *entry_tracker = (1, now);
                            } else {
                                entry_tracker.0 += 1;

                                // Trigger alert if threshold exceeded (e.g., 50 blocks in 60s)
                                if entry_tracker.0 == 50 {
                                    let alert_id = uuid::Uuid::new_v4().to_string();
                                    let message = format!("Client {} triggered 50 blocked queries within a minute. Potential malware activity.", entry.client_ip);
                                    let created_at = chrono::Utc::now().to_rfc3339();

                                    // Insert alert asynchronously so it doesn't block the writer
                                    let db_ref = db.clone();
                                    let cid = entry.client_ip.clone();
                                    let msg = message.clone();

                                    tokio::spawn(async move {
                                        let _ = sqlx::query(
                                            "INSERT INTO alerts (id, alert_type, client_id, message, is_read, created_at) VALUES (?, ?, ?, ?, 0, ?)"
                                        )
                                        .bind(&alert_id)
                                        .bind("high_frequency_block")
                                        .bind(&cid)
                                        .bind(&msg)
                                        .bind(&created_at)
                                        .execute(&db_ref)
                                        .await;
                                    });

                                    // Broadcast alert to WebSocket
                                    let _ = alert_tx.send(serde_json::json!({
                                        "type": "alert",
                                        "alert_type": "high_frequency_block",
                                        "client_id": entry.client_ip,
                                        "message": message,
                                        "created_at": chrono::Utc::now().to_rfc3339()
                                    }));
                                }
                            }
                        }

                        batch.push(entry);
                        if batch.len() >= BATCH_SIZE {
                            flush(&db, &mut batch).await;
                        }
                    }
                    None => {
                        // Channel closed (process shutting down) — flush remainder
                        if !batch.is_empty() {
                            flush(&db, &mut batch).await;
                        }
                        tracing::info!("QueryLogWriter: channel closed, exiting");
                        return;
                    }
                }
            }
            // Periodic flush tick
            _ = ticker.tick() => {
                if !batch.is_empty() {
                    flush(&db, &mut batch).await;
                }
            }
        }
    }
}

/// Write all entries in `batch` inside a single SQLite transaction, then clear it.
async fn flush(db: &DbPool, batch: &mut Vec<QueryLogEntry>) {
    let count = batch.len();
    match write_batch(db, batch).await {
        Ok(_) => tracing::debug!("QueryLogWriter: flushed {} entries", count),
        Err(e) => tracing::warn!(
            "QueryLogWriter: batch write failed ({} entries): {}",
            count,
            e
        ),
    }
    batch.clear();
}

async fn write_batch(db: &DbPool, batch: &[QueryLogEntry]) -> Result<(), sqlx::Error> {
    if batch.is_empty() {
        return Ok(());
    }

    let mut tx = db.begin().await?;

    for entry in batch {
        // High-performance insert: app_id is now resolved in-memory by DnsHandler
        // before the entry is sent to the writer, eliminating the extremely slow
        // SQLite LIKE wildcard subquery.
        sqlx::query(
            "INSERT INTO query_log \
             (time, client_ip, question, qtype, status, reason, answer, elapsed_ns, upstream_ns, app_id) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.time)
        .bind(&entry.client_ip)
        .bind(&entry.question)
        .bind(&entry.qtype)
        .bind(&entry.status)
        .bind(&entry.reason)
        .bind(&entry.answer)
        .bind(entry.elapsed_ns)
        .bind(entry.upstream_ns)
        .bind(entry.app_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}
