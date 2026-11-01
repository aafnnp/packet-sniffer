//! NSH 应用状态与事件分发。
//!
//! 该模块不依赖任何 UI 库：headless 模式与 NSH 界面共用同一份状态机，
//! 界面层只负责把 `App` 的状态渲染出来。

use std::path::PathBuf;

use common::types::{CapturedExchange, EngineEvent, LogLevel, RequestId};
use indexmap::IndexMap;

use crate::state::{RequestList, Settings};

const MAX_LOG_LINES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineStatus {
    Stopped,
    Running,
    Failed,
}

impl EngineStatus {
    pub fn label(self) -> &'static str {
        match self {
            EngineStatus::Stopped => "已停止",
            EngineStatus::Running => "运行中",
            EngineStatus::Failed => "异常",
        }
    }
}

/// 全局应用状态。
pub struct App {
    pub requests: RequestList,
    pub settings: Settings,
    pub status: EngineStatus,
    pub listen_addr: Option<String>,
    pub ca_cert_path: Option<PathBuf>,
    pub logs: Vec<String>,
    pub last_error: Option<String>,
    pub last_exchange: Option<CapturedExchange>,
    /// 完整记录，供详情面板按需读取；与请求列表同容量、同顺序。
    exchanges: IndexMap<RequestId, CapturedExchange>,
}

impl App {
    pub fn new(settings: Settings) -> Self {
        let requests = RequestList::new(settings.store_capacity);
        Self {
            requests,
            settings,
            status: EngineStatus::Stopped,
            listen_addr: None,
            ca_cert_path: None,
            logs: Vec::new(),
            last_error: None,
            last_exchange: None,
            exchanges: IndexMap::new(),
        }
    }

    /// 选中请求的完整记录（请求首部、响应、body）。
    pub fn exchange(&self, id: RequestId) -> Option<&CapturedExchange> {
        self.exchanges.get(&id)
    }

    /// 删除单条记录及其完整交换数据。
    pub fn remove(&mut self, id: RequestId) -> bool {
        let removed = self.requests.remove(id);
        self.exchanges.shift_remove(&id);
        if self
            .last_exchange
            .as_ref()
            .is_some_and(|e| e.request.id == id)
        {
            self.last_exchange = None;
        }
        removed
    }

    /// 从持久化历史恢复记录（按原顺序回放完成事件）。
    pub fn restore(&mut self, exchanges: impl IntoIterator<Item = CapturedExchange>) {
        for exchange in exchanges {
            self.handle_event(EngineEvent::RequestCompleted { exchange });
        }
    }

    /// 清空抓包记录，保留设置与引擎状态。
    pub fn clear(&mut self) {
        self.requests.clear();
        self.exchanges.clear();
        self.last_exchange = None;
    }

