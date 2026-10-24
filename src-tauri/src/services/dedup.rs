use std::collections::{HashMap, HashSet};

use crate::models::RawRecord;
use crate::services::data_source::StreamingRecord;

/// 对 RawRecord 做 RequestFingerprint 去重，先到先保留。
/// codex 记录使用归一化指纹（不含 model，input 在 DB 读取时已减去 cache_read）。
pub fn dedup_records(records: Vec<RawRecord>) -> Vec<RawRecord> {
    let mut seen = HashSet::new();
    records.into_iter().filter(|r| {
        let fp = if r.is_codex {
            // input_tokens 已在 ExternalDb 中归一化（raw - cache_read），直接使用
            RequestFingerprint::new_codex(&r.session_id, r.input_tokens, r.output_tokens, r.cache_creation)
        } else {
            RequestFingerprint::new(&r.session_id, &r.model, r.input_tokens, r.output_tokens, r.cache_creation)
        };
        seen.insert(fp)
    }).collect()
}

/// 请求级去重缓存。
/// 维护跨刷新周期的去重状态，支持增量合并。
pub struct RequestCache {
    records: Vec<crate::models::SessionRequestToken>,
    seen: HashSet<RequestFingerprint>,
    max_size: usize,
}

impl RequestCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            records: Vec::new(),
            seen: HashSet::new(),
            max_size,
        }
    }

    pub fn records(&self) -> &[crate::models::SessionRequestToken] {
        &self.records
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 增量合并：插入新记录，跳过已有的 fingerprint。
    /// 返回实际新增的记录数（淘汰前）。
    pub fn merge(&mut self, new_records: Vec<crate::models::SessionRequestToken>) -> usize {
        let mut added = 0;
        for record in new_records {
            let fp = RequestFingerprint::new(&record.session_id, &record.model, record.input_tokens, record.output_tokens, record.cache_creation);
            if self.seen.insert(fp) {
                self.records.push(record);
                added += 1;
            }
        }
        // 按 created_at 降序排序，保留最新的 max_size 条
        if self.records.len() > self.max_size {
            self.records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            for r in &self.records[self.max_size..] {
                self.seen.remove(&RequestFingerprint::new(&r.session_id, &r.model, r.input_tokens, r.output_tokens, r.cache_creation));
            }
            self.records.truncate(self.max_size);
        }
        added
    }

    pub fn clear(&mut self) {
        self.records.clear();
        self.seen.clear();
    }
}

/// 请求指纹 — 用于跨数据源去重
/// codex: session_id + output_tokens + cache_creation_tokens（model 不参与，input 归一化）
/// 非codex: session_id + model + input_tokens + output_tokens + cache_creation_tokens
/// cache_creation_tokens 参与指纹：相同 input/output 但缓存状态不同的记录视为不同请求，
/// 避免缓存命中（高 cache_read）和缓存重建（高 cache_creation）的情况被误判为重复。
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub struct RequestFingerprint {
    session_id: String,
    model: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    is_codex: bool,
}

impl RequestFingerprint {
    /// 非codex 指纹（is_codex = false）
    pub fn new(session_id: &str, model: &str, input_tokens: i64, output_tokens: i64, cache_creation_tokens: i64) -> Self {
        Self {
            session_id: session_id.to_string(),
            model: model.to_string(),
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            is_codex: false,
        }
    }

    /// codex 指纹：不用 model，input 已归一化（不含 cache_read）
    pub fn new_codex(session_id: &str, normalized_input: i64, output_tokens: i64, cache_creation_tokens: i64) -> Self {
        Self {
            session_id: session_id.to_string(),
            model: String::new(),
            input_tokens: normalized_input,
            output_tokens,
            cache_creation_tokens,
            is_codex: true,
        }
    }
}

/// 对请求级数据做跨源去重。
/// 返回去重后的 Vec，保留先到达的记录（先到先保留）。
pub fn dedup_request_tokens(items: Vec<crate::models::SessionRequestToken>) -> Vec<crate::models::SessionRequestToken> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| {
            let fp = RequestFingerprint::new(&item.session_id, &item.model, item.input_tokens, item.output_tokens, item.cache_creation);
            seen.insert(fp)
        })
        .collect()
}

// ========== 跨源优先级去重（去重键抽象） ==========

/// 第一优先级：端侧直录源（harness 会话日志 / 客户端本地库）。
pub const CROSS_SOURCE_TIER1: &[&str] = &[
    "DSH", "Cursor", "OpenCode", "ZCode", "MiniMax", "Proma", "Antigravity", "Kimi", "PI",
];

