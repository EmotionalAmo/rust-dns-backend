# DNS 性能对比基准测试

用于对比 rust-dns 与公共 DNS 服务器的性能。

## 快速开始

### 使用运行脚本（推荐）

```bash
cd /Users/emotionalamo/Developer/DNS-Project/projects/rust-dns-backend/benchmark

# 使用默认参数（并发100，时长30秒）
./run-compare.sh

# 自定义参数
./run-compare.sh 50 60  # 并发50，测试60秒
```

### 手动运行

1. 确保 rust-dns 服务正在运行（监听 127.0.0.1:5354）

2. 编译工具：
```bash
cargo build --release --bin compare-dns
```

3. 运行测试：
```bash
./target/release/compare-dns [并发级别] [测试时长秒数]

# 示例
./target/release/compare-dns 100 30  # 100并发，30秒
```

## 测试的 DNS 服务器

- **rust-dns** (127.0.0.1:5354) - 本地 DNS 服务器
- **Cloudflare DNS** (1.1.1.1) - 公共 DNS
- **Google DNS** (8.8.8.8) - 公共 DNS

## 测试指标

| 指标 | 说明 |
|------|------|
| 平均延迟 | 所有成功查询的平均响应时间 |
| P50/P95/P99 | 第50/95/99百分位的延迟值 |
| QPS | 每秒查询数 |
| 错误率 | 失败查询占总查询的比例 |

## 输出

测试完成后会生成 Markdown 格式的报告：
`/Users/emotionalamo/Developer/DNS-Project/docs/fullstack/dns-benchmark-report.md`

## 示例报告

```markdown
| DNS 服务器 | 平均延迟 (ms) | P50 (ms) | P95 (ms) | P99 (ms) | QPS | 错误率 |
|-----------|--------------|---------|---------|---------|-----|--------|
| rust-dns | 5.33 | 4.30 | 10.59 | 14.86 | 9376.57 | 0.00% |
| Cloudflare | 25.30 | 19.52 | 61.68 | 111.00 | 1950.59 | 0.01% |
```

## 测试域名

工具随机选择 45 个常见互联网域名进行 A 记录查询，包括：
- google.com, youtube.com, facebook.com, baidu.com, wikipedia.org
- github.com, reddit.com, twitter.com, amazon.com, linkedin.com
- 等等...

## 依赖

- Rust 1.70+
- hickory-client
- tokio
- chrono
- rand
