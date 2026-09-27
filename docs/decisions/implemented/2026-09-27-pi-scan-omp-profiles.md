# DR: PI 数据源纳入 OMP profiles 桌面版会话目录并如实展示全部扫描根

Status: implemented

## Problem

PI 数据源接入（2026-09-22，`c825b1d`）时本机尚未安装 omp-desktop，实勘只能看到 CLI 会话目录；`~/.omp/profiles/<profile>/agent/sessions`（桌面版每 profile 一份）未纳入扫描，桌面版全部用量漏统计。同时设置页"数据目录"只展示 `primary_pi_dir()` 返回的单个目录（恒为第一个根），与实际加载目录数不符——旧注释声称 profiles 与 OMP 自身 stats.db"口径一致，暂不扫描"，但本机无 stats.db 且代码从未读取任何 OMP 数据库，排除掉的数据没有任何来源兜底。

## Decision

`pi_scanner::get_all_pi_session_roots()` 在 `~/.pi`、`~/.omp` 两个根之后追加 `omp_profile_session_roots()` 展开的 `~/.omp/profiles/<profile>/agent/sessions`（每 profile 一个根，按目录名排序保证扫描与展示稳定；`PI_CONFIG_DIR` 覆盖时同样生效）。profiles 会话 entry 与 CLI 同构，解析与去重逻辑零改动——跨目录重复（fork 复制等）仍由 `request_id = "{entry_id}:{毫秒时间戳}"` 主键去重兜底。

设置页展示同步修正：`DefaultPaths.pi` 由 `Option<String>` 改为 `Vec<String>` 列出全部实际存在的扫描根；PI 卡片多目录时每目录一行（`.source-path.multiline`，`pre-line` 换行），"已导入 N 条"为数据源合计追加在末行，单目录保持原有同行格式。`primary_pi_dir()` 随之删除。

## Alternatives considered

- **读取 OMP 自身统计库（stats.db / agent.db）** —— 上游无稳定 schema 契约，且本机根本没有 stats.db；会话 JSONL 是与 pi 官方 getSessionStats 对齐的稳定源，不引入对上游数据库格式的耦合。
- **只扫 profiles、放弃 CLI 目录** —— CLI 与 desktop 并存使用，两边都是真实用量，必须全收。
- **DefaultPaths 保持单目录、其余目录放 tooltip** —— 展示层隐藏实际扫描范围，与"从几个目录加载就展示几个"的诉求相悖，且用户无从得知 desktop 数据已入库。

## Consequences

- 代价：PI 卡片在多目录时占多行（仅此卡片，其他数据源 UI 不变）；扫描文件数随 profile 数量增长，但 mtime 游标使未变文件零成本跳过。
- 换来：omp-desktop 桌面版用量纳入统计；设置页如实反映全部加载目录；后续新增 profile 自动被发现，无需改代码。
