# Ent-DNS Design System
## UI 设计总监规范文档 (Matías Duarte 思维模型)

---

## 一、视觉哲学

Ent-DNS 是一个给 IT 工程师用的工具，不是给普通消费者的。这决定了我们的设计方向：

- **信息密度优先于留白美学** — 工程师需要在一屏内看到尽量多的有效数据
- **状态可读性优先于装饰** — 颜色必须承载语义，不能只是审美
- **深色侧边栏 + 浅色内容区** — 这个 layout pattern 被 Vercel、Linear、Grafana 验证有效：侧边栏退出视觉焦点，让内容区成为主角
- **克制的品牌色** — 蓝紫色作为 primary，只在关键交互点使用，不泛滥

---

## 二、色板

### 品牌色 (Primary) — 靛蓝紫

| Token | HSL 值 | 用途 |
|-------|--------|------|
| primary | `235 85% 60%` | 主按钮、激活导航项、链接 |
| primary-foreground | `0 0% 100%` | primary 背景上的文字 |

选色理由：235° 靛蓝偏紫，比纯蓝（221°）更有辨识度，参考 Linear 的品牌色系。在深色侧边栏上作为激活态，视觉权重恰当。

### 语义色

| 语义 | HSL 值 | 用途 |
|------|--------|------|
| success | `142 71% 45%` | DNS 服务 Running、规则启用、允许状态 |
| warning | `38 92% 50%` | 过滤列表待更新、配置警告 |
| destructive | `0 72% 51%` | 拦截状态徽章、删除操作、错误 |
| info | `199 89% 48%` | 缓存命中、信息提示 |

### 中性色 (Light Mode)

| Token | 值 | 描述 |
|-------|-----|------|
| background | `210 20% 98%` | 页面背景，非纯白，带微量蓝灰 |
| card | `0 0% 100%` | 卡片白色，与背景有层次差 |
| sidebar-bg | `225 25% 14%` | 侧边栏深色背景 |
| sidebar-border | `225 20% 20%` | 侧边栏分割线 |
| border | `214 20% 88%` | 卡片、表格边框 |
| muted | `210 16% 93%` | Skeleton、标签背景 |
| muted-foreground | `215 14% 48%` | 次要文字、图标 |

### Dark Mode 增量

Dark Mode 的侧边栏变得更深（近黑），主内容区背景为深灰，卡片比背景略亮。

---

## 三、Elevation 系统

| 层级 | 描述 | 实现 |
|------|------|------|
| 0 — Ground | 页面背景 | `bg-background` |
| 1 — Surface | 卡片、面板 | `bg-card shadow-sm` |
| 2 — Floating | Dropdown、Tooltip | `shadow-md` + `bg-popover` |
| 3 — Modal | Dialog、Sheet | `shadow-xl` |

卡片不需要重阴影——`shadow-sm` + `border` 组合在浅色模式下已经足够建立层次。

---

## 四、Typography

使用系统字体栈，无需加载额外字体（工程师产品，加载速度优先）：

```
font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "Inter", sans-serif;
```

| 层级 | 用途 | 类名 |
|------|------|------|
| Display | 页面数字大值（Dashboard 统计卡片） | `text-3xl font-bold tracking-tight` |
| Heading 1 | 页面标题（顶部栏） | `text-lg font-semibold` |
| Heading 2 | 卡片标题 | `text-sm font-semibold` |
| Body | 表格内容、表单 | `text-sm` |
| Caption | 次要说明、时间戳 | `text-xs text-muted-foreground` |

---

## 五、间距系统

基于 4px 网格，Tailwind 默认 spacing 已符合。

| 场景 | 间距 |
|------|------|
| 页面内边距 (desktop) | `p-8` (32px) |
| 页面内边距 (tablet) | `p-6` (24px) |
| 页面内边距 (mobile) | `p-4` (16px) |
| 卡片内边距 | `p-6` (24px) |
| 表格行高 | `py-3 px-4` |
| 导航项 | `px-4 py-2.5` |
| 组件间距 | `gap-4` 或 `space-y-4` |
| 卡片栅格间距 | `gap-4` |

---

