use std::collections::HashSet;
use std::sync::mpsc;
use std::thread;

use super::data_source::SourceEntry;
use super::dedup::RequestFingerprint;

/// 流式去重 Pipeline。
///
/// 并行查询所有数据源，通过 channel 流式传输记录，
/// 消费端边收边去重，真正流式处理（不在内存中积累全量数据再过滤）。
///
/// 数据流：Producer(源1) ──┐
///         Producer(源2) ──┼→ channel → Deduplicator(逐条) → Vec<Record>
///         Producer(源N) ──┘
pub fn run_streaming_dedup(
    sources: &[SourceEntry],
    since: Option<i64>,
) -> Vec<(String, String, String, i64, i64, i64, i64, i64, i64)> {
    let (tx, rx) = mpsc::channel();

    // 并行 Producer：每个数据源一个线程，逐条发送到 channel
    thread::scope(|s| {
        for entry in sources {
            let tx = tx.clone();
            s.spawn(move || {
                if let Err(e) = entry.source.stream_records(since, &mut |record| {
                    let _ = tx.send(record);
                }) {
                    log::warn!("[PIPELINE] 数据源({}) stream_records 失败: {}", entry.db_type.label(), e);
                }
            });
        }
        drop(tx);
    });

    // Consumer：边收边去重（不在内存中积累全量再过滤）
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for record in rx {
        let fp = RequestFingerprint::new(&record.0, &record.1, record.4, record.5);
        if seen.insert(fp) {
            result.push(record);
        }
    }
    result
}
