//! Cursor CSV 本机归因：分钟±5 + 模型家族匹配

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{FixedOffset, TimeZone};
use serde::{Deserialize, Serialize};

pub const ATTRIBUTION_SETTING_KEY: &str = "cursor_local_attribution_enabled";
/// 本机归因过滤起始时刻（Unix 秒）配置键；未设置时用默认值。
pub const ATTRIBUTION_FILTER_START_SETTING_KEY: &str = "cursor_attribution_filter_start";
pub const ATTRIBUTION_SLACK_SECS: i64 = 300;
pub const OVERRIDES_FILE_NAME: &str = "attribution-overrides.json";

/// 默认本机归因过滤起始时刻：2026-07-13 15:30:00（东八区）。
/// CSV 的 `created_at` 为 Unix 秒（UTC）；此之前的行不过滤，保留账号全量。
pub fn default_attribution_filter_start_epoch() -> i64 {
    FixedOffset::east_opt(8 * 3600)
        .expect("UTC+8")
        .with_ymd_and_hms(2026, 7, 13, 15, 30, 0)
        .single()
        .expect("valid cutoff datetime")
        .timestamp()
}

/// 该 CSV 时间戳是否需要做本机归因过滤（`start_epoch` 为配置或默认起始点）。
pub fn should_apply_attribution_for_ts(csv_ts: i64, start_epoch: i64) -> bool {
    csv_ts >= start_epoch
}

/// 四项 token 合计（输入 / 输出 / 缓存读 / 缓存写）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenQuad {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
}

impl TokenQuad {
    pub fn add_record_tokens(&mut self, input: i64, output: i64, cache_read: i64, cache_creation: i64) {
        self.input += input;
        self.output += output;
        self.cache_read += cache_read;
        self.cache_creation += cache_creation;
    }
}

/// CSV 全量与归因过滤掉的 token 对比。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributionTokenStats {
    pub csv_total: TokenQuad,
    pub filtered_out: TokenQuad,
}

/// 被滤掉的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterReason {
    /// 时间窗内有本机 Hook，但模型家族不一致
    Model,
    /// 存在同家族 Hook，但不在 ±slack 内
    Time,
    /// 既无窗内事件，也无同家族事件
    None,
}

/// 行级手动改判方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OverrideAction {
    Keep,
    Filter,
}

/// sidecar 中单条改判记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverrideEntry {
    pub action: OverrideAction,
    pub created_at: i64,
    pub model: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverridesFile {
    version: u32,
    #[serde(default)]
    overrides: HashMap<String, OverrideEntry>,
}

/// 算法原因 + 手动改判后的最终决议。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveAttribution {
    pub filtered: bool,
    pub reason: Option<FilterReason>,
    pub override_action: Option<OverrideAction>,
}

/// CSV 预览单行。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorCsvPreviewRow {
    pub created_at: i64,
    pub model: String,
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
    pub filtered: bool,
    pub reason: Option<FilterReason>,
    pub row_key: String,
    /// 手动改判；无记录时为 null（取消申诉后回归算法）。
    #[serde(rename = "override", default, skip_serializing_if = "Option::is_none")]
    pub override_action: Option<OverrideAction>,
}

/// CSV 预览分页结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorCsvPreviewPage {
    pub items: Vec<CursorCsvPreviewRow>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
    /// 当前过滤条件下可选模型列表（未应用 model 筛选前的 distinct）
    #[serde(default)]
    pub available_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalHookEvent {
    pub ts_epoch: i64,
    pub model: String,
    pub family: String,
    pub hook_event_name: String,
}

/// 归一化模型家族名，便于 CSV 与 hook 对齐。
/// 例：`cursor-grok-4.5-high` / `grok-4.5` → `grok-4.5`
pub fn normalize_model_family(name: &str) -> String {
    let mut n = name.trim().to_lowercase();
    if let Some(rest) = n.strip_prefix("cursor-") {
        n = rest.to_string();
    }
    // 注意：不剥 `-fast` / `-fast-high`。composer-2.5-fast 是独立 SKU（更快更贵），
    // 不是 thinking/effort 档，不能与 composer-2.5 归为同一家族。
    const SUFFIXES: &[&str] = &[
        "-thinking-high",
        "-thinking-medium",
        "-thinking-low",
        "-high",
        "-medium",
        "-low",
        "-max",
    ];
    for suf in SUFFIXES {
        if let Some(rest) = n.strip_suffix(suf) {
            n = rest.to_string();
            break;
        }
    }
    n
}

