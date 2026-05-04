# CC-Switch Analyzer 合并执行计划

> 本文档是完整的合并操作指南，按步骤执行即可将 Electron 和 Tauri 两个项目合并为一个共享前端的 monorepo。

---

## 一、项目概况

**源项目：**
- Electron: `/Users/laoyuehanni/Desktop/cc-switch-analyzer-electron/`
- Tauri: `/Users/laoyuehanni/Desktop/cc-switch-analyzer-tauri/`

**目标：** `/Users/laoyuehanni/Desktop/cc-switch-analyzer-combined/`

**核心差异：** 40 个前端文件中，28 个完全相同。12 个文件的差异全部是 IPC 调用方式不同（`window.api.xxx()` vs `invoke('xxx')`）。通过**平台适配器模式**消除差异。

---

## 二、目标目录结构

```
cc-switch-analyzer-combined/
├── package.json
├── index.html
├── vite.config.ts
├── electron.vite.config.ts
├── electron-builder.yml
├── tsconfig.json
├── tsconfig.web.json
├── tsconfig.node.json
├── .gitignore
│
├── src/                            # ===== 共享 Vue 前端 =====
│   ├── main.ts
│   ├── App.vue
│   ├── env.d.ts
│   ├── platform/                   # 新增：平台适配层
│   │   ├── types.ts
│   │   ├── electron.ts
│   │   ├── tauri.ts
│   │   └── index.ts
│   ├── router/
│   │   └── index.ts
│   ├── types/
│   │   ├── common.ts
│   │   ├── database.ts
│   │   └── pricing.ts
│   ├── utils/
│   │   ├── constants.ts
│   │   └── format.ts
│   ├── stores/
│   │   ├── database.ts
│   │   ├── filter.ts
│   │   ├── pricing.ts
│   │   ├── query.ts
│   │   └── realtime.ts
│   ├── composables/
│   │   ├── useAutoRefresh.ts
│   │   ├── useDatabase.ts           # 重写
│   │   ├── useFilter.ts             # 重写
│   │   ├── usePricing.ts
│   │   └── useRealtimePolling.ts    # 重写
│   ├── views/
│   │   ├── ByModel.vue              # 重写
│   │   ├── ByProvider.vue           # 重写
│   │   ├── SessionAnalysis.vue      # 重写
│   │   ├── PricingCalculator.vue    # 重写
│   │   └── RealtimeToken.vue
│   └── components/
│       ├── charts/
│       │   ├── DensityChart.vue
│       │   └── RealtimeAreaChart.vue
│       ├── common/
│       │   ├── CacheWindowDialog.vue  # 重写
│       │   ├── PricingGrid.vue
│       │   └── StatCard.vue
│       ├── layout/
│       │   ├── AppLayout.vue
│       │   ├── FilterBar.vue
│       │   ├── SummaryBar.vue
│       │   └── Toolbar.vue
│       ├── model/
│       │   ├── ModelCard.vue          # 用 Tauri 版（含 bug 修复）
│       │   └── ModelCompareDialog.vue
│       ├── pricing/
│       │   ├── PricingCard.vue
│       │   ├── PricingEditDialog.vue
│       │   ├── PricingEditForm.vue
│       │   └── TimePricingDialog.vue  # 用 Tauri 版（size 修复）
│       ├── provider/
│       │   └── ProviderCard.vue
│       └── session/
│           ├── ModelBreakdown.vue
│           └── SessionCard.vue
│
├── electron/                       # ===== Electron 后端 =====
│   ├── main/
│   │   ├── index.ts
│   │   ├── window.ts
│   │   ├── ipc/
│   │   │   ├── index.ts
│   │   │   ├── database.ipc.ts
│   │   │   ├── dialog.ipc.ts
│   │   │   └── pricing.ipc.ts
│   │   ├── services/
│   │   │   ├── app-db.ts
│   │   │   ├── external-db.ts
│   │   │   ├── precompute.ts
│   │   │   └── pricing-engine.ts
│   │   └── utils/
│   │       ├── constants.ts
│   │       └── format.ts
│   └── preload/
│       ├── index.ts
│       └── index.d.ts
│
├── src-tauri/                      # ===== Tauri Rust 后端 =====
│   ├── Cargo.toml
│   ├── Cargo.lock
│   ├── build.rs
│   ├── tauri.conf.json             # 需更新脚本名
│   ├── capabilities/
│   │   └── default.json
│   ├── icons/
│   │   └── (所有图标文件)
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── models.rs
│       ├── utils.rs
│       ├── commands/
│       │   ├── mod.rs
│       │   ├── database.rs
│       │   ├── query.rs
│       │   └── pricing.rs
│       └── services/
│           ├── mod.rs
│           ├── app_db.rs
│           ├── external_db.rs
│           ├── precompute.rs
│           └── pricing_engine.rs
│
├── resources/                      # Electron 打包资源
│   └── (图标文件)
│
├── docs/
│   ├── DEVELOPMENT_PLAN.md
│   └── PRD.md
│
└── .claude/
    └── commands/
        └── build.md
```

