use rusqlite::{params, Connection};
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
                "CREATE TABLE IF NOT EXISTS pricing_overrides (
                    model_id TEXT PRIMARY KEY,
                    input_cost_per_million REAL NOT NULL,
                    output_cost_per_million REAL NOT NULL,
                    cache_read_cost_per_million REAL NOT NULL,
                    cache_creation_cost_per_million REAL NOT NULL,
                    updated_at INTEGER DEFAULT (strftime('%s','now'))
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
                    label TEXT DEFAULT ''
                );

                CREATE TABLE IF NOT EXISTS settings (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL,
                    updated_at INTEGER DEFAULT (strftime('%s','now'))
                );",
            )
            .map_err(|e| format!("初始化数据库表失败: {}", e))?;
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
            .prepare("SELECT * FROM pricing_overrides ORDER BY model_id")
            .map_err(|e| format!("查询定价覆盖失败: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(PricingOverride {
                    model_id: row.get("model_id")?,
                    input_cost_per_million: row.get("input_cost_per_million")?,
                    output_cost_per_million: row.get("output_cost_per_million")?,
                    cache_read_cost_per_million: row.get("cache_read_cost_per_million")?,
                    cache_creation_cost_per_million: row.get("cache_creation_cost_per_million")?,
                    updated_at: row.get("updated_at")?,
                })
            })
            .map_err(|e| format!("查询定价覆盖失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("读取定价覆盖失败: {}", e))?);
        }
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
                    (model_id, input_cost_per_million, output_cost_per_million,
                     cache_read_cost_per_million, cache_creation_cost_per_million, updated_at)
                 VALUES (?, ?, ?, ?, ?, strftime('%s','now'))",
                params![model_id, input_cost, output_cost, cache_read_cost, cache_creation_cost],
            )
            .map_err(|e| format!("保存定价覆盖失败: {}", e))?;
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
            .prepare("SELECT * FROM time_pricing_overrides ORDER BY model_id, start_time")
            .map_err(|e| format!("查询时间定价失败: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(TimePricingRule {
                    id: row.get("id")?,
                    model_id: row.get("model_id")?,
                    start_time: row.get("start_time")?,
                    end_time: row.get("end_time")?,
                    input_cost_per_million: row.get("input_cost_per_million")?,
                    output_cost_per_million: row.get("output_cost_per_million")?,
                    cache_read_cost_per_million: row.get("cache_read_cost_per_million")?,
                    cache_creation_cost_per_million: row.get("cache_creation_cost_per_million")?,
                    label: row.get("label")?,
                })
            })
            .map_err(|e| format!("查询时间定价失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("读取时间定价失败: {}", e))?);
        }
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
                     cache_read_cost_per_million, cache_creation_cost_per_million, label)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
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

    pub fn delete_time_override(&self, id: i64) -> Result<(), String> {
        self.db
            .execute(
                "DELETE FROM time_pricing_overrides WHERE id = ?",
                params![id],
            )
            .map_err(|e| format!("删除时间定价失败: {}", e))?;
        Ok(())
    }
}
