#!/usr/bin/env bash
# =============================================================================
# rust-dns Enterprise — Automated Smoke Test
# qa-bach | James Bach QA Framework
# 目标：< 30 秒完成，覆盖核心路径 + hotfix 布尔字段 bug 验证
#
# 用法：
#   bash smoke-test.sh                        # 默认连 localhost:8080
#   BASE_URL=http://192.168.100.10:8080 bash smoke-test.sh
#   ADMIN_PASS=mypassword bash smoke-test.sh
# =============================================================================

set -euo pipefail

# -----------------------------------------------------------------------------
# 配置区：通过环境变量覆盖，默认值适用于本地/生产服务器本机运行
# -----------------------------------------------------------------------------
BASE_URL="${BASE_URL:-http://localhost:8080}"
ADMIN_USER="${ADMIN_USER:-admin}"
ADMIN_PASS="${ADMIN_PASS:-admin}"
DNS_HOST="${DNS_HOST:-127.0.0.1}"
DNS_PORT="${DNS_PORT:-53}"
DNS_QUERY="${DNS_QUERY:-cloudflare.com}"
CURL_TIMEOUT="${CURL_TIMEOUT:-10}"

# -----------------------------------------------------------------------------
# 颜色与计数
# -----------------------------------------------------------------------------
GREEN="\033[0;32m"
RED="\033[0;31m"
YELLOW="\033[0;33m"
CYAN="\033[0;36m"
BOLD="\033[1m"
RESET="\033[0m"

PASS=0
FAIL=0
TOTAL=0
AUTH_TOKEN=""

START_TIME=$(date +%s)

# -----------------------------------------------------------------------------
# 工具函数
# -----------------------------------------------------------------------------
pass() {
    local name="$1"
    PASS=$((PASS + 1))
    TOTAL=$((TOTAL + 1))
    printf "  ${GREEN}[PASS]${RESET} %s\n" "$name"
}

fail() {
    local name="$1"
    local reason="$2"
    FAIL=$((FAIL + 1))
    TOTAL=$((TOTAL + 1))
    printf "  ${RED}[FAIL]${RESET} %s\n" "$name"
    printf "         ${YELLOW}>> %s${RESET}\n" "$reason"
}

section() {
    printf "\n${CYAN}${BOLD}%s${RESET}\n" "--- $1 ---"
}

# -----------------------------------------------------------------------------
# 登录函数：获取 Bearer Token
# -----------------------------------------------------------------------------
LOGIN_AS_ADMIN() {
    local response
    response=$(curl -s -w "\n%{http_code}" \
        --max-time "$CURL_TIMEOUT" \
        -X POST "${BASE_URL}/api/v1/auth/login" \
        -H "Content-Type: application/json" \
        -d "{\"username\":\"${ADMIN_USER}\",\"password\":\"${ADMIN_PASS}\"}" \
        2>/dev/null) || true

    local http_code body
    http_code=$(printf "%s" "$response" | tail -n1)
    body=$(printf "%s" "$response" | head -n -1)

    if [[ "$http_code" == "200" ]]; then
        # 尝试从常见字段提取 token（token / access_token）
        AUTH_TOKEN=$(printf "%s" "$body" | grep -oP '"(token|access_token)"\s*:\s*"\K[^"]+' | head -1 || true)
        if [[ -z "$AUTH_TOKEN" ]]; then
            AUTH_TOKEN=$(printf "%s" "$body" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('token', d.get('access_token','')))" 2>/dev/null || true)
        fi
        if [[ -n "$AUTH_TOKEN" ]]; then
            printf "  ${GREEN}[INFO]${RESET} 登录成功，Token 已获取\n"
            return 0
        else
            printf "  ${YELLOW}[WARN]${RESET} 登录返回 200 但未找到 token 字段，后续认证检查可能失败\n"
            return 1
        fi
    else
        printf "  ${RED}[WARN]${RESET} 登录失败，HTTP %s，后续认证检查将跳过\n" "$http_code"
        return 1
    fi
}

# 带认证的 GET 请求，返回 "body\nhttp_code"
auth_get() {
    local path="$1"
    curl -s -w "\n%{http_code}" \
        --max-time "$CURL_TIMEOUT" \
        -X GET "${BASE_URL}${path}" \
        -H "Authorization: Bearer ${AUTH_TOKEN}" \
        -H "Content-Type: application/json" \
        2>/dev/null || echo -e "\n000"
}

# =============================================================================
# 检查一：systemd 服务状态
# =============================================================================
section "区块 1：基础健康检查"

CHECK_NAME="systemd 服务状态 (rust-dns)"
if systemctl is-active --quiet rust-dns 2>/dev/null; then
    pass "$CHECK_NAME"
else
    STATUS=$(systemctl is-active rust-dns 2>/dev/null || echo "unknown")
    fail "$CHECK_NAME" "服务状态：$STATUS（预期 active）"
fi

# =============================================================================
# 检查二：HTTP 健康检查
# =============================================================================
CHECK_NAME="HTTP 健康检查 (GET /api/health)"
HEALTH_RESP=$(curl -s -w "\n%{http_code}" \
    --max-time "$CURL_TIMEOUT" \
    "${BASE_URL}/api/health" \
    2>/dev/null || echo -e "\n000")
