use hickory_client::client::{AsyncClient, ClientHandle};
use hickory_client::udp::UdpClientStream;
use hickory_proto::rr::{DNSClass, Name, RecordType};
use rand::seq::SliceRandom;
use rand::SeedableRng;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <server_ip:port> <concurrency> [duration_secs]", args[0]);
        eprintln!("Example: {} 127.0.0.1:54 100 10", args[0]);
        std::process::exit(1);
    }

    let server_addr: SocketAddr = args[1].parse()?;
    let concurrency: usize = args[2].parse()?;
    let duration_secs: u64 = args.get(3).map(|s| s.parse().unwrap_or(10)).unwrap_or(10);

    let domains = vec![
        "google.com",
        "youtube.com",
        "facebook.com",
        "baidu.com",
        "wikipedia.org",
        "yahoo.com",
        "reddit.com",
        "amazon.com",
        "twitter.com",
        "instagram.com",
        "linkedin.com",
        "netflix.com",
        "bing.com",
        "live.com",
        "microsoft.com",
        "apple.com",
        "github.com",
        "stackoverflow.com",
        "twitch.tv",
        "office.com",
    ];

    println!(
        "Benchmarking DNS server at {} with {} concurrent workers for {} seconds...",
        server_addr, concurrency, duration_secs
    );

    let success_count = Arc::new(AtomicUsize::new(0));
    let error_count = Arc::new(AtomicUsize::new(0));
    let total_latency_ns = Arc::new(AtomicUsize::new(0));

    let stop_signal = Arc::new(AtomicUsize::new(0));
    let mut tasks = Vec::with_capacity(concurrency);

    let start_time = Instant::now();

    for _ in 0..concurrency {
        let sc = Arc::clone(&success_count);
        let ec = Arc::clone(&error_count);
        let tl = Arc::clone(&total_latency_ns);
        let stop = Arc::clone(&stop_signal);
        let local_domains = domains.clone();

        let task = tokio::spawn(async move {
            // Establish a new UDP connection for the worker
            let stream = UdpClientStream::<tokio::net::UdpSocket>::new(server_addr);
            let client_res = AsyncClient::connect(stream).await;
            
            if let Ok((mut client, bg)) = client_res {
                tokio::spawn(bg);
                
                let mut rng = rand::rngs::SmallRng::from_entropy();

                while stop.load(Ordering::Relaxed) == 0 {
                    let domain = local_domains.choose(&mut rng).unwrap();
                    let name = Name::from_str(&format!("{}.", domain)).unwrap();
                    
                    let req_start = Instant::now();
                    match client.query(name, DNSClass::IN, RecordType::A).await {
                        Ok(_) => {
                            let elap = req_start.elapsed().as_nanos() as usize;
                            tl.fetch_add(elap, Ordering::Relaxed);
                            sc.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(_) => {
                            ec.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            } else {
                eprintln!("Worker failed to connect to {}", server_addr);
            }
        });

        tasks.push(task);
    }

    tokio::time::sleep(Duration::from_secs(duration_secs)).await;
    stop_signal.store(1, Ordering::Relaxed);

    for task in tasks {
        let _ = task.await;
    }

    let end_time = start_time.elapsed();
    let total_queries = success_count.load(Ordering::Relaxed);
    let total_errors = error_count.load(Ordering::Relaxed);
    let total_ms = end_time.as_millis() as f64;
    let qps = (total_queries as f64) / (end_time.as_secs_f64());
    
    let avg_latency_ms = if total_queries > 0 {
        (total_latency_ns.load(Ordering::Relaxed) as f64) / (total_queries as f64) / 1_000_000.0
    } else {
        0.0
    };

    println!("\n--- Benchmark Results ---");
    println!("Server: {}", server_addr);
    println!("Time taken: {:.2} ms", total_ms);
    println!("Total successful queries: {}", total_queries);
    println!("Total failed queries: {}", total_errors);
    println!("Queries Per Second (QPS): {:.2}", qps);
    println!("Average Latency (successful): {:.2} ms", avg_latency_ms);

    Ok(())
}
