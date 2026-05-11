# Build Commands

## 快速打包（开发调试）
当用户提到"快速打包"、"打包启动"等开发调试场景时使用：

```bash
taskkill //F //IM cc-switch-analyzer.exe 2>/dev/null
pnpm tauri build --no-bundle
start "" "src-tauri/target/release/cc-switch-analyzer.exe"
```

- `--no-bundle` 跳过 NSIS/MSI 安装包打包（省 10-20s）
- 仍走 release profile（全量优化）
- 自动构建前端（via beforeBuildCommand）
- exe 路径：`src-tauri/target/release/cc-switch-analyzer.exe`

## Tauri
pnpm dev:tauri          # 开发模式
pnpm build:tauri        # 生产构建

## 仅前端
pnpm dev:tauri-frontend   # Vite 开发服务器 (port 1420)
pnpm build:tauri-frontend # Vite 生产构建

## 架构
- src/ = Vue 前端
- src/platform/ = 平台适配器
- src-tauri/ = Tauri Rust 后端
