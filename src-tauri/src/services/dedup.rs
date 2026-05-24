use std::collections::HashSet;

use crate::models::RawRecord;

/// 对 RawRecord 做 RequestFingerprint 去重，先到先保留。
pub fn dedup_records(records: Vec<RawRecord>) -> Vec<RawRecord> {
    let mut seen = HashSet::new();
    records.into_iter().filter(|r| {
        let fp = RequestFingerprint::new(&r.session_id, &r.model, r.input_tokens, r.output_tokens, r.created_at);
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
            let fp = RequestFingerprint::new(&record.session_id, &record.model, record.input_tokens, record.output_tokens, record.created_at);
            if self.seen.insert(fp) {
                self.records.push(record);
                added += 1;
            }
        }
        // 按 created_at 降序排序，保留最新的 max_size 条
        if self.records.len() > self.max_size {
            self.records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            for r in &self.records[self.max_size..] {
                self.seen.remove(&RequestFingerprint::new(&r.session_id, &r.model, r.input_tokens, r.output_tokens, r.created_at));
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
/// 4 字段：session_id + model + input_tokens + output_tokens
///
/// 设计依据：
/// - 不含 created_at：不同源记录时间语义不同（请求开始 vs 请求结束），
///   差值可达数分钟，无法用时间桶可靠匹配
/// - 不含 provider_id：同一请求可能被不同源以不同 provider 记录
/// - 不含 cache_*：不参与判定，但同 fingerprint 的 cache 值理论上一致
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub struct RequestFingerprint {
    session_id: String,
    model: String,
    input_tokens: i64,
    output_tokens: i64,
}

impl RequestFingerprint {
    pub fn new(session_id: &str, model: &str, input_tokens: i64, output_tokens: i64, _created_at: i64) -> Self {
        Self {
            session_id: session_id.to_string(),
            model: model.to_string(),
            input_tokens,
            output_tokens,
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
            let fp = RequestFingerprint::new(&item.session_id, &item.model, item.input_tokens, item.output_tokens, item.created_at);
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
        let a = RequestFingerprint::new("s1", "m1", 100, 200, 1000);
        let b = RequestFingerprint::new("s1", "m1", 100, 200, 2000);
        assert_eq!(a, b);
    }

    #[test]
    fn fingerprint_inequality_different_session() {
        let a = RequestFingerprint::new("s1", "m1", 100, 200, 1000);
        let b = RequestFingerprint::new("s2", "m1", 100, 200, 1000);
        assert_ne!(a, b);
    }

    #[test]
    fn fingerprint_inequality_different_tokens() {
        let a = RequestFingerprint::new("s1", "m1", 100, 200, 1000);
        let b = RequestFingerprint::new("s1", "m1", 100, 300, 1000);
        assert_ne!(a, b);
    }

    #[test]
    fn fingerprint_ignores_timestamp() {
        // 不同源记录时间语义不同（请求开始 vs 请求结束），时间差可达数分钟
        let a = RequestFingerprint::new("s1", "m1", 100, 200, 1000);
        let b = RequestFingerprint::new("s1", "m1", 100, 200, 9999);
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
}