---

## 三、分步执行

### Phase 1: 创建骨架

```bash
TARGET=~/Desktop/cc-switch-analyzer-combined
ELECTRON=~/Desktop/cc-switch-analyzer-electron
TAURI=~/Desktop/cc-switch-analyzer-tauri

# 以 Tauri 项目为基底
cp -r "$TAURI"/* "$TARGET/"
cp "$TAURI/.gitignore" "$TARGET/" 2>/dev/null || true

# 删除不需要的文件
rm -f "$TARGET/DEVELOPMENT_PLAN.md"
rm -rf "$TARGET/src-tauri/gen"
rm -rf "$TARGET/dist"

# 创建需要的目录
mkdir -p "$TARGET/src/platform"
mkdir -p "$TARGET/electron/main/ipc"
mkdir -p "$TARGET/electron/main/services"
mkdir -p "$TARGET/electron/main/utils"
mkdir -p "$TARGET/electron/preload"
mkdir -p "$TARGET/resources"
mkdir -p "$TARGET/docs"
mkdir -p "$TARGET/.claude/commands"
```

### Phase 2: 复制 Electron 后端

```bash
ELECTRON=~/Desktop/cc-switch-analyzer-electron
TARGET=~/Desktop/cc-switch-analyzer-combined

# 主进程
cp "$ELECTRON/src/main/index.ts" "$TARGET/electron/main/"
cp "$ELECTRON/src/main/window.ts" "$TARGET/electron/main/"
cp -r "$ELECTRON/src/main/ipc/"* "$TARGET/electron/main/ipc/"
cp -r "$ELECTRON/src/main/services/"* "$TARGET/electron/main/services/"
cp -r "$ELECTRON/src/main/utils/"* "$TARGET/electron/main/utils/"

# 预加载
cp "$ELECTRON/src/preload/index.ts" "$TARGET/electron/preload/"
cp "$ELECTRON/src/preload/index.d.ts" "$TARGET/electron/preload/"

# 资源
cp -r "$ELECTRON/resources/"* "$TARGET/resources/" 2>/dev/null || true

# 文档
cp -r "$ELECTRON/docs/"* "$TARGET/docs/" 2>/dev/null || true
```

### Phase 3: 用 Tauri 版本覆盖前端文件（含 bug 修复）

```bash
TAURI=~/Desktop/cc-switch-analyzer-tauri
TARGET=~/Desktop/cc-switch-analyzer-combined

# 这些文件用 Tauri 版本（包含 TS 修复和 bug 修复）
cp "$TAURI/src/stores/filter.ts" "$TARGET/src/stores/"
cp "$TAURI/src/components/model/ModelCard.vue" "$TARGET/src/components/model/"
cp "$TAURI/src/components/pricing/TimePricingDialog.vue" "$TARGET/src/components/pricing/"
```

> **注意**: `src/stores/query.ts` 用 Tauri 版本（含 TS cast 修复: `as Record<string, number>`）。

### Phase 4: 创建平台适配层（4 个新文件）

这 4 个文件是新建的，不存在于任何一个源项目中。

#### 4.1 `src/platform/types.ts`