    /// 事件分发的唯一入口：headless 与 UI 模式都走这里。
    pub fn handle_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::Started {
                listen_addr,
                ca_cert_path,
            } => {
                self.status = EngineStatus::Running;
                self.listen_addr = Some(listen_addr);
                self.ca_cert_path = Some(PathBuf::from(ca_cert_path));
            }
            EngineEvent::Stopped => {
                self.status = EngineStatus::Stopped;
            }
            EngineEvent::RequestStarted { request } => {
                self.requests.upsert(request.summary());
                self.exchanges.insert(
                    request.id,
                    CapturedExchange {
                        request,
                        response: None,
                        error: None,
                        duration_ms: 0,
                        completed_at: None,
                    },
                );
                self.trim_exchanges();
            }
            EngineEvent::RequestCompleted { exchange } => {
                self.requests.upsert(exchange.summary());
                self.exchanges.insert(exchange.request.id, exchange.clone());
                self.trim_exchanges();
                self.last_exchange = Some(exchange);
            }
            EngineEvent::Log { level, message } => {
                if level == LogLevel::Error {
                    self.last_error = Some(message.clone());
                    self.status = EngineStatus::Failed;
                }
                self.log(level, message);
            }
        }
    }

    fn trim_exchanges(&mut self) {
        while self.exchanges.len() > self.requests.capacity() {
            if self.exchanges.shift_remove_index(0).is_none() {
                break;
            }
        }
    }

    pub fn log(&mut self, level: LogLevel, message: impl Into<String>) {
        self.logs.push(format!("[{level:?}] {}", message.into()));
        if self.logs.len() > MAX_LOG_LINES {
            let overflow = self.logs.len() - MAX_LOG_LINES;
            self.logs.drain(..overflow);
        }
    }

    /// 底部状态栏文本。
    pub fn status_line(&self) -> String {
        let addr = self.listen_addr.as_deref().unwrap_or("-");
        format!(
            "引擎：{} | 监听：{addr} | 已抓取：{} 条",
            self.status.label(),
            self.requests.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use common::types::{CapturedRequest, Scheme};
    use uuid::Uuid;

    fn request() -> CapturedRequest {
        CapturedRequest {
            id: Uuid::new_v4(),
            scheme: Scheme::Https,
            method: "GET".into(),
            url: "https://example.com/".into(),
            host: "example.com".into(),
            port: 443,
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: Vec::new(),
            body: Vec::new(),
            body_truncated: false,
            body_complete: true,
            started_at: Utc::now(),
        }
    }

    #[test]
    fn started_event_updates_status() {
        let mut app = App::new(Settings::default());
        app.handle_event(EngineEvent::Started {
            listen_addr: "127.0.0.1:8899".into(),
            ca_cert_path: "/tmp/root-ca.pem".into(),
        });

        assert_eq!(app.status, EngineStatus::Running);
        assert_eq!(app.listen_addr.as_deref(), Some("127.0.0.1:8899"));
        assert_eq!(
            app.ca_cert_path.as_deref(),
            Some(std::path::Path::new("/tmp/root-ca.pem"))
        );
        assert!(app.status_line().contains("运行中"));
    }

    #[test]
    fn request_events_fill_list_and_last_exchange() {
        let mut app = App::new(Settings::default());
        let request = request();

        app.handle_event(EngineEvent::RequestStarted {
            request: request.clone(),
        });
        assert_eq!(app.requests.len(), 1);
        assert!(app.last_exchange.is_none());

        let exchange = CapturedExchange {
            request,
            response: None,
            error: Some("boom".into()),
            duration_ms: 5,
            completed_at: Some(Utc::now()),
        };
        app.handle_event(EngineEvent::RequestCompleted {
            exchange: exchange.clone(),
        });
        assert_eq!(app.requests.len(), 1);
        assert_eq!(app.last_exchange.map(|e| e.duration_ms), Some(5));
        let summary = app.requests.latest().expect("列表非空");
        assert_eq!(summary.error.as_deref(), Some("boom"));
    }

    #[test]
    fn error_log_marks_status_failed_and_logs_are_capped() {
        let mut app = App::new(Settings::default());
        app.handle_event(EngineEvent::Log {
            level: LogLevel::Error,
            message: "端口被占用".into(),
        });
        assert_eq!(app.status, EngineStatus::Failed);
        assert_eq!(app.last_error.as_deref(), Some("端口被占用"));

        for index in 0..(MAX_LOG_LINES + 20) {
            app.handle_event(EngineEvent::Log {
                level: LogLevel::Info,
                message: format!("log-{index}"),
            });
        }
        assert_eq!(app.logs.len(), MAX_LOG_LINES);
        assert!(app.logs.last().unwrap().ends_with("log-219"));
    }

    #[test]
    fn completed_exchanges_are_kept_and_cleared() {
        let mut app = App::new(Settings::default());
        let request = request();
        let id = request.id;
        app.handle_event(EngineEvent::RequestStarted {
            request: request.clone(),
        });
        assert!(app.exchange(id).is_some());
        assert!(app.exchange(id).unwrap().response.is_none());

        app.handle_event(EngineEvent::RequestCompleted {
            exchange: CapturedExchange {
                request,
                response: None,
                error: Some("boom".into()),
                duration_ms: 3,
                completed_at: Some(Utc::now()),
            },
        });
        assert_eq!(app.exchange(id).unwrap().error.as_deref(), Some("boom"));

        app.clear();
        assert!(app.exchange(id).is_none());
        assert!(app.requests.is_empty());
        assert!(app.last_exchange.is_none());
    }

    #[test]
    fn exchanges_are_capped_with_the_request_list() {
        let mut app = App::new(Settings {
            store_capacity: 2,
            ..Settings::default()
        });
        let mut ids = Vec::new();
        for _ in 0..3 {
            let request = request();
            ids.push(request.id);
            app.handle_event(EngineEvent::RequestCompleted {
                exchange: CapturedExchange {
                    request,
                    response: None,
                    error: None,
                    duration_ms: 1,
                    completed_at: None,
                },
            });
        }
        assert!(app.exchange(ids[0]).is_none());
        assert!(app.exchange(ids[1]).is_some());
        assert!(app.exchange(ids[2]).is_some());
    }

    #[test]
    fn remove_drops_the_exchange() {
        let mut app = App::new(Settings::default());
        let request = request();
        let id = request.id;
        app.handle_event(EngineEvent::RequestCompleted {
            exchange: CapturedExchange {
                request,
                response: None,
                error: None,
                duration_ms: 1,
                completed_at: None,
            },
        });
        assert!(app.remove(id));
        assert!(app.exchange(id).is_none());
        assert!(app.requests.get(id).is_none());
        assert!(!app.remove(id));
    }

    #[test]
    fn restore_replays_completed_exchanges() {
        let mut app = App::new(Settings::default());
        let request = request();
        let exchange = CapturedExchange {
            request,
            response: None,
            error: None,
            duration_ms: 2,
            completed_at: Some(Utc::now()),
        };
        app.restore([exchange]);
        assert_eq!(app.requests.len(), 1);
        assert!(app.last_exchange.is_some());
    }

    #[test]
    fn stopped_event_resets_status() {
        let mut app = App::new(Settings::default());
        app.handle_event(EngineEvent::Stopped);
        assert_eq!(app.status, EngineStatus::Stopped);
    }
}