/// 第二优先级：统一网关转发账本（同一请求已被端侧记录时仅作备份口径）。
pub const CROSS_SOURCE_TIER2: &[&str] = &["AIProxy", "Magpie", "CCS"];

/// 匹配窗口（秒）：网关记录带耗时（latency>0）时用 请求开始+耗时 对齐端侧完成时刻；
/// 无耗时字段时退化为创建时刻直比，窗口放宽。
const CROSS_SOURCE_WINDOW_ACTIVE_SECS: i64 = 10;
const CROSS_SOURCE_WINDOW_FALLBACK_SECS: i64 = 30;

/// 数据源跨源去重层级：1 = 端侧直录（优先保留），2 = 统一网关（可被覆盖），0 = 不参与。
pub fn cross_source_tier(source_label: &str) -> u8 {
    if CROSS_SOURCE_TIER1.contains(&source_label) {
        1
    } else if CROSS_SOURCE_TIER2.contains(&source_label) {
        2
    } else {
        0
    }
}

/// 单条记录可提供的判重键。键分强弱：强键相等即判定，弱键需时间窗佐证。
///
/// 扩展模型：数据源只负责「供给键」（一个或多个），匹配器只负责「键之间比较」。
/// 新源接入或某源新增键类型（如上游 response id），只改供给侧，匹配核心不动。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DedupKey {
    /// 强键：请求唯一标识（应用库 request_id 主键，或跨源可比的真实请求 ID）。
    /// 双方均存在且相等 ⇒ 直接判定同一请求（ID 已判断过，跳过 Token 环节）；
    /// 不相等或任一侧缺失 ⇒ 忽略本键，落入弱键判定。
    /// 当前各源主键体系互不相干（DSH=`dsh:<消息id>`、Magpie=行内容哈希），
    /// 跨源不会误配；同源重复行则被正确吸收。将来两个源共享真实请求 ID 时自动生效。
    RequestId(String),
    /// 弱键：四类 Token 指纹 + 时间锚窗（原跨源配对规则原样收编）。
    /// Token 全等且时间对齐（anchor±window）⇒ 判定同一请求。
    TokenWindow {
        tokens: [i64; 4],
        anchor: i64,
        window_secs: i64,
    },
}

/// 记录自带的判重证据：数据源层级 + 一个或多个键。
#[derive(Clone, Debug, Default)]
pub struct DedupEvidence {
    /// 数据源跨源去重层级（`cross_source_tier`）：1=端侧、2=网关、0=不参与。
    pub tier: u8,
    /// 该记录可提供的全部键；供给越多，可配对途径越多，互不冲突。
    pub keys: Vec<DedupKey>,
}

impl DedupEvidence {
    pub fn new(tier: u8) -> Self {
        Self { tier, keys: Vec::new() }
    }

    pub fn with_key(mut self, key: DedupKey) -> Self {
        self.keys.push(key);
        self
    }
}

/// 去重键泛型匹配器（跨源优先级判定的核心，供给与判定解耦）。
///
/// 同一真实请求被端侧源（tier 1）与网关源（tier 2）同时记录时，保留 tier 1、
/// 丢弃能配对的 tier 2；未能配对的 tier 2（端侧未覆盖的流量）照常保留。
///
/// 按键强度分阶段配对（宁可漏去重、不误杀）：
/// ① 强键：tier1 侧 RequestId 建索引，tier2 命中即吸收（一对一占用）；
/// ② 弱键：tier1 按 anchor 排序，tier2 TokenWindow 命中（Token 全等 + 窗口内）即吸收。
/// tier 1 内部、tier 2 内部及未知层级（0）互不匹配。
pub fn dedup_by_keys<T>(items: Vec<T>, evidence_of: impl Fn(&T) -> DedupEvidence) -> Vec<T> {
    dedup_by_keys_with_removed(items, evidence_of).0
}