```typescript
export interface DbResult {
  path: string
  recordCount: number
  dateRange: { min: number; max: number }
  providers: { id: string; name: string }[]
  models: string[]
}

export interface RefreshResult {
  hasNew: boolean
  recordCount: number | null
}

export interface FilterParams {
  fromDate: Date | null
  toDate: Date | null
  providerId: string
  modelId: string
}

export interface PricingOverrideData {
  modelId: string
  input: number
  output: number
  cacheRead: number
  cacheCreation: number
}

export interface TimePricingRuleData {
  modelId: string
  startTime: number
  endTime: number
  input: number
  output: number
  cacheRead: number
  cacheCreation: number
  label: string
}

export interface UpdateTimePricingRuleData {
  id: number
  startTime: number
  endTime: number
  input: number
  output: number
  cacheRead: number
  cacheCreation: number
  label: string
}

export interface PlatformAdapter {
  // 数据库
  selectDatabase(): Promise<DbResult | null>
  autoLoadDatabase(): Promise<DbResult | null>
  refreshDatabase(): Promise<RefreshResult>
  // 查询
  querySummary(params: FilterParams): Promise<any>
  queryByModel(params: FilterParams): Promise<any>
  queryByProvider(params: FilterParams): Promise<any>
  queryPrecompute(params: FilterParams): Promise<any>
  queryRealtime(): Promise<any>
  queryCacheWindows(modelId: string): Promise<any[]>
  querySessionsWithCost(params: FilterParams): Promise<any[]>
  // 定价
  getExchangeRate(): Promise<number>
  setExchangeRate(rate: number): Promise<void>
  getAllPricing(): Promise<any[]>
  setPricingOverride(data: PricingOverrideData): Promise<void>
  removePricingOverride(modelId: string): Promise<void>
  addTimePricingRule(data: TimePricingRuleData): Promise<any>
  updateTimePricingRule(data: UpdateTimePricingRuleData): Promise<void>
  deleteTimePricingRule(id: number): Promise<void>
  refreshPricing(): Promise<void>
}
```

#### 4.2 `src/platform/electron.ts`

```typescript
import type { PlatformAdapter, DbResult, RefreshResult, FilterParams, PricingOverrideData, TimePricingRuleData, UpdateTimePricingRuleData } from './types'

export const platformAdapter: PlatformAdapter = {
  // 数据库
  async selectDatabase(): Promise<DbResult | null> {
    return window.api.selectDatabase()
  },
  async autoLoadDatabase(): Promise<DbResult | null> {
    return window.api.autoLoadDatabase()
  },
  async refreshDatabase(): Promise<RefreshResult> {
    return window.api.refreshDatabase()
  },
  // 查询 — Electron 直接传 Date 对象
  async querySummary(params: FilterParams) {
    return window.api.querySummary(params)
  },
  async queryByModel(params: FilterParams) {
    return window.api.queryByModel(params)
  },
  async queryByProvider(params: FilterParams) {
    return window.api.queryByProvider(params)
  },
  async queryPrecompute(params: FilterParams) {
    return window.api.queryPrecompute(params)
  },
  async queryRealtime() {
    return window.api.queryRealtime()
  },
  async queryCacheWindows(modelId: string) {
    return window.api.queryCacheWindows(modelId)
  },
  async querySessionsWithCost(params: FilterParams) {
    return window.api.querySessionsWithCost(params)
  },
  // 定价
  async getExchangeRate() {
    return window.api.getExchangeRate()
  },
  async setExchangeRate(rate: number) {
    return window.api.setExchangeRate(rate)
  },
  async getAllPricing() {
    return window.api.getAllPricing()
  },
  async setPricingOverride(data: PricingOverrideData) {
    return window.api.setPricingOverride(data)
  },
  async removePricingOverride(modelId: string) {
    return window.api.removePricingOverride(modelId)
  },
  async addTimePricingRule(data: TimePricingRuleData) {
    return window.api.addTimePricingRule(data)
  },
  async updateTimePricingRule(data: UpdateTimePricingRuleData) {
    return window.api.updateTimePricingRule(data)
  },
  async deleteTimePricingRule(id: number) {
    return window.api.deleteTimePricingRule(id)
  },
  async refreshPricing() {
    return window.api.refreshPricing()
  }
}
```

#### 4.3 `src/platform/tauri.ts`