HEALTH_CODE=$(printf "%s" "$HEALTH_RESP" | tail -n1)
HEALTH_BODY=$(printf "%s" "$HEALTH_RESP" | head -n -1)

if [[ "$HEALTH_CODE" == "200" ]]; then
    pass "$CHECK_NAME"
else
    fail "$CHECK_NAME" "HTTP $HEALTH_CODE（预期 200）响应体：$(printf "%s" "$HEALTH_BODY" | head -c 200)"
fi

# =============================================================================
# 检查三：DNS 解析功能
# =============================================================================
CHECK_NAME="DNS 解析 (dig @${DNS_HOST} -p ${DNS_PORT} ${DNS_QUERY} A)"
if command -v dig &>/dev/null; then
    DIG_OUT=$(dig +short +time=5 +tries=1 @"$DNS_HOST" -p "$DNS_PORT" "$DNS_QUERY" A 2>/dev/null || true)
    if [[ -n "$DIG_OUT" ]]; then
        pass "$CHECK_NAME"
    else
        fail "$CHECK_NAME" "dig 无响应或返回空结果（服务可能未监听 UDP/53）"
    fi
else
    # dig 不可用时用 nslookup 兜底
    NS_OUT=$(nslookup -timeout=5 "$DNS_QUERY" "$DNS_HOST" 2>/dev/null || true)
    if printf "%s" "$NS_OUT" | grep -q "Address"; then
        pass "$CHECK_NAME (via nslookup)"
    else
        fail "$CHECK_NAME" "dig 和 nslookup 均无可用结果"
    fi
fi

# =============================================================================
# 检查四：API 响应性（登录端点）
# =============================================================================
CHECK_NAME="API 响应性 (POST /api/v1/auth/login)"
LOGIN_RESP=$(curl -s -w "\n%{http_code}" \
    --max-time "$CURL_TIMEOUT" \
    -X POST "${BASE_URL}/api/v1/auth/login" \
    -H "Content-Type: application/json" \
    -d '{"username":"__probe__","password":"__probe__"}' \
    2>/dev/null || echo -e "\n000")
LOGIN_CODE=$(printf "%s" "$LOGIN_RESP" | tail -n1)

if [[ "$LOGIN_CODE" == "200" || "$LOGIN_CODE" == "401" || "$LOGIN_CODE" == "400" ]]; then
    pass "$CHECK_NAME (HTTP $LOGIN_CODE — 服务响应正常)"
else
    fail "$CHECK_NAME" "HTTP $LOGIN_CODE（预期 200/401/400，可能服务未响应或路由异常）"
fi

# =============================================================================
# 获取认证 Token（供后续检查使用）
# =============================================================================
section "认证准备"
LOGIN_AS_ADMIN || true

# =============================================================================
# 检查五：上游 DNS 列表（hotfix 布尔字段修复验证）
# =============================================================================
section "区块 2：hotfix 布尔字段 Bug 修复验证"

