#!/usr/bin/env bash
# ==============================================================================
# run-benchmark.sh — DNS 竞品性能对比测试脚本
#
# 功能：
#   1. 等待三个服务（rust-dns、Pi-hole、AdGuard Home）就绪
#   2. 用 dnsperf 对每个服务跑 30 秒 DNS 解析测试
#   3. 记录内存占用快照（通过 docker stats）
#   4. 输出对比表格到 results/benchmark-YYYYMMDD.txt
#
# 前置条件：
#   - 已安装 Docker、dnsperf
#   - 已在 projects/rust-dns-backend/ 执行 docker build -t rust-dns:latest .
#   - 已在 benchmarks/ 目录执行 docker compose up -d
#
# 用法：
#   cd projects/rust-dns-backend/benchmarks
#   docker compose up -d
#   bash run-benchmark.sh
# ==============================================================================

set -euo pipefail

# ------------------------------------------------------------------------------
# 配置项
# ------------------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DATASET="${SCRIPT_DIR}/../benchmark/datasets/top-10k-dnsperf.txt"
RESULTS_DIR="${SCRIPT_DIR}/results"
RESULT_FILE="${RESULTS_DIR}/benchmark-$(date +%Y%m%d-%H%M%S).txt"

# dnsperf 参数
BENCH_DURATION=30       # 测试持续时间（秒）
BENCH_CLIENTS=50        # 并发客户端数
BENCH_QPS=500           # 目标 QPS 上限（0 表示不限）

# 服务端口（对应 docker-compose.yml 中的映射）
RUST_DNS_PORT=5301
PIHOLE_PORT=5302
ADGUARD_PORT=5303

# Docker 容器名
RUST_DNS_CONTAINER="bench-rust-dns"
PIHOLE_CONTAINER="bench-pihole"
ADGUARD_CONTAINER="bench-adguard"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

# ------------------------------------------------------------------------------
# 工具函数
# ------------------------------------------------------------------------------

log_info()  { echo -e "${GREEN}[INFO]${NC}  $*"; }
log_warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }
log_step()  { echo -e "\n${BOLD}${BLUE}>>> $*${NC}"; }

