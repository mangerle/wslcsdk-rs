//! 异步有界通道接收端的安全包装
//!
//! 本模块把底层异步运行时提供的通道接收端封装为自有类型，使本库的公开 API
//! 不出现任何第三方运行时的具体类型。否则该运行时的破坏性升级会直接传导为
//! 本库的破坏性变更，使用者也会被间接锁定在其版本演进之上。

use tokio::sync::mpsc;

/// 有界异步通道的接收端
///
/// 与底层实现解耦：使用者只需 `recv().await` 消费数据，或通过 `try_recv`
/// 非阻塞探测，无需关心底层由哪个异步运行时实现。
pub struct AsyncReceiver<T> {
    rx: mpsc::Receiver<T>,
    dropped_count: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
}

impl<T> AsyncReceiver<T> {
    /// 由底层通道接收端构造包装对象
    pub(crate) fn new(rx: mpsc::Receiver<T>) -> Self {
        Self {
            rx,
            dropped_count: None,
        }
    }

    /// 由底层通道接收端及丢弃计数器构造包装对象
    pub(crate) fn new_with_drop_counter(
        rx: mpsc::Receiver<T>,
        dropped_count: std::sync::Arc<std::sync::atomic::AtomicU64>,
    ) -> Self {
        Self {
            rx,
            dropped_count: Some(dropped_count),
        }
    }

    /// 获取因背压满载而丢弃的消息累计计数
    ///
    /// 若底层通道未配置背压丢弃策略，则恒返回 0。
    #[must_use]
    pub fn dropped_count(&self) -> u64 {
        self.dropped_count
            .as_ref()
            .map_or(0, |c| c.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// 异步接收下一个条目；通道关闭且剩余条目耗尽后返回 `None`
    pub async fn recv(&mut self) -> Option<T> {
        self.rx.recv().await
    }

    /// 非阻塞地尝试接收一个条目
    ///
    /// # Errors
    /// 当前无可用条目时返回 [`AsyncRecvError::Empty`]；通道已关闭且条目耗尽时
    /// 返回 [`AsyncRecvError::Disconnected`]。
    pub fn try_recv(&mut self) -> Result<T, AsyncRecvError> {
        self.rx.try_recv().map_err(|e| match e {
            mpsc::error::TryRecvError::Empty => AsyncRecvError::Empty,
            mpsc::error::TryRecvError::Disconnected => AsyncRecvError::Disconnected,
        })
    }

    /// 主动关闭接收端，使后续 `recv` 在缓存条目耗尽后立即返回 `None`
    pub fn close(&mut self) {
        self.rx.close();
    }
}

impl<T> std::fmt::Debug for AsyncReceiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncReceiver").finish_non_exhaustive()
    }
}

/// [`AsyncReceiver::try_recv`] 的接收错误
#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsyncRecvError {
    /// 通道当前无可用条目
    #[error("异步通道当前无可用数据")]
    Empty,
    /// 通道已关闭且剩余条目已耗尽
    #[error("异步通道已关闭且剩余数据已耗尽")]
    Disconnected,
}
