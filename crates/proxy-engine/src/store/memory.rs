//! 内存存储：按请求顺序保留最近 N 条交换记录。

use std::collections::{HashMap, VecDeque};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use common::types::{CapturedExchange, RequestId, RequestSummary};

#[derive(Debug, Default)]
struct Inner {
    /// 仅保存 id 顺序，真正的记录在 `records` 中，更新时顺序不变。
    order: VecDeque<RequestId>,
    records: HashMap<RequestId, CapturedExchange>,
}

/// 线程安全、有容量上限的请求存储。
#[derive(Debug)]
pub struct MemoryStore {
    capacity: usize,
    inner: RwLock<Inner>,
}

impl MemoryStore {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            inner: RwLock::new(Inner::default()),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.read().records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 插入或更新记录；新记录超出容量时淘汰最旧的一条。
    pub fn upsert(&self, exchange: CapturedExchange) {
        let id = exchange.id();
        let evicted = {
            let mut inner = self.write();
            if !inner.records.contains_key(&id) {
                inner.order.push_back(id);
            }
            inner.records.insert(id, exchange);
            let mut evicted = Vec::new();
            while inner.order.len() > self.capacity {
                if let Some(oldest) = inner.order.pop_front() {
                    inner.records.remove(&oldest);
                    evicted.push(oldest);
                }
            }
            evicted
        };
        if !evicted.is_empty() {
            tracing::debug!(count = evicted.len(), "内存存储已淘汰旧记录");
        }
    }

    pub fn get(&self, id: RequestId) -> Option<CapturedExchange> {
        self.read().records.get(&id).cloned()
    }

    /// 按抓取顺序返回摘要列表。
    pub fn list(&self) -> Vec<RequestSummary> {
        let inner = self.read();
        inner
            .order
            .iter()
            .filter_map(|id| inner.records.get(id))
            .map(CapturedExchange::summary)
            .collect()
    }

    /// 按抓取顺序返回完整记录，供导出 / 持久化使用。
    pub fn list_all(&self) -> Vec<CapturedExchange> {
        let inner = self.read();
        inner
            .order
            .iter()
            .filter_map(|id| inner.records.get(id).cloned())
            .collect()
    }

    pub fn clear(&self) {
        let mut inner = self.write();
        inner.order.clear();
        inner.records.clear();
    }

    fn read(&self) -> RwLockReadGuard<'_, Inner> {
        self.inner.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, Inner> {
        self.inner.write().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use common::types::{CapturedRequest, Scheme};
    use uuid::Uuid;

    fn exchange(method: &str) -> CapturedExchange {
        CapturedExchange {
            request: CapturedRequest {
                id: Uuid::new_v4(),
                scheme: Scheme::Http,
                method: method.into(),
                url: format!("http://example.com/{method}"),
                host: "example.com".into(),
                port: 80,
                path: format!("/{method}"),
                version: "HTTP/1.1".into(),
                headers: Vec::new(),
                body: Vec::new(),
                body_truncated: false,
                body_complete: true,
                started_at: Utc::now(),
            },
            response: None,
            error: None,
            duration_ms: 0,
            completed_at: None,
        }
    }

    #[test]
    fn upsert_keeps_insertion_order() {
        let store = MemoryStore::new(10);
        let first = exchange("GET");
        let second = exchange("POST");
        store.upsert(first.clone());
        store.upsert(second.clone());
        store.upsert(first.clone());

        let list = store.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, first.id());
        assert_eq!(list[1].id, second.id());
    }

    #[test]
    fn evicts_oldest_when_full() {
        let store = MemoryStore::new(2);
        let a = exchange("a");
        let b = exchange("b");
        let c = exchange("c");
        store.upsert(a.clone());
        store.upsert(b.clone());
        store.upsert(c.clone());

        assert_eq!(store.len(), 2);
        assert!(store.get(a.id()).is_none());
        assert!(store.get(b.id()).is_some());
        assert!(store.get(c.id()).is_some());
    }

    #[test]
    fn clear_removes_everything() {
        let store = MemoryStore::new(4);
        store.upsert(exchange("GET"));
        store.clear();
        assert!(store.is_empty());
        assert!(store.list().is_empty());
    }
}