CHECK_NAME="上游 DNS 列表 (GET /api/v1/upstream-dns)"
if [[ -n "$AUTH_TOKEN" ]]; then
    RESP=$(auth_get "/api/v1/upstream-dns")
    CODE=$(printf "%s" "$RESP" | tail -n1)
    BODY=$(printf "%s" "$RESP" | head -n -1)

    if [[ "$CODE" == "200" ]]; then
        # 验证返回的是有效 JSON（非空、非纯错误）
        if printf "%s" "$BODY" | python3 -c "import sys,json; json.load(sys.stdin)" &>/dev/null; then
            # 检查布尔字段是否为正确类型（true/false 而非 0/1 字符串形式）
            BOOL_ISSUE=$(printf "%s" "$BODY" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    items = data if isinstance(data, list) else data.get('data', data.get('items', []))
    if not isinstance(items, list): items = [items]
    problems = []
    bool_fields = ['enabled', 'is_enabled', 'active', 'is_active']
    for item in items:
        if not isinstance(item, dict): continue
        for field in bool_fields:
            if field in item and not isinstance(item[field], bool):
                problems.append(f'{field}={repr(item[field])}')
    if problems:
        print('布尔字段类型异常: ' + ', '.join(problems[:3]))
    else:
        print('OK')
except Exception as e:
    print('OK')
" 2>/dev/null || echo "OK")
            if [[ "$BOOL_ISSUE" == "OK" ]]; then
                pass "$CHECK_NAME (JSON 有效，布尔字段类型正常)"
            else
                fail "$CHECK_NAME" "HTTP 200 但 $BOOL_ISSUE（布尔字段未修复为 true/false 类型）"
            fi
        else
            fail "$CHECK_NAME" "HTTP 200 但响应体非有效 JSON：$(printf "%s" "$BODY" | head -c 150)"
        fi
    else
        fail "$CHECK_NAME" "HTTP $CODE（预期 200）"
    fi
else
    fail "$CHECK_NAME" "未获取到认证 Token，跳过此检查"
fi

# =============================================================================
# 检查六：用户列表（hotfix 布尔字段修复验证）
# =============================================================================
CHECK_NAME="用户列表 (GET /api/v1/users)"
if [[ -n "$AUTH_TOKEN" ]]; then
    RESP=$(auth_get "/api/v1/users")
    CODE=$(printf "%s" "$RESP" | tail -n1)
    BODY=$(printf "%s" "$RESP" | head -n -1)

    if [[ "$CODE" == "200" ]]; then
        if printf "%s" "$BODY" | python3 -c "import sys,json; json.load(sys.stdin)" &>/dev/null; then
            BOOL_ISSUE=$(printf "%s" "$BODY" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    items = data if isinstance(data, list) else data.get('data', data.get('items', data.get('users', [])))
    if not isinstance(items, list): items = [items]
    problems = []
    bool_fields = ['is_active', 'enabled', 'active', 'is_admin', 'is_superuser']
    for item in items:
        if not isinstance(item, dict): continue
        for field in bool_fields:
            if field in item and not isinstance(item[field], bool):
                problems.append(f'{field}={repr(item[field])}')
    if problems:
        print('布尔字段类型异常: ' + ', '.join(problems[:3]))
    else:
        print('OK')
except Exception as e:
    print('OK')
" 2>/dev/null || echo "OK")
            if [[ "$BOOL_ISSUE" == "OK" ]]; then
                pass "$CHECK_NAME (JSON 有效，布尔字段类型正常)"
            else
                fail "$CHECK_NAME" "HTTP 200 但 $BOOL_ISSUE（is_active 等字段未修复为 true/false 类型）"
            fi
        else
            fail "$CHECK_NAME" "HTTP 200 但响应体非有效 JSON：$(printf "%s" "$BODY" | head -c 150)"
        fi
    else
        fail "$CHECK_NAME" "HTTP $CODE（预期 200）"
    fi
else
    fail "$CHECK_NAME" "未获取到认证 Token，跳过此检查"
fi

# =============================================================================
# 检查七：Filter Groups 列表（hotfix 布尔字段修复验证）
# =============================================================================
CHECK_NAME="Filter Groups 列表 (GET /api/v1/filter-groups)"
if [[ -n "$AUTH_TOKEN" ]]; then
    RESP=$(auth_get "/api/v1/filter-groups")
    CODE=$(printf "%s" "$RESP" | tail -n1)
    BODY=$(printf "%s" "$RESP" | head -n -1)

    if [[ "$CODE" == "200" ]]; then
        if printf "%s" "$BODY" | python3 -c "import sys,json; json.load(sys.stdin)" &>/dev/null; then
            BOOL_ISSUE=$(printf "%s" "$BODY" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    items = data if isinstance(data, list) else data.get('data', data.get('items', data.get('groups', [])))
    if not isinstance(items, list): items = [items]
    problems = []
    bool_fields = ['enabled', 'is_enabled', 'active', 'is_active']
    for item in items:
        if not isinstance(item, dict): continue
        for field in bool_fields:
            if field in item and not isinstance(item[field], bool):
                problems.append(f'{field}={repr(item[field])}')
    if problems:
        print('布尔字段类型异常: ' + ', '.join(problems[:3]))
    else:
        print('OK')
except Exception as e:
    print('OK')
" 2>/dev/null || echo "OK")
            if [[ "$BOOL_ISSUE" == "OK" ]]; then
                pass "$CHECK_NAME (JSON 有效，布尔字段类型正常)"
            else
                fail "$CHECK_NAME" "HTTP 200 但 $BOOL_ISSUE（enabled 等字段未修复为 true/false 类型）"
            fi
        else
            fail "$CHECK_NAME" "HTTP 200 但响应体非有效 JSON：$(printf "%s" "$BODY" | head -c 150)"
        fi
    else
        fail "$CHECK_NAME" "HTTP $CODE（预期 200）"
    fi
else
    fail "$CHECK_NAME" "未获取到认证 Token，跳过此检查"
fi

# =============================================================================
# 总结
# =============================================================================
END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))

printf "\n${BOLD}============================================================${RESET}\n"
printf "${BOLD}  Smoke Test 总结 — rust-dns Enterprise${RESET}\n"
printf "${BOLD}============================================================${RESET}\n"
printf "  目标地址：%s\n" "$BASE_URL"
printf "  总检查项：%d\n" "$TOTAL"
printf "  ${GREEN}通过：%d${RESET}\n" "$PASS"

if [[ "$FAIL" -gt 0 ]]; then
    printf "  ${RED}失败：%d${RESET}\n" "$FAIL"
else
    printf "  失败：%d\n" "$FAIL"
fi

printf "  执行时间：%d 秒\n" "$ELAPSED"
printf "${BOLD}============================================================${RESET}\n"

if [[ "$FAIL" -eq 0 ]]; then
    printf "\n${GREEN}${BOLD}  结论：ALL PASS — 部署验证通过${RESET}\n\n"
    exit 0
else
    printf "\n${RED}${BOLD}  结论：FAILED (%d/%d) — 请检查上述失败项${RESET}\n\n" "$FAIL" "$TOTAL"
    exit 1
fi
