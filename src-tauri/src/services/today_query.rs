//! Minimal daily totals for native status displays. Sources are independent read-only instances.
use crate::models::FilterParams;
use crate::services::data_source::{
    create_source_entry_with_type, pricing_context_width, DbType, SourceEntry,
};
use crate::services::pipeline::{aggregate_summary, fetch_deduped_records_report};
use crate::SharedState;
use chrono::{Offset, TimeZone};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Instant, SystemTime};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayData {
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_cost: String,
    pub request_count: i64,
    pub stale: bool,
    pub errors: Vec<String>,
    pub updated_at: i64,
    #[serde(skip)]
    pub total_cost_value: f64,
}

#[derive(Default)]
pub struct TodaySourceCache {
    sources: Vec<SourceEntry>,
    // Only file-backed Cursor requires reopening on local CSV/hook changes.
    cursor_mtimes: HashMap<String, Option<SystemTime>>,
    last_cursor_refresh: Option<Instant>,
}

#[derive(Clone, Copy)]
pub enum TodayTimezone {
    Local,
    OffsetMinutes(i32),
}

/// Calendar midnights are converted independently, so DST days may be 23 or 25 hours.
fn calendar_bounds<T: TimeZone>(zone: &T, now: i64) -> Result<(i64, i64), String> {
    let date = zone
        .timestamp_opt(now, 0)
        .single()
        .ok_or("invalid_time")?
        .date_naive();
    let next = date.succ_opt().ok_or("invalid_date")?;
    let start = zone
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .ok_or("invalid_midnight")?;
    let end = zone
        .from_local_datetime(&next.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .ok_or("invalid_midnight")?;
    Ok((start.timestamp(), end.timestamp()))
}

pub fn query_today_data(
    shared: &SharedState,
    timezone: TodayTimezone,
    source_cache: &Mutex<TodaySourceCache>,
) -> Result<TodayData, String> {
    let now = crate::utils::now_epoch_seconds();
    let bounds = match timezone {
        TodayTimezone::Local => calendar_bounds(&chrono::Local, now)?,
        TodayTimezone::OffsetMinutes(minutes) => {
            let zone = chrono::FixedOffset::east_opt(minutes * 60).ok_or("invalid_timezone")?;
            calendar_bounds(&zone, now)?
        }
    };
    let params = FilterParams {
        from_epoch: Some(bounds.0),
        to_epoch: Some(bounds.1),
        tz_offset: None,
        provider_id: None,
        model_id: None,
        ccs_filter_session_apps: shared
            .ccs_filter_session_apps
            .read()
            .map_err(|e| e.to_string())?
            .clone(),
    };
    let entries: Vec<_> = shared
        .data_sources
        .read()
        .map_err(|e| e.to_string())?
        .iter()
        .filter(|s| s.enabled)
        .map(|s| (s.path.clone(), s.db_type.clone()))
        .collect();
    if entries.is_empty() {
        return Err("no_database_loaded".into());
    }
    let mut sc = source_cache.lock().map_err(|e| e.to_string())?;
    let mut errors: Vec<_> = shared
        .collection_errors
        .read()
        .map_err(|e| e.to_string())?
        .iter()
        .filter(|e| {
            entries
                .iter()
                .any(|(_, t)| e.starts_with(&format!("{}:", t.label())))
        })
        .cloned()
        .collect();
    sc.sources
        .retain(|s| entries.contains(&(s.path.clone(), s.db_type.clone())));
    sc.cursor_mtimes.retain(|p, _| {
        entries
            .iter()
            .any(|(path, t)| path == p && *t == DbType::Cursor)
    });
    for (path, db_type) in &entries {
        let mtime = if *db_type == DbType::Cursor {
            std::fs::metadata(std::path::Path::new(path).join("usage.csv"))
                .ok()
                .and_then(|m| m.modified().ok())
        } else {
            None
        };
        let changed = *db_type == DbType::Cursor && sc.cursor_mtimes.get(path) != Some(&mtime);
        if changed {
            sc.sources
                .retain(|s| s.path != *path || s.db_type != *db_type);
        }
        if !sc
            .sources
            .iter()
            .any(|s| s.path == *path && s.db_type == *db_type)
        {
            match create_source_entry_with_type(path, Some(db_type)) {
                Ok(s) => {
                    sc.sources.push(s);
                    if *db_type == DbType::Cursor {
                        sc.cursor_mtimes.insert(path.clone(), mtime);
                    }
                }
                Err(e) => errors.push(format!("{}: {e}", db_type.label())),
            }
        }
    }
    if sc.sources.is_empty() {
        return Err(errors.join("; "));
    }
    let refresh_cursor = sc
        .last_cursor_refresh
        .map_or(true, |t| t.elapsed().as_secs() >= 5);
    for s in &mut sc.sources {
        s.source
            .set_ccs_filter_apps(params.ccs_filter_session_apps.as_deref());
        if s.db_type == DbType::Cursor && refresh_cursor {
            s.source.refresh_cursor_local_events();
        }
    }
    if refresh_cursor {
        sc.last_cursor_refresh = Some(Instant::now());
    }
    let (records, failures) = match fetch_deduped_records_report(&sc.sources, &params) {
        Ok(result) => result,
        Err(e) => {
            sc.sources.clear();
            sc.cursor_mtimes.clear();
            return Err(e);
        }
    };
    // Drop failed instances, retry opening them next time even without a file mtime change.
    sc.sources.retain(|s| {
        !failures
            .iter()
            .any(|e| e.starts_with(&format!("{}:", s.db_type.label())))
    });
    errors.extend(failures);
    let summary = aggregate_summary(&records);
    let pricing = shared.pricing_engine.read().map_err(|e| e.to_string())?;
    let total_cost = records
        .iter()
        .map(|r| {
            let offset = match timezone {
                TodayTimezone::Local => chrono::Local
                    .timestamp_opt(r.created_at, 0)
                    .single()
                    .map(|t| t.offset().fix().local_minus_utc())
                    .unwrap_or(0),
                TodayTimezone::OffsetMinutes(minutes) => minutes * 60,
            };
            let context =
                pricing_context_width(&r.db_type, &r.provider_id, r.input_tokens, r.cache_read);
            pricing
                .get_pricing_at_with_context_seconds(&r.model, r.created_at, context, offset as i64)
                .map(|p| {
                    pricing.calculate_cost(
                        &p,
                        r.input_tokens,
                        r.output_tokens,
                        r.cache_read,
                        r.cache_creation,
                    )
                })
                .unwrap_or(0.0)
        })
        .sum::<f64>();
    Ok(TodayData {
        total_tokens: summary.total_input
            + summary.total_output
            + summary.total_cache_read
            + summary.total_cache_creation,
        input_tokens: summary.total_input,
        output_tokens: summary.total_output,
        cache_read_tokens: summary.total_cache_read,
        cache_creation_tokens: summary.total_cache_creation,
        total_cost: format!("{total_cost:.2}¥"),
        request_count: summary.total_requests,
        total_cost_value: total_cost,
        stale: !errors.is_empty(),
        errors,
        updated_at: now,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::models::{CloudPricingData, CloudPricingModel, ContextTier};
    use crate::services::{app_db::AppDbService, pricing_engine::PricingEngine};
    use std::sync::Arc;

    pub(crate) fn fixture() -> (
        tempfile::TempDir,
        AppDbService,
        Arc<SharedState>,
        Arc<Mutex<TodaySourceCache>>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pricing.db");
        let db = AppDbService::new_test_file(&path).unwrap();
        let shared = Arc::new(SharedState::new(PricingEngine::new(), None));
        shared.data_sources.write().unwrap().push(
            create_source_entry_with_type(path.to_str().unwrap(), Some(&DbType::Pi)).unwrap(),
        );
        (
            dir,
            db,
            shared,
            Arc::new(Mutex::new(TodaySourceCache::default())),
        )
    }

    pub(crate) fn insert(db: &AppDbService, source: &str, id: &str, at: i64, input: i64) {
        AppDbService::insert_session_log_on_conn(
            db.conn(),
            source,
            id,
            id,
            "test-model",
            source,
            input,
            10,
            20,
            30,
            at,
            0,
            0,
        )
        .unwrap();
    }

    #[test]
    fn live_wal_same_second_and_historical_rows_respect_today_bounds() {
        let (_dir, db, shared, cache) = fixture();
        let now = crate::utils::now_epoch_seconds();
        insert(&db, "PI", "old", now - 3 * 86400, 900);
        insert(&db, "PI", "first", now, 100);
        assert_eq!(
            query_today_data(&shared, TodayTimezone::Local, &cache)
                .unwrap()
                .total_tokens,
            160
        );
        insert(&db, "PI", "same-second", now, 600);
        let data = query_today_data(&shared, TodayTimezone::Local, &cache).unwrap();
        assert_eq!(data.total_tokens, 820);
        assert_eq!(data.request_count, 2);
        assert!(!data.stale);
    }

    #[test]
    fn errors_are_exposed_and_failed_sources_recover_without_mtime_changes() {
        let (_dir, db, shared, cache) = fixture();
        let now = crate::utils::now_epoch_seconds();
        insert(&db, "PI", "p", now, 100);
        let path = shared.data_sources.read().unwrap()[0].path.clone();
        let failed_dir = tempfile::tempdir().unwrap();
        let failed_path = failed_dir.path().join("empty.db");
        let _writer = rusqlite::Connection::open(&failed_path).unwrap();
        shared.data_sources.write().unwrap().push(
            create_source_entry_with_type(failed_path.to_str().unwrap(), Some(&DbType::Dsh))
                .unwrap(),
        );
        let partial = query_today_data(&shared, TodayTimezone::Local, &cache).unwrap();
        assert!(partial.stale);
        assert!(partial.errors.iter().any(|e| e.contains("DSH")));
        assert_eq!(partial.total_tokens, 160);
        shared
            .data_sources
            .write()
            .unwrap()
            .retain(|s| s.db_type == DbType::Dsh);
        assert!(query_today_data(&shared, TodayTimezone::Local, &cache).is_err());
        // Repair the isolated fixture, not an external/user database.
        let repaired = AppDbService::new_test_file(&failed_path).unwrap();
        insert(&repaired, "DSH", "repaired", now, 600);
        let result = query_today_data(&shared, TodayTimezone::Local, &cache).unwrap();
        assert_eq!(result.total_tokens, 660);
        assert!(!result.stale);
        assert_ne!(path, failed_path.to_string_lossy());
    }

    fn model() -> CloudPricingModel {
        CloudPricingModel {
            model_id: "test-model".into(),
            input_cost_per_million: 2.0,
            output_cost_per_million: 6.0,
            cache_read_cost_per_million: 0.2,
            cache_creation_cost_per_million: 3.0,
            context_tiers: vec![ContextTier {
                id: None,
                threshold: 100,
                input_cost_per_million: 4.0,
                output_cost_per_million: 12.0,
                cache_read_cost_per_million: 0.4,
                cache_creation_cost_per_million: 6.0,
                daily_slots: vec![],
            }],
            time_rules: vec![],
            aliases: vec!["test-alias".into()],
            alias_patterns: vec!["^test-model(-(low|high))?$".into()],
            no_cache_support: false,
            family: "test".into(),
            daily_slots: vec![],
        }
    }

    #[test]
    fn minimal_totals_match_frontend_pricing_and_apply_changes_immediately() {
        let (_dir, db, shared, cache) = fixture();
        let now = crate::utils::now_epoch_seconds();
        insert(&db, "PI", "wide", now, 1000);
        insert(&db, "PI", "narrow", now, 10);
        db.conn()
            .execute(
                "UPDATE session_request_logs SET model='TEST-MODEL-HIGH' WHERE request_id='wide'",
                [],
            )
            .unwrap();
        let mut pricing = CloudPricingData {
            version: 1,
            updated_at: now,
            currency: "RMB".into(),
            families: vec![],
            models: vec![model()],
        };
        db.save_cloud_pricing(&pricing).unwrap();
        shared.pricing_engine.write().unwrap().refresh(&db).unwrap();
        let today = query_today_data(&shared, TodayTimezone::OffsetMinutes(0), &cache).unwrap();
        // Wide request uses tier 4/12/.4/6, narrow uses base 2/6/.2/3.
        assert!((today.total_cost_value - 0.004482).abs() < 1e-10);
        let bounds = calendar_bounds(&chrono::Utc, now).unwrap();
        let params = FilterParams {
            from_epoch: Some(bounds.0),
            to_epoch: Some(bounds.1),
            tz_offset: Some(0),
            provider_id: None,
            model_id: None,
            ccs_filter_session_apps: None,
        };
        let front = crate::commands::query::compute_precompute(
            &shared.data_sources.read().unwrap(),
            &shared.pricing_engine.read().unwrap(),
            &params,
        )
        .unwrap();
        assert!(
            (today.total_cost_value - front.precomputed.model_costs.values().sum::<f64>()).abs()
                < 1e-10
        );
        pricing.models[0].input_cost_per_million *= 2.0;
        pricing.models[0].context_tiers[0].input_cost_per_million *= 2.0;
        db.save_cloud_pricing(&pricing).unwrap();
        shared.pricing_engine.write().unwrap().refresh(&db).unwrap();
        assert!(
            query_today_data(&shared, TodayTimezone::OffsetMinutes(0), &cache)
                .unwrap()
                .total_cost_value
                > today.total_cost_value
        );
        shared.data_sources.write().unwrap()[0].enabled = false;
        assert!(query_today_data(&shared, TodayTimezone::OffsetMinutes(0), &cache).is_err());
    }

    #[test]
    fn every_scanned_sql_source_filters_history_before_decoding_rows() {
        let (dir, db, _shared, _cache) = fixture();
        let path = dir.path().join("pricing.db");
        let types = [
            DbType::Pi,
            DbType::Dsh,
            DbType::Proma,
            DbType::Minimax,
            DbType::Antigravity,
            DbType::Kimi,
            DbType::ZCode,
        ];
        for kind in types {
            insert(
                &db,
                kind.label(),
                &format!("{}-today", kind.label()),
                2000,
                100,
            );
            insert(
                &db,
                kind.label(),
                &format!("{}-history", kind.label()),
                1000,
                900,
            );
            db.conn()
                .execute(
                    "UPDATE session_request_logs SET input_tokens='invalid' WHERE request_id=?",
                    [format!("{}-history", kind.label())],
                )
                .unwrap();
            let mut source: Box<dyn crate::services::data_source::DataSource> =
                if kind == DbType::ZCode {
                    Box::new(crate::services::zcode_db::ZCodeDbService::new())
                } else {
                    create_source_entry_with_type(path.to_str().unwrap(), Some(&kind))
                        .unwrap()
                        .source
                };
            source.open(path.to_str().unwrap()).unwrap();
            let result = source
                .get_filtered_records(&FilterParams {
                    from_epoch: Some(2000),
                    to_epoch: Some(3000),
                    tz_offset: None,
                    provider_id: None,
                    model_id: None,
                    ccs_filter_session_apps: None,
                })
                .unwrap();
            assert_eq!(result.len(), 1, "{}", kind.label());
            assert_eq!(result[0].input_tokens, 100);
        }
        let plan: String = db.conn().query_row("EXPLAIN QUERY PLAN SELECT * FROM session_request_logs WHERE source='PI' AND created_at>=2000 AND created_at<3000", [], |r| r.get(3)).unwrap();
        assert!(plan.contains("idx_srl_source_created"), "{plan}");
    }

    #[test]
    fn fractional_timezone_and_midnight_use_calendar_boundaries() {
        let zone = chrono::FixedOffset::east_opt(5 * 3600 + 30 * 60).unwrap();
        let now = chrono::Utc
            .with_ymd_and_hms(2026, 10, 9, 18, 45, 0)
            .unwrap()
            .timestamp();
        let (start, end) = calendar_bounds(&zone, now).unwrap();
        assert_eq!(
            start,
            chrono::Utc
                .with_ymd_and_hms(2026, 10, 9, 18, 30, 0)
                .unwrap()
                .timestamp()
        );
        assert_eq!(end - start, 86400);
        assert_eq!(calendar_bounds(&zone, start - 1).unwrap().1, start);
        assert_eq!(calendar_bounds(&zone, end).unwrap().0, end);
    }

    #[test]
    fn calendar_day_can_have_23_or_25_hours() {
        // Deterministic timezone transition avoids modifying the host timezone/TZ environment.
        #[derive(Clone)]
        struct ShiftZone {
            day: chrono::NaiveDate,
            after: i32,
        }
        impl TimeZone for ShiftZone {
            type Offset = chrono::FixedOffset;
            fn from_offset(_: &Self::Offset) -> Self {
                unreachable!()
            }
            fn offset_from_local_date(
                &self,
                d: &chrono::NaiveDate,
            ) -> chrono::LocalResult<Self::Offset> {
                chrono::LocalResult::Single(
                    chrono::FixedOffset::east_opt(if *d > self.day { self.after } else { 0 })
                        .unwrap(),
                )
            }
            fn offset_from_local_datetime(
                &self,
                d: &chrono::NaiveDateTime,
            ) -> chrono::LocalResult<Self::Offset> {
                self.offset_from_local_date(&d.date())
            }
            fn offset_from_utc_date(&self, d: &chrono::NaiveDate) -> Self::Offset {
                self.offset_from_local_date(d).unwrap()
            }
            fn offset_from_utc_datetime(&self, d: &chrono::NaiveDateTime) -> Self::Offset {
                self.offset_from_utc_date(&d.date())
            }
        }
        let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
        let now = day.and_hms_opt(12, 0, 0).unwrap().and_utc().timestamp();
        for (after, hours) in [(3600, 23), (-3600, 25)] {
            let bounds = calendar_bounds(&ShiftZone { day, after }, now).unwrap();
            assert_eq!(bounds.1 - bounds.0, hours * 3600);
        }
    }
}
