use std::collections::HashSet;

use crate::models::RawRecord;

/// 对 RawRecord 做 RequestFingerprint 去重，先到先保留。
/// codex 记录使用归一化指纹（不含 model，input 减去 cache_read）。
pub fn dedup_records(records: Vec<RawRecord>) -> Vec<RawRecord> {
    let mut seen = HashSet::new();
    records.into_iter().filter(|r| {
        let fp = if r.is_codex {
            let normalized_input = if r.provider_id == "ai-proxy" {
                r.input_tokens
            } else {
                r.input_tokens.saturating_sub(r.cache_read)
            };
            RequestFingerprint::new_codex(&r.session_id, normalized_input, r.output_tokens)
        } else {
            RequestFingerprint::new(&r.session_id, &r.model, r.input_tokens, r.output_tokens)
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
            let fp = RequestFingerprint::new(&record.session_id, &record.model, record.input_tokens, record.output_tokens);
            if self.seen.insert(fp) {
                self.records.push(record);
                added += 1;
            }
        }
        // 按 created_at 降序排序，保留最新的 max_size 条
        if self.records.len() > self.max_size {
            self.records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            for r in &self.records[self.max_size..] {
                self.seen.remove(&RequestFingerprint::new(&r.session_id, &r.model, r.input_tokens, r.output_tokens));
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
/// codex: session_id + output_tokens（model 不参与，input 归一化）
/// 非codex: session_id + model + input_tokens + output_tokens
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub struct RequestFingerprint {
    session_id: String,
    model: String,
    input_tokens: i64,
    output_tokens: i64,
    is_codex: bool,
}

impl RequestFingerprint {
    /// 非codex 指纹（is_codex = false）
    pub fn new(session_id: &str, model: &str, input_tokens: i64, output_tokens: i64) -> Self {
        Self {
            session_id: session_id.to_string(),
            model: model.to_string(),
            input_tokens,
            output_tokens,
            is_codex: false,
        }
    }

    /// codex 指纹：不用 model，input 已归一化（不含 cache_read）
    pub fn new_codex(session_id: &str, normalized_input: i64, output_tokens: i64) -> Self {
        Self {
            session_id: session_id.to_string(),
            model: String::new(),
            input_tokens: normalized_input,
            output_tokens,
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
            let fp = RequestFingerprint::new(&item.session_id, &item.model, item.input_tokens, item.output_tokens);
            seen.insert(fp)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(session_id: &str, model: &str, created_at: i64, input: i64, output: i64) -> crate::models::SessionRequestToken {
        crate::models::SessionRequestToken {
            session_id: session_id.to_string(),
            model: model.to_string(),
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
        let a = RequestFingerprint::new("s1", "m1", 100, 200);
        let b = RequestFingerprint::new("s1", "m1", 100, 200);
        assert_eq!(a, b);
    }

    #[test]
    fn fingerprint_inequality_different_session() {
        let a = RequestFingerprint::new("s1", "m1", 100, 200);
        let b = RequestFingerprint::new("s2", "m1", 100, 200);
        assert_ne!(a, b);
    }

    #[test]
    fn fingerprint_inequality_different_tokens() {
        let a = RequestFingerprint::new("s1", "m1", 100, 200);
        let b = RequestFingerprint::new("s1", "m1", 100, 300);
        assert_ne!(a, b);
    }

    #[test]
    fn fingerprint_does_not_include_timestamp() {
        // 指纹仅由 session_id + model + input/output tokens 组成，
        // 不含 created_at（不同源时间语义不同，差值可达数分钟）
        let a = RequestFingerprint::new("s1", "m1", 100, 200);
        let b = RequestFingerprint::new("s1", "m1", 100, 200);
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
        // CCS: input=27505 (含 cache_read=26624), model=gpt-5.4-mini
        // AI Proxy: input=881 (不含 cache_read), model=deepseek-v4-flash
        // 归一化后 CCS input: 27505-26624=881，两边指纹应相同
        let ccs = raw_record("s1", "gpt-5.4-mini", "_codex_session", 27505, 16857, 26624, true);
        let aiproxy = raw_record("s1", "deepseek-v4-flash", "ai-proxy", 881, 16857, 26624, true);

        let result = dedup_records(vec![aiproxy, ccs]);
        assert_eq!(result.len(), 1);
        // 先到先保留：AI Proxy 在前，保留 target_model
        assert_eq!(result[0].model, "deepseek-v4-flash");
        assert_eq!(result[0].provider_id, "ai-proxy");
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
        let codex = raw_record("s1", "deepseek-v4-flash", "ai-proxy", 881, 200, 0, true);
        let non_codex = raw_record("s1", "deepseek-v4-flash", "ai-proxy", 881, 200, 0, false);

        let result = dedup_records(vec![codex, non_codex]);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn codex_fingerprint_ignores_model() {
        // codex 指纹不包含 model，不同 model 应去重
        let a = raw_record("s1", "model-a", "ai-proxy", 100, 200, 0, true);
        let b = raw_record("s1", "model-b", "ai-proxy", 100, 200, 0, true);

        let result = dedup_records(vec![a, b]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn codex_ai_proxy_priority() {
        // AI Proxy 排在前面应被优先保留（由调用方排序保证）
        let ccs = raw_record("s1", "gpt-5.4-mini", "_codex_session", 1000, 500, 900, true);
        let aiproxy = raw_record("s1", "deepseek-v4-flash", "ai-proxy", 100, 500, 900, true);

        let result = dedup_records(vec![aiproxy.clone(), ccs]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].provider_id, "ai-proxy");
        // AI Proxy 的 input 不含 cache，是准确值
        assert_eq!(result[0].input_tokens, 100);
    }

    #[test]
    fn codex_fingerprint_equality() {
        // CCS input=1000 cache_read=900 → 归一化 100
        // AI Proxy input=100 → 归一化 100
        let fp_ccs = {
            let r = raw_record("s1", "gpt-5.3-codex", "_codex_session", 1000, 200, 900, true);
            let normalized = r.input_tokens.saturating_sub(r.cache_read);
            RequestFingerprint::new_codex(&r.session_id, normalized, r.output_tokens)
        };
        let fp_aiproxy = {
            let r = raw_record("s1", "deepseek-v4-flash", "ai-proxy", 100, 200, 900, true);
            RequestFingerprint::new_codex(&r.session_id, r.input_tokens, r.output_tokens)
        };
        assert_eq!(fp_ccs, fp_aiproxy);
    }
}
