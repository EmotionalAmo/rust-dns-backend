// SQLite Performance Optimization Test
// Tests PRAGMA settings and batch write improvements

use chrono::Utc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("SQLite Performance Optimization Test");
    println!("================================\n");

    // Initialize database with optimizations
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(20)
        .connect("sqlite::memory:")
        .await?;

    // Apply PRAGMA optimizations
    println!("Applying PRAGMA optimizations...");

    let wal_mode: (String,) = sqlx::query_as("PRAGMA journal_mode")
        .fetch_one(&pool)
        .await?;
    println!("  Initial journal_mode: {}", wal_mode.0);

    sqlx::query("PRAGMA journal_mode=WAL")
        .execute(&pool)
        .await?;

    let wal_mode: (String,) = sqlx::query_as("PRAGMA journal_mode")
        .fetch_one(&pool)
        .await?;
    println!("  After WAL: {}", wal_mode.0);

    sqlx::query("PRAGMA synchronous=NORMAL")
        .execute(&pool)
        .await?;

    let sync: (String,) = sqlx::query_as("PRAGMA synchronous")
        .fetch_one(&pool)
        .await?;
    println!("  synchronous: {}", sync.0);

    sqlx::query("PRAGMA cache_size=-64000")
        .execute(&pool)
        .await?;

    let cache: (i64,) = sqlx::query_as("PRAGMA cache_size").fetch_one(&pool).await?;
    println!("  cache_size: {} KB", cache.0 / 1024);

    sqlx::query("PRAGMA mmap_size=268435456")
        .execute(&pool)
        .await?;

    let mmap: (i64,) = sqlx::query_as("PRAGMA mmap_size").fetch_one(&pool).await?;
    println!("  mmap_size: {} MB", mmap.0 / (1024 * 1024));

    sqlx::query("PRAGMA wal_autocheckpoint=1000")
        .execute(&pool)
        .await?;

    let checkpoint: (i64,) = sqlx::query_as("PRAGMA wal_autocheckpoint")
        .fetch_one(&pool)
        .await?;
    println!("  wal_autocheckpoint: {}", checkpoint.0);

    println!("\n✅ All PRAGMA optimizations applied successfully!");

    // Create test table
    println!("\nCreating test table...");
    sqlx::query(
        "CREATE TABLE test_query_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            time TEXT NOT NULL,
            client_ip TEXT NOT NULL,
            question TEXT NOT NULL,
            qtype TEXT NOT NULL,
            status TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    println!("✅ Test table created");

    // Test batch write performance (500 records per batch)
    println!("\nTesting batch write performance (500 records per batch)...");
    let start = std::time::Instant::now();
    let mut total_inserted = 0;
    let num_batches = 10;
    let batch_size = 500;

    for batch_num in 0..num_batches {
        let mut tx = pool.begin().await?;

        for i in 0..batch_size {
            let offset = batch_num * batch_size + i;
            sqlx::query(
                "INSERT INTO test_query_log (time, client_ip, question, qtype, status)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(Utc::now().to_rfc3339())
            .bind("192.168.1.100")
            .bind(format!("test{}.example.com", offset))
            .bind("A")
            .bind("NOERROR")
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        total_inserted += batch_size;
        println!(
            "  Batch {}/{}: {} records",
            batch_num + 1,
            num_batches,
            batch_size
        );
    }

    let duration = start.elapsed();
    let qps = total_inserted as f64 / duration.as_secs_f64();

    println!("\nBatch Write Results:");
    println!("  Total records: {}", total_inserted);
    println!("  Total time: {:?}", duration);
    println!("  Throughput: {:.0} records/sec", qps);
    println!(
        "  Avg time per record: {:.2} ms",
        duration.as_millis() as f64 / total_inserted as f64
    );

    // Verify records
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM test_query_log")
        .fetch_one(&pool)
        .await?;

    println!("\nVerification: {} records inserted", count.0);

    println!("\n✅ SQLite Performance Optimization Test Complete!");

    Ok(())
}
