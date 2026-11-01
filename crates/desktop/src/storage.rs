//! SQLite 持久化：请求历史与高亮 / 备注。
//!
//! 写入在后台线程完成，UI 只把记录投递到通道，避免 SQLite 提交阻塞渲染。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use common::types::{CapturedExchange, RequestId};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// 请求行的标记颜色。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Highlight {
    #[default]
    None,
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    Gray,
}

impl Highlight {
    pub const ALL: [Highlight; 8] = [
        Highlight::None,
        Highlight::Red,
        Highlight::Orange,
        Highlight::Yellow,
        Highlight::Green,
        Highlight::Blue,
        Highlight::Purple,
        Highlight::Gray,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Highlight::None => "none",
            Highlight::Red => "red",
            Highlight::Orange => "orange",
            Highlight::Yellow => "yellow",
            Highlight::Green => "green",
            Highlight::Blue => "blue",
            Highlight::Purple => "purple",
            Highlight::Gray => "gray",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "red" => Highlight::Red,
            "orange" => Highlight::Orange,
            "yellow" => Highlight::Yellow,
            "green" => Highlight::Green,
            "blue" => Highlight::Blue,
            "purple" => Highlight::Purple,
            "gray" => Highlight::Gray,
            _ => Highlight::None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Highlight::None => "无",
            Highlight::Red => "红色",
            Highlight::Orange => "橙色",
            Highlight::Yellow => "黄色",
            Highlight::Green => "绿色",
            Highlight::Blue => "蓝色",
            Highlight::Purple => "紫色",
            Highlight::Gray => "灰色",
        }
    }
}

/// 一条请求的高亮与备注。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotation {
    pub highlight: Highlight,
    pub comment: String,
}

impl Annotation {
    pub fn is_empty(&self) -> bool {
        self.highlight == Highlight::None && self.comment.trim().is_empty()
    }
}

/// 从数据库恢复的一条记录。
#[derive(Debug, Clone)]
pub struct StoredRecord {
    pub exchange: CapturedExchange,
    pub annotation: Annotation,
}

enum DbOp {
    Upsert {
        id: String,
        started_at: String,
        method: String,
        host: String,
        url: String,
        status: Option<u16>,
        summary_json: String,
        exchange_json: String,
        capacity: usize,
    },
    Annotate {
        id: String,
        highlight: String,
        comment: String,
    },
    Delete(String),
    Clear,
    Prune(usize),
    Shutdown,
}

/// 请求历史数据库。
pub struct HistoryStore {
    tx: Sender<DbOp>,
    path: PathBuf,
    writer: Option<JoinHandle<()>>,
}