/// `dedup_by_keys` 的报告变体：额外返回被吸收（移除）的完整记录，
/// 供实时链路把「本轮被端侧吸收的网关行」下发给前端移除已显示行。
pub fn dedup_by_keys_with_removed<T>(
    items: Vec<T>,
    evidence_of: impl Fn(&T) -> DedupEvidence,
) -> (Vec<T>, Vec<T>) {
    let evidences: Vec<DedupEvidence> = items.iter().map(&evidence_of).collect();

    // 快速路径：没有网关层记录时无需任何配对
    if !evidences.iter().any(|e| e.tier == 2) {
        return (items, Vec::new());
    }

    // 占用与移除标记按 items 索引；一个 tier1 记录至多吸收一条 tier2（一对一）
    let mut removed = vec![false; items.len()];
    {
        let mut tier1_used = vec![false; items.len()];
        // 阶段一索引：强键 → tier1 记录下标列表
        let mut tier1_ids: HashMap<&str, Vec<usize>> = HashMap::new();
        // 阶段二索引：(anchor, 记录下标)，供窗口二分
        let mut tier1_weak: Vec<(i64, usize)> = Vec::new();
        for (i, e) in evidences.iter().enumerate() {
            if e.tier != 1 {
                continue;
            }
            for key in &e.keys {
                match key {
                    DedupKey::RequestId(id) => {
                        tier1_ids.entry(id.as_str()).or_default().push(i);
                    }
                    DedupKey::TokenWindow { anchor, .. } => {
                        tier1_weak.push((*anchor, i));
                    }
                }
            }
        }
        tier1_weak.sort_unstable();
        let weak_anchors: Vec<i64> = tier1_weak.iter().map(|(t, _)| *t).collect();

        for (i, e) in evidences.iter().enumerate() {
            if e.tier != 2 || removed[i] {
                continue;
            }
            let mut matched = false;
            for key in &e.keys {
                match key {
                    DedupKey::RequestId(id) => {
                        if let Some(cands) = tier1_ids.get(id.as_str()) {
                            for &k in cands {
                                if !tier1_used[k] {
                                    tier1_used[k] = true;
                                    matched = true;
                                    break;
                                }
                            }
                        }
                    }
                    DedupKey::TokenWindow { tokens, anchor, window_secs } => {
                        let lo = weak_anchors.partition_point(|&t| t < anchor - window_secs);
                        let hi = weak_anchors.partition_point(|&t| t <= anchor + window_secs);
                        for w in lo..hi {
                            let k = tier1_weak[w].1;
                            if tier1_used[k] {
                                continue;
                            }
                            let token_hit = evidences[k].keys.iter().any(|x| match x {
                                DedupKey::TokenWindow { tokens: t1, .. } => t1 == tokens,
                                _ => false,
                            });
                            if token_hit {
                                tier1_used[k] = true;
                                matched = true;
                                break;
                            }
                        }
                    }
                }
                if matched {
                    removed[i] = true;
                    break;
                }
            }
        }
    }

    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for (i, x) in items.into_iter().enumerate() {
        if removed[i] {
            dropped.push(x);
        } else {
            kept.push(x);
        }
    }
    (kept, dropped)
}

/// tier2 弱键的锚点与窗口：网关记录带耗时（latency>0）时锚 = 创建时刻+耗时
/// （完成时刻），窗口 ±10s；无耗时时退化为创建时刻直比，窗口 ±30s。
/// tier1（端侧）记录的 created_at 本身就是完成时刻，锚恒为裸 created_at——
/// 两侧比较的是「完成时刻对齐」，tier1 不得再加自己的 latency，否则比较式
/// 退化为 |d.created - m.created| ≈ 请求耗时，长请求（>10s）全部漏配。
fn cross_source_anchor_window(created_at: i64, latency_ms: i64) -> (i64, i64) {
    if latency_ms > 0 {
        (created_at + latency_ms / 1000, CROSS_SOURCE_WINDOW_ACTIVE_SECS)
    } else {
        (created_at, CROSS_SOURCE_WINDOW_FALLBACK_SECS)
    }
}

/// RawRecord 版跨源优先级去重（db_type 为空时按 provider_id 兜底判定层级）。
pub fn cross_source_priority_dedup_records(records: Vec<RawRecord>) -> Vec<RawRecord> {
    dedup_by_keys(records, |r| {
        let label = if r.db_type.is_empty() { r.provider_id.as_str() } else { r.db_type.as_str() };
        let tier = cross_source_tier(label);
        let (anchor, window) = if tier == 2 {
            cross_source_anchor_window(r.created_at, r.latency)
        } else {
            (r.created_at, 0)
        };
        let mut ev = DedupEvidence::new(tier).with_key(DedupKey::TokenWindow {
            tokens: [r.input_tokens, r.output_tokens, r.cache_read, r.cache_creation],
            anchor,
            window_secs: window,
        });
        if !r.request_id.is_empty() {
            ev = ev.with_key(DedupKey::RequestId(r.request_id.clone()));
        }
        ev
    })
}

