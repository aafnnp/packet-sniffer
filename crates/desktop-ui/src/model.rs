//! 界面状态：引擎生命周期、抓包记录、筛选、选中项与详情文本。
//!
//! 这里不直接依赖 NSH 元素；视图层只读取 [`Model`] 并把事件转成它的方法调用，
//! 再用 `revision` signal 触发重绘。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_channel::Receiver;
use common::types::{EngineEvent, LogLevel, RequestId, RequestSummary, Scheme};
use desktop::app::{App, EngineStatus};
use desktop::bridge::EventBridge;
use desktop::state::{Settings, Theme as AppearanceSetting, default_settings_path};
use desktop::storage::{Annotation, Highlight, HistoryStore, default_history_path};
use proxy_engine::ProxyEngine;
use proxy_engine::config::UpstreamProxy;

use crate::format;
use crate::ui::icons::IconName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Traffic,
    Settings,
    Rules,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Index,
    Url,
    Client,
    Method,
    Status,
    Time,
    Duration,
    RequestSize,
    ResponseSize,
}

impl SortKey {
    pub fn label(self) -> &'static str {
        match self {
            SortKey::Index => "#",
            SortKey::Url => "URL",
            SortKey::Client => "客户端",
            SortKey::Method => "方法",
            SortKey::Status => "状态",
            SortKey::Time => "时间",
            SortKey::Duration => "耗时",
            SortKey::RequestSize => "请求",
            SortKey::ResponseSize => "响应",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindFilter {
    All,
    Http,
    Https,
    Json,
    Errors,
}

impl KindFilter {
    pub const ALL: [KindFilter; 5] = [
        KindFilter::All,
        KindFilter::Http,
        KindFilter::Https,
        KindFilter::Json,
        KindFilter::Errors,
    ];

    pub fn label(self) -> &'static str {
        match self {
            KindFilter::All => "全部",
            KindFilter::Http => "HTTP",
            KindFilter::Https => "HTTPS",
            KindFilter::Json => "JSON",
            KindFilter::Errors => "错误",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestTab {
    Summary,
    Headers,
    Query,
    Body,
    Raw,
}

impl RequestTab {
    pub const ALL: [RequestTab; 5] = [
        RequestTab::Summary,
        RequestTab::Headers,
        RequestTab::Query,
        RequestTab::Body,
        RequestTab::Raw,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RequestTab::Summary => "概要",
            RequestTab::Headers => "首部",
            RequestTab::Query => "查询",
            RequestTab::Body => "正文",
            RequestTab::Raw => "原始",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseTab {
    Summary,
    Headers,
    Body,
    Raw,
    Json,
}

impl ResponseTab {
    pub const ALL: [ResponseTab; 5] = [
        ResponseTab::Summary,
        ResponseTab::Headers,
        ResponseTab::Body,
        ResponseTab::Raw,
        ResponseTab::Json,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ResponseTab::Summary => "概要",
            ResponseTab::Headers => "首部",
            ResponseTab::Body => "正文",
            ResponseTab::Raw => "原始",
            ResponseTab::Json => "JSON",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Facet {
    pub name: String,
    pub count: usize,
    pub icon: IconName,
}

#[derive(Debug, Clone)]
pub struct RowItem {
    pub index: usize,
    pub summary: RequestSummary,
    pub client: String,
    pub pinned: bool,
    pub highlight: Highlight,
    pub comment: String,
}

#[derive(Debug, Clone)]
pub struct Detail {
    pub summary: RequestSummary,
    pub client: String,
    pub status: Option<u16>,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub request_bytes: u64,
    pub response_bytes: u64,
    pub content_type: Option<String>,
    pub request_headers: Vec<(String, String)>,
    pub response_headers: Vec<(String, String)>,
    pub query: Vec<(String, String)>,
    pub request_body: String,
    pub response_body: String,
    pub request_raw: String,
    pub response_raw: String,
    pub response_content_type: Option<String>,
    pub pinned: bool,
    pub highlight: Highlight,
    pub comment: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub label: String,
    pub running: bool,
    pub failed: bool,
    pub listen: Option<String>,
    pub ca_path: Option<PathBuf>,
    pub error: Option<String>,
    pub captured: usize,
    pub system_proxy: bool,
}

impl Status {
    pub fn is_running(&self) -> bool {
        self.running
    }
}

pub struct Model {
    pub app: App,
    pub settings: Settings,
    pub view: View,
    pub kind: KindFilter,
    pub method: Option<String>,
    pub query: String,
    pub host: Option<String>,
    pub client: Option<String>,
    pub pinned_only: bool,
    pub pinned: HashSet<RequestId>,
    pub selected: Option<RequestId>,
    pub auto_select: bool,
    pub request_tab: RequestTab,
    pub response_tab: ResponseTab,
    pub sort: SortKey,
    pub ascending: bool,
    pub rows: Vec<RowItem>,
    pub hosts: Vec<Facet>,
    pub clients: Vec<Facet>,
    pub detail: Option<Detail>,
    pub status: Status,
    pub settings_message: Option<(bool, String)>,
    pub notice: Option<String>,
    pub rules_dirty: bool,
    pub draft: SettingsDraft,
    pub expanded_sections: Sections,
    pub annotations: HashMap<RequestId, Annotation>,
    pub history_count: usize,
    pub store: Option<HistoryStore>,
    clients_by_id: HashMap<RequestId, String>,
    detail_dirty: bool,
    rx: Option<Receiver<EngineEvent>>,
    engine: Option<Arc<ProxyEngine>>,
    runtime: Option<tokio::runtime::Runtime>,
    failure: Arc<Mutex<Option<String>>>,
    started: bool,
}

#[derive(Debug, Clone)]
pub struct SettingsDraft {
    pub port: String,
    pub upstream: String,
    pub max_body_mb: String,
    pub capture: bool,
    pub system_proxy: bool,
    pub appearance: AppearanceSetting,
}

impl SettingsDraft {
    fn from_settings(settings: &Settings) -> Self {
        Self {
            port: settings.listen_addr.port().to_string(),
            upstream: settings
                .upstream_proxy
                .as_ref()
                .map(UpstreamProxy::authority)
                .unwrap_or_default(),
            max_body_mb: (settings.max_body_bytes / (1024 * 1024)).max(1).to_string(),
            capture: settings.capture_enabled,
            system_proxy: settings.system_proxy,
            appearance: settings.theme,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Sections {
    pub favorites: bool,
    pub apps: bool,
    pub domains: bool,
}

impl Default for Sections {
    fn default() -> Self {
        Self {
            favorites: true,
            apps: true,
            domains: true,
        }
    }
}

impl Model {
    pub fn new(settings: Settings) -> Self {
        let store = HistoryStore::open(default_history_path()).ok();
        Self::with_store(settings, store)
    }

    /// 指定历史数据库（`None` 表示只保存在内存）。
    pub fn with_store(settings: Settings, store: Option<HistoryStore>) -> Self {
        let mut app = App::new(settings.clone());
        let mut annotations = HashMap::new();
        if let Some(store) = &store {
            let limit = settings.store_capacity.clamp(1, 5000);
            let records = store.load_recent(limit);
            for record in records {
                annotations.insert(record.exchange.request.id, record.annotation);
                app.restore([record.exchange]);
            }
        }
        let history_count = store.as_ref().map_or(0, HistoryStore::count);
        let draft = SettingsDraft::from_settings(&settings);
        let mut model = Self {
            app,
            settings,
            view: View::Traffic,
            kind: KindFilter::All,
            method: None,
            query: String::new(),
            host: None,
            client: None,
            pinned_only: false,
            pinned: HashSet::new(),
            selected: None,
            auto_select: false,
            request_tab: RequestTab::Summary,
            response_tab: ResponseTab::Summary,
            sort: SortKey::Index,
            ascending: true,
            rows: Vec::new(),
            hosts: Vec::new(),
            clients: Vec::new(),
            detail: None,
            status: Status::default(),
            settings_message: None,
            notice: None,
            rules_dirty: false,
            draft,
            expanded_sections: Sections::default(),
            annotations,
            history_count,
            store,
            clients_by_id: HashMap::new(),
            detail_dirty: false,
            rx: None,
            engine: None,
            runtime: None,
            failure: Arc::new(Mutex::new(None)),
            started: false,
        };
        for summary in model.app.requests.iter() {
            if let Some(exchange) = model.app.exchange(summary.id) {
                model
                    .clients_by_id
                    .insert(summary.id, crate::format::client_name(&exchange.request));
            }
        }
        model.refresh();
        model
    }

    pub fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        self.apply_appearance();
        self.start_engine();
        self.refresh();
    }

    pub fn shutdown(&mut self) {
        self.stop_engine();
    }

    /// 从事件桥取走已到达的事件；有变化时返回 `true`。
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        let mut events = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            self.apply_event(event);
            changed = true;
        }
        if let Some(error) = self.failure.lock().ok().and_then(|mut slot| slot.take()) {
            self.app.handle_event(EngineEvent::Log {
                level: LogLevel::Error,
                message: error,
            });
            changed = true;
        }
        if changed {
            self.refresh();
        }
        changed
    }

    fn apply_event(&mut self, event: EngineEvent) {
        if let EngineEvent::RequestCompleted { exchange } = &event {
            if let Some(store) = &self.store {
                store.insert_exchange(exchange, self.app.requests.capacity());
            }
            self.history_count += 1;
        }
        match &event {
            EngineEvent::RequestStarted { request } => {
                self.clients_by_id
                    .insert(request.id, format::client_name(request));
            }
            EngineEvent::RequestCompleted { exchange } => {
                self.clients_by_id
                    .insert(exchange.request.id, format::client_name(&exchange.request));
                if self.auto_select {
                    self.selected = Some(exchange.request.id);
                    self.detail_dirty = true;
                } else if self.selected == Some(exchange.request.id) {
                    self.detail_dirty = true;
                }
            }
            EngineEvent::Started { .. } => {
                self.status.error = None;
            }
            _ => {}
        }
        self.app.handle_event(event);
        self.detail_dirty = true;
    }

    fn refresh(&mut self) {
        let live: HashSet<RequestId> = self.app.requests.iter().map(|row| row.id).collect();
        self.clients_by_id.retain(|id, _| live.contains(id));
        self.rebuild_rows();
        self.rebuild_facets();
        if self.detail_dirty || self.detail.is_none() {
            self.rebuild_detail();
            self.detail_dirty = false;
        }
        self.refresh_status();
    }

    fn refresh_status(&mut self) {
        let mut status = Status {
            label: self.app.status.label().to_string(),
            running: self.app.status == EngineStatus::Running,
            failed: self.app.status == EngineStatus::Failed,
            listen: self.app.listen_addr.clone(),
            ca_path: self.app.ca_cert_path.clone(),
            error: self.app.last_error.clone(),
            captured: self.app.requests.len(),
            system_proxy: self.settings.system_proxy,
        };
        if self.engine.is_some() && !status.running && !status.failed {
            status.label = "启动中".to_string();
        }
        if let Some(error) = self.failure.lock().ok().and_then(|slot| slot.clone()) {
            status.error = Some(error);
        }
        self.status = status;
    }

    fn rebuild_rows(&mut self) {
        let mut rows = Vec::with_capacity(self.app.requests.len());
        for (position, summary) in self.app.requests.iter().enumerate() {
            if !self.matches(summary) {
                continue;
            }
            let client = self
                .clients_by_id
                .get(&summary.id)
                .cloned()
                .unwrap_or_else(|| "未知".to_string());
            let annotation = self
                .annotations
                .get(&summary.id)
                .cloned()
                .unwrap_or_default();
            rows.push(RowItem {
                index: position + 1,
                summary: summary.clone(),
                client,
                pinned: self.pinned.contains(&summary.id),
                highlight: annotation.highlight,
                comment: annotation.comment,
            });
        }
        let sort = self.sort;
        let ascending = self.ascending;
        rows.sort_by(|a, b| {
            let order = match sort {
                SortKey::Index => a.index.cmp(&b.index),
                SortKey::Url => a.summary.url.cmp(&b.summary.url),
                SortKey::Client => a.client.cmp(&b.client),
                SortKey::Method => a.summary.method.cmp(&b.summary.method),
                SortKey::Status => a.summary.status.cmp(&b.summary.status),
                SortKey::Time => a.summary.started_at.cmp(&b.summary.started_at),
                SortKey::Duration => a.summary.duration_ms.cmp(&b.summary.duration_ms),
                SortKey::RequestSize => a.summary.request_bytes.cmp(&b.summary.request_bytes),
                SortKey::ResponseSize => a.summary.response_bytes.cmp(&b.summary.response_bytes),
            }
            .then_with(|| a.index.cmp(&b.index));
            if ascending { order } else { order.reverse() }
        });
        self.rows = rows;
    }

    fn rebuild_facets(&mut self) {
        let mut hosts: HashMap<String, usize> = HashMap::new();
        let mut clients: HashMap<String, usize> = HashMap::new();
        for summary in self.app.requests.iter() {
            *hosts.entry(summary.host.clone()).or_default() += 1;
            let client = self
                .clients_by_id
                .get(&summary.id)
                .cloned()
                .unwrap_or_else(|| "未知".to_string());
            *clients.entry(client).or_default() += 1;
        }
        self.hosts = facet_list(hosts, IconName::Globe);
        self.clients = facet_list(clients, IconName::App);
    }

    fn rebuild_detail(&mut self) {
        let Some(id) = self.selected else {
            self.detail = None;
            return;
        };
        let Some(exchange) = self.app.exchange(id) else {
            self.detail = None;
            self.selected = None;
            return;
        };
        let summary = self
            .app
            .requests
            .get(id)
            .cloned()
            .unwrap_or_else(|| exchange.summary());
        let client = self
            .clients_by_id
            .get(&id)
            .cloned()
            .unwrap_or_else(|| "未知".to_string());
        let request_content_type =
            format::content_type(&exchange.request.headers).map(str::to_string);
        let mut request_body =
            format::body_preview(&exchange.request.body, request_content_type.as_deref());
        if exchange.request.body_truncated {
            request_body.push_str("\n\n… 已按抓取上限截断");
        } else if !exchange.request.body_complete {
            request_body.push_str("\n\n… body 尚未读完");
        }
        let response_content_type = exchange
            .response
            .as_ref()
            .and_then(|response| format::content_type(&response.headers))
            .map(str::to_string);
        let mut response_body = exchange.response.as_ref().map_or_else(
            || "（等待响应）".to_string(),
            |response| format::body_preview(&response.body, response_content_type.as_deref()),
        );
        if let Some(response) = &exchange.response {
            if response.body_truncated {
                response_body.push_str("\n\n… 已按抓取上限截断");
            } else if !response.body_complete {
                response_body.push_str("\n\n… body 尚未读完");
            }
        }
        self.detail = Some(Detail {
            summary,
            client,
            status: exchange.status(),
            error: exchange.error.clone(),
            duration_ms: exchange.duration_ms,
            request_bytes: exchange.request.body.len() as u64,
            response_bytes: exchange
                .response
                .as_ref()
                .map_or(0, |response| response.body.len() as u64),
            content_type: request_content_type.clone(),
            request_headers: format::headers_rows(&exchange.request.headers),
            response_headers: exchange
                .response
                .as_ref()
                .map(|response| format::headers_rows(&response.headers))
                .unwrap_or_default(),
            query: format::query_pairs(&exchange.request.url),
            request_body,
            response_body,
            request_raw: format::raw_request(&exchange.request),
            response_raw: exchange
                .response
                .as_ref()
                .map(format::raw_response)
                .unwrap_or_else(|| "（等待响应）".to_string()),
            response_content_type,
            pinned: self.pinned.contains(&id),
            highlight: self
                .annotations
                .get(&id)
                .map_or(Highlight::None, |annotation| annotation.highlight),
            comment: self
                .annotations
                .get(&id)
                .map(|annotation| annotation.comment.clone())
                .unwrap_or_default(),
        });
    }

    fn matches(&self, summary: &RequestSummary) -> bool {
        if self.pinned_only && !self.pinned.contains(&summary.id) {
            return false;
        }
        if let Some(host) = &self.host
            && &summary.host != host
        {
            return false;
        }
        if let Some(client) = &self.client {
            let row_client = self
                .clients_by_id
                .get(&summary.id)
                .cloned()
                .unwrap_or_else(|| "未知".to_string());
            if &row_client != client {
                return false;
            }
        }
        if let Some(method) = &self.method
            && !summary.method.eq_ignore_ascii_case(method)
        {
            return false;
        }
        match self.kind {
            KindFilter::All => {}
            KindFilter::Http => {
                if summary.scheme != Scheme::Http {
                    return false;
                }
            }
            KindFilter::Https => {
                if summary.scheme != Scheme::Https {
                    return false;
                }
            }
            KindFilter::Json => {
                let is_json = self.app.exchange(summary.id).is_some_and(|exchange| {
                    format::is_json(format::content_type(&exchange.request.headers))
                        || exchange.response.as_ref().is_some_and(|response| {
                            format::is_json(format::content_type(&response.headers))
                        })
                });
                if !is_json {
                    return false;
                }
            }
            KindFilter::Errors => {
                let is_error =
                    summary.error.is_some() || summary.status.is_some_and(|status| status >= 400);
                if !is_error {
                    return false;
                }
            }
        }
        let query = self.query.trim().to_lowercase();
        query.is_empty()
            || summary.url.to_lowercase().contains(&query)
            || summary.host.to_lowercase().contains(&query)
            || summary.method.to_lowercase().contains(&query)
    }

    pub fn select(&mut self, id: RequestId) {
        self.selected = Some(id);
        self.request_tab = RequestTab::Summary;
        self.response_tab = ResponseTab::Summary;
        self.detail_dirty = true;
        self.refresh();
    }

    pub fn clear(&mut self) {
        self.app.clear();
        self.pinned.clear();
        self.annotations.clear();
        self.selected = None;
        self.detail = None;
        self.clients_by_id.clear();
        self.refresh();
    }

    pub fn toggle_pin(&mut self) {
        let Some(id) = self.selected else {
            return;
        };
        self.toggle_pin_id(id);
    }

    pub fn toggle_pin_id(&mut self, id: RequestId) {
        if !self.pinned.remove(&id) {
            self.pinned.insert(id);
        }
        if self.selected == Some(id) {
            self.detail_dirty = true;
        }
        self.refresh();
    }

    pub fn remove(&mut self, id: RequestId) {
        self.app.remove(id);
        self.pinned.remove(&id);
        self.clients_by_id.remove(&id);
        self.annotations.remove(&id);
        if let Some(store) = &self.store {
            store.delete(id);
        }
        self.history_count = self.history_count.saturating_sub(1);
        if self.selected == Some(id) {
            self.selected = None;
            self.detail = None;
            self.detail_dirty = true;
        }
        self.refresh();
    }

    pub fn annotation(&self, id: RequestId) -> Annotation {
        self.annotations.get(&id).cloned().unwrap_or_default()
    }

    pub fn set_highlight(&mut self, id: RequestId, highlight: Highlight) {
        let annotation = self.annotations.entry(id).or_default();
        annotation.highlight = highlight;
        if let Some(store) = &self.store {
            store.update_annotation(id, annotation);
        }
        if self.selected == Some(id) {
            self.detail_dirty = true;
        }
        self.refresh();
    }

    pub fn set_comment(&mut self, id: RequestId, comment: String) {
        let annotation = self.annotations.entry(id).or_default();
        annotation.comment = comment;
        if let Some(store) = &self.store {
            store.update_annotation(id, annotation);
        }
        if self.selected == Some(id) {
            self.detail_dirty = true;
        }
        self.refresh();
    }

    pub fn persist_annotation(&self, id: RequestId) {
        if let Some(store) = &self.store
            && let Some(annotation) = self.annotations.get(&id)
        {
            store.update_annotation(id, annotation);
        }
    }

    pub fn clear_history(&mut self) {
        if let Some(store) = &self.store {
            store.clear();
        }
        self.annotations.clear();
        self.history_count = 0;
        self.clear();
    }

    pub fn url_of(&self, id: RequestId) -> Option<String> {
        self.app.requests.get(id).map(|summary| summary.url.clone())
    }

    pub fn curl_of(&self, id: RequestId) -> Option<String> {
        self.app
            .exchange(id)
            .map(|exchange| crate::format::curl(&exchange.request))
    }

    pub fn request_raw_of(&self, id: RequestId) -> Option<String> {
        self.app
            .exchange(id)
            .map(|exchange| crate::format::raw_request(&exchange.request))
    }

    pub fn response_raw_of(&self, id: RequestId) -> Option<String> {
        self.app.exchange(id).map(|exchange| {
            exchange
                .response
                .as_ref()
                .map(crate::format::raw_response)
                .unwrap_or_else(String::new)
        })
    }

    pub fn set_kind(&mut self, kind: KindFilter) {
        self.kind = kind;
        self.refresh();
    }

    pub fn set_method(&mut self, method: Option<String>) {
        self.method = method;
        self.refresh();
    }

    pub fn reset_filters(&mut self) {
        self.query.clear();
        self.kind = KindFilter::All;
        self.method = None;
        self.host = None;
        self.client = None;
        self.pinned_only = false;
        self.refresh();
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.refresh();
    }

    pub fn set_host(&mut self, host: Option<String>) {
        self.host = host;
        self.refresh();
    }

    pub fn set_client(&mut self, client: Option<String>) {
        self.client = client;
        self.refresh();
    }

    pub fn set_pinned_only(&mut self, pinned: bool) {
        self.pinned_only = pinned;
        self.refresh();
    }

    pub fn set_request_tab(&mut self, tab: RequestTab) {
        self.request_tab = tab;
    }

    pub fn set_response_tab(&mut self, tab: ResponseTab) {
        self.response_tab = tab;
    }

    pub fn set_view(&mut self, view: View) {
        self.view = view;
        self.settings_message = None;
        if view == View::Settings {
            self.draft = SettingsDraft::from_settings(&self.settings);
        }
    }

    pub fn set_draft_port(&mut self, value: String) {
        self.draft.port = value;
    }

    pub fn set_draft_upstream(&mut self, value: String) {
        self.draft.upstream = value;
    }

    pub fn set_draft_max_body(&mut self, value: String) {
        self.draft.max_body_mb = value;
    }

    pub fn set_draft_capture(&mut self, value: bool) {
        self.draft.capture = value;
    }

    pub fn set_draft_system_proxy(&mut self, value: bool) {
        self.draft.system_proxy = value;
    }

    pub fn set_draft_appearance(&mut self, value: AppearanceSetting) {
        self.draft.appearance = value;
        self.settings.theme = value;
        self.apply_appearance();
        let _ = self.settings.save(&default_settings_path());
    }

    pub fn apply_draft(&mut self) {
        let port = self.draft.port.trim().parse::<u16>();
        let max_body = self.draft.max_body_mb.trim().parse::<usize>();
        let (Ok(port), Ok(max_body)) = (port, max_body) else {
            self.settings_message = Some((false, "端口与正文上限必须是数字".to_string()));
            return;
        };
        let upstream = if self.draft.upstream.trim().is_empty() {
            None
        } else {
            match UpstreamProxy::parse(self.draft.upstream.trim()) {
                Ok(proxy) => Some(proxy),
                Err(error) => {
                    self.settings_message = Some((false, format!("上游代理无效：{error}")));
                    return;
                }
            }
        };
        let capture = self.draft.capture;
        let system_proxy = self.draft.system_proxy;
        let appearance = self.draft.appearance;
        self.apply_settings(port, capture, system_proxy, max_body, upstream, appearance);
    }

    pub fn set_auto_select(&mut self, value: bool) {
        self.auto_select = value;
    }

    pub fn set_sort(&mut self, key: SortKey) {
        if self.sort == key {
            self.ascending = !self.ascending;
        } else {
            self.sort = key;
            self.ascending = true;
        }
        self.refresh();
    }

    pub fn toggle_sort(&mut self) {
        if self.sort == SortKey::Index {
            self.ascending = !self.ascending;
        } else {
            self.sort = SortKey::Index;
            self.ascending = false;
        }
        self.refresh();
    }

    pub fn toggle_section(&mut self, section: Section) {
        match section {
            Section::Favorites => {
                self.expanded_sections.favorites = !self.expanded_sections.favorites
            }
            Section::Apps => self.expanded_sections.apps = !self.expanded_sections.apps,
            Section::Domains => self.expanded_sections.domains = !self.expanded_sections.domains,
        }
    }

    pub fn toggle_engine(&mut self) {
        if self.engine.is_some() && !self.status.failed {
            self.stop_engine();
        } else {
            self.start_engine();
        }
        self.refresh();
    }

    pub fn start_engine(&mut self) {
        if self.engine.is_some() {
            if self.status.failed {
                self.stop_engine();
            } else {
                return;
            }
        }
        if let Ok(mut slot) = self.failure.lock() {
            *slot = None;
        }
        self.app.status = EngineStatus::Stopped;
        self.app.last_error = None;
        self.status.error = None;
        self.status.label = "启动中".to_string();
        let engine = match ProxyEngine::new(self.settings.engine_config()) {
            Ok(engine) => Arc::new(engine),
            Err(error) => {
                let message = error.to_string();
                self.app.status = EngineStatus::Failed;
                self.app.last_error = Some(message.clone());
                self.status.error = Some(message);
                self.status.label = "异常".to_string();
                self.status.failed = true;
                return;
            }
        };
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                let message = error.to_string();
                self.app.status = EngineStatus::Failed;
                self.app.last_error = Some(message.clone());
                self.status.error = Some(message);
                self.status.label = "异常".to_string();
                self.status.failed = true;
                return;
            }
        };
        let bridge = runtime.block_on(async { EventBridge::spawn(engine.subscribe()) });
        self.rx = Some(bridge.receiver());
        let failure = self.failure.clone();
        let runner = engine.clone();
        runtime.spawn(async move {
            if let Err(error) = runner.run().await
                && let Ok(mut slot) = failure.lock()
            {
                *slot = Some(error.to_string());
            }
        });
        if self.settings.system_proxy
            && let Err(error) = system_proxy_apply(
                self.settings.listen_addr.ip().to_string(),
                self.settings.listen_addr.port(),
            )
        {
            self.app.last_error = Some(error.clone());
            self.status.error = Some(error);
        }
        self.engine = Some(engine);
        self.runtime = Some(runtime);
    }

    pub fn stop_engine(&mut self) {
        if let Some(engine) = self.engine.take() {
            engine.shutdown();
        }
        self.rx = None;
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(Duration::from_millis(400));
        }
        if let Ok(mut slot) = self.failure.lock() {
            *slot = None;
        }
        if self.settings.system_proxy {
            let _ = system_bridge::proxy::clear();
        }
        self.app.last_error = None;
        self.app.handle_event(EngineEvent::Stopped);
        self.status.label = "已停止".to_string();
        self.status.running = false;
        self.status.failed = false;
        self.refresh_status();
    }

    pub fn apply_settings(
        &mut self,
        port: u16,
        capture: bool,
        system_proxy: bool,
        max_body_mb: usize,
        upstream: Option<UpstreamProxy>,
        appearance: AppearanceSetting,
    ) {
        let was_running = self.engine.is_some();
        if was_running {
            self.stop_engine();
        }
        self.settings.listen_addr.set_port(port);
        self.settings.capture_enabled = capture;
        self.settings.system_proxy = system_proxy;
        self.settings.max_body_bytes = max_body_mb.max(1) * 1024 * 1024;
        self.settings.upstream_proxy = upstream;
        self.settings.theme = appearance;
        self.detail_dirty = true;
        let saved = self.settings.save(&default_settings_path());
        self.settings_message = Some(match saved {
            Ok(()) => (true, "设置已保存".to_string()),
            Err(error) => (false, format!("保存设置失败：{error}")),
        });
        self.apply_appearance();
        if was_running {
            self.start_engine();
        }
        self.refresh();
    }

    pub fn apply_appearance(&self) {
        use nsh_core::platform::Appearance;
        let appearance = match self.settings.theme {
            AppearanceSetting::System => Appearance::System,
            AppearanceSetting::Light => Appearance::Light,
            AppearanceSetting::Dark => Appearance::Dark,
        };
        nsh_core::element::ambient::set_color_scheme(appearance);
    }

    pub fn install_ca(&mut self) {
        let path = self
            .app
            .ca_cert_path
            .clone()
            .unwrap_or_else(|| self.settings.engine_config().ca_cert_path());
        if !path.exists() {
            self.settings_message = Some((
                false,
                format!("根证书尚未生成，请先启动代理：{}", path.display()),
            ));
            return;
        }
        let common_name = self.settings.engine_config().ca_common_name;
        self.settings_message = Some(match system_bridge::cert::install(&path, &common_name) {
            Ok(()) => (true, "根证书已安装到系统信任存储".to_string()),
            Err(error) => (
                false,
                format!(
                    "{error}\n可手动安装：{}",
                    system_bridge::cert::install_hint(&path)
                ),
            ),
        });
    }

    pub fn export_ca(&mut self) {
        let source = self
            .app
            .ca_cert_path
            .clone()
            .unwrap_or_else(|| self.settings.engine_config().ca_cert_path());
        if !source.exists() {
            self.settings_message = Some((
                false,
                format!("根证书尚未生成，请先启动代理：{}", source.display()),
            ));
            return;
        }
        let out = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Downloads")
            .join("packet-sniffer-root-ca.pem");
        if let Some(parent) = out.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        self.settings_message = Some(match std::fs::copy(&source, &out) {
            Ok(_) => (true, format!("已导出到 {}", out.display())),
            Err(error) => (false, format!("导出失败：{error}")),
        });
    }

    pub fn clear_settings_message(&mut self) {
        self.settings_message = None;
    }

    pub fn add_rule(&mut self) {
        self.settings
            .rules
            .push(proxy_engine::rules::Rule::default());
        self.rules_dirty = true;
        self.refresh();
    }

    pub fn remove_rule(&mut self, index: usize) {
        if index < self.settings.rules.len() {
            self.settings.rules.remove(index);
            self.rules_dirty = true;
            self.refresh();
        }
    }

    pub fn update_rule(
        &mut self,
        index: usize,
        update: impl FnOnce(&mut proxy_engine::rules::Rule),
    ) {
        if let Some(rule) = self.settings.rules.get_mut(index) {
            update(rule);
            self.rules_dirty = true;
        }
    }

    /// 保存规则并重启正在运行的引擎，使新规则生效。
    pub fn apply_rules(&mut self) {
        let was_running = self.engine.is_some();
        if was_running {
            self.stop_engine();
        }
        self.rules_dirty = false;
        let saved = self.settings.save(&default_settings_path());
        if was_running {
            self.start_engine();
        }
        self.notice = Some(match saved {
            Ok(()) if was_running => format!(
                "规则已保存并应用（{} 条），代理已重启",
                self.settings.rules.len()
            ),
            Ok(()) => format!("规则已保存（{} 条）", self.settings.rules.len()),
            Err(error) => format!("保存规则失败：{error}"),
        });
        self.refresh();
    }

    pub fn clear_notice(&mut self) {
        self.notice = None;
    }

    /// 把当前筛选 / 排序后的请求摘要导出为 JSON 文件。
    pub fn export_requests(&mut self) {
        let rows: Vec<serde_json::Value> = self
            .rows
            .iter()
            .map(|row| {
                let summary = &row.summary;
                serde_json::json!({
                    "id": summary.id,
                    "method": summary.method,
                    "scheme": summary.scheme.as_str(),
                    "host": summary.host,
                    "path": summary.path,
                    "url": summary.url,
                    "status": summary.status,
                    "duration_ms": summary.duration_ms,
                    "request_bytes": summary.request_bytes,
                    "response_bytes": summary.response_bytes,
                    "started_at": summary.started_at.to_rfc3339(),
                    "completed": summary.completed,
                    "error": summary.error,
                })
            })
            .collect();
        let text = match serde_json::to_string_pretty(&rows) {
            Ok(text) => text,
            Err(error) => {
                self.notice = Some(format!("导出失败：{error}"));
                return;
            }
        };
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let out = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Downloads")
            .join(format!("packet-sniffer-requests-{stamp}.json"));
        if let Some(parent) = out.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        self.notice = Some(match std::fs::write(&out, text) {
            Ok(()) => format!("已导出 {} 条请求到 {}", rows.len(), out.display()),
            Err(error) => format!("导出失败：{error}"),
        });
    }

    pub fn clear_error(&mut self) {
        if self.status.failed {
            self.stop_engine();
        } else {
            self.app.last_error = None;
            self.refresh_status();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Favorites,
    Apps,
    Domains,
}

fn facet_list(counts: HashMap<String, usize>, icon: IconName) -> Vec<Facet> {
    let mut list: Vec<Facet> = counts
        .into_iter()
        .map(|(name, count)| Facet { name, count, icon })
        .collect();
    list.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    list.truncate(24);
    list
}

fn system_proxy_apply(host: String, port: u16) -> Result<(), String> {
    let settings = system_bridge::proxy::ProxySettings::new(host, port);
    system_bridge::proxy::apply(&settings).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use common::types::{CapturedRequest, CapturedResponse, Header, Scheme};
    use uuid::Uuid;

    fn request(host: &str, method: &str, agent: &str) -> CapturedRequest {
        CapturedRequest {
            id: Uuid::new_v4(),
            scheme: Scheme::Https,
            method: method.into(),
            url: format!("https://{host}/"),
            host: host.into(),
            port: 443,
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: vec![Header::new("User-Agent", agent)],
            body: Vec::new(),
            body_truncated: false,
            body_complete: true,
            started_at: Utc::now(),
        }
    }

    fn exchange(request: CapturedRequest, status: u16) -> common::types::CapturedExchange {
        common::types::CapturedExchange {
            request,
            response: Some(CapturedResponse {
                status,
                status_text: "OK".into(),
                version: "HTTP/1.1".into(),
                headers: vec![Header::new("Content-Type", "application/json")],
                body: br#"{"ok":true}"#.to_vec(),
                body_truncated: false,
                body_complete: true,
                received_at: Utc::now(),
            }),
            error: None,
            duration_ms: 12,
            completed_at: Some(Utc::now()),
        }
    }

    #[test]
    fn events_build_rows_and_detail() {
        let mut model = Model::with_store(Settings::default(), None);
        let first = request("api.example.com", "GET", "curl/8.7.1");
        let second = request("web.example.com", "POST", "curl/8.7.1");
        model.apply_event(EngineEvent::RequestCompleted {
            exchange: exchange(first.clone(), 200),
        });
        model.apply_event(EngineEvent::RequestCompleted {
            exchange: exchange(second.clone(), 500),
        });
        model.refresh();

        assert_eq!(model.rows.len(), 2);
        assert_eq!(model.rows[0].summary.id, first.id);
        assert_eq!(model.rows[0].index, 1);
        assert_eq!(model.rows[0].client, "curl");
        assert_eq!(model.rows[1].summary.id, second.id);
        assert_eq!(model.hosts.len(), 2);
        assert_eq!(model.clients[0].name, "curl");

        model.select(first.id);
        let detail = model.detail.as_ref().expect("详情已生成");
        assert_eq!(detail.status, Some(200));
        assert!(detail.request_raw.contains("GET / HTTP/1.1"));

        model.set_kind(KindFilter::Errors);
        assert_eq!(model.rows.len(), 1);
        assert_eq!(model.rows[0].summary.id, second.id);

        model.set_kind(KindFilter::All);
        model.set_host(Some("api.example.com".into()));
        assert_eq!(model.rows.len(), 1);
        assert_eq!(model.rows[0].summary.id, first.id);

        model.set_host(None);
        model.set_client(Some("curl".into()));
        assert_eq!(model.rows.len(), 2);
    }

    #[test]
    fn query_filters_url_and_method() {
        let mut model = Model::with_store(Settings::default(), None);
        model.apply_event(EngineEvent::RequestCompleted {
            exchange: exchange(request("api.example.com", "GET", "curl/8.7.1"), 200),
        });
        model.refresh();

        model.set_query("POST");
        assert!(model.rows.is_empty());
        model.set_query("api.example");
        assert_eq!(model.rows.len(), 1);
        model.set_query("");
        model.set_method(Some("POST".into()));
        assert!(model.rows.is_empty());
        model.set_method(Some("get".into()));
        assert_eq!(model.rows.len(), 1);
    }

    #[test]
    fn pinning_and_clearing_update_the_list() {
        let mut model = Model::with_store(Settings::default(), None);
        let request = request("api.example.com", "GET", "curl/8.7.1");
        model.apply_event(EngineEvent::RequestCompleted {
            exchange: exchange(request.clone(), 200),
        });
        model.refresh();
        model.select(request.id);
        model.toggle_pin();
        assert!(model.pinned.contains(&request.id));
        assert_eq!(model.pinned.len(), 1);
        model.set_pinned_only(true);
        assert_eq!(model.rows.len(), 1);

        model.clear();
        assert!(model.rows.is_empty());
        assert!(model.pinned.is_empty());
        assert!(model.detail.is_none());
        assert!(model.app.requests.is_empty());
    }

    #[test]
    fn remove_deletes_the_request_and_its_detail() {
        let mut model = Model::with_store(Settings::default(), None);
        let first = request("api.example.com", "GET", "curl/8.7.1");
        let second = request("web.example.com", "GET", "curl/8.7.1");
        model.apply_event(EngineEvent::RequestCompleted {
            exchange: exchange(first.clone(), 200),
        });
        model.apply_event(EngineEvent::RequestCompleted {
            exchange: exchange(second.clone(), 200),
        });
        model.refresh();
        model.select(first.id);
        assert!(model.detail.is_some());

        model.remove(first.id);
        assert_eq!(model.rows.len(), 1);
        assert_eq!(model.rows[0].summary.id, second.id);
        assert_eq!(model.selected, None);
        assert!(model.detail.is_none());
        assert!(model.app.exchange(first.id).is_none());
    }

    #[test]
    fn sorting_by_duration_and_status() {
        let mut model = Model::with_store(Settings::default(), None);
        let fast = request("a.example.com", "GET", "curl/8.7.1");
        let slow = request("b.example.com", "GET", "curl/8.7.1");
        model.apply_event(EngineEvent::RequestCompleted {
            exchange: common::types::CapturedExchange {
                request: fast.clone(),
                response: Some(common::types::CapturedResponse {
                    status: 500,
                    status_text: "Server Error".into(),
                    version: "HTTP/1.1".into(),
                    headers: Vec::new(),
                    body: Vec::new(),
                    body_truncated: false,
                    body_complete: true,
                    received_at: Utc::now(),
                }),
                error: None,
                duration_ms: 5,
                completed_at: Some(Utc::now()),
            },
        });
        model.apply_event(EngineEvent::RequestCompleted {
            exchange: common::types::CapturedExchange {
                request: slow.clone(),
                response: Some(common::types::CapturedResponse {
                    status: 200,
                    status_text: "OK".into(),
                    version: "HTTP/1.1".into(),
                    headers: Vec::new(),
                    body: Vec::new(),
                    body_truncated: false,
                    body_complete: true,
                    received_at: Utc::now(),
                }),
                error: None,
                duration_ms: 50,
                completed_at: Some(Utc::now()),
            },
        });
        model.refresh();

        model.set_sort(SortKey::Duration);
        assert_eq!(model.rows[0].summary.id, fast.id);
        model.set_sort(SortKey::Duration);
        assert_eq!(model.rows[0].summary.id, slow.id);
        model.set_sort(SortKey::Status);
        assert_eq!(model.rows[0].summary.id, slow.id);
    }

    #[test]
    fn highlights_and_comments_reach_rows_and_detail() {
        let mut model = Model::with_store(Settings::default(), None);
        let request = request("api.example.com", "GET", "curl/8.7.1");
        model.apply_event(EngineEvent::RequestCompleted {
            exchange: exchange(request.clone(), 200),
        });
        model.refresh();
        model.select(request.id);
        model.set_highlight(request.id, Highlight::Orange);
        model.set_comment(request.id, "重点".into());

        assert_eq!(model.rows[0].highlight, Highlight::Orange);
        assert_eq!(model.rows[0].comment, "重点");
        let detail = model.detail.as_ref().expect("详情存在");
        assert_eq!(detail.highlight, Highlight::Orange);
        assert_eq!(detail.comment, "重点");
        assert_eq!(model.annotation(request.id).comment, "重点");
    }

    #[test]
    fn rules_can_be_added_edited_and_removed() {
        let mut model = Model::with_store(Settings::default(), None);
        model.add_rule();
        assert_eq!(model.settings.rules.len(), 1);
        assert!(model.rules_dirty);
        model.update_rule(0, |rule| {
            rule.name = "block".into();
            rule.enabled = false;
        });
        assert_eq!(model.settings.rules[0].name, "block");
        assert!(!model.settings.rules[0].enabled);
        model.remove_rule(0);
        assert!(model.settings.rules.is_empty());
    }

    #[test]
    fn newest_first_toggle_reverses_rows() {
        let mut model = Model::with_store(Settings::default(), None);
        for index in 0..3 {
            model.apply_event(EngineEvent::RequestCompleted {
                exchange: exchange(
                    request(&format!("host{index}.example.com"), "GET", "curl/8.7.1"),
                    200,
                ),
            });
        }
        model.refresh();
        let first = model.rows[0].summary.id;
        model.toggle_sort();
        assert_ne!(model.rows[0].summary.id, first);
        model.toggle_sort();
        assert_eq!(model.rows[0].summary.id, first);
    }
}