```typescript
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import type { PlatformAdapter, DbResult, RefreshResult, FilterParams, PricingOverrideData, TimePricingRuleData, UpdateTimePricingRuleData } from './types'

function dateToStr(d: Date | null): string | undefined {
  if (!d) return undefined
  const yyyy = d.getFullYear()
  const mm = String(d.getMonth() + 1).padStart(2, '0')
  const dd = String(d.getDate()).padStart(2, '0')
  return `${yyyy}-${mm}-${dd}`
}

function toTauriParams(params: FilterParams): any {
  return {
    fromDate: dateToStr(params.fromDate),
    toDate: dateToStr(params.toDate),
    providerId: params.providerId || undefined,
    modelId: params.modelId || undefined
  }
}

export const platformAdapter: PlatformAdapter = {
  // 数据库 — Tauri 在前端开对话框，再传路径给后端
  async selectDatabase(): Promise<DbResult | null> {
    const selected = await open({
      title: '选择 CC-Switch 数据库文件',
      filters: [{ name: 'SQLite 数据库', extensions: ['db'] }],
      multiple: false
    })
    if (!selected) return null
    const filePath = typeof selected === 'string' ? selected : (selected as any).path
    return invoke<DbResult>('load_database', { filePath })
  },
  async autoLoadDatabase(): Promise<DbResult | null> {
    return invoke<DbResult | null>('auto_load_database')
  },
  async refreshDatabase(): Promise<RefreshResult> {
    return invoke<RefreshResult>('refresh_database')
  },
  // 查询 — 日期转字符串给 Rust
  async querySummary(params: FilterParams) {
    return invoke('query_summary', { params: toTauriParams(params) })
  },
  async queryByModel(params: FilterParams) {
    return invoke('query_by_model', { params: toTauriParams(params) })
  },
  async queryByProvider(params: FilterParams) {
    return invoke('query_by_provider', { params: toTauriParams(params) })
  },
  async queryPrecompute(params: FilterParams) {
    return invoke('query_precompute', { params: toTauriParams(params) })
  },
  async queryRealtime() {
    return invoke('query_realtime')
  },
  async queryCacheWindows(modelId: string) {
    return invoke('query_cache_windows', { modelId })
  },
  async querySessionsWithCost(params: FilterParams) {
    return invoke('query_sessions_with_cost', { params: toTauriParams(params) })
  },
  // 定价
  async getExchangeRate() {
    return invoke<number>('get_exchange_rate')
  },
  async setExchangeRate(rate: number) {
    return invoke('set_exchange_rate', { rate })
  },
  async getAllPricing() {
    return invoke('get_all_pricing')
  },
  async setPricingOverride(data: PricingOverrideData) {
    return invoke('set_pricing_override', {
      modelId: data.modelId, input: data.input, output: data.output,
      cacheRead: data.cacheRead, cacheCreation: data.cacheCreation
    })
  },
  async removePricingOverride(modelId: string) {
    return invoke('remove_pricing_override', { modelId })
  },
  async addTimePricingRule(data: TimePricingRuleData) {
    return invoke('add_time_pricing_rule', {
      modelId: data.modelId, startTime: data.startTime, endTime: data.endTime,
      input: data.input, output: data.output, cacheRead: data.cacheRead,
      cacheCreation: data.cacheCreation, label: data.label
    })
  },
  async updateTimePricingRule(data: UpdateTimePricingRuleData) {
    return invoke('update_time_pricing_rule', {
      id: data.id, startTime: data.startTime, endTime: data.endTime,
      input: data.input, output: data.output, cacheRead: data.cacheRead,
      cacheCreation: data.cacheCreation, label: data.label
    })
  },
  async deleteTimePricingRule(id: number) {
    return invoke('delete_time_pricing_rule', { id })
  },
  async refreshPricing() {
    return invoke('refresh_pricing')
  }
}
```

#### 4.4 `src/platform/index.ts`

```typescript
// 通过 Vite alias @platform-impl 在编译时切换实现
export { platformAdapter } from '@platform-impl'
```

---

### Phase 5: 重写前端文件使用 platformAdapter

以下 8 个文件需要修改：去掉所有 `window.api.xxx()` 和 `invoke()` 调用，替换为 `platformAdapter.xxx()`。去掉 `import { invoke } from '@tauri-apps/api/core'` 和 `import { open } from '@tauri-apps/plugin-dialog'`。去掉 `dateToStr()` 辅助函数。