## 六、布局规范

### 整体结构

```
┌─────────────────────────────────────────┐
│ sidebar (w-64, fixed)  │  main content  │
│                        │                │
│  [Logo]                │  [TopBar h-14] │
│  ─────────────────     │  ─────────────  │
│  [Nav Items]           │  [Page Content]│
│                        │                │
│  [User Footer]         │                │
└─────────────────────────────────────────┘
```

### 侧边栏规范

宽度固定 `w-64` (256px)，desktop 常驻，mobile 抽屉模式。

```
侧边栏背景色：sidebar-bg (#1B1F2E，深蓝灰)
侧边栏 border-right：1px solid sidebar-border

Logo 区域：
  高度 h-14 (56px)
  内边距 px-5
  Shield 图标用品牌色，白色文字

导航项（非激活）：
  text-slate-400
  hover: bg-white/5 text-white
  transition-colors duration-150

导航项（激活）：
  bg-primary (靛蓝紫，不透明)
  text-white
  rounded-lg

用户信息（底部）：
  border-top: 1px solid sidebar-border
  用户头像：bg-primary 圆形，显示首字母
  用户名：text-sm text-white
  角色：text-xs text-slate-500
```

Tailwind 类名方案（侧边栏容器）：
```
fixed left-0 top-0 z-50 h-screen w-64
bg-[#141824] border-r border-[#1E2433]
flex flex-col
```

导航项激活态：
```
bg-primary text-primary-foreground rounded-lg
px-4 py-2.5 text-sm font-medium
flex items-center gap-3
```

导航项非激活态：
```
text-slate-400 hover:bg-white/5 hover:text-white rounded-lg
px-4 py-2.5 text-sm font-medium
flex items-center gap-3
transition-colors duration-150
```

### 顶部栏规范

高度 `h-14` (56px)，sticky，`bg-background/95 backdrop-blur`。

```
border-bottom: 1px solid border
内边距：px-6
左侧：当前页面标题 (text-lg font-semibold)
右侧：状态指示器 + 退出按钮
```

### 主内容区

```
padding: p-6 lg:p-8
内容宽度不限，由栅格自适应
```

---

## 七、组件规范

### 统计卡片 (Stats Card)

Dashboard 顶部 4 个核心指标卡片。

```
结构：
  Card (border + shadow-sm)
    CardHeader (flex-row items-center justify-between pb-2)
      CardTitle (text-sm font-medium text-muted-foreground)  ← 指标名称
      [Icon] (h-4 w-4 text-muted-foreground)
    CardContent
      [大数字] text-3xl font-bold tracking-tight           ← 核心数值
      [副标题] text-xs text-muted-foreground mt-1          ← 变化率/说明

视觉细节：
  卡片背景白色 (bg-card)
  无特殊颜色强调，数字本身够大够粗
  Loading 态：data 位置用 animate-pulse bg-muted rounded h-8 w-24
  图标不是装饰，承载语义（Activity=查询、Shield=拦截、Database=缓存、Filter=规则）
```

### 状态徽章 (Status Badge)

用于 Query Log 的 ALLOWED/BLOCKED 状态，规则的启用/禁用。

```
ALLOWED / 运行中 / 启用：
  bg-emerald-500/10 text-emerald-600 dark:text-emerald-400
  border border-emerald-500/20
  rounded-full px-2.5 py-0.5 text-xs font-medium

BLOCKED / 停止 / 禁用：
  bg-red-500/10 text-red-600 dark:text-red-400
  border border-red-500/20
  rounded-full px-2.5 py-0.5 text-xs font-medium

CACHED / 缓存：
  bg-sky-500/10 text-sky-600 dark:text-sky-400
  border border-sky-500/20
  rounded-full px-2.5 py-0.5 text-xs font-medium

REWRITE / 重写：
  bg-violet-500/10 text-violet-600 dark:text-violet-400
  border border-violet-500/20
  rounded-full px-2.5 py-0.5 text-xs font-medium
```

使用低饱和度背景（10% 透明度）+ 同色系文字，不抢主内容的视线。

### 数据表格

