//! 边转发边录制的 body 包装器。
//!
//! 抓包不能改变转发语义，因此不能先缓冲完再发送：这里对 body 做「旁路录制」，
//! 原始字节仍然原样转发，只把前 N 个字节复制一份用于存储与展示。

use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};

use bytes::{Bytes, BytesMut};
use hyper::body::{Body, Frame, SizeHint};
use tokio::sync::oneshot;

#[derive(Debug)]
struct RecorderState {
    buffer: BytesMut,
    limit: usize,
    truncated: bool,
    finished: bool,
}

/// 录制快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedBody {
    pub bytes: Vec<u8>,
    pub truncated: bool,
    pub finished: bool,
}

impl RecordedBody {
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// 可克隆的录制器：body 包装器持有一个克隆，调用方持有另一个用于取快照。
#[derive(Debug, Clone)]
pub struct BodyRecorder {
    state: Arc<Mutex<RecorderState>>,
    done_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

impl BodyRecorder {
    /// 返回录制器与「body 读完」通知接收端。接收端被丢弃不会影响录制。
    pub fn new(limit: usize) -> (Self, oneshot::Receiver<()>) {
        let (done_tx, done_rx) = oneshot::channel();
        let recorder = Self {
            state: Arc::new(Mutex::new(RecorderState {
                buffer: BytesMut::new(),
                limit,
                truncated: false,
                finished: false,
            })),
            done_tx: Arc::new(Mutex::new(Some(done_tx))),
        };
        (recorder, done_rx)
    }

    fn lock(&self) -> MutexGuard<'_, RecorderState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn record(&self, chunk: &[u8]) {
        let mut state = self.lock();
        if state.finished || state.truncated || chunk.is_empty() {
            return;
        }
        let remaining = state.limit.saturating_sub(state.buffer.len());
        if chunk.len() > remaining {
            state.buffer.extend_from_slice(&chunk[..remaining]);
            state.truncated = true;
        } else {
            state.buffer.extend_from_slice(chunk);
        }
    }

    /// 标记 body 结束（正常读完或出错），只通知一次。
    pub fn finish(&self) {
        {
            let mut state = self.lock();
            if state.finished {
                return;
            }
            state.finished = true;
        }
        let sender = self
            .done_tx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(sender) = sender {
            let _ = sender.send(());
        }
    }

    pub fn snapshot(&self) -> RecordedBody {
        let state = self.lock();
        RecordedBody {
            bytes: state.buffer.to_vec(),
            truncated: state.truncated,
            finished: state.finished,
        }
    }
}

/// 透传 body 同时录制的包装器。
#[derive(Debug)]
pub struct RecordingBody<B> {
    inner: B,
    recorder: BodyRecorder,
}

impl<B> RecordingBody<B> {
    pub fn new(inner: B, recorder: BodyRecorder) -> Self {
        Self { inner, recorder }
    }

    pub fn recorder(&self) -> &BodyRecorder {
        &self.recorder
    }
}

impl<B> Body for RecordingBody<B>
where
    B: Body<Data = Bytes> + Unpin,
{
    type Data = Bytes;
    type Error = B::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Bytes>, Self::Error>>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    this.recorder.record(data);
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(err))) => {
                this.recorder.finish();
                Poll::Ready(Some(Err(err)))
            }
            Poll::Ready(None) => {
                this.recorder.finish();
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

impl<B> Drop for RecordingBody<B> {
    fn drop(&mut self) {
        // hyper 写满 content-length 后可能不再把 body 轮询到 end-of-stream，
        // 这种情况下靠 Drop 收尾，避免抓取结果要等到超时才入库。
        self.recorder.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use http_body_util::Full;

    #[tokio::test]
    async fn records_body_while_passing_through() {
        let (recorder, done) = BodyRecorder::new(4);
        let body = RecordingBody::new(
            Full::new(Bytes::from_static(b"hello world")),
            recorder.clone(),
        );

        let collected = body.collect().await.unwrap().to_bytes();
        assert_eq!(&collected[..], b"hello world");
        done.await.unwrap();

        let snapshot = recorder.snapshot();
        assert_eq!(snapshot.bytes, b"hell");
        assert!(snapshot.truncated);
        assert!(snapshot.finished);
    }

    #[tokio::test]
    async fn small_body_is_not_truncated() {
        let (recorder, _done) = BodyRecorder::new(64);
        let body = RecordingBody::new(Full::new(Bytes::from_static(b"abc")), recorder.clone());
        body.collect().await.unwrap();

        let snapshot = recorder.snapshot();
        assert_eq!(snapshot.bytes, b"abc");
        assert!(!snapshot.truncated);
        assert!(snapshot.finished);
    }

    #[tokio::test]
    async fn finish_is_idempotent() {
        let (recorder, mut done) = BodyRecorder::new(8);
        recorder.finish();
        recorder.finish();
        assert!(done.try_recv().is_ok());
        assert!(done.try_recv().is_err());
    }
}