/// 判断 CSV 行是否较像本机用量。
/// `events` 应已筛过归因相关 hook（含 subagent / compact 等），且带非空 model。
pub fn is_likely_local(csv_ts: i64, csv_model: &str, events: &[LocalHookEvent], slack_secs: i64) -> bool {
    explain_filter_reason(csv_ts, csv_model, events, slack_secs).is_none()
}

/// 稳定行指纹（FNV-1a 双 64 位 hex，无额外依赖）。
pub fn row_key(
    created_at: i64,
    model: &str,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_creation: i64,
) -> String {
    let raw = format!(
        "{}|{}|{}|{}|{}|{}",
        created_at, model, input, output, cache_read, cache_creation
    );
    let h1 = fnv1a64(raw.as_bytes(), 0xcbf29ce484222325);
    let h2 = fnv1a64(raw.as_bytes(), 0x84222325cbf29ce4);
    format!("{:016x}{:016x}", h1, h2)
}

fn fnv1a64(data: &[u8], offset_basis: u64) -> u64 {
    let mut hash = offset_basis;
    for &b in data {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// 算法原因 + 可选手动改判 → 最终是否过滤。
/// `algo_reason`：`Some` 表示算法要滤；`None` 表示算法保留（含未启用归因 / 截止前）。
pub fn resolve_effective(
    algo_reason: Option<FilterReason>,
    override_action: Option<OverrideAction>,
) -> EffectiveAttribution {
    match override_action {
        Some(OverrideAction::Keep) => EffectiveAttribution {
            filtered: false,
            reason: algo_reason,
            override_action,
        },
        Some(OverrideAction::Filter) => EffectiveAttribution {
            filtered: true,
            reason: algo_reason.or(Some(FilterReason::None)),
            override_action,
        },
        None => EffectiveAttribution {
            filtered: algo_reason.is_some(),
            reason: algo_reason,
            override_action: None,
        },
    }
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn overrides_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(OVERRIDES_FILE_NAME)
}

/// 读取 sidecar；文件不存在返回空 map。
pub fn load_overrides(cache_dir: &Path) -> Result<HashMap<String, OverrideEntry>, String> {
    let path = overrides_path(cache_dir);
    if !path.is_file() {
        return Ok(HashMap::new());
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("读取改判文件失败: {}", e))?;
    if text.trim().is_empty() {
        return Ok(HashMap::new());
    }
    let file: OverridesFile =
        serde_json::from_str(&text).map_err(|e| format!("解析改判文件失败: {}", e))?;
    Ok(file.overrides)
}

/// 原子写回 sidecar。
pub fn save_overrides(
    cache_dir: &Path,
    overrides: &HashMap<String, OverrideEntry>,
) -> Result<(), String> {
    let path = overrides_path(cache_dir);
    let file = OverridesFile {
        version: 1,
        overrides: overrides.clone(),
    };
    let json =
        serde_json::to_string_pretty(&file).map_err(|e| format!("序列化改判失败: {}", e))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json.as_bytes()).map_err(|e| format!("写入改判临时文件失败: {}", e))?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("替换改判文件失败: {}", e))?;
    }
    fs::rename(&tmp, &path).map_err(|e| format!("提交改判文件失败: {}", e))?;
    Ok(())
}

/// 丢掉不在 live_keys 中的孤儿记录；有删除时写回。返回清理后的 map。
pub fn gc_overrides(
    cache_dir: &Path,
    overrides: &mut HashMap<String, OverrideEntry>,
    live_keys: &HashSet<String>,
) -> Result<usize, String> {
    let before = overrides.len();
    overrides.retain(|k, _| live_keys.contains(k));
    let removed = before - overrides.len();
    if removed > 0 {
        save_overrides(cache_dir, overrides)?;
        log::info!("[CURSOR] 改判 GC 删除 {} 条孤儿", removed);
    }
    Ok(removed)
}

pub fn upsert_override(
    cache_dir: &Path,
    overrides: &mut HashMap<String, OverrideEntry>,
    key: &str,
    action: OverrideAction,
    created_at: i64,
    model: &str,
) -> Result<(), String> {
    let now = now_epoch();
    let entry = overrides.entry(key.to_string()).or_insert_with(|| OverrideEntry {
        action,
        created_at,
        model: model.to_string(),
        updated_at: now,
    });
    entry.action = action;
    entry.created_at = created_at;
    entry.model = model.to_string();
    entry.updated_at = now;
    save_overrides(cache_dir, overrides)
}

pub fn delete_override(
    cache_dir: &Path,
    overrides: &mut HashMap<String, OverrideEntry>,
    key: &str,
) -> Result<(), String> {
    overrides.remove(key);
    save_overrides(cache_dir, overrides)
}

