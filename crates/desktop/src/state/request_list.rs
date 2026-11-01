//! 请求列表状态：有序哈希表维护抓取顺序，并支持轻量过滤。

use common::types::{RequestId, RequestSummary, Scheme};
use indexmap::IndexMap;

/// 列表过滤条件，供工具栏使用。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListFilter {
    pub query: String,
    pub method: Option<String>,
    pub only_errors: bool,
    pub only_https: bool,
}

impl ListFilter {
    pub fn is_active(&self) -> bool {
        !self.query.trim().is_empty()
            || self.method.is_some()
            || self.only_errors
            || self.only_https
    }

    pub fn matches(&self, summary: &RequestSummary) -> bool {
        if let Some(method) = &self.method {
            if !summary.method.eq_ignore_ascii_case(method) {
                return false;
            }
        }
        if self.only_https && summary.scheme != Scheme::Https {
            return false;
        }
        if self.only_errors {
            let is_error =
                summary.error.is_some() || summary.status.is_some_and(|status| status >= 400);
            if !is_error {
                return false;
            }
        }

        let query = self.query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        summary.url.to_lowercase().contains(&query)
            || summary.host.to_lowercase().contains(&query)
            || summary.method.to_lowercase().contains(&query)
    }
}

/// 有容量上限的请求列表，插入顺序与抓取顺序一致。
#[derive(Debug)]
pub struct RequestList {
    items: IndexMap<RequestId, RequestSummary>,
    selected: Option<RequestId>,
    capacity: usize,
}

impl RequestList {
    pub fn new(capacity: usize) -> Self {
        Self {
            items: IndexMap::new(),
            selected: None,
            capacity: capacity.max(1),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 插入或就地更新（更新不会改变列表顺序）。
    pub fn upsert(&mut self, summary: RequestSummary) {
        if let Some(existing) = self.items.get_mut(&summary.id) {
            *existing = summary;
            return;
        }
        self.items.insert(summary.id, summary);
        self.trim();
    }

    fn trim(&mut self) {
        while self.items.len() > self.capacity {
            let Some((evicted, _)) = self.items.shift_remove_index(0) else {
                break;
            };
            if self.selected == Some(evicted) {
                self.selected = None;
            }
        }
    }

    pub fn get(&self, id: RequestId) -> Option<&RequestSummary> {
        self.items.get(&id)
    }

    pub fn select(&mut self, id: Option<RequestId>) {
        self.selected = id.filter(|id| self.items.contains_key(id));
    }

    pub fn selected(&self) -> Option<&RequestSummary> {
        self.selected.and_then(|id| self.items.get(&id))
    }

    pub fn selected_id(&self) -> Option<RequestId> {
        self.selected
    }

    pub fn iter(&self) -> impl Iterator<Item = &RequestSummary> {
        self.items.values()
    }

    /// 最新一条请求。
    pub fn latest(&self) -> Option<&RequestSummary> {
        self.items.last().map(|(_, summary)| summary)
    }

    pub fn filtered(&self, filter: &ListFilter) -> Vec<&RequestSummary> {
        self.items
            .values()
            .filter(|summary| filter.matches(summary))
            .collect()
    }

    /// 删除单条记录；被删的是选中项时同时清空选中。
    pub fn remove(&mut self, id: RequestId) -> bool {
        let removed = self.items.shift_remove(&id).is_some();
        if removed && self.selected == Some(id) {
            self.selected = None;
        }
        removed
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.selected = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use common::types::RequestSummary;
    use uuid::Uuid;

    fn summary(method: &str, url: &str, status: Option<u16>) -> RequestSummary {
        RequestSummary {
            id: Uuid::new_v4(),
            scheme: if url.starts_with("https") {
                Scheme::Https
            } else {
                Scheme::Http
            },
            method: method.into(),
            host: "example.com".into(),
            path: "/".into(),
            url: url.into(),
            status,
            duration_ms: 1,
            request_bytes: 0,
            response_bytes: 0,
            started_at: Utc::now(),
            completed: status.is_some(),
            error: None,
        }
    }

    #[test]
    fn upsert_keeps_order_and_updates_in_place() {
        let mut list = RequestList::new(10);
        let first = summary("GET", "http://example.com/a", None);
        let second = summary("POST", "http://example.com/b", None);
        list.upsert(first.clone());
        list.upsert(second.clone());

        let mut completed = first.clone();
        completed.status = Some(200);
        completed.completed = true;
        list.upsert(completed);

        let all: Vec<_> = list.iter().cloned().collect();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, first.id);
        assert_eq!(all[0].status, Some(200));
        assert_eq!(all[1].id, second.id);
    }

    #[test]
    fn evicts_oldest_and_clears_selection() {
        let mut list = RequestList::new(2);
        let a = summary("GET", "http://example.com/a", None);
        let b = summary("GET", "http://example.com/b", None);
        let c = summary("GET", "http://example.com/c", None);
        list.upsert(a.clone());
        list.select(Some(a.id));
        list.upsert(b.clone());
        list.upsert(c.clone());

        assert_eq!(list.len(), 2);
        assert!(list.get(a.id).is_none());
        assert_eq!(list.selected_id(), None);
        assert_eq!(list.latest().map(|s| s.id), Some(c.id));
    }

    #[test]
    fn filter_matches_query_method_scheme_and_errors() {
        let https = summary("GET", "https://api.example.com/users", Some(200));
        let failing = summary("POST", "http://api.example.com/login", Some(500));

        let by_query = ListFilter {
            query: "USERS".into(),
            ..Default::default()
        };
        assert!(by_query.matches(&https));
        assert!(!by_query.matches(&failing));

        let by_method = ListFilter {
            method: Some("post".into()),
            ..Default::default()
        };
        assert!(by_method.matches(&failing));
        assert!(!by_method.matches(&https));

        let only_https = ListFilter {
            only_https: true,
            ..Default::default()
        };
        assert!(only_https.matches(&https));
        assert!(!only_https.matches(&failing));

        let only_errors = ListFilter {
            only_errors: true,
            ..Default::default()
        };
        assert!(only_errors.matches(&failing));
        assert!(!only_errors.matches(&https));
        assert!(only_errors.is_active());
    }

    #[test]
    fn select_ignores_unknown_ids() {
        let mut list = RequestList::new(4);
        list.select(Some(Uuid::new_v4()));
        assert_eq!(list.selected_id(), None);
    }
}