# 检查依赖工具
check_dependencies() {
    local missing=()
    for cmd in dnsperf docker dig; do
        if ! command -v "$cmd" &>/dev/null; then
            missing+=("$cmd")
        fi
    done

    if [[ ${#missing[@]} -gt 0 ]]; then
        log_error "缺少依赖工具: ${missing[*]}"
        echo ""
        echo "  安装 dnsperf："
        echo "    macOS:  brew install bind  # 包含 dnsperf"
        echo "    Ubuntu: sudo apt install dnsperf"
        echo ""
        exit 1
    fi
    log_info "依赖检查通过: dnsperf, docker, dig"
}

# 等待 DNS 服务就绪（最多等 60 秒）
wait_for_dns() {
    local name="$1"
    local port="$2"
    local max_wait=60
    local waited=0

    log_info "等待 ${name} (端口 ${port}) 就绪..."
    while ! dig +short +time=2 +tries=1 google.com @127.0.0.1 -p "$port" &>/dev/null; do
        if [[ $waited -ge $max_wait ]]; then
            log_error "${name} 在 ${max_wait}s 内未就绪，跳过"
            return 1
        fi
        sleep 3
        waited=$((waited + 3))
        log_info "  还在等待 ${name}... (${waited}s)"
    done
    log_info "${name} 已就绪 (等待了 ${waited}s)"
    return 0
}

# 运行 dnsperf 测试，返回解析后的指标
run_dnsperf() {
    local name="$1"
    local port="$2"
    local output_file="$3"

    log_info "开始测试 ${name} (端口 ${port})，持续 ${BENCH_DURATION}s..."

    # 运行 dnsperf，-Q 0 表示不限制 QPS（让服务器自己跑最快速度）
    # 用 -Q ${BENCH_QPS} 可以限速，更公平地对比延迟
    dnsperf \
        -s 127.0.0.1 \
        -p "$port" \
        -d "$DATASET" \
        -l "$BENCH_DURATION" \
        -c "$BENCH_CLIENTS" \
        -Q "$BENCH_QPS" \
        -e \
        2>&1 | tee "${output_file}"

    log_info "${name} 测试完成，结果写入 ${output_file}"
}

# 从 dnsperf 输出中提取关键指标
parse_dnsperf_output() {
    local file="$1"

    if [[ ! -f "$file" ]]; then
        echo "N/A|N/A|N/A|N/A|N/A"
        return
    fi

    local qps avg_latency min_latency max_latency completion_rate
    qps=$(grep "Queries per second" "$file" | awk '{print $NF}' | xargs printf "%.1f" 2>/dev/null || echo "N/A")
    avg_latency=$(grep "Average Latency" "$file" | awk '{print $4}' | tr -d '(' | xargs printf "%.4f" 2>/dev/null || echo "N/A")
    min_latency=$(grep "Average Latency" "$file" | grep -oP 'min \K[0-9.]+' 2>/dev/null | xargs printf "%.4f" || echo "N/A")
    max_latency=$(grep "Average Latency" "$file" | grep -oP 'max \K[0-9.]+' 2>/dev/null | xargs printf "%.4f" || echo "N/A")
    completion_rate=$(grep "Queries completed" "$file" | grep -oP '\([\d.]+%\)' | tr -d '()%' 2>/dev/null || echo "N/A")

    echo "${qps}|${avg_latency}|${min_latency}|${max_latency}|${completion_rate}"
}

# 获取容器内存占用（MiB）
get_container_memory() {
    local container="$1"

    if ! docker ps --format '{{.Names}}' | grep -q "^${container}$"; then
        echo "N/A"
        return
    fi

    # docker stats --no-stream 输出格式: CONTAINER  CPU  MEM USAGE / LIMIT  MEM%  ...
    # 取 MEM USAGE 字段
    local mem
    mem=$(docker stats --no-stream --format "{{.MemUsage}}" "$container" 2>/dev/null | awk '{print $1}')

    # 统一转换为 MiB（可能是 MiB 或 GiB）
    if [[ "$mem" == *"GiB"* ]]; then
        echo "$mem ($(echo "$mem" | sed 's/GiB//' | awk '{printf "%.1f MiB", $1 * 1024}'))"
    else
        echo "$mem"
    fi
}

# 获取容器 CPU 使用率
get_container_cpu() {
    local container="$1"

    if ! docker ps --format '{{.Names}}' | grep -q "^${container}$"; then
        echo "N/A"
        return
    fi

    docker stats --no-stream --format "{{.CPUPerc}}" "$container" 2>/dev/null || echo "N/A"
}

# 打印分隔线
print_separator() {
    printf '%0.s-' {1..80}
    echo ""
}

# ------------------------------------------------------------------------------
# 主流程
# ------------------------------------------------------------------------------

main() {
    echo ""
    echo -e "${BOLD}${CYAN}=================================================================${NC}"
    echo -e "${BOLD}${CYAN}   DNS 竞品 Benchmark 对比测试                                   ${NC}"
    echo -e "${BOLD}${CYAN}   rust-dns vs Pi-hole vs AdGuard Home                           ${NC}"
    echo -e "${BOLD}${CYAN}   $(date '+%Y-%m-%d %H:%M:%S')                                  ${NC}"
    echo -e "${BOLD}${CYAN}=================================================================${NC}"
    echo ""

    # 1. 前置检查
    log_step "Step 1: 环境检查"
    check_dependencies

    # 检查数据集文件
    if [[ ! -f "$DATASET" ]]; then
        log_error "数据集文件不存在: ${DATASET}"
        log_error "请确保在 projects/rust-dns-backend/benchmark/datasets/ 目录下有 top-10k-dnsperf.txt"
        exit 1
    fi
    log_info "数据集: ${DATASET} ($(wc -l < "$DATASET") 条记录)"

    # 创建结果目录
    mkdir -p "$RESULTS_DIR"
    log_info "结果目录: ${RESULTS_DIR}"

    # 2. 等待服务就绪
    log_step "Step 2: 等待服务就绪"
    declare -A SERVICE_READY
    SERVICE_READY["rust-dns"]=0
    SERVICE_READY["pihole"]=0
    SERVICE_READY["adguard"]=0

    wait_for_dns "rust-dns"     "$RUST_DNS_PORT" && SERVICE_READY["rust-dns"]=1  || true
    wait_for_dns "Pi-hole"      "$PIHOLE_PORT"   && SERVICE_READY["pihole"]=1    || true
    wait_for_dns "AdGuard Home" "$ADGUARD_PORT"  && SERVICE_READY["adguard"]=1   || true

    # 检查是否有任何服务就绪
    local any_ready=0
    for svc in rust-dns pihole adguard; do
        [[ ${SERVICE_READY[$svc]} -eq 1 ]] && any_ready=1
    done

    if [[ $any_ready -eq 0 ]]; then
        log_error "所有服务均未就绪，请先运行: docker compose up -d"
        exit 1
    fi

    # 3. 记录测试前的内存基准（空闲状态）
    log_step "Step 3: 记录空闲内存基准"
    local idle_mem_rust idle_mem_pihole idle_mem_adguard
    idle_mem_rust=$(get_container_memory "$RUST_DNS_CONTAINER")
    idle_mem_pihole=$(get_container_memory "$PIHOLE_CONTAINER")
    idle_mem_adguard=$(get_container_memory "$ADGUARD_CONTAINER")
    log_info "rust-dns     空闲内存: ${idle_mem_rust}"
    log_info "Pi-hole      空闲内存: ${idle_mem_pihole}"
    log_info "AdGuard Home 空闲内存: ${idle_mem_adguard}"

    # 4. 依次运行 dnsperf 测试
    log_step "Step 4: 运行 dnsperf 测试"
    log_warn "每个服务测试 ${BENCH_DURATION}s，共需约 $((BENCH_DURATION * 3 + 30))s"

    local raw_rust="${RESULTS_DIR}/raw-rust-dns-$(date +%Y%m%d-%H%M%S).txt"
    local raw_pihole="${RESULTS_DIR}/raw-pihole-$(date +%Y%m%d-%H%M%S).txt"
    local raw_adguard="${RESULTS_DIR}/raw-adguard-$(date +%Y%m%d-%H%M%S).txt"

    # 测试 rust-dns
    if [[ ${SERVICE_READY["rust-dns"]} -eq 1 ]]; then
        run_dnsperf "rust-dns" "$RUST_DNS_PORT" "$raw_rust"
        local load_mem_rust
        load_mem_rust=$(get_container_memory "$RUST_DNS_CONTAINER")
        log_info "rust-dns 负载内存: ${load_mem_rust}"
    else
        log_warn "rust-dns 未就绪，跳过测试"
    fi

    # 等待 5 秒，让系统稳定
    sleep 5

    # 测试 Pi-hole
    if [[ ${SERVICE_READY["pihole"]} -eq 1 ]]; then
        run_dnsperf "Pi-hole" "$PIHOLE_PORT" "$raw_pihole"
        local load_mem_pihole
        load_mem_pihole=$(get_container_memory "$PIHOLE_CONTAINER")
        log_info "Pi-hole 负载内存: ${load_mem_pihole}"
    else
        log_warn "Pi-hole 未就绪，跳过测试"
    fi

    sleep 5

    # 测试 AdGuard Home
    if [[ ${SERVICE_READY["adguard"]} -eq 1 ]]; then
        run_dnsperf "AdGuard Home" "$ADGUARD_PORT" "$raw_adguard"
        local load_mem_adguard
        load_mem_adguard=$(get_container_memory "$ADGUARD_CONTAINER")
        log_info "AdGuard Home 负载内存: ${load_mem_adguard}"
    else
        log_warn "AdGuard Home 未就绪，跳过测试"
    fi

    # 5. 解析结果
    log_step "Step 5: 解析测试结果"
    IFS='|' read -r rust_qps rust_avg_lat rust_min_lat rust_max_lat rust_completion \
        <<< "$(parse_dnsperf_output "$raw_rust" 2>/dev/null || echo "N/A|N/A|N/A|N/A|N/A")"
    IFS='|' read -r pihole_qps pihole_avg_lat pihole_min_lat pihole_max_lat pihole_completion \
        <<< "$(parse_dnsperf_output "$raw_pihole" 2>/dev/null || echo "N/A|N/A|N/A|N/A|N/A")"
    IFS='|' read -r adguard_qps adguard_avg_lat adguard_min_lat adguard_max_lat adguard_completion \
        <<< "$(parse_dnsperf_output "$raw_adguard" 2>/dev/null || echo "N/A|N/A|N/A|N/A|N/A")"

    # 6. 输出结果报告
    log_step "Step 6: 生成报告"

    {
        echo "================================================================="
        echo "  DNS 竞品 Benchmark 对比报告"
        echo "  测试时间: $(date '+%Y-%m-%d %H:%M:%S')"
        echo "  测试参数: 持续 ${BENCH_DURATION}s，并发 ${BENCH_CLIENTS} 客户端，目标 QPS ${BENCH_QPS}"
        echo "  测试数据集: $(basename "$DATASET") ($(wc -l < "$DATASET") 条)"
        echo "================================================================="
        echo ""
        echo "--- 内存占用对比（空闲状态）---"
        printf "  %-20s  %s\n" "rust-dns:"     "${idle_mem_rust:-N/A}"
        printf "  %-20s  %s\n" "Pi-hole:"      "${idle_mem_pihole:-N/A}"
        printf "  %-20s  %s\n" "AdGuard Home:" "${idle_mem_adguard:-N/A}"
        echo ""
        echo "--- DNS 延迟 & 吞吐量对比 ---"
        printf "  %-16s  %10s  %12s  %12s  %12s  %12s\n" \
            "服务" "QPS" "平均延迟(s)" "最小延迟(s)" "最大延迟(s)" "完成率(%)"
        print_separator
        printf "  %-16s  %10s  %12s  %12s  %12s  %12s\n" \
            "rust-dns"     "${rust_qps:-N/A}"     "${rust_avg_lat:-N/A}"     "${rust_min_lat:-N/A}"     "${rust_max_lat:-N/A}"     "${rust_completion:-N/A}"
        printf "  %-16s  %10s  %12s  %12s  %12s  %12s\n" \
            "Pi-hole"      "${pihole_qps:-N/A}"   "${pihole_avg_lat:-N/A}"   "${pihole_min_lat:-N/A}"   "${pihole_max_lat:-N/A}"   "${pihole_completion:-N/A}"
        printf "  %-16s  %10s  %12s  %12s  %12s  %12s\n" \
            "AdGuard Home" "${adguard_qps:-N/A}"  "${adguard_avg_lat:-N/A}"  "${adguard_min_lat:-N/A}"  "${adguard_max_lat:-N/A}"  "${adguard_completion:-N/A}"
        echo ""
        echo "--- 原始数据文件 ---"
        echo "  rust-dns:     $(basename "${raw_rust}")"
        echo "  Pi-hole:      $(basename "${raw_pihole}")"
        echo "  AdGuard Home: $(basename "${raw_adguard}")"
        echo ""
        echo "================================================================="
        echo "  说明"
        echo "================================================================="
        echo "  QPS             - 每秒查询数，越高越好"
        echo "  平均延迟        - 单次查询平均耗时（秒），越低越好"
        echo "  最小/最大延迟   - 延迟抖动范围，范围越小越稳定"
        echo "  完成率          - 成功完成的查询比例，越高越好"
        echo "  内存占用        - Docker 容器实际使用内存（空闲状态）"
        echo ""
        echo "  预期基准（参考市场研究数据）："
        echo "  rust-dns:     目标 10-15 MiB RAM，Rust 无 GC，单二进制"
        echo "  Pi-hole:      通常 50-100 MiB RAM，Python+FTL 多进程"
        echo "  AdGuard Home: 通常 30-50 MiB RAM，Go 语言带 GC"
        echo "================================================================="
    } | tee "$RESULT_FILE"

    # 终端额外输出彩色总结
    echo ""
    echo -e "${BOLD}${GREEN}报告已保存至: ${RESULT_FILE}${NC}"
    echo -e "${BOLD}原始数据目录: ${RESULTS_DIR}/${NC}"
    echo ""
}

# 捕获 Ctrl+C，清理临时状态
trap 'echo ""; log_warn "测试中断"; exit 130' INT TERM

main "$@"
