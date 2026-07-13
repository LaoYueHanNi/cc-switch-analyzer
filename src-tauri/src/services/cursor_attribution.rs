//! Cursor CSV 本机归因：分钟±1 + 模型家族匹配

use serde::{Deserialize, Serialize};

use chrono::{FixedOffset, TimeZone};

pub const ATTRIBUTION_SETTING_KEY: &str = "cursor_local_attribution_enabled";
pub const ATTRIBUTION_SLACK_SECS: i64 = 60;

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
    const SUFFIXES: &[&str] = &[
        "-thinking-high",
        "-thinking-medium",
        "-thinking-low",
        "-fast-high",
        "-fast",
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
/// `events` 应已筛过 beforeSubmitPrompt/stop，且带非空 model。
pub fn is_likely_local(csv_ts: i64, csv_model: &str, events: &[LocalHookEvent], slack_secs: i64) -> bool {
    if events.is_empty() {
        return true; // fail-open：由调用方在「无事件」时跳过过滤
    }
    let fam = normalize_model_family(csv_model);
    if fam.is_empty() {
        return false;
    }
    events.iter().any(|e| {
        !e.family.is_empty()
            && e.family == fam
            && (e.ts_epoch - csv_ts).abs() <= slack_secs
    })
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
        assert_eq!(normalize_model_family("composer-2.5-fast"), "composer-2.5");
        assert_eq!(normalize_model_family("glm-5.2-max"), "glm-5.2");
        assert_eq!(normalize_model_family("gpt-5.6-terra-medium"), "gpt-5.6-terra");
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
    }
}
