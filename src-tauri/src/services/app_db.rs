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
        let db_path = get_app_db_path();
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
        // 未来版本: if version < 2 { self.backup_before_migration(1)?; self.migrate_v2()?; }

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
        let db_path = get_app_db_path();
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

    pub fn get_exchange_rate(&self) -> f64 {
        self.get_setting("exchange_rate")
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_EXCHANGE_RATE)
    }

    pub fn set_exchange_rate(&self, rate: f64) -> Result<(), String> {
        self.set_setting("exchange_rate", &rate.to_string())
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
                    row.get::<_, String>("label")?,
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
        result.sort_by(|a, b| (&a.model_id, a.start_time).cmp(&(&b.model_id, b.start_time)));
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
                    if label.is_empty() { "" } else { label }
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
                    if label.is_empty() { "" } else { label },
                    id
                ],
            )
            .map_err(|e| format!("更新时间定价失败: {}", e))?;
        Ok(())
    }

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
                    if label.is_empty() { "" } else { label },
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

    pub fn get_session_titles(&self, session_ids: &[String]) -> Result<HashMap<String, String>, String> {
        if session_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders: Vec<String> = session_ids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            "SELECT session_id, title FROM session_titles WHERE session_id IN ({})",
            placeholders.join(",")
        );
        let refs: Vec<&dyn rusqlite::types::ToSql> = session_ids
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();
        let mut stmt = self.db.prepare(&sql).map_err(|e| format!("查询会话标题失败: {}", e))?;
        let rows = stmt.query_map(refs.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }).map_err(|e| format!("查询会话标题失败: {}", e))?;
        let mut result = HashMap::new();
        for row in rows {
            let (sid, title) = row.map_err(|e| format!("读取会话标题失败: {}", e))?;
            result.insert(sid, title);
        }
        Ok(result)
    }

    pub fn save_session_title(&self, session_id: &str, title: &str) -> Result<(), String> {
        self.db.execute(
            "INSERT OR REPLACE INTO session_titles (session_id, title, created_at) VALUES (?, ?, strftime('%s','now'))",
            params![session_id, title],
        ).map_err(|e| format!("保存会话标题失败: {}", e))?;
        Ok(())
    }
}