/// StreamingRecord 的判重证据构造（实时流无 db_type，经 provider→数据源映射判定层级）。
/// 供 `cross_source_priority_dedup_streaming` 与 pipeline 的吸收报告路径共用。
/// tier2 锚 = 创建+耗时（完成时刻）；tier1 锚 = 裸 created_at（见
/// `cross_source_anchor_window` 的对齐语义说明）。
pub fn streaming_evidence(
    r: &StreamingRecord,
    provider_to_db: &HashMap<String, String>,
) -> DedupEvidence {
    let label = provider_to_db
        .get(&r.2)
        .map(|s| s.as_str())
        .unwrap_or(r.2.as_str());
    let tier = cross_source_tier(label);
    let (anchor, window) = if tier == 2 {
        cross_source_anchor_window(r.3, r.8)
    } else {
        (r.3, 0)
    };
    let mut ev = DedupEvidence::new(tier).with_key(DedupKey::TokenWindow {
        tokens: [r.4, r.5, r.6, r.7],
        anchor,
        window_secs: window,
    });
    if !r.11.is_empty() {
        ev = ev.with_key(DedupKey::RequestId(r.11.clone()));
    }
    ev
}

/// StreamingRecord 版跨源优先级去重（实时流无 db_type，经 provider→数据源映射判定层级）。
pub fn cross_source_priority_dedup_streaming(
    records: Vec<StreamingRecord>,
    provider_to_db: &HashMap<String, String>,
) -> Vec<StreamingRecord> {
    dedup_by_keys(records, |r| streaming_evidence(r, provider_to_db))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(session_id: &str, model: &str, created_at: i64, input: i64, output: i64) -> crate::models::SessionRequestToken {
        crate::models::SessionRequestToken {
            session_id: session_id.to_string(),
            model: model.to_string(),
            provider_id: "p1".to_string(),
            created_at,
            input_tokens: input,
            output_tokens: output,
            cache_read: 0,
            cache_creation: 0,
        }
    }

    // ========== RequestFingerprint ==========

    #[test]
    fn fingerprint_equality() {
        let a = RequestFingerprint::new("s1", "m1", 100, 200, 0);
        let b = RequestFingerprint::new("s1", "m1", 100, 200, 0);
        assert_eq!(a, b);
    }

    #[test]
    fn fingerprint_inequality_different_session() {
        let a = RequestFingerprint::new("s1", "m1", 100, 200, 0);
        let b = RequestFingerprint::new("s2", "m1", 100, 200, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn fingerprint_inequality_different_tokens() {
        let a = RequestFingerprint::new("s1", "m1", 100, 200, 0);
        let b = RequestFingerprint::new("s1", "m1", 100, 300, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn fingerprint_does_not_include_timestamp() {
        // 指纹仅由 session_id + model + input/output tokens 组成，
        // 不含 created_at（不同源时间语义不同，差值可达数分钟）
        let a = RequestFingerprint::new("s1", "m1", 100, 200, 0);
        let b = RequestFingerprint::new("s1", "m1", 100, 200, 0);
        assert_eq!(a, b);
    }

    // ========== dedup_request_tokens ==========

    #[test]
    fn dedup_empty() {
        let result = dedup_request_tokens(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn dedup_no_duplicates() {
        let items = vec![
            token("s1", "m1", 1000, 100, 200),
            token("s1", "m2", 1001, 100, 200),
            token("s2", "m1", 1002, 100, 200),
        ];
        let result = dedup_request_tokens(items);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn dedup_removes_duplicates() {
        let items = vec![
            token("s1", "m1", 1000, 100, 200),
            token("s1", "m1", 1000, 100, 200), // 重复
            token("s1", "m1", 1000, 100, 200), // 重复
        ];
        let result = dedup_request_tokens(items);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn dedup_first_wins() {
        // 第一条记录的 cache_read 应被保留，后续重复的被丢弃
        let mut first = token("s1", "m1", 1000, 100, 200);
        first.cache_read = 42;
        let mut second = token("s1", "m1", 1000, 100, 200);
        second.cache_read = 99;
        let result = dedup_request_tokens(vec![first, second]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].cache_read, 42); // 先到先保留
    }

    #[test]
    fn dedup_same_session_different_model() {
        let items = vec![
            token("s1", "claude-3", 1000, 100, 200),
            token("s1", "claude-4", 1001, 100, 200),
        ];
        let result = dedup_request_tokens(items);
        assert_eq!(result.len(), 2); // 不同 model，都保留
    }

    #[test]
    fn dedup_same_session_same_model_different_tokens() {
        let items = vec![
            token("s1", "m1", 1000, 100, 200),
            token("s1", "m1", 1001, 150, 300), // 不同 token 数，视为不同请求
        ];
        let result = dedup_request_tokens(items);
        assert_eq!(result.len(), 2);
    }

    // ========== RequestCache ==========

    #[test]
    fn cache_new_empty() {
        let cache = RequestCache::new(100);
        assert_eq!(cache.len(), 0);
        assert!(cache.records().is_empty());
    }

    #[test]
    fn cache_merge_adds_records() {
        let mut cache = RequestCache::new(100);
        let added = cache.merge(vec![
            token("s1", "m1", 1000, 100, 200),
            token("s2", "m1", 1001, 300, 400),
        ]);
        assert_eq!(added, 2);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn cache_merge_deduplicates() {
        let mut cache = RequestCache::new(100);
        cache.merge(vec![token("s1", "m1", 1000, 100, 200)]);
        let added = cache.merge(vec![token("s1", "m1", 1000, 100, 200)]); // 重复
        assert_eq!(added, 0);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn cache_merge_incremental() {
        let mut cache = RequestCache::new(100);
        cache.merge(vec![
            token("s1", "m1", 1000, 100, 200),
            token("s2", "m1", 1001, 300, 400),
        ]);
        // 增量合并：s1 重复（跳过），s3 新增
        let added = cache.merge(vec![
            token("s1", "m1", 1000, 100, 200), // 已存在
            token("s3", "m1", 1002, 500, 600), // 新增
        ]);
        assert_eq!(added, 1);
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn cache_eviction_keeps_newest() {
        let mut cache = RequestCache::new(3);
        cache.merge(vec![
            token("s1", "m1", 1000, 100, 200),
            token("s2", "m1", 2000, 300, 400),
            token("s3", "m1", 3000, 500, 600),
        ]);
        assert_eq!(cache.len(), 3);
        // 添加第 4 条，触发淘汰
        cache.merge(vec![token("s4", "m1", 4000, 700, 800)]);
        assert_eq!(cache.len(), 3);
        // 最旧的 s1 (created_at=1000) 应被淘汰
        let session_ids: Vec<_> = cache.records().iter().map(|r| r.session_id.as_str()).collect();
        assert!(!session_ids.contains(&"s1"));
        assert!(session_ids.contains(&"s4"));
    }

    #[test]
    fn cache_eviction_cleans_seen_set() {
        let mut cache = RequestCache::new(2);
        cache.merge(vec![
            token("s1", "m1", 1000, 100, 200),
            token("s2", "m1", 2000, 300, 400),
        ]);
        // 触发淘汰 s1
        let added = cache.merge(vec![token("s3", "m1", 3000, 500, 600)]);
        assert_eq!(added, 1);
        assert_eq!(cache.len(), 2);
        // s1 被淘汰后，其 fingerprint 应从 seen 中移除，允许重新插入
        let added = cache.merge(vec![token("s1", "m1", 1000, 100, 200)]);
        assert_eq!(added, 1);
    }

    #[test]
    fn cache_clear() {
        let mut cache = RequestCache::new(100);
        cache.merge(vec![
            token("s1", "m1", 1000, 100, 200),
            token("s2", "m1", 2001, 300, 400),
        ]);
        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.records().is_empty());
        // clear 后可以重新插入相同数据
        let added = cache.merge(vec![token("s1", "m1", 1000, 100, 200)]);
        assert_eq!(added, 1);
    }

    // ========== codex 去重 ==========

    fn raw_record(session_id: &str, model: &str, provider_id: &str, input: i64, output: i64, cache_read: i64, is_codex: bool) -> RawRecord {
        RawRecord {
            session_id: session_id.to_string(),
            model: model.to_string(),
            provider_id: provider_id.to_string(),
            request_id: String::new(),
            db_type: String::new(),
            created_at: 0,
            input_tokens: input,
            output_tokens: output,
            cache_read,
            cache_creation: 0,
            latency: 0,
            is_codex,
        }
    }

    #[test]
    fn codex_cross_source_dedup() {
        // CCS: DB 读取时 input 已归一化为 raw-cache_read=27505-26624=881
        // AI Proxy: input=881（不含 cache_read）
        // 两边指纹应相同
        let ccs = raw_record("s1", "gpt-5.4-mini", "_codex_session", 881, 16857, 26624, true);
        let aiproxy = raw_record("s1", "deepseek-v4-flash", "AIProxy", 881, 16857, 26624, true);

        let result = dedup_records(vec![aiproxy, ccs]);
        assert_eq!(result.len(), 1);
        // 先到先保留：AI Proxy 在前，保留 target_model
        assert_eq!(result[0].model, "deepseek-v4-flash");
        assert_eq!(result[0].provider_id, "AIProxy");
    }

    #[test]
    fn codex_ccs_only_preserved() {
        // 只有 CCS codex 记录，无匹配，应保留
        let ccs = raw_record("s1", "gpt-5.3-codex", "_codex_session", 15468, 7923, 13952, true);
        let result = dedup_records(vec![ccs]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].model, "gpt-5.3-codex");
    }

    #[test]
    fn codex_and_noncodex_never_cross() {
        // codex 和非 codex 即使 session_id/output_tokens 相同也不应交叉
        let codex = raw_record("s1", "deepseek-v4-flash", "AIProxy", 881, 200, 0, true);
        let non_codex = raw_record("s1", "deepseek-v4-flash", "AIProxy", 881, 200, 0, false);

        let result = dedup_records(vec![codex, non_codex]);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn codex_fingerprint_ignores_model() {
        // codex 指纹不包含 model，不同 model 应去重
        let a = raw_record("s1", "model-a", "AIProxy", 100, 200, 0, true);
        let b = raw_record("s1", "model-b", "AIProxy", 100, 200, 0, true);

        let result = dedup_records(vec![a, b]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn codex_ai_proxy_priority() {
        // AI Proxy 排在前面应被优先保留（由调用方排序保证）
        // CCS: DB 读取时 input 已归一化为 1000-900=100
        let ccs = raw_record("s1", "gpt-5.4-mini", "_codex_session", 100, 500, 900, true);
        let aiproxy = raw_record("s1", "deepseek-v4-flash", "AIProxy", 100, 500, 900, true);

        let result = dedup_records(vec![aiproxy.clone(), ccs]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].provider_id, "AIProxy");
        // AI Proxy 的 input 不含 cache，是准确值
        assert_eq!(result[0].input_tokens, 100);
    }

    #[test]
    fn codex_fingerprint_equality() {
        // 两边 input_tokens 已在 DB 读取时归一化，直接比较
        let fp_ccs = {
            let r = raw_record("s1", "gpt-5.3-codex", "_codex_session", 100, 200, 900, true);
            RequestFingerprint::new_codex(&r.session_id, r.input_tokens, r.output_tokens, r.cache_creation)
        };
        let fp_aiproxy = {
            let r = raw_record("s1", "deepseek-v4-flash", "AIProxy", 100, 200, 900, true);
            RequestFingerprint::new_codex(&r.session_id, r.input_tokens, r.output_tokens, r.cache_creation)
        };
        assert_eq!(fp_ccs, fp_aiproxy);
    }

    // ========== 跨源优先级去重 ==========

    fn tier_rec(db_type: &str, provider: &str, created_at: i64, latency: i64, input: i64, output: i64, cache_read: i64, cache_write: i64) -> RawRecord {
        RawRecord {
            session_id: "s".to_string(),
            model: "m".to_string(),
            provider_id: provider.to_string(),
            request_id: String::new(),
            db_type: db_type.to_string(),
            created_at,
            input_tokens: input,
            output_tokens: output,
            cache_read,
            cache_creation: cache_write,
            latency,
            is_codex: false,
        }
    }

    fn labels(records: &[RawRecord]) -> Vec<String> {
        records.iter().map(|r| r.db_type.clone()).collect()
    }

    #[test]
    fn cross_source_drops_gateway_record_aligned_by_start_plus_latency() {
        // 网关 t=90 请求开始，耗时 10s → anchor=100，对齐端侧完成时刻 t=100
        let dsh = tier_rec("DSH", "DSH", 100, 8000, 100, 50, 4096, 0);
        let magpie = tier_rec("Magpie", "Magpie", 90, 10_000, 100, 50, 4096, 0);
        let out = cross_source_priority_dedup_records(vec![dsh, magpie]);
        assert_eq!(labels(&out), vec!["DSH".to_string()]);
    }

    #[test]
    fn cross_source_long_request_with_slightly_different_latencies_still_pairs() {
        // 回归守卫：两侧都是长请求（耗时 15s/15.4s）且端侧 created 已是完成时刻。
        // 若 tier1 锚误加自身 latency，比较式会退化为 |d.created - m.created| ≈ 请求
        // 耗时（15s）> 10s 窗口，长请求全部漏配（曾导致首页统计双计约 18%）。
        // 网关 t=955 开始耗时 15417ms → 锚 970；端侧完成时刻 971（latency 不参与锚）。
        let dsh = tier_rec("DSH", "DSH", 971, 15_444, 22543, 483, 0, 0);
        let magpie = tier_rec("Magpie", "Magpie", 955, 15_417, 22543, 483, 0, 0);
        let out = cross_source_priority_dedup_records(vec![dsh, magpie]);
        assert_eq!(labels(&out), vec!["DSH".to_string()]);
    }

    #[test]
    fn cross_source_streaming_long_request_pairs() {
        // 流式路径同语义守卫：端侧锚用裸 created_at
        let mut map = HashMap::new();
        let dsh: StreamingRecord = ("s1".into(), "m".into(), "DSH".into(), 971, 100, 50, 0, 0, 15_444, 0, false, String::new());
        let magpie: StreamingRecord = ("s2".into(), "m".into(), "Magpie".into(), 955, 100, 50, 0, 0, 15_417, 0, false, "mag-x".into());
        let out = cross_source_priority_dedup_streaming(vec![dsh, magpie], &map);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].2, "DSH");
    }

    #[test]
    fn cross_source_fallback_window_when_gateway_has_no_latency() {
        // 无耗时时退化为创建时刻直比，窗口放宽到 ±30s
        let dsh = tier_rec("DSH", "DSH", 200, 0, 10, 2, 0, 0);
        let ccs = tier_rec("CCS", "uuid-1", 225, 0, 10, 2, 0, 0);
        let out = cross_source_priority_dedup_records(vec![dsh, ccs]);
        assert_eq!(labels(&out), vec!["DSH".to_string()]);

        // 超出 ±30s：保留两侧
        let dsh = tier_rec("DSH", "DSH", 200, 0, 10, 2, 0, 0);
        let ccs = tier_rec("CCS", "uuid-1", 231, 0, 10, 2, 0, 0);
        let out = cross_source_priority_dedup_records(vec![dsh, ccs]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn cross_source_keeps_records_when_tokens_differ() {
        // token 不一致（宁可漏去重不误杀）
        let dsh = tier_rec("DSH", "DSH", 100, 0, 100, 50, 4096, 0);
        let magpie = tier_rec("Magpie", "Magpie", 100, 0, 100, 51, 4096, 0);
        let out = cross_source_priority_dedup_records(vec![dsh, magpie]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn cross_source_keeps_gateway_when_no_tier1_match() {
        // 端侧未覆盖的网关流量照常保留
        let magpie = tier_rec("Magpie", "Magpie", 100, 5000, 10, 2, 0, 0);
        let dsh = tier_rec("DSH", "DSH", 500, 0, 99, 99, 0, 0);
        let out = cross_source_priority_dedup_records(vec![dsh, magpie]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn cross_source_pairs_one_to_one() {
        // 两条网关记录、一条端侧记录：只吸收一条，另一条保留（防 1 对多双杀）
        let dsh = tier_rec("DSH", "DSH", 100, 0, 10, 2, 0, 0);
        let g1 = tier_rec("Magpie", "Magpie", 95, 5000, 10, 2, 0, 0);
        let g2 = tier_rec("Magpie", "Magpie", 96, 5000, 10, 2, 0, 0);
        let out = cross_source_priority_dedup_records(vec![dsh, g1, g2]);
        assert_eq!(out.len(), 2);
        assert_eq!(labels(&out), vec!["DSH".to_string(), "Magpie".to_string()]);

        // 两条端侧、两条网关：全部配对
        let d1 = tier_rec("DSH", "DSH", 100, 0, 10, 2, 0, 0);
        let d2 = tier_rec("DSH", "DSH", 200, 0, 10, 2, 0, 0);
        let g1 = tier_rec("Magpie", "Magpie", 95, 5000, 10, 2, 0, 0);
        let g2 = tier_rec("Magpie", "Magpie", 195, 5000, 10, 2, 0, 0);
        let out = cross_source_priority_dedup_records(vec![d1, d2, g1, g2]);
        assert_eq!(labels(&out), vec!["DSH".to_string(), "DSH".to_string()]);
    }

    #[test]
    fn cross_source_same_tier_never_matches() {
        // 网关层内部（Magpie vs AIProxy）互不去重，交给既有指纹逻辑
        let magpie = tier_rec("Magpie", "Magpie", 100, 0, 10, 2, 0, 0);
        let aiproxy = tier_rec("AIProxy", "AIProxy", 100, 0, 10, 2, 0, 0);
        let out = cross_source_priority_dedup_records(vec![aiproxy, magpie]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn cross_source_unknown_source_not_involved() {
        // 未知层级（db_type 与 provider_id 都不在两级清单）不参与配对
        let unknown = tier_rec("Foo", "Foo", 100, 0, 10, 2, 0, 0);
        let magpie = tier_rec("Magpie", "Magpie", 100, 0, 10, 2, 0, 0);
        let out = cross_source_priority_dedup_records(vec![unknown, magpie]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn cross_source_db_type_falls_back_to_provider_id() {
        // 旧路径 db_type 为空时按 provider_id 判定层级
        let dsh = tier_rec("", "DSH", 100, 0, 10, 2, 0, 0);
        let magpie = tier_rec("", "Magpie", 100, 0, 10, 2, 0, 0);
        let out = cross_source_priority_dedup_records(vec![dsh, magpie]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].provider_id, "DSH");
    }

    #[test]
    fn cross_source_fast_path_when_no_gateway_records() {
        let d1 = tier_rec("DSH", "DSH", 100, 0, 10, 2, 0, 0);
        let d2 = tier_rec("OpenCode", "opencode", 200, 0, 30, 5, 0, 0);
        let out = cross_source_priority_dedup_records(vec![d1, d2]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn cross_source_streaming_uses_provider_to_db_map() {
        // 实时流无 db_type：CCS 的动态 provider id 经映射归并到 "CCS"（tier 2）
        let mut map = HashMap::new();
        map.insert("uuid-ccs".to_string(), "CCS".to_string());
        let dsh: StreamingRecord = ("s1".into(), "m".into(), "DSH".into(), 100, 10, 2, 0, 0, 8000, 500, false, String::new());
        let ccs: StreamingRecord = ("s2".into(), "m2".into(), "uuid-ccs".into(), 92, 10, 2, 0, 0, 8000, 500, false, String::new());
        let out = cross_source_priority_dedup_streaming(vec![dsh, ccs], &map);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].2, "DSH");
    }

    // ========== 去重键（DedupKey） ==========

    fn tier_rec_id(db_type: &str, provider: &str, created_at: i64, latency: i64, input: i64, output: i64, request_id: &str) -> RawRecord {
        let mut r = tier_rec(db_type, provider, created_at, latency, input, output, 0, 0);
        r.request_id = request_id.to_string();
        r
    }

    #[test]
    fn strong_key_matches_across_tiers_even_when_tokens_differ() {
        // 强键（request_id）相等 ⇒ 直接判定同一请求，无需 Token/时间佐证；
        // token 不同（如一侧落库后被回填更新）不影响强判定。
        let dsh = tier_rec_id("DSH", "DSH", 100, 0, 10, 2, "req-1");
        let mut magpie = tier_rec_id("Magpie", "Magpie", 5000, 0, 999, 999, "req-1");
        magpie.cache_read = 777;
        let out = cross_source_priority_dedup_records(vec![dsh, magpie]);
        assert_eq!(labels(&out), vec!["DSH".to_string()]);
    }

    #[test]
    fn different_ids_fall_back_to_weak_token_key() {
        // ID 不一致 ⇒ 忽略强键，落入弱键（token + 时间窗）判定——现有行为保持
        let dsh = tier_rec_id("DSH", "DSH", 100, 0, 10, 2, "a");
        let magpie = tier_rec_id("Magpie", "Magpie", 100, 0, 10, 2, "b");
        let out = cross_source_priority_dedup_records(vec![dsh, magpie]);
        assert_eq!(labels(&out), vec!["DSH".to_string()]);

        // 弱键也不满足（token 不同）时保留双方
        let dsh = tier_rec_id("DSH", "DSH", 100, 0, 10, 2, "a");
        let magpie = tier_rec_id("Magpie", "Magpie", 100, 0, 10, 3, "b");
        let out = cross_source_priority_dedup_records(vec![dsh, magpie]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn strong_key_never_crosses_within_same_tier() {
        // 同 tier 记录即使 request_id 相同也互不吸收（层级内不匹配）
        let a = tier_rec_id("DSH", "DSH", 100, 0, 10, 2, "same");
        let b = tier_rec_id("OpenCode", "oc", 200, 0, 10, 2, "same");
        let out = cross_source_priority_dedup_records(vec![a, b]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn streaming_strong_key_via_new_tuple_column() {
        // 实时流第 12 列（request_id）参与强键配对
        let mut map = HashMap::new();
        map.insert("uuid-ccs".to_string(), "CCS".to_string());
        let dsh: StreamingRecord = ("s1".into(), "m".into(), "DSH".into(), 100, 10, 2, 0, 0, 0, 0, false, "rid-9".into());
        let ccs: StreamingRecord = ("s2".into(), "m2".into(), "uuid-ccs".into(), 5000, 888, 888, 0, 0, 0, 0, false, "rid-9".into());
        let out = cross_source_priority_dedup_streaming(vec![dsh, ccs], &map);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].2, "DSH");
    }
}
