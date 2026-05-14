# 项目约定

## 铁律

- **禁止修改外部数据源数据库**：CC-Switch、OpenCode 等外部数据库只做读取操作（SELECT），绝不执行任何 ALTER TABLE、CREATE INDEX、INSERT、UPDATE、DELETE 等修改操作。这是不可违反的约束。