/// 解释为何应被滤掉。`None` = 保留（含 fail-open、空家族以外的匹配成功）。
/// 优先级：model > time > none。
pub fn explain_filter_reason(
    csv_ts: i64,
    csv_model: &str,
    events: &[LocalHookEvent],
    slack_secs: i64,
) -> Option<FilterReason> {
    if events.is_empty() {
        return None; // fail-open
    }
    let fam = normalize_model_family(csv_model);
    if fam.is_empty() {
        return Some(FilterReason::None);
    }

    let mut family_in_slack = false;
    let mut any_in_slack = false;
    let mut family_outside_slack = false;
    for e in events {
        if e.family.is_empty() {
            continue;
        }
        let in_slack = (e.ts_epoch - csv_ts).abs() <= slack_secs;
        let same_family = e.family == fam;
        if in_slack {
            any_in_slack = true;
            if same_family {
                family_in_slack = true;
            }
        } else if same_family {
            family_outside_slack = true;
        }
    }

    if family_in_slack {
        return None;
    }
    if any_in_slack {
        return Some(FilterReason::Model);
    }
    if family_outside_slack {
        return Some(FilterReason::Time);
    }
    Some(FilterReason::None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_model_family() {
        assert_eq!(normalize_model_family("cursor-grok-4.5-high"), "grok-4.5");
        assert_eq!(normalize_model_family("grok-4.5"), "grok-4.5");
        assert_eq!(normalize_model_family("grok-4.5-high"), "grok-4.5");
        assert_eq!(
            normalize_model_family("claude-sonnet-5-thinking-high"),
            "claude-sonnet-5"
        );
        assert_eq!(normalize_model_family("composer-2.5-fast"), "composer-2.5-fast");
        assert_eq!(normalize_model_family("composer-2.5"), "composer-2.5");
        assert_eq!(normalize_model_family("glm-5.2-max"), "glm-5.2");
        assert_eq!(normalize_model_family("gpt-5.6-terra-medium"), "gpt-5.6-terra");
    }

    #[test]
    fn test_composer_fast_not_same_family_as_composer() {
        let events = vec![LocalHookEvent {
            ts_epoch: 1_000_000,
            model: "composer-2.5".into(),
            family: "composer-2.5".into(),
            hook_event_name: "beforeSubmitPrompt".into(),
        }];
        // 同秒级时间窗内：本机只有 composer-2.5，CSV 的 fast 应判模型不对
        assert_eq!(
            explain_filter_reason(1_000_000, "composer-2.5-fast", &events, 60),
            Some(FilterReason::Model)
        );
        assert_eq!(
            explain_filter_reason(1_000_000, "composer-2.5", &events, 60),
            None
        );
    }

    #[test]
    fn test_attribution_cutoff_is_cst_1530() {
        // 2026-07-13 15:30:00 +08:00 == 2026-07-13 07:30:00 UTC
        let start = default_attribution_filter_start_epoch();
        assert_eq!(start, 1_783_927_800);
        let utc = chrono::DateTime::from_timestamp(start, 0).unwrap();
        assert_eq!(utc.format("%Y-%m-%d %H:%M:%S").to_string(), "2026-07-13 07:30:00");

        // 东八区 15:29:59 → 不过滤；15:30:00 → 过滤
        assert!(!should_apply_attribution_for_ts(start - 1, start));
        assert!(should_apply_attribution_for_ts(start, start));
        assert!(should_apply_attribution_for_ts(start + 1, start));
    }

    #[test]
    fn test_is_likely_local_same_family_within_slack() {
        let events = vec![LocalHookEvent {
            ts_epoch: 1_000_000,
            model: "grok-4.5".into(),
            family: "grok-4.5".into(),
            hook_event_name: "beforeSubmitPrompt".into(),
        }];
        assert!(is_likely_local(
            1_000_030,
            "cursor-grok-4.5-high",
            &events,
            60
        ));
        assert!(!is_likely_local(
            1_000_120,
            "cursor-grok-4.5-high",
            &events,
            60
        ));
    }

    #[test]
    fn test_is_likely_local_different_family() {
        let events = vec![LocalHookEvent {
            ts_epoch: 1_000_000,
            model: "grok-4.5".into(),
            family: "grok-4.5".into(),
            hook_event_name: "beforeSubmitPrompt".into(),
        }];
        assert!(!is_likely_local(
            1_000_000,
            "gpt-5.6-terra-medium",
            &events,
            60
        ));
    }

    #[test]
    fn test_is_likely_local_empty_events_fail_open() {
        assert!(is_likely_local(1, "grok-4.5", &[], 60));
        assert_eq!(explain_filter_reason(1, "grok-4.5", &[], 60), None);
    }

    #[test]
    fn test_explain_filter_reason_model() {
        let events = vec![LocalHookEvent {
            ts_epoch: 1_000_000,
            model: "grok-4.5".into(),
            family: "grok-4.5".into(),
            hook_event_name: "beforeSubmitPrompt".into(),
        }];
        assert_eq!(
            explain_filter_reason(1_000_000, "gpt-5.6-terra-medium", &events, 60),
            Some(FilterReason::Model)
        );
    }

    #[test]
    fn test_explain_filter_reason_time() {
        let events = vec![LocalHookEvent {
            ts_epoch: 1_000_000,
            model: "grok-4.5".into(),
            family: "grok-4.5".into(),
            hook_event_name: "beforeSubmitPrompt".into(),
        }];
        assert_eq!(
            explain_filter_reason(1_000_120, "cursor-grok-4.5-high", &events, 60),
            Some(FilterReason::Time)
        );
    }

    #[test]
    fn test_explain_filter_reason_none() {
        let events = vec![LocalHookEvent {
            ts_epoch: 1_000_000,
            model: "grok-4.5".into(),
            family: "grok-4.5".into(),
            hook_event_name: "beforeSubmitPrompt".into(),
        }];
        assert_eq!(
            explain_filter_reason(2_000_000, "gpt-5.6-terra-medium", &events, 60),
            Some(FilterReason::None)
        );
    }

    #[test]
    fn test_explain_filter_reason_model_over_time() {
        // 窗内错模型 + 窗外同家族 → 优先模型不对
        let events = vec![
            LocalHookEvent {
                ts_epoch: 1_000_000,
                model: "gpt".into(),
                family: "gpt-5.6-terra".into(),
                hook_event_name: "beforeSubmitPrompt".into(),
            },
            LocalHookEvent {
                ts_epoch: 1_000_200,
                model: "grok".into(),
                family: "grok-4.5".into(),
                hook_event_name: "stop".into(),
            },
        ];
        assert_eq!(
            explain_filter_reason(1_000_000, "cursor-grok-4.5-high", &events, 60),
            Some(FilterReason::Model)
        );
    }

    #[test]
    fn test_explain_filter_reason_kept() {
        let events = vec![LocalHookEvent {
            ts_epoch: 1_000_000,
            model: "grok-4.5".into(),
            family: "grok-4.5".into(),
            hook_event_name: "beforeSubmitPrompt".into(),
        }];
        assert_eq!(
            explain_filter_reason(1_000_030, "cursor-grok-4.5-high", &events, 60),
            None
        );
    }

    #[test]
    fn test_row_key_stable() {
        let a = row_key(100, "grok-4.5", 1, 2, 3, 4);
        let b = row_key(100, "grok-4.5", 1, 2, 3, 4);
        let c = row_key(100, "grok-4.5", 1, 2, 3, 5);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 32);
    }

    #[test]
    fn test_resolve_effective_override() {
        let algo = Some(FilterReason::Time);
        let keep = resolve_effective(algo, Some(OverrideAction::Keep));
        assert!(!keep.filtered);
        assert_eq!(keep.reason, algo);
        assert_eq!(keep.override_action, Some(OverrideAction::Keep));

        let filter = resolve_effective(None, Some(OverrideAction::Filter));
        assert!(filter.filtered);
        assert_eq!(filter.reason, Some(FilterReason::None));
        assert_eq!(filter.override_action, Some(OverrideAction::Filter));

        let plain = resolve_effective(algo, None);
        assert!(plain.filtered);
        assert_eq!(plain.override_action, None);
    }

    #[test]
    fn test_overrides_gc_and_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut map = HashMap::new();
        upsert_override(
            dir.path(),
            &mut map,
            "aaa",
            OverrideAction::Keep,
            1,
            "m1",
        )
        .unwrap();
        upsert_override(
            dir.path(),
            &mut map,
            "bbb",
            OverrideAction::Filter,
            2,
            "m2",
        )
        .unwrap();
        assert_eq!(map.len(), 2);

        let mut live = HashSet::new();
        live.insert("aaa".to_string());
        let removed = gc_overrides(dir.path(), &mut map, &live).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("aaa"));

        let loaded = load_overrides(dir.path()).unwrap();
        assert_eq!(loaded.len(), 1);

        delete_override(dir.path(), &mut map, "aaa").unwrap();
        assert!(map.is_empty());
        let loaded2 = load_overrides(dir.path()).unwrap();
        assert!(loaded2.is_empty());
    }
}
