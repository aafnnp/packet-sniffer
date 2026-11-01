//! 请求转发：抓取请求 / 响应并推送给 UI，同时保证转发语义不变。

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use chrono::Utc;
use common::types::{CapturedExchange, CapturedRequest, CapturedResponse, EngineEvent, Scheme};
use http::header::{HeaderName, HeaderValue};
use http::{Request, Response, StatusCode, Version};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::client::conn::http1 as client_http1;
use hyper_util::rt::TokioIo;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::protocol::body::{BodyRecorder, RecordingBody};
use crate::protocol::http1::{self, RequestTarget};
use crate::proxy::upstream;
use crate::rules::{self, RuleAction};
use crate::{BoxError, EngineError, ProxyBody, ProxyContext, Result};

/// 处理一个普通 HTTP 请求（代理 absolute-form 或 MITM 后的 origin-form）。
pub(crate) async fn handle(
    ctx: Arc<ProxyContext>,
    scheme: Scheme,
    tunnel_authority: Option<String>,
    req: Request<Incoming>,
) -> Response<ProxyBody> {
    match forward(ctx, scheme, tunnel_authority, req).await {
        Ok(response) => response,
        Err(err) => {
            warn!(error = %err, "请求转发失败");
            error_response(StatusCode::BAD_GATEWAY, &format!("代理转发失败：{err}"))
        }
    }
}

async fn forward(
    ctx: Arc<ProxyContext>,
    scheme: Scheme,
    tunnel_authority: Option<String>,
    req: Request<Incoming>,
) -> Result<Response<ProxyBody>> {
    let started_at = Utc::now();
    let clock = Instant::now();

    let (parts, body) = req.into_parts();
    let target = http1::resolve_target(
        &parts.uri,
        &parts.headers,
        scheme,
        tunnel_authority.as_deref(),
    )?;

    // 目标就是代理自己（例如浏览器直接打开了代理端口）：返回内置状态页。
    // 否则会把自己再当成上游转发，形成无限递归并最终全部 502。
    if is_self_target(&target, ctx.listen_addr()) {
        return Ok(self_request_response(&target.path));
    }

    let (request_recorder, _request_done) = BodyRecorder::new(ctx.config.max_body_bytes);
    let request_body = RecordingBody::new(body, request_recorder.clone());

    let mut captured_request = CapturedRequest {
        id: Uuid::new_v4(),
        scheme: target.scheme,
        method: parts.method.as_str().to_string(),
        url: target.url.clone(),
        host: target.host.clone(),
        port: target.port,
        path: target.path.clone(),
        version: http1::version_to_string(parts.version),
        headers: http1::header_map_to_headers(&parts.headers),
        body: Vec::new(),
        body_truncated: false,
        body_complete: false,
        started_at,
    };
    ctx.emit(EngineEvent::RequestStarted {
        request: captured_request.clone(),
    });

    let matched = rules::matching(&ctx.config.rules, &captured_request);
    if let Some(rule) = rules::terminal(&matched) {
        return short_circuit(&ctx, rule, &matched, captured_request, clock);
    }

    let mut upstream_request = Request::builder()
        .method(parts.method.clone())
        .uri(target.path.clone())
        .version(Version::HTTP_11)
        .body(request_body)?;
    let mut upstream_headers = http1::filter_headers(&parts.headers);
    for rule in &matched {
        if let RuleAction::AddRequestHeader { name, value } = &rule.action {
            apply_header(&mut upstream_headers, name, value, "请求头规则");
        }
    }
    *upstream_request.headers_mut() = upstream_headers;

    let stream = match upstream::connect(
        &target.host,
        target.port,
        target.scheme == Scheme::Https,
        ctx.config.connect_timeout,
        ctx.tls_client.clone(),
        ctx.config.upstream_proxy.as_ref(),
    )
    .await
    {
        Ok(stream) => stream,
        Err(err) => {
            record_failure(
                &ctx,
                captured_request,
                &request_recorder,
                clock,
                err.to_string(),
            );
            return Err(err);
        }
    };

    let (mut sender, connection) = match client_http1::handshake(TokioIo::new(stream)).await {
        Ok(parts) => parts,
        Err(err) => {
            record_failure(
                &ctx,
                captured_request,
                &request_recorder,
                clock,
                err.to_string(),
            );
            return Err(EngineError::Http(err));
        }
    };
    tokio::spawn(async move {
        if let Err(err) = connection.await {
            debug!(error = %err, "上游连接关闭");
        }
    });

    let response = match sender.send_request(upstream_request).await {
        Ok(response) => response,
        Err(err) => {
            let snapshot = request_recorder.snapshot();
            captured_request.body = snapshot.bytes;
            captured_request.body_truncated = snapshot.truncated;
            captured_request.body_complete = snapshot.finished;

            let exchange = CapturedExchange {
                request: captured_request,
                response: None,
                error: Some(err.to_string()),
                duration_ms: clock.elapsed().as_millis() as u64,
                completed_at: Some(Utc::now()),
            };
            ctx.store.upsert(exchange.clone());
            ctx.emit(EngineEvent::RequestCompleted { exchange });
            return Err(EngineError::Http(err));
        }
    };

    let (response_parts, response_body) = response.into_parts();
    let (response_recorder, mut response_done) = BodyRecorder::new(ctx.config.max_body_bytes);
    let tapped_body: ProxyBody = RecordingBody::new(response_body, response_recorder.clone())
        .map_err(|err| -> BoxError { Box::new(err) })
        .boxed();

    let mut client_response = Response::builder()
        .status(response_parts.status)
        .version(Version::HTTP_11)
        .body(tapped_body)?;
    let mut response_headers = http1::filter_headers(&response_parts.headers);
    for rule in &matched {
        if let RuleAction::AddResponseHeader { name, value } = &rule.action {
            apply_header(&mut response_headers, name, value, "响应头规则");
        }
    }
    *client_response.headers_mut() = response_headers;

    let request_snapshot = request_recorder.snapshot();
    captured_request.body = request_snapshot.bytes;
    captured_request.body_truncated = request_snapshot.truncated;
    captured_request.body_complete = request_snapshot.finished;

    let status = response_parts.status;
    let response_version = http1::version_to_string(response_parts.version);
    let response_headers = http1::header_map_to_headers(&response_parts.headers);
    let duration_ms = clock.elapsed().as_millis() as u64;
    let idle_timeout = ctx.config.body_idle_timeout;

    // 响应体仍在向客户端流动，等它读完（或超时）后再把完整记录入库并通知 UI。
    let capture_ctx = ctx.clone();
    tokio::spawn(async move {
        let _ = tokio::time::timeout(idle_timeout, &mut response_done).await;
        let snapshot = response_recorder.snapshot();

        let captured_response = CapturedResponse {
            status: status.as_u16(),
            status_text: http1::status_text(status),
            version: response_version,
            headers: response_headers,
            body: snapshot.bytes,
            body_truncated: snapshot.truncated,
            body_complete: snapshot.finished,
            received_at: Utc::now(),
        };

        let exchange = CapturedExchange {
            request: captured_request,
            response: Some(captured_response),
            error: None,
            duration_ms,
            completed_at: Some(Utc::now()),
        };
        capture_ctx.store.upsert(exchange.clone());
        capture_ctx.emit(EngineEvent::RequestCompleted { exchange });
    });

    Ok(client_response)
}

