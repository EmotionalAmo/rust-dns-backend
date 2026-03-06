use hickory_client::client::{AsyncClient, ClientHandle};
use hickory_client::udp::UdpClientStream;
use hickory_proto::rr::{DNSClass, Name, RecordType};
use rand::seq::SliceRandom;
use rand::SeedableRng;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

// DNS 服务器配置
#[derive(Debug, Clone)]
struct DnsServer {
    name: String,
    addr: SocketAddr,
}

// 基准测试结果
#[derive(Debug, Clone)]
struct BenchmarkResult {
    server_name: String,
    total_queries: usize,
    total_errors: usize,
    total_time_ms: f64,
    latencies_ms: Vec<f64>,
    qps: f64,
}

fn calculate_percentiles(data: &mut [f64]) -> (f64, f64, f64) {
    data.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let len = data.len();

    if len == 0 {
        return (0.0, 0.0, 0.0);
    }

    let p50_idx = (len * 50) / 100;
    let p95_idx = (len * 95) / 100;
    let p99_idx = (len * 99) / 100;

    (
        data[p50_idx.min(len - 1)],
        data[p95_idx.min(len - 1)],
        data[p99_idx.min(len - 1)],
    )
}

async fn benchmark_server(
    server: &DnsServer,
    domains: &[&str],
    concurrency: usize,
    duration_secs: u64,
) -> BenchmarkResult {
    println!(
        "Benchmarking {} ({}) with {} concurrent workers for {} seconds...",
        server.name, server.addr, concurrency, duration_secs
    );

    let success_count = Arc::new(AtomicUsize::new(0));
    let error_count = Arc::new(AtomicUsize::new(0));
    let latencies = Arc::new(std::sync::Mutex::new(Vec::new()));

    let stop_signal = Arc::new(AtomicBool::new(false));
    let mut tasks = Vec::with_capacity(concurrency);

    let start_time = Instant::now();

    for _ in 0..concurrency {
        let sc = Arc::clone(&success_count);
        let ec = Arc::clone(&error_count);
        let lats = Arc::clone(&latencies);
        let stop = Arc::clone(&stop_signal);
        let local_domains: Vec<String> = domains.iter().map(|s| s.to_string()).collect();
        let server_addr = server.addr;

        let task = tokio::spawn(async move {
            let client_res = {
                let stream = UdpClientStream::<tokio::net::UdpSocket>::new(server_addr);
                AsyncClient::connect(stream).await
            };

            if let Ok((mut client, bg)) = client_res {
                tokio::spawn(bg);

                let mut rng = rand::rngs::SmallRng::from_entropy();

                while !stop.load(Ordering::Relaxed) {
                    let domain = local_domains.choose(&mut rng).unwrap();
                    let name = Name::from_str(&format!("{}.", domain)).unwrap_or_else(|_| Name::from_str("example.com.").unwrap());

                    let req_start = Instant::now();
                    match client.query(name, DNSClass::IN, RecordType::A).await {
                        Ok(_) => {
                            let elapsed_ms = req_start.elapsed().as_secs_f64() * 1000.0;
                            {
                                let mut lats_guard = lats.lock().unwrap();
                                lats_guard.push(elapsed_ms);
                            }
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
    stop_signal.store(true, Ordering::Relaxed);

    for task in tasks {
        let _ = task.await;
    }

    let end_time = start_time.elapsed();
    let total_queries = success_count.load(Ordering::Relaxed);
    let total_errors = error_count.load(Ordering::Relaxed);
    let total_time_ms = end_time.as_secs_f64() * 1000.0;
    let qps = (total_queries as f64) / end_time.as_secs_f64();

    let latencies_vec = {
        let guard = latencies.lock().unwrap();
        guard.clone()
    };

    BenchmarkResult {
        server_name: server.name.clone(),
        total_queries,
        total_errors,
        total_time_ms,
        latencies_ms: latencies_vec,
        qps,
    }
}

fn generate_markdown_report(results: &[BenchmarkResult]) -> String {
    let mut report = String::new();

    report.push_str("# DNS 性能基准测试报告\n\n");
    report.push_str(&format!("测试时间: {}\n\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));

    // 总结表格
    report.push_str("## 测试总结\n\n");
    report.push_str("| DNS 服务器 | 平均延迟 (ms) | P50 (ms) | P95 (ms) | P99 (ms) | QPS | 总查询数 | 错误率 |\n");
    report.push_str("|-----------|--------------|---------|---------|---------|-----|---------|--------|\n");

    for result in results {
        let mut latencies = result.latencies_ms.clone();
        let (p50, p95, p99) = calculate_percentiles(&mut latencies);
        let avg = latencies.iter().sum::<f64>() / latencies.len().max(1) as f64;
        let error_rate = if result.total_queries + result.total_errors > 0 {
            (result.total_errors as f64) / ((result.total_queries + result.total_errors) as f64) * 100.0
        } else {
            0.0
        };

        report.push_str(&format!(
            "| {} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {} | {:.2}% |\n",
            result.server_name,
            avg,
            p50,
            p95,
            p99,
            result.qps,
            result.total_queries,
            error_rate
        ));
    }

    report.push_str("\n");

    // 详细结果
    report.push_str("## 详细结果\n\n");

    for result in results {
        let mut latencies = result.latencies_ms.clone();
        let (p50, p95, p99) = calculate_percentiles(&mut latencies);
        let avg = latencies.iter().sum::<f64>() / latencies.len().max(1) as f64;
        let error_rate = if result.total_queries + result.total_errors > 0 {
            (result.total_errors as f64) / ((result.total_queries + result.total_errors) as f64) * 100.0
        } else {
            0.0
        };

        report.push_str(&format!("### {}\n\n", result.server_name));
        report.push_str(&format!("- **总耗时**: {:.2} ms\n", result.total_time_ms));
        report.push_str(&format!("- **成功查询**: {}\n", result.total_queries));
        report.push_str(&format!("- **失败查询**: {}\n", result.total_errors));
        report.push_str(&format!("- **错误率**: {:.2}%\n", error_rate));
        report.push_str(&format!("- **QPS**: {:.2}\n", result.qps));
        report.push_str(&format!("- **平均延迟**: {:.2} ms\n", avg));
        report.push_str(&format!("- **P50 延迟**: {:.2} ms\n", p50));
        report.push_str(&format!("- **P95 延迟**: {:.2} ms\n", p95));
        report.push_str(&format!("- **P99 延迟**: {:.2} ms\n", p99));
        report.push_str("\n");
    }

    // 性能对比
    if results.len() >= 2 {
        report.push_str("## 性能对比\n\n");

        let rust_dns = results.iter().find(|r| r.server_name.contains("rust-dns"));
        let upstream = results.iter().find(|r| r.server_name.contains("Upstream") || r.server_name.contains("Cloudflare") || r.server_name.contains("Google"));

        if let (Some(rust), Some(up)) = (rust_dns, upstream) {
            let rust_avg = rust.latencies_ms.iter().sum::<f64>() / rust.latencies_ms.len().max(1) as f64;
            let up_avg = up.latencies_ms.iter().sum::<f64>() / up.latencies_ms.len().max(1) as f64;

            let _speedup = up_avg / rust_avg;
            let qps_improvement = if up.qps > 0.0 { (rust.qps / up.qps - 1.0) * 100.0 } else { 0.0 };

            report.push_str(&format!("### rust-dns vs {}\n\n", up.server_name));
            report.push_str(&format!("- **延迟改进**: rust-dns 比 {} 快 {:.1}%\n", up.server_name, (1.0 - rust_avg / up_avg) * 100.0));
            report.push_str(&format!("- **QPS 提升**: {:.1}%\n", qps_improvement));
            report.push_str("\n");
        }
    }

    report.push_str("---\n\n");
    report.push_str("*测试说明: 每个服务器使用相同的并发级别和测试时长，随机选择常见域名进行 A 记录查询。*\n");

    report
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    // 默认参数
    let concurrency = args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);
    let duration_secs = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);

    println!("DNS 性能基准测试");
    println!("=================");
    println!("并发级别: {}", concurrency);
    println!("测试时长: {} 秒\n", duration_secs);

    // 定义要测试的 DNS 服务器
    let servers = vec![
        DnsServer {
            name: "rust-dns (127.0.0.1:5354)".to_string(),
            addr: "127.0.0.1:5354".parse()?,
        },
        DnsServer {
            name: "Cloudflare DNS (1.1.1.1)".to_string(),
            addr: "1.1.1.1:53".parse()?,
        },
        DnsServer {
            name: "Google DNS (8.8.8.8)".to_string(),
            addr: "8.8.8.8:53".parse()?,
        },
    ];

    // 测试域名列表
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
        "spotify.com",
        "pinterest.com",
        "tiktok.com",
        "twitch.tv",
        "discord.com",
        "zoom.us",
        "salesforce.com",
        "slack.com",
        "dropbox.com",
        "airbnb.com",
        "uber.com",
        "lyft.com",
        "stripe.com",
        "shopify.com",
        "wordpress.com",
        "medium.com",
        "producthunt.com",
        "hackernews.com",
        "npmjs.com",
        "docker.com",
        "kubernetes.io",
        "redis.io",
        "mongodb.com",
        "postgresql.org",
        "nginx.org",
        "apache.org",
    ];

    let mut results = Vec::new();

    // 运行基准测试
    for server in &servers {
        let result = benchmark_server(server, &domains, concurrency, duration_secs).await;
        println!(
            "  完成: {} - QPS: {:.2}, 平均延迟: {:.2} ms\n",
            result.server_name, result.qps,
            if !result.latencies_ms.is_empty() {
                result.latencies_ms.iter().sum::<f64>() / result.latencies_ms.len() as f64
            } else {
                0.0
            }
        );
        results.push(result);
    }

    // 生成报告
    let report = generate_markdown_report(&results);

    // 保存到文件
    let report_file = "/Users/emotionalamo/Developer/DNS-Project/docs/fullstack/dns-benchmark-report.md";
    tokio::fs::write(report_file, report).await?;

    println!("\n--- 测试完成 ---");
    println!("报告已保存到: {}", report_file);

    Ok(())
}
