//! 面向 Tokio 运行时的异步非阻塞扩展
//!
//! 将长耗时的会话初始化、镜像下载拉取、容器控制与状态检查安全卸载到
//! `tokio::task::spawn_blocking` 阻塞线程池中执行，并在闭包内自动注入 COM MTA 环境，
//! 彻底避免占用 Tokio 异步运行时工作线程导致的界面卡顿与线程饥饿。

use crate::container::{ContainerBuilder, WslcContainerHandle};
use crate::error::WslcError;
use crate::image::{ImageInfo, WslcImageManager};
use crate::session::{SessionBuilder, WslcSessionHandle};
use wslcsdk_sys::types::WslcSignal;

impl SessionBuilder {
    /// 异步创建并激活会话 (防止阻塞 Tokio 工作线程)
    pub async fn build_async(self) -> Result<WslcSessionHandle, WslcError> {
        tokio::task::spawn_blocking(move || crate::com::with_mta(|| self.build()))
            .await
            .map_err(|e| WslcError::TaskJoin(format!("异步任务执行失败: {e}")))?
    }
}

impl ContainerBuilder {
    /// 异步创建容器
    pub async fn build_async(
        self,
        session: &WslcSessionHandle,
    ) -> Result<WslcContainerHandle, WslcError> {
        let session = session.clone();
        tokio::task::spawn_blocking(move || crate::com::with_mta(|| self.build(&session)))
            .await
            .map_err(|e| WslcError::TaskJoin(format!("异步创建容器任务失败: {e}")))?
    }
}

impl WslcSessionHandle {
    /// 异步获取会话内的镜像列表
    pub async fn list_images_async(&self) -> Result<Vec<ImageInfo>, WslcError> {
        let session = self.clone();
        tokio::task::spawn_blocking(move || {
            crate::com::with_mta(|| WslcImageManager::list_images(&session))
        })
        .await
        .map_err(|e| WslcError::TaskJoin(format!("异步获取镜像列表失败: {e}")))?
    }

    /// 异步拉取远程镜像
    pub async fn pull_image_async(
        &self,
        uri: String,
        registry_auth: Option<String>,
    ) -> Result<(), WslcError> {
        let session = self.clone();
        tokio::task::spawn_blocking(move || {
            crate::com::with_mta(|| {
                WslcImageManager::pull_image(
                    &session,
                    &uri,
                    registry_auth.as_deref(),
                    None::<fn(&crate::image::ImageProgress<'_>) -> bool>,
                )
            })
        })
        .await
        .map_err(|e| WslcError::TaskJoin(format!("异步拉取镜像失败: {e}")))?
    }

    /// 异步删除镜像
    pub async fn delete_image_async(&self, name_or_id: String) -> Result<(), WslcError> {
        let session = self.clone();
        tokio::task::spawn_blocking(move || {
            crate::com::with_mta(|| WslcImageManager::delete_image(&session, &name_or_id))
        })
        .await
        .map_err(|e| WslcError::TaskJoin(format!("异步删除镜像失败: {e}")))?
    }

    /// 异步终止会话
    pub async fn terminate_async(&self) -> Result<(), WslcError> {
        let session = self.clone();
        tokio::task::spawn_blocking(move || crate::com::with_mta(|| session.terminate()))
            .await
            .map_err(|e| WslcError::TaskJoin(format!("异步终止会话失败: {e}")))?
    }
}

impl WslcContainerHandle {
    /// 异步启动容器
    pub async fn start_async(&self, attach: bool) -> Result<(), WslcError> {
        let container = self.clone();
        tokio::task::spawn_blocking(move || crate::com::with_mta(|| container.start(attach)))
            .await
            .map_err(|e| WslcError::TaskJoin(format!("异步启动容器失败: {e}")))?
    }

    /// 异步停止容器
    pub async fn stop_async(
        &self,
        signal: WslcSignal,
        timeout_seconds: u32,
    ) -> Result<(), WslcError> {
        let container = self.clone();
        tokio::task::spawn_blocking(move || {
            crate::com::with_mta(|| container.stop(signal, timeout_seconds))
        })
        .await
        .map_err(|e| WslcError::TaskJoin(format!("异步停止容器失败: {e}")))?
    }

    /// 异步删除容器
    pub async fn delete_async(&self, force: bool) -> Result<(), WslcError> {
        let container = self.clone();
        tokio::task::spawn_blocking(move || crate::com::with_mta(|| container.delete(force)))
            .await
            .map_err(|e| WslcError::TaskJoin(format!("异步删除容器失败: {e}")))?
    }

    /// 异步获取容器 JSON 检查快照
    pub async fn inspect_async(&self) -> Result<serde_json::Value, WslcError> {
        let container = self.clone();
        tokio::task::spawn_blocking(move || crate::com::with_mta(|| container.inspect()))
            .await
            .map_err(|e| WslcError::TaskJoin(format!("异步检查容器失败: {e}")))?
    }
}
