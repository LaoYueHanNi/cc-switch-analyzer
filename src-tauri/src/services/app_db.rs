use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;

use crate::models::*;
use crate::utils::*;

// 应用自有数据库服务（读写）
pub struct AppDbService {
    db: Connection,
}

impl AppDbService {
    pub fn new() -> Result<Self, String> {
        let db_path = get_app_db_path()?;
        let conn = Connection::open(Path::new(&db_path))
            .map_err(|e| format!("打开应用数据库失败: {}", e))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| format!("设置 WAL 模式失败: {}", e))?;
        let mut svc = Self { db: conn };
        svc.init_schema()?;
        Ok(svc)
    }

    fn init_schema(&mut self) -> Result<(), String> {
        self.db
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS settings (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL,
                    updated_at INTEGER DEFAULT (strftime('%s','now'))
                );

                CREATE TABLE IF NOT EXISTS session_titles (
                    session_id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    created_at INTEGER DEFAULT (strftime('%s','now'))
                );",
            )
            .map_err(|e| format!("初始化基础表失败: {}", e))?;

        let version = self.get_schema_version();

        if version < 1 {
            self.backup_before_migration(version)?;
            self.migrate_v1()?;
        }
        if version < 2 {
            self.backup_before_migration(1)?;
            self.migrate_v2()?;
        }
        if version < 3 {
            self.backup_before_migration(2)?;
            self.migrate_v3()?;
        }
        if version < 4 {
            self.backup_before_migration(3)?;
            self.migrate_v4()?;
        }
        if version < 5 {
            self.backup_before_migration(4)?;
            self.migrate_v5()?;
        }
        if version < 6 {
            self.backup_before_migration(5)?;
            self.migrate_v6()?;
        }
        if version < 7 {
            self.migrate_v7()?;
        }
        if version < 8 {
            self.backup_before_migration(7)?;
            self.migrate_v8()?;
        }
        if version < 9 {
            self.migrate_v9()?;
        }

        Ok(())
    }

    fn get_schema_version(&self) -> i64 {
        self.get_setting("schema_version")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }

    fn set_schema_version(&self, version: i64) -> Result<(), String> {
        self.set_setting("schema_version", &version.to_string())
    }

    fn backup_before_migration(&self, from_version: i64) -> Result<(), String> {
        let db_path = get_app_db_path()?;
        let backup_path = format!("{}.v{}.bak", db_path.display(), from_version);
        let backup = Path::new(&backup_path);
        if !backup.exists() {
            std::fs::copy(Path::new(&db_path), backup)
                .map_err(|e| format!("备份数据库失败: {}", e))?;
        }
        Ok(())
    }

    fn migrate_v1(&mut self) -> Result<(), String> {
        // pricing_overrides: 改 PK 为复合主键 (model_id, threshold)
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS pricing_overrides (
                model_id TEXT NOT NULL,
                threshold INTEGER NOT NULL DEFAULT 0,
                input_cost_per_million REAL NOT NULL,
                output_cost_per_million REAL NOT NULL,
                cache_read_cost_per_million REAL NOT NULL,
                cache_creation_cost_per_million REAL NOT NULL,
                updated_at INTEGER DEFAULT (strftime('%s','now')),
                PRIMARY KEY (model_id, threshold)
            );"
        ).map_err(|e| format!("创建 pricing_overrides 失败: {}", e))?;

        // 如果旧表结构是单列 PK（无 threshold 列），需要迁移
        let has_threshold = self.db
            .prepare("SELECT threshold FROM pricing_overrides LIMIT 0")
            .is_ok();
        if !has_threshold {
            self.db.execute_batch(
                "CREATE TABLE pricing_overrides_new (
                    model_id TEXT NOT NULL,
                    threshold INTEGER NOT NULL DEFAULT 0,
                    input_cost_per_million REAL NOT NULL,
                    output_cost_per_million REAL NOT NULL,
                    cache_read_cost_per_million REAL NOT NULL,
                    cache_creation_cost_per_million REAL NOT NULL,
                    updated_at INTEGER DEFAULT (strftime('%s','now')),
                    PRIMARY KEY (model_id, threshold)
                );
                INSERT INTO pricing_overrides_new
                    (model_id, threshold, input_cost_per_million, output_cost_per_million,
                     cache_read_cost_per_million, cache_creation_cost_per_million, updated_at)
                SELECT model_id, 0, input_cost_per_million, output_cost_per_million,
                       cache_read_cost_per_million, cache_creation_cost_per_million, updated_at
                FROM pricing_overrides;
                DROP TABLE pricing_overrides;
                ALTER TABLE pricing_overrides_new RENAME TO pricing_overrides;"
            ).map_err(|e| format!("迁移 pricing_overrides 失败: {}", e))?;
        }

        // time_pricing_overrides: 添加 threshold 列
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS time_pricing_overrides (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                model_id TEXT NOT NULL,
                start_time INTEGER NOT NULL,
                end_time INTEGER NOT NULL,
                input_cost_per_million REAL NOT NULL,
                output_cost_per_million REAL NOT NULL,
                cache_read_cost_per_million REAL NOT NULL,
                cache_creation_cost_per_million REAL NOT NULL,
                label TEXT DEFAULT '',
                threshold INTEGER NOT NULL DEFAULT 0
            );"
        ).map_err(|e| format!("创建 time_pricing_overrides 失败: {}", e))?;

        // 检查是否已有 threshold 列，没有则添加
        let has_time_threshold = self.db
            .prepare("SELECT threshold FROM time_pricing_overrides LIMIT 0")
            .is_ok();
        if !has_time_threshold {
            self.db.execute_batch(
                "ALTER TABLE time_pricing_overrides ADD COLUMN threshold INTEGER NOT NULL DEFAULT 0;"
            ).map_err(|e| format!("迁移 time_pricing_overrides 失败: {}", e))?;
        }

        self.set_schema_version(1)?;
        Ok(())
    }

    fn migrate_v2(&mut self) -> Result<(), String> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS cloud_pricing_cache (
                model_id TEXT NOT NULL,
                display_name TEXT NOT NULL,
                input_cost_per_million REAL NOT NULL,
                output_cost_per_million REAL NOT NULL,
                cache_read_cost_per_million REAL NOT NULL,
                cache_creation_cost_per_million REAL NOT NULL,
                threshold INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (model_id, threshold)
            );

            CREATE TABLE IF NOT EXISTS cloud_time_rules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                model_id TEXT NOT NULL,
                start_time INTEGER NOT NULL,
                end_time INTEGER NOT NULL,
                input_cost_per_million REAL NOT NULL,
                output_cost_per_million REAL NOT NULL,
                cache_read_cost_per_million REAL NOT NULL,
                cache_creation_cost_per_million REAL NOT NULL,
                label TEXT DEFAULT '',
                threshold INTEGER NOT NULL DEFAULT 0
            );"
        ).map_err(|e| format!("迁移 v2 (云端定价缓存) 失败: {}", e))?;

        self.set_schema_version(2)?;
        Ok(())
    }

    fn migrate_v3(&mut self) -> Result<(), String> {
        // cloud_pricing_cache 加 aliases 列
        let has_cloud_aliases = self.db
            .prepare("SELECT aliases FROM cloud_pricing_cache LIMIT 0")
            .is_ok();
        if !has_cloud_aliases {
            self.db.execute_batch(
                "ALTER TABLE cloud_pricing_cache ADD COLUMN aliases TEXT NOT NULL DEFAULT '';"
            ).map_err(|e| format!("迁移 v3 (cloud aliases) 失败: {}", e))?;
        }
        // pricing_overrides 加 user_aliases 列
        let has_user_aliases = self.db
            .prepare("SELECT user_aliases FROM pricing_overrides LIMIT 0")
            .is_ok();
        if !has_user_aliases {
            self.db.execute_batch(
                "ALTER TABLE pricing_overrides ADD COLUMN user_aliases TEXT NOT NULL DEFAULT '';"
            ).map_err(|e| format!("迁移 v3 (user aliases) 失败: {}", e))?;
        }

        self.set_schema_version(3)?;
        Ok(())
    }

    fn migrate_v4(&mut self) -> Result<(), String> {
        // 新建 model_aliases 独立表，将别名与定价覆盖解耦
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS model_aliases (
                model_id TEXT NOT NULL,
                alias TEXT NOT NULL,
                PRIMARY KEY (model_id, alias)
            );"
        ).map_err(|e| format!("迁移 v4 (model_aliases) 失败: {}", e))?;
        // 迁移已有数据：从 pricing_overrides 读取逗号分隔的 user_aliases，逐个插入新表
        let existing: Vec<(String, String)> = {
            let mut stmt = self.db
                .prepare("SELECT model_id, user_aliases FROM pricing_overrides WHERE user_aliases != ''")
                .map_err(|e| format!("迁移 v4 查询失败: {}", e))?;
            let mut rows = stmt.query([]).map_err(|e| format!("迁移 v4 查询失败: {}", e))?;
            let mut pairs = Vec::new();
            while let Some(row) = rows.next().map_err(|e| format!("迁移 v4 查询失败: {}", e))? {
                let model_id: String = row.get(0).map_err(|e| format!("迁移 v4 读取失败: {}", e))?;
                let aliases_str: String = row.get(1).map_err(|e| format!("迁移 v4 读取失败: {}", e))?;
                pairs.push((model_id, aliases_str));
            }
            pairs
        };
        let mut insert = self.db
            .prepare("INSERT OR IGNORE INTO model_aliases (model_id, alias) VALUES (?, ?)")
            .map_err(|e| format!("迁移 v4 准备插入失败: {}", e))?;
        for (model_id, aliases_str) in existing {
            for alias in aliases_str.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
                insert.execute(params![model_id, alias])
                    .map_err(|e| format!("迁移 v4 插入失败: {}", e))?;
            }
        }
        self.set_schema_version(4)?;
        Ok(())
    }

    fn migrate_v5(&mut self) -> Result<(), String> {
        // 先添加 source 列（0.5.1 的 session_titles 没有此列）
        self.db.execute_batch("ALTER TABLE session_titles ADD COLUMN source TEXT NOT NULL DEFAULT ''")
            .map_err(|e| format!("迁移 v5 添加 source 列失败: {}", e))?;
        // 清空旧标题缓存，触发按新 source 逻辑重新拉取
        self.db.execute_batch("DELETE FROM session_titles")
            .map_err(|e| format!("迁移 v5 清空标题缓存失败: {}", e))?;
        self.set_schema_version(5)?;
        Ok(())
    }

    fn migrate_v6(&mut self) -> Result<(), String> {
        // cloud_pricing_cache 加 no_cache_support 列
        let has_no_cache = self.db
            .prepare("SELECT no_cache_support FROM cloud_pricing_cache LIMIT 0")
            .is_ok();
        if !has_no_cache {
            self.db.execute_batch(
                "ALTER TABLE cloud_pricing_cache ADD COLUMN no_cache_support INTEGER NOT NULL DEFAULT 0;"
            ).map_err(|e| format!("迁移 v6 (no_cache_support) 失败: {}", e))?;
        }
        self.set_schema_version(6)?;
        Ok(())
    }

    fn migrate_v7(&mut self) -> Result<(), String> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                session_id TEXT PRIMARY KEY,
                project_dir TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '',
                source TEXT NOT NULL DEFAULT ''
            );"
        ).map_err(|e| format!("迁移 v7 (sessions 表) 失败: {}", e))?;
        self.set_schema_version(7)?;
        Ok(())
    }

    fn migrate_v8(&mut self) -> Result<(), String> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS tasks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'todo'
                    CHECK (status IN ('todo','in_progress','done','archived')),
                created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
                updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
            );

            CREATE TABLE IF NOT EXISTS task_sessions (
                task_id INTEGER NOT NULL,
                session_id TEXT NOT NULL,
                source TEXT NOT NULL DEFAULT '',
                project_dir TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '',
                added_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
                PRIMARY KEY (task_id, session_id, source),
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_task_sessions_task ON task_sessions(task_id);"
        ).map_err(|e| format!("迁移 v8 (tasks/task_sessions 表) 失败: {}", e))?;
        self.set_schema_version(8)?;
        Ok(())
    }

    fn migrate_v9(&mut self) -> Result<(), String> {
        let has_family = self.db
            .prepare("SELECT family FROM cloud_pricing_cache LIMIT 0")
            .is_ok();
        if !has_family {
            self.db.execute_batch(
                "ALTER TABLE cloud_pricing_cache ADD COLUMN family TEXT NOT NULL DEFAULT '';"
            ).map_err(|e| format!("迁移 v9 (cloud family) 失败: {}", e))?;
        }
        self.set_schema_version(9)?;
        Ok(())
    }

    // ========== 任务 CRUD ==========

    pub fn list_tasks(&self) -> Result<Vec<Task>, String> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT id, title, description, status, created_at, updated_at
                 FROM tasks ORDER BY updated_at DESC, id DESC",
            )
            .map_err(|e| format!("查询任务列表失败: {}", e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Task {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    description: row.get(2)?,
                    status: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })
            .map_err(|e| format!("查询任务列表失败: {}", e))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("读取任务失败: {}", e))?);
        }
        Ok(out)
    }

    pub fn get_task(&self, id: i64) -> Result<Option<Task>, String> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT id, title, description, status, created_at, updated_at
                 FROM tasks WHERE id = ?",
            )
            .map_err(|e| format!("查询任务失败: {}", e))?;
        let mut rows = stmt
            .query_map([id], |row| {
                Ok(Task {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    description: row.get(2)?,
                    status: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })
            .map_err(|e| format!("查询任务失败: {}", e))?;
        if let Some(r) = rows.next() {
            Ok(Some(r.map_err(|e| format!("读取任务失败: {}", e))?))
        } else {
            Ok(None)
        }
    }

    pub fn create_task(
        &self,
        title: &str,
        description: &str,
        status: &str,
    ) -> Result<i64, String> {
        if title.trim().is_empty() {
            return Err("任务标题不能为空".to_string());
        }
        if !matches!(status, "todo" | "in_progress" | "done" | "archived") {
            return Err(format!("非法任务状态: {}", status));
        }
        self.db
            .execute(
                "INSERT INTO tasks (title, description, status) VALUES (?, ?, ?)",
                params![title, description, status],
            )
            .map_err(|e| format!("创建任务失败: {}", e))?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn update_task(
        &self,
        id: i64,
        title: &str,
        description: &str,
        status: &str,
    ) -> Result<(), String> {
        if title.trim().is_empty() {
            return Err("任务标题不能为空".to_string());
        }
        if !matches!(status, "todo" | "in_progress" | "done" | "archived") {
            return Err(format!("非法任务状态: {}", status));
        }
        let affected = self
            .db
            .execute(
                "UPDATE tasks
                 SET title = ?, description = ?, status = ?,
                     updated_at = strftime('%s','now')
                 WHERE id = ?",
                params![title, description, status, id],
            )
            .map_err(|e| format!("更新任务失败: {}", e))?;
        if affected == 0 {
            return Err(format!("任务 {} 不存在", id));
        }
        Ok(())
    }

    pub fn delete_task(&self, id: i64) -> Result<(), String> {
        let affected = self
            .db
            .execute("DELETE FROM tasks WHERE id = ?", params![id])
            .map_err(|e| format!("删除任务失败: {}", e))?;
        if affected == 0 {
            return Err(format!("任务 {} 不存在", id));
        }
        // ON DELETE CASCADE 会自动清掉 task_sessions
        Ok(())
    }

    pub fn list_task_sessions(&self, task_id: i64) -> Result<Vec<TaskSession>, String> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT task_id, session_id, source, project_dir, title, added_at
                 FROM task_sessions
                 WHERE task_id = ?
                 ORDER BY added_at DESC, session_id ASC",
            )
            .map_err(|e| format!("查询任务会话失败: {}", e))?;
        let rows = stmt
            .query_map([task_id], |row| {
                Ok(TaskSession {
                    task_id: row.get(0)?,
                    session_id: row.get(1)?,
                    source: row.get(2)?,
                    project_dir: row.get(3)?,
                    title: row.get(4)?,
                    added_at: row.get(5)?,
                })
            })
            .map_err(|e| format!("查询任务会话失败: {}", e))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("读取任务会话失败: {}", e))?);
        }
        Ok(out)
    }

    pub fn add_task_sessions(
        &self,
        task_id: i64,
        sessions: &[TaskSessionInput],
    ) -> Result<(), String> {
        if sessions.is_empty() {
            return Ok(());
        }
        // 校验 task 存在
        self.get_task(task_id)?
            .ok_or_else(|| format!("任务 {} 不存在", task_id))?;
        let tx = self
            .db
            .unchecked_transaction()
            .map_err(|e| format!("开启事务失败: {}", e))?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR REPLACE INTO task_sessions
                        (task_id, session_id, source, project_dir, title, added_at)
                     VALUES (?, ?, ?, ?, ?, strftime('%s','now'))",
                )
                .map_err(|e| format!("准备插入任务会话失败: {}", e))?;
            for s in sessions {
                stmt.execute(params![
                    task_id,
                    s.session_id,
                    s.source,
                    s.project_dir,
                    s.title
                ])
                .map_err(|e| format!("插入任务会话失败: {}", e))?;
            }
        }
        // 顺带刷新任务的 updated_at
        tx.execute(
            "UPDATE tasks SET updated_at = strftime('%s','now') WHERE id = ?",
            params![task_id],
        )
        .map_err(|e| format!("刷新任务时间失败: {}", e))?;
        tx.commit()
            .map_err(|e| format!("提交任务会话失败: {}", e))?;
        Ok(())
    }

    // ========== 设置管理 ==========

    pub fn get_setting(&self, key: &str) -> Option<String> {
        self.db
            .query_row(
                "SELECT value FROM settings WHERE key = ?",
                params![key],
                |row| row.get(0),
            )
            .ok()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        self.db
            .execute(
                "INSERT OR REPLACE INTO settings (key, value, updated_at) VALUES (?, ?, strftime('%s','now'))",
                params![key, value],
            )
            .map_err(|e| format!("保存设置失败: {}", e))?;
        Ok(())
    }

    // ========== 定价覆盖 CRUD ==========

    pub fn get_all_overrides(&self) -> Result<Vec<PricingOverride>, String> {
        let mut stmt = self
            .db
            .prepare("SELECT * FROM pricing_overrides ORDER BY model_id, threshold")
            .map_err(|e| format!("查询定价覆盖失败: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>("model_id")?,
                    row.get::<_, i64>("threshold")?,
                    row.get::<_, f64>("input_cost_per_million")?,
                    row.get::<_, f64>("output_cost_per_million")?,
                    row.get::<_, f64>("cache_read_cost_per_million")?,
                    row.get::<_, f64>("cache_creation_cost_per_million")?,
                    row.get::<_, i64>("updated_at")?,
                ))
            })
            .map_err(|e| format!("查询定价覆盖失败: {}", e))?;

        let mut map: HashMap<String, PricingOverride> = HashMap::new();
        for row in rows {
            let (model_id, threshold, inp, out, cr, cc, updated_at) =
                row.map_err(|e| format!("读取定价覆盖失败: {}", e))?;
            let entry = map.entry(model_id.clone()).or_insert_with(|| PricingOverride {
                model_id: model_id.clone(),
                input_cost_per_million: inp,
                output_cost_per_million: out,
                cache_read_cost_per_million: cr,
                cache_creation_cost_per_million: cc,
                updated_at,
                context_tiers: Vec::new(),
            });
            if threshold > 0 {
                entry.context_tiers.push(ContextTier {
                    id: None,
                    threshold,
                    input_cost_per_million: inp,
                    output_cost_per_million: out,
                    cache_read_cost_per_million: cr,
                    cache_creation_cost_per_million: cc,
                });
            }
        }

        let mut result: Vec<PricingOverride> = map.into_values().collect();
        result.sort_by(|a, b| a.model_id.cmp(&b.model_id));
        Ok(result)
    }

    pub fn save_override(
        &self,
        model_id: &str,
        input_cost: f64,
        output_cost: f64,
        cache_read_cost: f64,
        cache_creation_cost: f64,
    ) -> Result<(), String> {
        self.db
            .execute(
                "INSERT OR REPLACE INTO pricing_overrides
                    (model_id, threshold, input_cost_per_million, output_cost_per_million,
                     cache_read_cost_per_million, cache_creation_cost_per_million, updated_at)
                 VALUES (?, 0, ?, ?, ?, ?, strftime('%s','now'))",
                params![model_id, input_cost, output_cost, cache_read_cost, cache_creation_cost],
            )
            .map_err(|e| format!("保存定价覆盖失败: {}", e))?;
        Ok(())
    }

    pub fn save_override_tier(
        &self,
        model_id: &str,
        threshold: i64,
        input_cost: f64,
        output_cost: f64,
        cache_read_cost: f64,
        cache_creation_cost: f64,
    ) -> Result<(), String> {
        self.db
            .execute(
                "INSERT OR REPLACE INTO pricing_overrides
                    (model_id, threshold, input_cost_per_million, output_cost_per_million,
                     cache_read_cost_per_million, cache_creation_cost_per_million, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, strftime('%s','now'))",
                params![model_id, threshold, input_cost, output_cost, cache_read_cost, cache_creation_cost],
            )
            .map_err(|e| format!("保存上下文档位失败: {}", e))?;
        Ok(())
    }

    pub fn delete_override_tier(
        &self,
        model_id: &str,
        threshold: i64,
    ) -> Result<(), String> {
        self.db
            .execute(
                "DELETE FROM pricing_overrides WHERE model_id = ? AND threshold = ?",
                params![model_id, threshold],
            )
            .map_err(|e| format!("删除上下文档位失败: {}", e))?;
        Ok(())
    }

    pub fn delete_override(&self, model_id: &str) -> Result<(), String> {
        self.db
            .execute(
                "DELETE FROM pricing_overrides WHERE model_id = ?",
                params![model_id],
            )
            .map_err(|e| format!("删除定价覆盖失败: {}", e))?;
        Ok(())
    }

    // ========== 时间定价 CRUD ==========

    pub fn get_all_time_overrides(&self) -> Result<Vec<TimePricingRule>, String> {
        let mut stmt = self
            .db
            .prepare("SELECT * FROM time_pricing_overrides ORDER BY model_id, start_time, threshold")
            .map_err(|e| format!("查询时间定价失败: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>("id")?,
                    row.get::<_, String>("model_id")?,
                    row.get::<_, i64>("start_time")?,
                    row.get::<_, i64>("end_time")?,
                    row.get::<_, f64>("input_cost_per_million")?,
                    row.get::<_, f64>("output_cost_per_million")?,
                    row.get::<_, f64>("cache_read_cost_per_million")?,
                    row.get::<_, f64>("cache_creation_cost_per_million")?,
                    row.get::<_, Option<String>>("label")?.unwrap_or_default(),
                    row.get::<_, i64>("threshold")?,
                ))
            })
            .map_err(|e| format!("查询时间定价失败: {}", e))?;

        // 按 (model_id, start_time, end_time) 分组
        let mut map: HashMap<(String, i64, i64), TimePricingRule> = HashMap::new();
        for row in rows {
            let (id, model_id, start_time, end_time, inp, out, cr, cc, label, threshold) =
                row.map_err(|e| format!("读取时间定价失败: {}", e))?;
            let key = (model_id.clone(), start_time, end_time);
            let entry = map.entry(key.clone()).or_insert_with(|| TimePricingRule {
                id,
                model_id,
                start_time,
                end_time,
                input_cost_per_million: inp,
                output_cost_per_million: out,
                cache_read_cost_per_million: cr,
                cache_creation_cost_per_million: cc,
                label,
                context_tiers: Vec::new(),
            });
            if threshold > 0 {
                entry.context_tiers.push(ContextTier {
                    id: Some(id),
                    threshold,
                    input_cost_per_million: inp,
                    output_cost_per_million: out,
                    cache_read_cost_per_million: cr,
                    cache_creation_cost_per_million: cc,
                });
            }
        }

        let mut result: Vec<TimePricingRule> = map.into_values().collect();
        result.sort_by(|a, b| (&a.model_id, a.start_time, a.end_time).cmp(&(&b.model_id, b.start_time, b.end_time)));
        Ok(result)
    }

    pub fn add_time_override(
        &self,
        model_id: &str,
        start_time: i64,
        end_time: i64,
        input_cost: f64,
        output_cost: f64,
        cache_read_cost: f64,
        cache_creation_cost: f64,
        label: &str,
    ) -> Result<i64, String> {
        self.db
            .execute(
                "INSERT INTO time_pricing_overrides
                    (model_id, start_time, end_time, input_cost_per_million, output_cost_per_million,
                     cache_read_cost_per_million, cache_creation_cost_per_million, label, threshold)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0)",
                params![
                    model_id,
                    start_time,
                    end_time,
                    input_cost,
                    output_cost,
                    cache_read_cost,
                    cache_creation_cost,
                    label
                ],
            )
            .map_err(|e| format!("添加时间定价失败: {}", e))?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn add_time_override_tier(
        &self,
        model_id: &str,
        start_time: i64,
        end_time: i64,
        threshold: i64,
        input_cost: f64,
        output_cost: f64,
        cache_read_cost: f64,
        cache_creation_cost: f64,
    ) -> Result<i64, String> {
        self.db
            .execute(
                "INSERT INTO time_pricing_overrides
                    (model_id, start_time, end_time, input_cost_per_million, output_cost_per_million,
                     cache_read_cost_per_million, cache_creation_cost_per_million, label, threshold)
                 SELECT ?, ?, ?, ?, ?, ?, ?, label, ?
                 FROM time_pricing_overrides
                 WHERE model_id = ? AND start_time = ? AND end_time = ? AND threshold = 0
                 LIMIT 1",
                params![
                    model_id,
                    start_time,
                    end_time,
                    input_cost,
                    output_cost,
                    cache_read_cost,
                    cache_creation_cost,
                    threshold,
                    model_id,
                    start_time,
                    end_time,
                ],
            )
            .map_err(|e| format!("添加时间定价上下文档位失败: {}", e))?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn update_time_override(
        &self,
        id: i64,
        start_time: i64,
        end_time: i64,
        input_cost: f64,
        output_cost: f64,
        cache_read_cost: f64,
        cache_creation_cost: f64,
        label: &str,
    ) -> Result<(), String> {
        self.db
            .execute(
                "UPDATE time_pricing_overrides SET
                    start_time = ?, end_time = ?,
                    input_cost_per_million = ?, output_cost_per_million = ?,
                    cache_read_cost_per_million = ?, cache_creation_cost_per_million = ?,
                    label = ?
                 WHERE id = ?",
                params![
                    start_time,
                    end_time,
                    input_cost,
                    output_cost,
                    cache_read_cost,
                    cache_creation_cost,
                    label,
                    id
                ],
            )
            .map_err(|e| format!("更新时间定价失败: {}", e))?;
        Ok(())
    }

    #[allow(dead_code)] // 按 model_id+时间范围批量更新，预留 API
    pub fn update_time_override_range(
        &self,
        model_id: &str,
        old_start: i64,
        old_end: i64,
        new_start: i64,
        new_end: i64,
        label: &str,
    ) -> Result<(), String> {
        self.db
            .execute(
                "UPDATE time_pricing_overrides SET
                    start_time = ?, end_time = ?, label = ?
                 WHERE model_id = ? AND start_time = ? AND end_time = ?",
                params![
                    new_start,
                    new_end,
                    label,
                    model_id,
                    old_start,
                    old_end,
                ],
            )
            .map_err(|e| format!("更新时间范围失败: {}", e))?;
        Ok(())
    }

    pub fn update_time_override_tier(
        &self,
        id: i64,
        input_cost: f64,
        output_cost: f64,
        cache_read_cost: f64,
        cache_creation_cost: f64,
    ) -> Result<(), String> {
        self.db
            .execute(
                "UPDATE time_pricing_overrides SET
                    input_cost_per_million = ?, output_cost_per_million = ?,
                    cache_read_cost_per_million = ?, cache_creation_cost_per_million = ?
                 WHERE id = ?",
                params![input_cost, output_cost, cache_read_cost, cache_creation_cost, id],
            )
            .map_err(|e| format!("更新时间定价档位失败: {}", e))?;
        Ok(())
    }

    pub fn delete_time_override(&self, id: i64) -> Result<(), String> {
        self.db
            .execute(
                "DELETE FROM time_pricing_overrides WHERE id = ?",
                params![id],
            )
            .map_err(|e| format!("删除时间定价失败: {}", e))?;
        Ok(())
    }

    #[allow(dead_code)] // 按 model_id+时间范围批量删除，预留 API（测试中使用）
    pub fn delete_time_override_group(
        &self,
        model_id: &str,
        start_time: i64,
        end_time: i64,
    ) -> Result<(), String> {
        self.db
            .execute(
                "DELETE FROM time_pricing_overrides WHERE model_id = ? AND start_time = ? AND end_time = ?",
                params![model_id, start_time, end_time],
            )
            .map_err(|e| format!("删除时间定价组失败: {}", e))?;
        Ok(())
    }

    // ========== 会话标题 ==========

    pub fn get_session_titles(&self, session_ids: &[String]) -> Result<HashMap<String, (String, String)>, String> {
        if session_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders: Vec<String> = session_ids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            "SELECT session_id, title, COALESCE(source, '') FROM session_titles WHERE session_id IN ({})",
            placeholders.join(",")
        );
        let refs: Vec<&dyn rusqlite::types::ToSql> = session_ids
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();
        let mut stmt = self.db.prepare(&sql).map_err(|e| format!("查询会话标题失败: {}", e))?;
        let rows = stmt.query_map(refs.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        }).map_err(|e| format!("查询会话标题失败: {}", e))?;
        let mut result = HashMap::new();
        for row in rows {
            let (sid, title, source) = row.map_err(|e| format!("读取会话标题失败: {}", e))?;
            result.insert(sid, (title, source));
        }
        Ok(result)
    }

    pub fn save_session_title(&self, session_id: &str, title: &str, source: &str) -> Result<(), String> {
        self.db.execute(
            "INSERT OR REPLACE INTO session_titles (session_id, title, source, created_at) VALUES (?, ?, ?, strftime('%s','now'))",
            params![session_id, title, source],
        ).map_err(|e| format!("保存会话标题失败: {}", e))?;
        Ok(())
    }

    // ========== sessions 表 ==========

    pub fn get_sessions(&self, session_ids: &[String]) -> Result<HashMap<String, (String, String, String)>, String> {
        if session_ids.is_empty() { return Ok(HashMap::new()); }
        let placeholders: Vec<String> = session_ids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            "SELECT session_id, project_dir, title, source FROM sessions WHERE session_id IN ({})",
            placeholders.join(",")
        );
        let refs: Vec<&dyn rusqlite::types::ToSql> = session_ids
            .iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
        let mut stmt = self.db.prepare(&sql).map_err(|e| format!("查询 sessions 失败: {}", e))?;
        let rows = stmt.query_map(refs.as_slice(), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        }).map_err(|e| format!("查询 sessions 失败: {}", e))?;
        let mut result = HashMap::new();
        for row in rows {
            let (sid, project_dir, title, source) = row.map_err(|e| format!("读取 sessions 行失败: {}", e))?;
            result.insert(sid, (project_dir, title, source));
        }
        Ok(result)
    }

    pub fn save_session(&self, session_id: &str, project_dir: &str, title: &str, source: &str) -> Result<(), String> {
        self.db.execute(
            "INSERT OR REPLACE INTO sessions (session_id, project_dir, title, source) VALUES (?, ?, ?, ?)",
            params![session_id, project_dir, title, source],
        ).map_err(|e| format!("保存 session 失败: {}", e))?;
        Ok(())
    }

    // ========== 云端定价缓存 ==========

    /// 将云端定价数据写入缓存（事务替换）
    pub fn save_cloud_pricing(&self, data: &crate::models::CloudPricingData) -> Result<(), String> {
        let tx = self.db.unchecked_transaction().map_err(|e| format!("开启事务失败: {}", e))?;

        tx.execute_batch("DELETE FROM cloud_pricing_cache")
            .map_err(|e| format!("清空云端定价缓存失败: {}", e))?;
        tx.execute_batch("DELETE FROM cloud_time_rules")
            .map_err(|e| format!("清空云端时间规则缓存失败: {}", e))?;

        for model in &data.models {
            let aliases_str = model.aliases.join(",");
            tx.execute(
                "INSERT INTO cloud_pricing_cache
                    (model_id, display_name, input_cost_per_million, output_cost_per_million,
                     cache_read_cost_per_million, cache_creation_cost_per_million, threshold, aliases, no_cache_support, family)
                 VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?, ?)",
                params![
                    model.model_id,
                    model.model_id,
                    model.input_cost_per_million,
                    model.output_cost_per_million,
                    model.cache_read_cost_per_million,
                    model.cache_creation_cost_per_million,
                    aliases_str,
                    model.no_cache_support,
                    model.family,
                ],
            ).map_err(|e| format!("写入云端定价缓存失败: {}", e))?;

            for tier in &model.context_tiers {
                tx.execute(
                    "INSERT INTO cloud_pricing_cache
                        (model_id, display_name, input_cost_per_million, output_cost_per_million,
                         cache_read_cost_per_million, cache_creation_cost_per_million, threshold)
                     VALUES (?, ?, ?, ?, ?, ?, ?)",
                    params![
                        model.model_id,
                        model.model_id,
                        tier.input_cost_per_million,
                        tier.output_cost_per_million,
                        tier.cache_read_cost_per_million,
                        tier.cache_creation_cost_per_million,
                        tier.threshold,
                    ],
                ).map_err(|e| format!("写入云端定价档位缓存失败: {}", e))?;
            }

            for rule in &model.time_rules {
                tx.execute(
                    "INSERT INTO cloud_time_rules
                        (model_id, start_time, end_time,
                         input_cost_per_million, output_cost_per_million,
                         cache_read_cost_per_million, cache_creation_cost_per_million,
                         label, threshold)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0)",
                    params![
                        model.model_id,
                        rule.start_time,
                        rule.end_time,
                        rule.input_cost_per_million,
                        rule.output_cost_per_million,
                        rule.cache_read_cost_per_million,
                        rule.cache_creation_cost_per_million,
                        rule.label,
                    ],
                ).map_err(|e| format!("写入云端时间规则缓存失败: {}", e))?;

                for tier in &rule.context_tiers {
                    tx.execute(
                        "INSERT INTO cloud_time_rules
                            (model_id, start_time, end_time,
                             input_cost_per_million, output_cost_per_million,
                             cache_read_cost_per_million, cache_creation_cost_per_million,
                             label, threshold)
                         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                        params![
                            model.model_id,
                            rule.start_time,
                            rule.end_time,
                            tier.input_cost_per_million,
                            tier.output_cost_per_million,
                            tier.cache_read_cost_per_million,
                            tier.cache_creation_cost_per_million,
                            rule.label,
                            tier.threshold,
                        ],
                    ).map_err(|e| format!("写入云端时间规则档位缓存失败: {}", e))?;
                }
            }
        }

        tx.execute(
            "INSERT OR REPLACE INTO settings (key, value, updated_at) VALUES ('cloud_pricing_version', ?, strftime('%s','now'))",
            params![data.version.to_string()],
        ).map_err(|e| format!("写入云端定价版本失败: {}", e))?;

        tx.execute(
            "INSERT OR REPLACE INTO settings (key, value, updated_at) VALUES ('cloud_pricing_updated_at', ?, strftime('%s','now'))",
            params![data.updated_at.to_string()],
        ).map_err(|e| format!("写入云端定价更新时间失败: {}", e))?;

        let families_json = serde_json::to_string(&data.families)
            .map_err(|e| format!("序列化 families 失败: {}", e))?;
        tx.execute(
            "INSERT OR REPLACE INTO settings (key, value, updated_at) VALUES ('cloud_pricing_families', ?, strftime('%s','now'))",
            params![families_json],
        ).map_err(|e| format!("写入云端定价 families 失败: {}", e))?;

        tx.commit().map_err(|e| format!("提交云端定价缓存失败: {}", e))?;
        Ok(())
    }

    /// 从缓存读取云端定价（返回基础行 + 按模型分组的上下文档位 + 按 model_id 分组的云端时间规则 + 云端别名 + 模型家族）
    pub fn load_cloud_pricing(&self) -> Result<(Vec<ModelPricing>, HashMap<String, Vec<ContextTier>>, HashMap<String, Vec<crate::models::CloudPricingTimeRule>>, HashMap<String, Vec<String>>, Vec<String>, HashMap<String, String>), String> {
        // 读取基础行 (threshold = 0)
        let mut stmt = self.db
            .prepare(
                "SELECT model_id,
                        input_cost_per_million, output_cost_per_million,
                        cache_read_cost_per_million, cache_creation_cost_per_million,
                        aliases, no_cache_support, family
                 FROM cloud_pricing_cache WHERE threshold = 0 ORDER BY model_id"
            )
            .map_err(|e| format!("查询云端定价缓存失败: {}", e))?;

        let base_rows = stmt.query_map([], |row| {
            Ok((
                ModelPricing {
                    model_id: row.get("model_id")?,
                    input_cost_per_million: row.get("input_cost_per_million")?,
                    output_cost_per_million: row.get("output_cost_per_million")?,
                    cache_read_cost_per_million: row.get("cache_read_cost_per_million")?,
                    cache_creation_cost_per_million: row.get("cache_creation_cost_per_million")?,
                },
                row.get::<_, String>("aliases")?,
                row.get::<_, bool>("no_cache_support")?,
                row.get::<_, String>("family")?,
            ))
        }).map_err(|e| format!("查询云端定价缓存失败: {}", e))?;

        let mut base = Vec::new();
        let mut cloud_aliases: HashMap<String, Vec<String>> = HashMap::new();
        let mut no_cache_models: Vec<String> = Vec::new();
        let mut model_families: HashMap<String, String> = HashMap::new();
        for row in base_rows {
            let (pricing, aliases_str, no_cache, family) = row.map_err(|e| format!("读取云端定价缓存失败: {}", e))?;
            let aliases: Vec<String> = if aliases_str.is_empty() {
                Vec::new()
            } else {
                aliases_str.split(',').map(|s| s.to_string()).collect()
            };
            cloud_aliases.insert(pricing.model_id.clone(), aliases);
            if no_cache { no_cache_models.push(pricing.model_id.clone()); }
            if !family.is_empty() {
                model_families.insert(pricing.model_id.clone(), family);
            }
            base.push(pricing);
        }

        // 读取档位行 (threshold > 0)
        let mut tier_stmt = self.db
            .prepare(
                "SELECT model_id, threshold,
                        input_cost_per_million, output_cost_per_million,
                        cache_read_cost_per_million, cache_creation_cost_per_million
                 FROM cloud_pricing_cache WHERE threshold > 0 ORDER BY model_id, threshold"
            )
            .map_err(|e| format!("查询云端定价档位缓存失败: {}", e))?;

        let tier_rows = tier_stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>("model_id")?,
                ContextTier {
                    id: None,
                    threshold: row.get("threshold")?,
                    input_cost_per_million: row.get("input_cost_per_million")?,
                    output_cost_per_million: row.get("output_cost_per_million")?,
                    cache_read_cost_per_million: row.get("cache_read_cost_per_million")?,
                    cache_creation_cost_per_million: row.get("cache_creation_cost_per_million")?,
                },
            ))
        }).map_err(|e| format!("查询云端定价档位缓存失败: {}", e))?;

        let mut tiers: HashMap<String, Vec<ContextTier>> = HashMap::new();
        for row in tier_rows {
            let (model_id, tier) = row.map_err(|e| format!("读取云端定价档位缓存失败: {}", e))?;
            tiers.entry(model_id).or_default().push(tier);
        }

        // 读取云端时间规则
        let cloud_time_rules = self.load_cloud_time_rules()?;

        Ok((base, tiers, cloud_time_rules, cloud_aliases, no_cache_models, model_families))
    }

    /// 从 settings 读取云端 families 有序列表
    pub fn load_cloud_families(&self) -> Result<Vec<crate::models::PricingFamily>, String> {
        match self.get_setting("cloud_pricing_families") {
            Some(json) if !json.is_empty() => {
                serde_json::from_str(&json).map_err(|e| format!("解析 cloud_pricing_families 失败: {}", e))
            }
            _ => Ok(Vec::new()),
        }
    }

    /// 从缓存读取云端时间规则（按 model_id 分组）
    fn load_cloud_time_rules(&self) -> Result<HashMap<String, Vec<crate::models::CloudPricingTimeRule>>, String> {
        let mut stmt = self.db
            .prepare(
                "SELECT model_id, start_time, end_time,
                        input_cost_per_million, output_cost_per_million,
                        cache_read_cost_per_million, cache_creation_cost_per_million,
                        label, threshold
                 FROM cloud_time_rules ORDER BY model_id, start_time, threshold"
            )
            .map_err(|e| format!("查询云端时间规则缓存失败: {}", e))?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>("model_id")?,
                row.get::<_, i64>("start_time")?,
                row.get::<_, i64>("end_time")?,
                row.get::<_, f64>("input_cost_per_million")?,
                row.get::<_, f64>("output_cost_per_million")?,
                row.get::<_, f64>("cache_read_cost_per_million")?,
                row.get::<_, f64>("cache_creation_cost_per_million")?,
                row.get::<_, Option<String>>("label")?.unwrap_or_default(),
                row.get::<_, i64>("threshold")?,
            ))
        }).map_err(|e| format!("查询云端时间规则缓存失败: {}", e))?;

        // 先按 (model_id, start_time, end_time) 分组合并上下文档位
        let mut group_map: HashMap<(String, i64, i64), crate::models::CloudPricingTimeRule> = HashMap::new();
        for row in rows {
            let (model_id, start_time, end_time, inp, out, cr, cc, label, threshold) =
                row.map_err(|e| format!("读取云端时间规则缓存失败: {}", e))?;
            let key = (model_id.clone(), start_time, end_time);
            let entry = group_map.entry(key).or_insert_with(|| crate::models::CloudPricingTimeRule {
                model_id: model_id.clone(),
                label,
                start_time,
                end_time,
                input_cost_per_million: inp,
                output_cost_per_million: out,
                cache_read_cost_per_million: cr,
                cache_creation_cost_per_million: cc,
                context_tiers: Vec::new(),
            });
            if threshold > 0 {
                entry.context_tiers.push(ContextTier {
                    id: None,
                    threshold,
                    input_cost_per_million: inp,
                    output_cost_per_million: out,
                    cache_read_cost_per_million: cr,
                    cache_creation_cost_per_million: cc,
                });
            }
        }

        // 再按 model_id 分组
        let mut result: HashMap<String, Vec<crate::models::CloudPricingTimeRule>> = HashMap::new();
        for rule in group_map.into_values() {
            let model_id = rule.model_id.clone();
            result.entry(model_id).or_default().push(rule);
        }
        // 每个模型内按 start_time 排序
        for rules in result.values_mut() {
            rules.sort_by_key(|r| r.start_time);
        }
        Ok(result)
    }

    // ========== 用户自定义别名 ==========

    pub fn get_user_aliases(&self) -> Result<HashMap<String, Vec<String>>, String> {
        let mut stmt = self.db
            .prepare("SELECT model_id, alias FROM model_aliases ORDER BY model_id, alias")
            .map_err(|e| format!("查询用户别名失败: {}", e))?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
            ))
        }).map_err(|e| format!("查询用户别名失败: {}", e))?;

        let mut result: HashMap<String, Vec<String>> = HashMap::new();
        for row in rows {
            let (model_id, alias) = row.map_err(|e| format!("读取用户别名失败: {}", e))?;
            result.entry(model_id).or_default().push(alias);
        }
        Ok(result)
    }

    pub fn add_user_alias(&self, model_id: &str, alias: &str) -> Result<(), String> {
        self.db.execute(
            "INSERT OR IGNORE INTO model_aliases (model_id, alias) VALUES (?, ?)",
            params![model_id, alias],
        ).map_err(|e| format!("添加用户别名失败: {}", e))?;
        Ok(())
    }

    pub fn remove_user_alias(&self, model_id: &str, alias: &str) -> Result<(), String> {
        self.db.execute(
            "DELETE FROM model_aliases WHERE model_id = ? AND alias = ?",
            params![model_id, alias],
        ).map_err(|e| format!("删除用户别名失败: {}", e))?;
        Ok(())
    }
}