**统一替换模式：**
```typescript
// 添加导入
import { platformAdapter } from '@/platform'

// 所有调用统一为
await platformAdapter.queryPrecompute(params)   // 参数传 Date 对象，适配器内部转换
await platformAdapter.selectDatabase()           // 适配器内部处理对话框
await platformAdapter.getExchangeRate()          // 无参数
```

需修改的文件：
1. `src/composables/useDatabase.ts`
2. `src/composables/useFilter.ts`
3. `src/composables/useRealtimePolling.ts`
4. `src/views/ByModel.vue`
5. `src/views/ByProvider.vue`
6. `src/views/SessionAnalysis.vue`
7. `src/views/PricingCalculator.vue`
8. `src/components/common/CacheWindowDialog.vue`

> **具体实现**：参考 Electron 版本的业务逻辑（用 Tauri 版本的代码质量改进如 Promise.all），但将所有 `window.api.xxx()` / `invoke()` 替换为 `platformAdapter.xxx()`。删掉所有 `invoke`/`open` 相关 import 和 `dateToStr` 函数。

---

### Phase 6: 创建构建配置

#### 6.1 `package.json`

```json
{
  "name": "cc-switch-analyzer",
  "version": "0.0.2",
  "description": "CC-Switch 使用分析器",
  "type": "module",
  "main": "./out/main/index.js",
  "scripts": {
    "dev:tauri-frontend": "vite",
    "build:tauri-frontend": "vue-tsc --noEmit && vite build",
    "dev:tauri": "tauri dev",
    "build:tauri": "tauri build",
    "dev:electron": "electron-vite dev",
    "build:electron": "electron-vite build",
    "preview:electron": "electron-vite preview",
    "postinstall": "electron-rebuild -f -w better-sqlite3"
  },
  "dependencies": {
    "@tauri-apps/api": "^2",
    "@tauri-apps/plugin-dialog": "^2",
    "better-sqlite3": "^11.0.0",
    "echarts": "^5.5.0",
    "naive-ui": "^2.39.0",
    "pinia": "^2.1.7",
    "vue": "^3.4.0",
    "vue-echarts": "^7.0.0",
    "vue-router": "^4.3.0"
  },
  "devDependencies": {
    "@electron-toolkit/utils": "^3.0.0",
    "@electron/rebuild": "^3.6.0",
    "@tauri-apps/cli": "^2",
    "@types/better-sqlite3": "^7.6.0",
    "@vicons/ionicons5": "^0.12.0",
    "@vitejs/plugin-vue": "^5.0.0",
    "electron": "^33.0.0",
    "electron-vite": "^2.3.0",
    "typescript": "^5.5.0",
    "vite": "^5.4.0",
    "vue-tsc": "^2.0.0"
  },
  "pnpm": {
    "onlyBuiltDependencies": ["better-sqlite3", "electron", "esbuild", "vue-demi"]
  }
}
```

#### 6.2 `vite.config.ts`（Tauri 构建用）

```typescript
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'path'

const host = process.env.TAURI_DEV_HOST

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src'),
      '@platform-impl': resolve(__dirname, 'src/platform/tauri.ts')
    }
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: { ignored: ['**/src-tauri/**'] }
  }
})
```

#### 6.3 `electron.vite.config.ts`

```typescript
import { resolve } from 'path'
import { defineConfig, externalizeDepsPlugin } from 'electron-vite'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  main: {
    root: resolve('electron/main'),
    plugins: [externalizeDepsPlugin()]
  },
  preload: {
    root: resolve('electron/preload'),
    plugins: [externalizeDepsPlugin()]
  },
  renderer: {
    root: resolve('.'),
    resolve: {
      alias: {
        '@': resolve('src'),
        '@platform-impl': resolve('src/platform/electron.ts')
      }
    },
    plugins: [vue()]
  }
})
```

#### 6.4 `electron-builder.yml`

```yaml
appId: com.cc-switch-analyzer
productName: CC-Switch Analyzer
directories:
  buildResources: resources
  output: dist-electron
files:
  - '!**/.vscode/*'
  - '!src-tauri/**'
  - '!src/**'
  - '!docs/*'
asarUnpack:
  - '**/*.node'
  - 'node_modules/better-sqlite3/**/*'
npmRebuild: true
mac:
  artifactName: ${name}-${version}-${arch}.${ext}
  target:
    - dmg
    - zip
win:
  artifactName: ${name}-${version}-setup.${ext}
  target:
    - nsis
nsis:
  oneClick: false
  allowToChangeInstallationDirectory: true
linux:
  target:
    - AppImage
    - deb
  category: Utility
```

