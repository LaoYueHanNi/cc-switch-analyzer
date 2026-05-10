# 版本号管理

## 版本规则

从 `package.json` 读取当前版本号（如 `0.3.8`），根据用户意图递增：

| 用户说法 | 递增规则 | 示例 |
|---------|---------|------|
| 最小版本、patch | `+0.0.1` | 0.3.8 → 0.3.9 |
| 中版本、minor | `+0.1.0` | 0.3.8 → 0.4.0 |
| 大版本、major | `+1.0.0` | 0.3.8 → 1.0.0 |

## 需要修改的文件（共 4 个）

以下 3 个文件手动修改版本号，1 个文件自动同步：

| # | 文件 | 字段 |
|---|------|------|
| 1 | `package.json` | `"version"` |
| 2 | `src-tauri/tauri.conf.json` | `"version"` |
| 3 | `src-tauri/Cargo.toml` | `version`（[package] 下） |
| 4 | `src-tauri/Cargo.lock` | **自动** — cargo check 时同步 |

## 执行步骤

1. 从 `package.json` 读取当前版本，计算新版本号
2. 编辑上述 3 个文件，替换版本号
3. 运行 `cargo check` 更新 `Cargo.lock`（只需几秒，不产生产物）
4. 提交，commit message 格式：`chore: 版本号 X.Y.Z → A.B.C`
