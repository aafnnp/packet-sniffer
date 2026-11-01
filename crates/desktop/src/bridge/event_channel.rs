//! 引擎事件 → UI 线程的管道。
//!
//! 引擎在 tokio 运行时内广播事件，UI 可能在 NSH 主线程（非 async 上下文）消费，
//! 因此这里用一个转发的 tokio 任务把广播接到 `async-channel`，UI 侧既能异步 `recv`
//! 也能用 `recv_blocking` 在同步循环里读取。

use async_channel::{Receiver, Sender};
use common::types::EngineEvent;
use tokio::sync::broadcast;

const UI_CHANNEL_CAPACITY: usize = 1024;

/// UI 侧的事件接收端。
pub struct EventBridge {
    rx: Receiver<EngineEvent>,
}

impl EventBridge {
    /// 必须在 tokio 运行时内调用（内部会 spawn 转发任务）。
    pub fn spawn(source: broadcast::Receiver<EngineEvent>) -> Self {
        let (tx, rx) = async_channel::bounded(UI_CHANNEL_CAPACITY);
        tokio::spawn(forward(source, tx));
        Self { rx }
    }

    /// 直接用已有的 async-channel 接收端构造，便于测试与复用。
    pub fn from_receiver(rx: Receiver<EngineEvent>) -> Self {
        Self { rx }
    }

    pub fn receiver(&self) -> Receiver<EngineEvent> {
        self.rx.clone()
    }

    /// 异步接收下一条事件，通道关闭时返回 `None`。
    pub async fn next(&self) -> Option<EngineEvent> {
        self.rx.recv().await.ok()
    }

    /// 同步接收，供 NSH 主循环 / 命令行使用。
    pub fn next_blocking(&self) -> Option<EngineEvent> {
        self.rx.recv_blocking().ok()
    }

    /// 非阻塞取出当前已到达的事件。
    pub fn try_next(&self) -> Option<EngineEvent> {
        self.rx.try_recv().ok()
    }

    pub fn close(&self) {
        self.rx.close();
    }

    pub fn is_closed(&self) -> bool {
        self.rx.is_closed()
    }
}

async fn forward(mut source: broadcast::Receiver<EngineEvent>, target: Sender<EngineEvent>) {
    loop {
        match source.recv().await {
            Ok(event) => {
                if target.send(event).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                tracing::warn!(skipped, "UI 消费滞后，已跳过部分事件");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::{EngineEvent, LogLevel};
    use std::time::Duration;

    #[tokio::test]
    async fn forwards_broadcast_events() {
        let (sender, receiver) = broadcast::channel(16);
        let bridge = EventBridge::spawn(receiver);

        sender
            .send(EngineEvent::Log {
                level: LogLevel::Info,
                message: "hello".into(),
            })
            .unwrap();

        let event = tokio::time::timeout(Duration::from_secs(1), bridge.next())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(event, EngineEvent::Log { .. }));
        assert!(bridge.try_next().is_none());
    }

    #[tokio::test]
    async fn bridge_closes_with_source() {
        let (sender, receiver) = broadcast::channel(16);
        let bridge = EventBridge::spawn(receiver);
        drop(sender);

        let event = tokio::time::timeout(Duration::from_secs(1), bridge.next())
            .await
            .unwrap();
        assert!(event.is_none());
    }
}