```
表头：
  bg-muted/50 text-xs font-medium text-muted-foreground uppercase tracking-wider
  border-b

表格行：
  border-b border-border/50 (比边框轻一点)
  hover:bg-muted/30 (轻度 hover 反馈)
  py-3 px-4

行操作（Edit/Delete）：
  默认 opacity-0 group-hover:opacity-100
  transition-opacity duration-150
  图标按钮 h-8 w-8 rounded-md
```

### 按钮层级

```
Primary Action (每页最多 1 个)：
  bg-primary text-primary-foreground hover:bg-primary/90
  h-9 px-4 text-sm font-medium rounded-lg

Secondary / Outline：
  border border-input bg-background hover:bg-accent
  h-9 px-4 text-sm rounded-lg

Destructive（删除确认）：
  bg-destructive text-destructive-foreground hover:bg-destructive/90
  h-9 px-4 text-sm rounded-lg

Ghost（表格行内操作）：
  hover:bg-accent text-muted-foreground
  h-8 w-8 rounded-md (icon only)
```

### 表单 Input

```
Input：
  border border-input bg-background
  focus-visible:ring-2 focus-visible:ring-ring ring-offset-background
  rounded-lg h-9 px-3 text-sm

Label：
  text-sm font-medium text-foreground mb-1.5
```

---

## 八、Login 页面规范

```
背景：bg-muted/40 (浅灰，与卡片白色形成对比)
卡片：w-full max-w-sm，居中垂直水平
卡片内容：
  顶部 Shield 图标 (text-primary, h-10 w-10)，居中
  产品名 "Ent-DNS"：text-2xl font-bold，居中
  副标题：text-sm text-muted-foreground，居中

不要显示 "默认账号: admin / admin" 在生产环境
可保留在开发/demo 环境，但字体更小更淡
```

---

## 九、Dark Mode 策略

通过在 `html` 或 `body` 标签上切换 `.dark` 类实现，已在 shadcn 组件体系下工作。

Dark Mode 关键色值：
- 页面背景：`222 20% 11%` (深蓝灰，非纯黑)
- 卡片：`222 20% 13%` (比背景稍亮)
- 侧边栏：`225 25% 9%` (比卡片更深)
- 边框：`215 20% 22%`

注意：侧边栏在 Dark Mode 下不需要大改，因为它本来就是深色的。主要变化在主内容区。

---

## 十、关键工程注意事项

### Tailwind v4 + shadcn CSS 变量的正确接线方式

这是当前 Bug 的根源。Tailwind v4 的 `@theme` 语法与 v3 完全不同。

**错误方式（当前代码的问题）**：
```css
/* 错误：在 @theme 里引用了还未定义的 CSS 变量，形成循环 */
@theme {
  --color-primary-50: rgb(var(--color-primary-50) / 1); /* 自引用！*/
}
```

**正确方式**：

shadcn 组件使用的是语义 token（`bg-primary`、`bg-card`、`text-muted-foreground`），这些在 Tailwind v4 里需要通过 `@theme inline` 将 CSS 变量映射为 Tailwind 颜色工具类。

完整实现见下方 `index.css` 方案。

### HSL 变量的消费方式

shadcn 的 CSS 变量存储的是"裸 HSL 值"（不含 `hsl()`），这样可以方便地添加透明度：
```css
:root { --primary: 235 85% 60%; }
/* 使用时：hsl(var(--primary)) 或 hsl(var(--primary) / 0.1) */
```

在 Tailwind v4 里必须通过 `@theme inline` 告诉 Tailwind 这些变量是颜色。

---

## 十一、完整 index.css

见文件：`/frontend/index.css`（工程师直接替换）

---

## 十二、响应式断点策略

| 断点 | 宽度 | 布局变化 |
|------|------|----------|
| 默认 (mobile) | < 768px | 侧边栏隐藏，汉堡菜单，单列 |
| md | 768px | 双列统计卡片 |
| lg | 1024px | 侧边栏常驻，四列统计卡片，图表+状态双列 |

Mobile First 原则：先写 mobile，再用 `md:` `lg:` 覆盖。

---

文档版本：v1.0 | 2026-02-19 | ui-duarte
