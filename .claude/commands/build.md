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