impl HistoryStore {
    /// 打开（或创建）数据库并启动后台写入线程。
    pub fn open(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(&path)?;
        init(&connection)?;
        drop(connection);

        let (tx, rx) = mpsc::channel();
        let writer_path = path.clone();
        let writer = thread::Builder::new()
            .name("packet-sniffer-history".to_string())
            .spawn(move || writer_loop(&writer_path, rx))?;
        Ok(Self {
            tx,
            path,
            writer: Some(writer),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn insert_exchange(&self, exchange: &CapturedExchange, capacity: usize) {
        let summary = exchange.summary();
        let Ok(summary_json) = serde_json::to_string(&summary) else {
            return;
        };
        let Ok(exchange_json) = serde_json::to_string(exchange) else {
            return;
        };
        let _ = self.tx.send(DbOp::Upsert {
            id: summary.id.to_string(),
            started_at: summary.started_at.to_rfc3339(),
            method: summary.method,
            host: summary.host,
            url: summary.url,
            status: summary.status,
            summary_json,
            exchange_json,
            capacity: capacity.max(1),
        });
    }

    pub fn update_annotation(&self, id: RequestId, annotation: &Annotation) {
        let _ = self.tx.send(DbOp::Annotate {
            id: id.to_string(),
            highlight: annotation.highlight.as_str().to_string(),
            comment: annotation.comment.clone(),
        });
    }

    pub fn delete(&self, id: RequestId) {
        let _ = self.tx.send(DbOp::Delete(id.to_string()));
    }

    pub fn clear(&self) {
        let _ = self.tx.send(DbOp::Clear);
    }

    /// 最近 `limit` 条完整记录，按时间从旧到新。
    pub fn load_recent(&self, limit: usize) -> Vec<StoredRecord> {
        let Ok(connection) = Connection::open(&self.path) else {
            return Vec::new();
        };
        let Ok(mut statement) = connection.prepare(
            "SELECT exchange_json, highlight, comment \
             FROM exchanges ORDER BY started_at DESC LIMIT ?1",
        ) else {
            return Vec::new();
        };
        let Ok(rows) = statement.query_map(params![limit as i64], |row| {
            let exchange_json: String = row.get(0)?;
            let highlight: String = row.get(1)?;
            let comment: String = row.get(2)?;
            Ok((exchange_json, highlight, comment))
        }) else {
            return Vec::new();
        };
        let mut records: Vec<StoredRecord> = rows
            .filter_map(Result::ok)
            .filter_map(|(exchange_json, highlight, comment)| {
                let exchange = serde_json::from_str(&exchange_json).ok()?;
                Some(StoredRecord {
                    exchange,
                    annotation: Annotation {
                        highlight: Highlight::parse(&highlight),
                        comment,
                    },
                })
            })
            .collect();
        records.reverse();
        records
    }

    pub fn count(&self) -> usize {
        Connection::open(&self.path)
            .and_then(|connection| {
                connection.query_row("SELECT COUNT(*) FROM exchanges", [], |row| {
                    row.get::<_, i64>(0)
                })
            })
            .unwrap_or(0) as usize
    }
}

impl Drop for HistoryStore {
    fn drop(&mut self) {
        let _ = self.tx.send(DbOp::Shutdown);
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

fn init(connection: &Connection) -> rusqlite::Result<()> {
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS exchanges (
            id TEXT PRIMARY KEY,
            started_at TEXT NOT NULL,
            method TEXT NOT NULL,
            host TEXT NOT NULL,
            url TEXT NOT NULL,
            status INTEGER,
            summary_json TEXT NOT NULL,
            exchange_json TEXT NOT NULL,
            highlight TEXT NOT NULL DEFAULT 'none',
            comment TEXT NOT NULL DEFAULT ''
         );
         CREATE INDEX IF NOT EXISTS idx_exchanges_started
            ON exchanges(started_at DESC);",
    )
}

fn writer_loop(path: &Path, rx: mpsc::Receiver<DbOp>) {
    let connection = match Connection::open(path) {
        Ok(connection) => connection,
        Err(error) => {
            tracing::warn!(error = %error, "打开请求历史数据库失败");
            return;
        }
    };
    if let Err(error) = init(&connection) {
        tracing::warn!(error = %error, "初始化请求历史数据库失败");
        return;
    }
    let mut since_prune = 0_u64;
    while let Ok(op) = rx.recv() {
        let result = match op {
            DbOp::Upsert {
                id,
                started_at,
                method,
                host,
                url,
                status,
                summary_json,
                exchange_json,
                capacity,
            } => connection
                .execute(
                    "INSERT INTO exchanges
                        (id, started_at, method, host, url, status, summary_json, exchange_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(id) DO UPDATE SET
                        started_at = excluded.started_at,
                        method = excluded.method,
                        host = excluded.host,
                        url = excluded.url,
                        status = excluded.status,
                        summary_json = excluded.summary_json,
                        exchange_json = excluded.exchange_json",
                    params![
                        id,
                        started_at,
                        method,
                        host,
                        url,
                        status,
                        summary_json,
                        exchange_json
                    ],
                )
                .map(|_| {
                    since_prune += 1;
                    if since_prune >= 64 {
                        since_prune = 0;
                        let _ = prune(&connection, capacity);
                    }
                }),
            DbOp::Annotate {
                id,
                highlight,
                comment,
            } => connection
                .execute(
                    "UPDATE exchanges SET highlight = ?1, comment = ?2 WHERE id = ?3",
                    params![highlight, comment, id],
                )
                .map(|_| ()),
            DbOp::Delete(id) => connection
                .execute("DELETE FROM exchanges WHERE id = ?1", [id])
                .map(|_| ()),
            DbOp::Clear => connection.execute("DELETE FROM exchanges", []).map(|_| ()),
            DbOp::Prune(capacity) => prune(&connection, capacity),
            DbOp::Shutdown => break,
        };
        if let Err(error) = result {
            tracing::warn!(error = %error, "写入请求历史数据库失败");
        }
    }
}

fn prune(connection: &Connection, capacity: usize) -> rusqlite::Result<()> {
    connection.execute(
        "DELETE FROM exchanges WHERE id NOT IN (
            SELECT id FROM exchanges ORDER BY started_at DESC LIMIT ?1
         )",
        [capacity as i64],
    )?;
    Ok(())
}

/// `~/.packet-sniffer/history.db`
pub fn default_history_path() -> PathBuf {
    match std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        Some(home) => PathBuf::from(home)
            .join(".packet-sniffer")
            .join("history.db"),
        None => PathBuf::from("history.db"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use common::types::{CapturedRequest, Scheme};
    use uuid::Uuid;

    fn exchange(url: &str) -> CapturedExchange {
        CapturedExchange {
            request: CapturedRequest {
                id: Uuid::new_v4(),
                scheme: Scheme::Https,
                method: "GET".into(),
                url: url.into(),
                host: "example.com".into(),
                port: 443,
                path: "/".into(),
                version: "HTTP/1.1".into(),
                headers: Vec::new(),
                body: Vec::new(),
                body_truncated: false,
                body_complete: true,
                started_at: Utc::now(),
            },
            response: None,
            error: None,
            duration_ms: 3,
            completed_at: Some(Utc::now()),
        }
    }

    #[test]
    fn stores_loads_annotates_and_clears() {
        let dir = std::env::temp_dir().join(format!(
            "packet-sniffer-history-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("history.db");
        let store = HistoryStore::open(&path).unwrap();
        let first = exchange("https://example.com/one");
        let second = exchange("https://example.com/two");
        store.insert_exchange(&first, 100);
        store.insert_exchange(&second, 100);
        store.update_annotation(
            second.request.id,
            &Annotation {
                highlight: Highlight::Red,
                comment: "看这里".into(),
            },
        );
        std::thread::sleep(std::time::Duration::from_millis(150));

        let records = store.load_recent(10);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].exchange.request.url, first.request.url);
        assert_eq!(records[1].annotation.highlight, Highlight::Red);
        assert_eq!(records[1].annotation.comment, "看这里");

        store.delete(first.request.id);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(store.load_recent(10).len(), 1);

        store.clear();
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(store.load_recent(10).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
