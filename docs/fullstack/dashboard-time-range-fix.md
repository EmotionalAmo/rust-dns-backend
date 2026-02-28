# Dashboard 时间范围显示修复

## 问题描述

用户在设置页选择"最近7天"或"最近30天"后，仪表盘页面仍显示"过去24小时"的文字，与实际选择的时间范围不一致。

### 影响范围

- 顶部"数据范围"标签
- "总查询数"卡片副标题
- "查询趋势"图表描述
- "TOP 10 被拦截域名"卡片描述
- "TOP 10 活跃客户端"卡片描述

## 根本原因

1. **hours 状态不是响应式的**：`useState` 只在组件挂载时从 localStorage 读取一次，之后即使 localStorage 被更新（在设置页修改），状态也不会改变。

2. **翻译字符串硬编码**：所有时间范围相关的翻译键（`last24h`、`queryTrendDesc`、`top10BlockedDesc`、`top10ClientsDesc`）都硬编码了"24小时"。

## 解决方案

### 1. 响应式 hours 状态

```typescript
// 原来的代码（只读取一次）
const [hours] = useState<number>(() => {
  const v = localStorage.getItem('dashboard-time-range');
  return v ? Number(v) : 24;
});

// 修复后的代码（响应式）
const [hours, setHours] = useState<number>(() => {
  const v = localStorage.getItem('dashboard-time-range');
  return v ? Number(v) : 24;
});

// 监听 localStorage 变化
useEffect(() => {
  const handleStorageChange = () => {
    const v = localStorage.getItem('dashboard-time-range');
    if (v) {
      const newHours = Number(v);
      setHours(newHours);
    }
  };

  // 监听跨标签页 storage 事件
  window.addEventListener('storage', handleStorageChange);

  // 定时检查同标签页 localStorage 变化
  const interval = setInterval(() => {
    const v = localStorage.getItem('dashboard-time-range');
    if (v) {
      const newHours = Number(v);
      setHours(prev => prev !== newHours ? newHours : prev);
    }
  }, 1000);

  return () => {
    window.removeEventListener('storage', handleStorageChange);
    clearInterval(interval);
  };
}, []);
```

### 2. 动态时间范围标签

新增 `getTimeRangeLabel` 和 `getShortTimeRangeLabel` 函数，支持中英文：

```typescript
function getTimeRangeLabel(hours: number, lang: string): string {
  if (lang === 'zh-CN') {
    if (hours <= 24) return '最近 1 天';
    if (hours <= 168) return '最近 7 天';
    return '最近 30 天';
  }
  // English
  if (hours <= 24) return 'the last 1 day';
  if (hours <= 168) return 'the last 7 days';
  return 'the last 30 days';
}
```

### 3. 更新翻译文件

添加支持动态时间范围变量的翻译键：

**zh-CN.json**:
```json
{
  "dashboard": {
    "top10BlockedDesc": "{{timeRange}}拦截次数最多的域名",
    "top10ClientsDesc": "{{timeRange}}查询次数最多的客户端",
    "queryTrendDynamic": "{{timeRange}} DNS 查询，每 5 秒自动刷新"
  }
}
```

**en-US.json**:
```json
{
  "dashboard": {
    "top10BlockedDesc": "Most blocked domains in {{timeRange}}",
    "top10ClientsDesc": "Most active clients in {{timeRange}}",
    "queryTrendDynamic": "DNS queries in {{timeRange}}, auto-refreshes every 5 seconds"
  }
}
```

### 4. 更新所有使用的地方

```typescript
// 获取当前语言和时间范围标签
const { t, i18n } = useTranslation();
const currentLang = i18n.language;
const timeRangeLabel = getTimeRangeLabel(hours, currentLang);

// 在各处使用动态标签
subtitle: getShortTimeRangeLabel(hours, currentLang)
<CardDescription>{t('dashboard.queryTrendDynamic', { timeRange: timeRangeLabel })}</CardDescription>
<CardDescription>{t('dashboard.top10BlockedDesc', { timeRange: timeRangeLabel })}</CardDescription>
<CardDescription>{t('dashboard.top10ClientsDesc', { timeRange: timeRangeLabel })}</CardDescription>
```

## 修改文件

- `src/pages/Dashboard.tsx`
- `src/locales/zh-CN.json`
- `src/locales/en-US.json`

## 测试验证

1. 设置选择"最近7天"，仪表盘所有显示均为"最近7天"
2. 设置选择"最近30天"，仪表盘所有显示均为"最近30天"
3. 切换语言后，时间范围标签正确显示中英文
4. API 请求参数正确传递（`hours=168` 或 `hours=720`）
5. 切换标签页后设置生效（storage 事件监听）

## Commit

```
fix(dashboard): 修复时间范围显示不一致问题

修复设置中更改时间范围后仪表盘仍显示"过去24小时"的问题：

- 将 hours 状态改为响应式，监听 localStorage 变化
- 添加 useEffect 监听 storage 事件和定时检查，实现跨标签页和同标签页同步
- 新增 getTimeRangeLabel/getShortTimeRangeLabel 函数支持中英文动态时间范围标签
- 更新翻译文件支持动态时间范围变量 ({{timeRange}})
- 修复所有硬编码的"过去24小时"显示
```

## 后续优化建议

1. 使用 `useSyncExternalStore` 或自定义 hook 封装 localStorage 监听逻辑
2. 考虑使用 Context API 或 Zustand 等状态管理库替代 localStorage
3. 抽取时间范围逻辑为独立的 hook (`useTimeRange`)
