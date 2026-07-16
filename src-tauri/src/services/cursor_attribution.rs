//! Cursor CSV 本机归因：分钟±5 + 模型家族匹配

use serde::{Deserialize, Serialize};

use chrono::{FixedOffset, TimeZone};

pub const ATTRIBUTION_SETTING_KEY: &str = "cursor_local_attribution_enabled";
pub const ATTRIBUTION_SLACK_SECS: i64 = 300;

/// 本机归因过滤起始时刻：2026-07-13 15:30:00（东八区）。
/// CSV 的 `created_at` 为 Unix 秒（UTC）；此之前的行不过滤，保留账号全量。
pub fn attribution_filter_start_epoch() -> i64 {
    FixedOffset::east_opt(8 * 3600)
        .expect("UTC+8")
        .with_ymd_and_hms(2026, 7, 13, 15, 30, 0)
        .single()
        .expect("valid cutoff datetime")
        .timestamp()
}

/// 该 CSV 时间戳是否需要做本机归因过滤。
pub fn should_apply_attribution_for_ts(csv_ts: i64) -> bool {
    csv_ts >= attribution_filter_start_epoch()
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
        let start = attribution_filter_start_epoch();
        assert_eq!(start, 1_783_927_800);
        let utc = chrono::DateTime::from_timestamp(start, 0).unwrap();
        assert_eq!(utc.format("%Y-%m-%d %H:%M:%S").to_string(), "2026-07-13 07:30:00");

        // 东八区 15:29:59 → 不过滤；15:30:00 → 过滤
        assert!(!should_apply_attribution_for_ts(start - 1));
        assert!(should_apply_attribution_for_ts(start));
        assert!(should_apply_attribution_for_ts(start + 1));
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
}
