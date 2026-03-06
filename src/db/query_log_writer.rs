/// Async batch writer for query log entries.
///
/// DnsHandler sends log entries via a bounded Sender (容量 32_768，背压保护 OOM).
/// 当 channel 满时，hot path 用 try_send() 静默丢弃（warn 日志记录）。
/// This background task drains the channel every second or
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
    pub upstream_name: Option<String>,
    pub app_id: Option<i64>,
}

/// How many entries to accumulate before forcing a flush.
const BATCH_SIZE: usize = 500; // Increased from 100 to 500 for better throughput
/// Maximum time between flushes even when batch is not full.
const FLUSH_INTERVAL: Duration = Duration::from_secs(2); // Increased from 1s to 2s

/// Bounded channel 容量：约 32K 条目 = 每秒 10K QPS 下约 3s 缓冲。
/// 超出后 hot path 静默丢弃（背压，防止 OOM）。
const CHANNEL_CAPACITY: usize = 32_768;

/// Spawn the background writer task.  Returns the sender end of the channel
/// which DnsHandler uses to enqueue entries (non-blocking, bounded).
pub fn spawn(
    db: DbPool,
    alert_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
) -> mpsc::Sender<QueryLogEntry> {
    let (tx, rx) = mpsc::channel::<QueryLogEntry>(CHANNEL_CAPACITY);
    tokio::spawn(run(db, rx, alert_tx));
    tx
}

async fn run(
    db: DbPool,
    mut rx: mpsc::Receiver<QueryLogEntry>,
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
                // Prune stale anomaly tracking entries (防止 HashMap 无限增长)
                let now = tokio::time::Instant::now();
                block_counts.retain(|_, (_, first_time)| {
                    now.duration_since(*first_time).as_secs() < 120
                });
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

    // SQLite SQLITE_MAX_VARIABLE_NUMBER defaults to 32766.
    // 11 fields/row → safe limit is 2978 rows/statement.
    // BATCH_SIZE=500 is well within limits; chunk defensively for future-proofing.
    const FIELDS_PER_ROW: usize = 11;
    const MAX_ROWS_PER_STMT: usize = 32766 / FIELDS_PER_ROW; // 2978

    let mut tx = db.begin().await?;

    for chunk in batch.chunks(MAX_ROWS_PER_STMT) {
        // Build multi-row VALUES placeholders: (?,?,?,?,?,?,?,?,?,?,?),(?,...),...
        // Values are bound via parameters — no SQL injection risk.
        let placeholders: String = chunk
            .iter()
            .map(|_| "(?,?,?,?,?,?,?,?,?,?,?)")
            .collect::<Vec<_>>()
            .join(",");

        let sql = format!(
            "INSERT INTO query_log \
             (time, client_ip, question, qtype, status, reason, answer, \
              elapsed_ns, upstream_ns, upstream, app_id) VALUES {}",
            placeholders
        );

        let mut query = sqlx::query(&sql);
        for entry in chunk {
            query = query
                .bind(&entry.time)
                .bind(&entry.client_ip)
                .bind(&entry.question)
                .bind(&entry.qtype)
                .bind(&entry.status)
                .bind(&entry.reason)
                .bind(&entry.answer)
                .bind(entry.elapsed_ns)
                .bind(entry.upstream_ns)
                .bind(&entry.upstream_name)
                .bind(entry.app_id);
        }
        query.execute(&mut *tx).await?;
    }

    tx.commit().await?;
    Ok(())
}
