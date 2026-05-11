# Tauri 开发环境（热加载）

启动带热加载的 Tauri 开发服务器。**已运行时不重复启动**，修改代码后靠热加载自动生效，不需要手动重启。

## 启动流程

### 1. 检测环境

先检查是否已有 dev 进程在运行：

```bash
tasklist 2>/dev/null | grep -i "cc-switch-analyzer"
```

- 如果 **cc-switch-analyzer.exe 已存在**，说明 dev 服务器已在运行，跳到第 3 步。
- 如果不存在，继续第 2 步。

### 2. 启动 dev 服务器

清理残留端口后启动：

```bash
# 清理残留进程（如有）
taskkill //F //IM cc-switch-analyzer.exe 2>/dev/null
# 等待端口释放
sleep 3
# 启动 dev 服务器（后台运行）
cd D:/Code/cc-switch-analyzer-combined && pnpm dev:tauri &
```

等待编译完成，轮询检测进程启动：

```bash
sleep 40 && tasklist 2>/dev/null | grep -i "cc-switch-analyzer"
```

如果进程已出现，说明启动成功，继续第 3 步。
如果未出现，再等 30 秒重试一次（Rust 增量编译可能较慢）。

### 3. 通知用户

告知用户热加载已就绪：

- **前端热加载（即时）**: 修改 `src/` 下的 Vue/TS 文件，Vite HMR 自动更新浏览器窗口，无需刷新。
- **Rust 热重编译（自动）**: 修改 `src-tauri/` 下的 Rust 文件，`tauri dev` 自动检测变更 → 增量编译 → 重启应用。前端状态会重置，但不需要手动操作。

## 注意事项

- `tauri dev` 进程会持续占用端口 1420，不要手动 kill 它
- Rust 首次编译约 1-2 分钟，后续增量编译通常 5-20 秒
- 如果 Vite 报端口占用，先 `taskkill //F //IM cc-switch-analyzer.exe` 再重启
