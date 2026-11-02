//! 上游 HTTP/2 连接池：按目标复用连接，让多路复用真正生效。
//!
//! 只缓存 HTTP/2 连接：`SendRequest` 可以克隆后并发开流；HTTP/1.1 一条连接同时
//! 只能跑一个请求，复用需要「借出 / 归还」模型，收益也远小于 h2，因此仍按请求新建。

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::proxy::upstream::UpstreamSender;

/// 池化键：同一目标且同样是否走 TLS，才可能复用同一条连接。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PoolKey {
    pub host: String,
    pub port: u16,
    pub tls: bool,
}

impl PoolKey {
    pub fn new(host: impl Into<String>, port: u16, tls: bool) -> Self {
        Self {
            host: host.into(),
            port,
            tls,
        }
    }
}

struct Entry {
    sender: UpstreamSender,
    last_used: Instant,
}

/// 按目标缓存可用连接；空闲超时或连接关闭后自动回收。
pub(crate) struct UpstreamPool {
    idle_timeout: Duration,
    max_per_key: usize,
    entries: Mutex<HashMap<PoolKey, Vec<Entry>>>,
}

impl UpstreamPool {
    pub fn new(idle_timeout: Duration, max_per_key: usize) -> Self {
        Self {
            idle_timeout,
            max_per_key,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// 池是否启用。空闲时间为 0 或每个目标上限为 0 都表示不复用。
    pub fn enabled(&self) -> bool {
        self.max_per_key > 0 && !self.idle_timeout.is_zero()
    }

    /// 借出一条可复用的连接；没有可用连接时返回 `None`，调用方应新建。
    pub fn acquire(&self, key: &PoolKey) -> Option<UpstreamSender> {
        if !self.enabled() {
            return None;
        }
        let idle = self.idle_timeout;
        let now = Instant::now();
        let mut entries = self.lock();
        let list = entries.get_mut(key)?;
        list.retain(|entry| Self::usable(entry, now, idle));
        let entry = list.last_mut()?;
        entry.last_used = now;
        entry.sender.reuse()
    }

    /// 把新建的连接放进池里；非 HTTP/2 连接会被直接忽略。
    pub fn store(&self, key: PoolKey, sender: UpstreamSender) {
        if !self.enabled() || !sender.is_reusable() {
            return;
        }
        let idle = self.idle_timeout;
        let max = self.max_per_key;
        let now = Instant::now();
        let mut entries = self.lock();
        let list = entries.entry(key).or_default();
        list.retain(|entry| Self::usable(entry, now, idle));
        if list.len() >= max {
            list.remove(0);
        }
        list.push(Entry {
            sender,
            last_used: now,
        });
    }

    /// 回收空闲与已关闭的连接。
    pub fn prune(&self) {
        if !self.enabled() {
            return;
        }
        let idle = self.idle_timeout;
        let now = Instant::now();
        let mut entries = self.lock();
        entries.retain(|_, list| {
            list.retain(|entry| Self::usable(entry, now, idle));
            !list.is_empty()
        });
    }

    fn usable(entry: &Entry, now: Instant, idle: Duration) -> bool {
        entry.sender.is_reusable() && now.duration_since(entry.last_used) < idle
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<PoolKey, Vec<Entry>>> {
        self.entries.lock().unwrap_or_else(|err| err.into_inner())
    }
}