#### 6.5 更新 `src-tauri/tauri.conf.json`

将 `beforeDevCommand` 和 `beforeBuildCommand` 改为：
```json
"beforeDevCommand": "pnpm dev:tauri-frontend",
"beforeBuildCommand": "pnpm build:tauri-frontend"
```

#### 6.6 TypeScript 配置

**`tsconfig.json`：**
```json
{
  "files": [],
  "references": [
    { "path": "./tsconfig.node.json" },
    { "path": "./tsconfig.web.json" }
  ]
}
```

**`tsconfig.web.json`：**
```json
{
  "compilerOptions": {
    "target": "ES2021",
    "module": "ESNext",
    "lib": ["ES2021", "DOM", "DOM.Iterable"],
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "preserve",
    "strict": true,
    "noUnusedLocals": false,
    "noUnusedParameters": false,
    "noFallthroughCasesInSwitch": true,
    "baseUrl": ".",
    "paths": { "@/*": ["./src/*"] }
  },
  "include": ["src/**/*.ts", "src/**/*.d.ts", "src/**/*.vue"],
  "references": [{ "path": "./tsconfig.node.json" }]
}
```

**`tsconfig.node.json`：**
```json
{
  "compilerOptions": {
    "composite": true,
    "skipLibCheck": true,
    "module": "ESNext",
    "moduleResolution": "bundler",
    "allowSyntheticDefaultImports": true
  },
  "include": ["vite.config.ts", "electron.vite.config.ts"]
}
```

#### 6.7 `index.html`

使用 Tauri 项目的 `index.html`（已在 Phase 1 复制），确保引用 `/src/main.ts`。

#### 6.8 `.claude/commands/build.md`

```markdown
# Build Commands

## Tauri (推荐，体积小)
pnpm dev:tauri          # 开发模式
pnpm build:tauri        # 生产构建 → .app + .dmg (~5MB)

## Electron
pnpm dev:electron       # 开发模式
pnpm build:electron     # 生产构建 → DMG/ZIP (~150MB)

## 仅前端
pnpm dev:tauri-frontend   # Vite 开发服务器 (port 1420)
pnpm build:tauri-frontend # Vite 生产构建

## 架构
- src/ = 共享 Vue 前端
- src/platform/ = 平台适配器（Vite alias 编译时切换）
- electron/ = Electron main + preload
- src-tauri/ = Tauri Rust 后端
```

---

### Phase 7: 安装与验证

```bash
cd ~/Desktop/cc-switch-analyzer-combined
rm -f pnpm-lock.yaml node_modules
pnpm install

# 验证 Tauri
pnpm dev:tauri

# 验证 Electron
pnpm dev:electron
```

验证清单：
- [ ] Tauri: 窗口启动 → 自动加载数据库 → 按模型/供应商/会话/实时/定价 各 Tab 数据正常
- [ ] Electron: 窗口启动 → 自动加载数据库 → 同上功能正常
- [ ] 两种构建都无 TypeScript 错误

---

## 注意事项

1. **`electron/main/window.ts` 中的 preload 路径**：`electron-vite` 输出 `out/main/` 和 `out/preload/`，相对路径 `join(__dirname, '../preload/index.js')` 无需修改。

2. **Electron preload 类型声明**：`electron/preload/index.d.ts` 声明了 `window.api`。`src/platform/electron.ts` 中直接使用 `window.api`。确保 Electron 构建时这些类型对 `src/platform/electron.ts` 可见（可能需要在 `src/platform/electron.ts` 顶部加 `/// <reference path="../../electron/preload/index.d.ts" />`）。

3. **`@tauri-apps/api` 在 Electron 构建中**：由于 Vite alias 指向 `electron.ts`，`tauri.ts` 永远不会被 import，所以不会报错。

4. **`better-sqlite3` 原生模块**：`postinstall` 会执行 `electron-rebuild`，确保原生模块匹配 Electron 的 Node 版本。
