---
name: version
description: 同步 package.json / tauri.conf.json / Cargo.toml 三个文件的版本号，跑 cargo check 更新 Cargo.lock，用 changelog 脚本生成变更要点并按项目约定提交。
whenToUse: 用户要求升级版本号、提到"版本升级"、"bump version"、patch/minor/major 任何递增意图
---

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
2. 用 **tag 区间**收集上次发版以来的所有 commit，AI 总结为变更要点（去重、合并同类项，不列举 ci/chore 类提交）
   ```bash
   git describe --tags --abbrev=0        # 取最近的 tag
   git log --oneline v0.8.7..HEAD        # 区间统计，不要用 git log -N
   ```
   **不要用 `git log -20` 之类的条数截断**——它会静默漏掉超出窗口的提交。实战教训：0.8.7 → 0.8.8 时用 `git log -20` 看到 tag 出现在第 5 行就误判"只有 1 个提交"，实际 `git log v0.8.7..HEAD` 有 4 个，changelog 漏写了 3 个功能提交。tag 可能远在 20 条之外（tag 打在上一次发版，之后连续开发多日），条数窗口和 tag 位置没有必然关系。
3. 编辑上述 3 个文件，替换版本号
4. 运行 `cargo check` 更新 `Cargo.lock`（只需几秒，不产生产物）
5. 更新 changelog：
   ```bash
   node changelog-site/generate.cjs > changelog-site/data.json
   ```
6. 提交所有文件（含 `changelog-site/data.json`），commit message 格式：`chore: 版本号 X.Y.Z → A.B.C`，**body 写入 AI 总结的变更要点**（每条一行 `- xxx`）

## 提交前自检

- `git log --oneline v<上次tag>..HEAD` 的条数与 changelog body 的要点条数是否对得上（chore/ci 类可排除，但**功能提交一个都不能少**）
- 已提交但写漏的要点，用 `git commit --amend` 修正，不要追加一个 "修正 changelog" 的提交