/// 连接或握手失败时也要落库并通知 UI，避免请求永远停在“进行中”。
fn record_failure(
    ctx: &ProxyContext,
    mut request: CapturedRequest,
    recorder: &BodyRecorder,
    clock: Instant,
    error: String,
) {
    let snapshot = recorder.snapshot();
    request.body = snapshot.bytes;
    request.body_truncated = snapshot.truncated;
    request.body_complete = snapshot.finished;

    let exchange = CapturedExchange {
        request,
        response: None,
        error: Some(error),
        duration_ms: clock.elapsed().as_millis() as u64,
        completed_at: Some(Utc::now()),
    };
    ctx.store.upsert(exchange.clone());
    ctx.emit(EngineEvent::RequestCompleted { exchange });
}

fn apply_header(headers: &mut http::HeaderMap, name: &str, value: &str, context: &str) {
    match (
        HeaderName::from_bytes(name.trim().as_bytes()),
        HeaderValue::from_str(value),
    ) {
        (Ok(name), Ok(value)) => {
            headers.insert(name, value);
        }
        _ => warn!(name, value, context, "规则中的首部名称或取值无效"),
    }
}

/// 规则要求阻止请求或直接返回模拟响应。
fn short_circuit(
    ctx: &ProxyContext,
    rule: &rules::Rule,
    matched: &[&rules::Rule],
    mut captured_request: CapturedRequest,
    clock: Instant,
) -> Result<Response<ProxyBody>> {
    let (status, content_type, body) = match &rule.action {
        RuleAction::Block { status } => (
            *status,
            "text/plain; charset=utf-8".to_string(),
            format!("Blocked by rule: {}\n", rule.name),
        ),
        RuleAction::Mock {
            status,
            content_type,
            body,
        } => (*status, content_type.clone(), body.clone()),
        _ => unreachable!("short_circuit 只用于终止型规则"),
    };
    let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::FORBIDDEN);
    captured_request.body.clear();
    captured_request.body_complete = false;
    let bytes = Bytes::from(body);

    let mut headers = http::HeaderMap::new();
    apply_header(
        &mut headers,
        "content-type",
        if content_type.is_empty() {
            "text/plain; charset=utf-8"
        } else {
            &content_type
        },
        "规则模拟响应",
    );
    apply_header(
        &mut headers,
        "x-packet-sniffer-rule",
        &rule.name,
        "规则标记",
    );
    for rule in matched {
        if let RuleAction::AddResponseHeader { name, value } = &rule.action {
            apply_header(&mut headers, name, value, "响应头规则");
        }
    }

    let captured_response = CapturedResponse {
        status: status_code.as_u16(),
        status_text: http1::status_text(status_code),
        version: "HTTP/1.1".to_string(),
        headers: http1::header_map_to_headers(&headers),
        body: bytes.to_vec(),
        body_truncated: false,
        body_complete: true,
        received_at: Utc::now(),
    };
    let exchange = CapturedExchange {
        request: captured_request,
        response: Some(captured_response),
        error: None,
        duration_ms: clock.elapsed().as_millis() as u64,
        completed_at: Some(Utc::now()),
    };
    ctx.store.upsert(exchange.clone());
    ctx.emit(EngineEvent::RequestCompleted { exchange });

    let mut response = Response::new(Full::new(bytes).map_err(|never| match never {}).boxed());
    *response.status_mut() = status_code;
    *response.headers_mut() = headers;
    Ok(response)
}