#[cfg(test)]
impl AppDbService {
    pub fn new_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory()
            .map_err(|e| format!("打开内存数据库失败: {}", e))?;
        let mut svc = Self { db: conn };
        svc.init_schema_in_memory()?;
        Ok(svc)
    }

    fn init_schema_in_memory(&mut self) -> Result<(), String> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at INTEGER DEFAULT (strftime('%s','now'))
            );
            CREATE TABLE IF NOT EXISTS session_titles (
                session_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                source TEXT NOT NULL DEFAULT '',
                created_at INTEGER DEFAULT (strftime('%s','now'))
            );
            CREATE TABLE IF NOT EXISTS pricing_overrides (
                model_id TEXT NOT NULL,
                threshold INTEGER NOT NULL DEFAULT 0,
                input_cost_per_million REAL NOT NULL,
                output_cost_per_million REAL NOT NULL,
                cache_read_cost_per_million REAL NOT NULL,
                cache_creation_cost_per_million REAL NOT NULL,
                updated_at INTEGER DEFAULT (strftime('%s','now')),
                user_aliases TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (model_id, threshold)
            );
            CREATE TABLE IF NOT EXISTS time_pricing_overrides (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                model_id TEXT NOT NULL,
                start_time INTEGER NOT NULL,
                end_time INTEGER NOT NULL,
                input_cost_per_million REAL NOT NULL,
                output_cost_per_million REAL NOT NULL,
                cache_read_cost_per_million REAL NOT NULL,
                cache_creation_cost_per_million REAL NOT NULL,
                label TEXT DEFAULT '',
                threshold INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS cloud_pricing_cache (
                model_id TEXT NOT NULL,
                display_name TEXT NOT NULL,
                input_cost_per_million REAL NOT NULL,
                output_cost_per_million REAL NOT NULL,
                cache_read_cost_per_million REAL NOT NULL,
                cache_creation_cost_per_million REAL NOT NULL,
                threshold INTEGER NOT NULL DEFAULT 0,
                aliases TEXT NOT NULL DEFAULT '',
                no_cache_support INTEGER NOT NULL DEFAULT 0,
                family TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (model_id, threshold)
            );
            CREATE TABLE IF NOT EXISTS cloud_time_rules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                model_id TEXT NOT NULL,
                start_time INTEGER NOT NULL,
                end_time INTEGER NOT NULL,
                input_cost_per_million REAL NOT NULL,
                output_cost_per_million REAL NOT NULL,
                cache_read_cost_per_million REAL NOT NULL,
                cache_creation_cost_per_million REAL NOT NULL,
                label TEXT DEFAULT '',
                threshold INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS model_aliases (
                model_id TEXT NOT NULL,
                alias TEXT NOT NULL,
                PRIMARY KEY (model_id, alias)
            );
            CREATE TABLE IF NOT EXISTS sessions (
                session_id TEXT PRIMARY KEY,
                project_dir TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '',
                source TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS tasks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'todo'
                    CHECK (status IN ('todo','in_progress','done','archived')),
                created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
                updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
            );
            CREATE TABLE IF NOT EXISTS task_sessions (
                task_id INTEGER NOT NULL,
                session_id TEXT NOT NULL,
                source TEXT NOT NULL DEFAULT '',
                project_dir TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '',
                added_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
                PRIMARY KEY (task_id, session_id, source),
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );"
        ).map_err(|e| format!("初始化内存表失败: {}", e))?;
        self.set_setting("schema_version", "9")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_db() -> AppDbService {
        AppDbService::new_in_memory().unwrap()
    }

    #[test]
    fn test_settings_crud() {
        let db = create_db();
        assert!(db.get_setting("nonexistent").is_none());
        db.set_setting("key1", "value1").unwrap();
        assert_eq!(db.get_setting("key1").unwrap(), "value1");
        db.set_setting("key1", "value2").unwrap();
        assert_eq!(db.get_setting("key1").unwrap(), "value2");
    }

    #[test]
    fn test_schema_version() {
        let db = create_db();
        assert_eq!(db.get_setting("schema_version").unwrap(), "9");
    }

    #[test]
    fn test_save_and_get_override() {
        let db = create_db();
        db.save_override("model-a", 10.0, 20.0, 1.0, 5.0).unwrap();
        let overrides = db.get_all_overrides().unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].model_id, "model-a");
        assert!((overrides[0].input_cost_per_million - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_save_override_tier() {
        let db = create_db();
        db.save_override("model-a", 10.0, 20.0, 1.0, 5.0).unwrap();
        db.save_override_tier("model-a", 5000, 15.0, 30.0, 1.5, 7.5).unwrap();
        let overrides = db.get_all_overrides().unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].context_tiers.len(), 1);
        assert_eq!(overrides[0].context_tiers[0].threshold, 5000);
    }

    #[test]
    fn test_delete_override() {
        let db = create_db();
        db.save_override("model-a", 10.0, 20.0, 1.0, 5.0).unwrap();
        db.delete_override("model-a").unwrap();
        assert!(db.get_all_overrides().unwrap().is_empty());
    }

    #[test]
    fn test_time_override_crud() {
        let db = create_db();
        let id = db.add_time_override("model-a", 100, 200, 10.0, 20.0, 1.0, 5.0, "test").unwrap();
        assert!(id > 0);
        let rules = db.get_all_time_overrides().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].model_id, "model-a");
        assert_eq!(rules[0].label, "test");

        db.update_time_override(id, 100, 300, 15.0, 25.0, 1.5, 6.0, "updated").unwrap();
        let rules = db.get_all_time_overrides().unwrap();
        assert_eq!(rules[0].label, "updated");
        assert_eq!(rules[0].end_time, 300);
    }

    #[test]
    fn test_time_override_tier() {
        let db = create_db();
        db.add_time_override("model-a", 100, 200, 10.0, 20.0, 1.0, 5.0, "test").unwrap();
        db.add_time_override_tier("model-a", 100, 200, 5000, 15.0, 30.0, 1.5, 7.5).unwrap();
        let rules = db.get_all_time_overrides().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].context_tiers.len(), 1);
        assert_eq!(rules[0].context_tiers[0].threshold, 5000);
    }

    #[test]
    fn test_delete_time_override_group() {
        let db = create_db();
        db.add_time_override("model-a", 100, 200, 10.0, 20.0, 1.0, 5.0, "test").unwrap();
        db.delete_time_override_group("model-a", 100, 200).unwrap();
        assert!(db.get_all_time_overrides().unwrap().is_empty());
    }

    #[test]
    fn test_session_titles() {
        let db = create_db();
        assert!(db.get_session_titles(&[]).unwrap().is_empty());
        db.save_session_title("sess-1", "title-1", "jsonl").unwrap();
        let titles = db.get_session_titles(&["sess-1".to_string()]).unwrap();
        assert_eq!(titles.get("sess-1").unwrap().0, "title-1");
        assert_eq!(titles.get("sess-1").unwrap().1, "jsonl");
        assert!(titles.get("sess-2").is_none());
    }

    #[test]
    fn test_cloud_pricing_roundtrip() {
        let db = create_db();
        let data = crate::models::CloudPricingData {
            version: 3,
            updated_at: 1700000000,
            currency: "RMB".to_string(),
            families: vec![
                crate::models::PricingFamily {
                    id: "gpt".to_string(),
                    label: "GPT".to_string(),
                },
            ],
            models: vec![
                crate::models::CloudPricingModel {
                    model_id: "model-a".to_string(),
                    input_cost_per_million: 10.0,
                    output_cost_per_million: 20.0,
                    cache_read_cost_per_million: 1.0,
                    cache_creation_cost_per_million: 5.0,
                    context_tiers: vec![crate::models::ContextTier {
                        id: None,
                        threshold: 5000,
                        input_cost_per_million: 15.0,
                        output_cost_per_million: 30.0,
                        cache_read_cost_per_million: 1.5,
                        cache_creation_cost_per_million: 7.5,
                    }],
                    time_rules: vec![crate::models::CloudPricingTimeRule {
                        model_id: "model-a".to_string(),
                        label: "折扣".to_string(),
                        start_time: 100,
                        end_time: 200,
                        input_cost_per_million: 5.0,
                        output_cost_per_million: 10.0,
                        cache_read_cost_per_million: 0.5,
                        cache_creation_cost_per_million: 2.5,
                        context_tiers: vec![],
                    }],
                    aliases: vec!["model-a-alias".to_string()],
                    no_cache_support: false,
                    family: "gpt".to_string(),
                },
            ],
        };
        db.save_cloud_pricing(&data).unwrap();
        let (base, tiers, cloud_time_rules, _cloud_aliases, _no_cache_models, model_families) = db.load_cloud_pricing().unwrap();
        assert_eq!(base.len(), 1);
        assert_eq!(base[0].model_id, "model-a");
        assert!(tiers.contains_key("model-a"));
        assert_eq!(tiers["model-a"].len(), 1);
        assert_eq!(tiers["model-a"][0].threshold, 5000);
        assert!(cloud_time_rules.contains_key("model-a"));
        assert_eq!(cloud_time_rules["model-a"].len(), 1);
        assert_eq!(cloud_time_rules["model-a"][0].label, "折扣");
        assert_eq!(model_families.get("model-a").map(|s| s.as_str()), Some("gpt"));
        let families = db.load_cloud_families().unwrap();
        assert_eq!(families.len(), 1);
        assert_eq!(families[0].id, "gpt");
    }

    #[test]
    fn test_cloud_pricing_version() {
        let db = create_db();
        let data = crate::models::CloudPricingData {
            version: 5,
            updated_at: 1700000000,
            currency: "RMB".to_string(),
            families: vec![],
            models: vec![],
        };
        db.save_cloud_pricing(&data).unwrap();
        assert_eq!(db.get_setting("cloud_pricing_version").unwrap(), "5");
    }

    #[test]
    fn test_task_crud_and_sessions() {
        let db = create_db();

        // 创建
        let id = db.create_task("重构 Auth", "用 JWT 替换旧 session", "todo").unwrap();
        assert!(id > 0);

        // 列出
        let tasks = db.list_tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "重构 Auth");
        assert_eq!(tasks[0].status, "todo");

        // 更新
        db.update_task(id, "重构 Auth 模块", "改用 JWT", "in_progress").unwrap();
        let t = db.get_task(id).unwrap().unwrap();
        assert_eq!(t.title, "重构 Auth 模块");
        assert_eq!(t.status, "in_progress");

        // 添加会话
        db.add_task_sessions(
            id,
            &[
                TaskSessionInput {
                    session_id: "ses-1".into(),
                    source: "claudecode".into(),
                    project_dir: "D:/proj".into(),
                    title: "title-1".into(),
                },
                TaskSessionInput {
                    session_id: "ses-2".into(),
                    source: "opencode".into(),
                    project_dir: "D:/proj".into(),
                    title: "title-2".into(),
                },
            ],
        )
        .unwrap();
        let sessions = db.list_task_sessions(id).unwrap();
        assert_eq!(sessions.len(), 2);

        // 删除 task,session 跟随 CASCADE 清理
        db.delete_task(id).unwrap();
        assert!(db.get_task(id).unwrap().is_none());
        let sessions = db.list_task_sessions(id).unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_create_task_validates_input() {
        let db = create_db();
        // 空标题应被拒绝
        assert!(db.create_task("   ", "x", "todo").is_err());
        // 非法 status 应被拒绝
        assert!(db.create_task("t", "x", "weird").is_err());
    }
}