/// 判断转发目标是否就是代理自身的监听地址。
fn is_self_target(target: &RequestTarget, listen_addr: SocketAddr) -> bool {
    if target.port != listen_addr.port() {
        return false;
    }
    match target.host.parse::<IpAddr>() {
        Ok(ip) => ip == listen_addr.ip() || ip.is_loopback() || ip.is_unspecified(),
        Err(_) => target.host.eq_ignore_ascii_case("localhost"),
    }
}

/// 代理端口被直接访问时的最小响应。
///
/// 必须拦截「请求指向自己」，否则代理会把自己当成上游，形成无限递归并全部 502。
/// 界面在桌面 GUI 里，这里只回一行纯文本，不提供任何网页。
fn self_request_response(path: &str) -> Response<ProxyBody> {
    if path.starts_with("/favicon") {
        let mut response = Response::new(empty_body());
        *response.status_mut() = StatusCode::NO_CONTENT;
        return response;
    }

    let mut response = Response::new(text_body(
        "packet-sniffer proxy is running; requests to this port are not captured.\n",
    ));
    response.headers_mut().insert(
        ::http::header::CONTENT_TYPE,
        ::http::header::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

/// 生成带文本内容的错误响应。
pub(crate) fn error_response(status: StatusCode, message: &str) -> Response<ProxyBody> {
    let mut response = Response::new(text_body(message));
    *response.status_mut() = status;
    response
}

pub(crate) fn text_body(text: &str) -> ProxyBody {
    Full::new(Bytes::from(text.to_string()))
        .map_err(infallible_to_box)
        .boxed()
}

pub(crate) fn empty_body() -> ProxyBody {
    Full::new(Bytes::new()).map_err(infallible_to_box).boxed()
}

fn infallible_to_box(never: std::convert::Infallible) -> BoxError {
    match never {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::Scheme;

    fn target(host: &str, port: u16) -> RequestTarget {
        RequestTarget {
            scheme: Scheme::Http,
            host: host.to_string(),
            port,
            path: "/".to_string(),
            url: format!("http://{host}:{port}/"),
        }
    }

    #[test]
    fn detects_self_target() {
        let listen: SocketAddr = "127.0.0.1:8899".parse().unwrap();
        assert!(is_self_target(&target("127.0.0.1", 8899), listen));
        assert!(is_self_target(&target("localhost", 8899), listen));
        assert!(is_self_target(&target("0.0.0.0", 8899), listen));
        assert!(is_self_target(&target("::1", 8899), listen));
    }

    #[test]
    fn other_targets_are_not_self() {
        let listen: SocketAddr = "127.0.0.1:8899".parse().unwrap();
        assert!(!is_self_target(&target("127.0.0.1", 80), listen));
        assert!(!is_self_target(&target("example.com", 8899), listen));
        assert!(!is_self_target(&target("192.168.1.10", 8899), listen));
    }
}
