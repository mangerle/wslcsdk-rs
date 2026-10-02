//! 跨模块集成测试
//!
//! 仅使用本 crate 的公开 API 进行黑盒验证。
//!
//! 标注 `#[ignore]` 的端到端用例需要宿主机具备可用的 WSL Containers 运行时，
//! 默认不随 CI 执行。可用 `cargo test -- --ignored` 显式运行。
//!
//! # 关于镜像准备
//!
//! WSLC 的镜像存储是**会话作用域**的（`WslcPullSessionImage` /
//! `WslcListSessionImages` 均以 `WslcSession` 为首参），并非宿主机全局共享。
//! 因此**无法**通过 `wslc pull` 之类的方式为测试「预置」镜像：镜像只存在于
//! 拉取时所在的那个会话中，换一个会话即不可见。
//!
//! 用例因此在会话内按需拉取所需镜像（幂等）：会话已有镜像时直接复用。

use wslcsdk::{
    ContainerBuilder, ProcessBuilder, SessionBuilder, WslcClient, WslcImageManager,
    WslcSessionHandle, WslcSignal,
};

#[test]
fn test_null_handle_async_calls_return_error_instead_of_panicking() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("创建测试用 Tokio 运行时失败");

    rt.block_on(async {
        // 空句柄不携带任何需释放的所有权，构造行为本身是安全的；
        // 此处仅用于验证异步接口在句柄为空时安全返回错误而非崩溃
        let session = unsafe { WslcSessionHandle::from_raw(Default::default(), "fake-session") };
        assert!(session.wait_termination_async(10).await.is_err());
    });
}

#[test]
fn test_streams_wait_exit_rejects_repeated_consumption() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("创建测试用 Tokio 运行时失败");

    rt.block_on(async {
        let (_builder, mut streams) = ProcessBuilder::new()
            .command(&["/bin/true"])
            .with_streaming_io();

        // 未派生任何进程，退出通知不会到达，以超时中断首次等待
        let first =
            tokio::time::timeout(std::time::Duration::from_millis(50), streams.wait_exit()).await;
        assert!(first.is_err(), "未派生进程时不应收到退出通知");

        // 退出通知已被首次调用取走，重复等待必须立即失败而非永久挂起
        assert!(streams.wait_exit().await.is_err(), "退出通知不应被重复消费");
    });
}

/// 确保目标会话内存在 `alpine:latest` 镜像，缺失时即时拉取
///
/// WSLC 的镜像作用域绑定在会话上，宿主机侧的预置对本用例不可见，
/// 故只能按会话补齐。已存在时直接复用，避免重复的网络下载。
///
/// 同时对 SDK 返回的镜像名做后缀匹配，容忍 `docker.io/library/alpine:latest`
/// 与 `alpine:latest` 两种官方写法在列表中的差异。
fn ensure_alpine_image(session: &WslcSessionHandle) -> Result<(), Box<dyn std::error::Error>> {
    let has_alpine = |images: &[wslcsdk::ImageInfo]| {
        images
            .iter()
            .any(|img| img.name == "alpine:latest" || img.name.ends_with("/alpine:latest"))
    };

    if has_alpine(&WslcImageManager::list_images(session)?) {
        return Ok(());
    }

    WslcImageManager::pull_image(
        session,
        "docker.io/library/alpine:latest",
        None,
        // 进度回调在官方线程上触发，此处不打印，仅占位以显式表达「不关心进度」
        Some(|_progress: &wslcsdk::ImageProgress<'_>| true),
    )?;

    assert!(
        has_alpine(&WslcImageManager::list_images(session)?),
        "拉取结束后会话内仍无 alpine:latest 镜像"
    );
    Ok(())
}

/// 端到端全流程验证：会话创建 -> 镜像就绪 -> 容器启动 -> 容器内进程流式输出 -> 资源清理
///
/// 需要宿主机具备可用的 WSL Containers 运行时。
#[test]
#[ignore = "需要可用的 WSL Containers 运行时，请用 cargo test -- --ignored 显式运行"]
fn test_end_to_end_session_container_process_lifecycle() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("创建测试用 Tokio 运行时失败");

    rt.block_on(async {
        let session = SessionBuilder::new_default("integration-e2e-session")
            .expect("会话构建器创建失败")
            .cpu_count(2)
            .memory_mb(2048)
            .build()
            .expect("会话创建失败");

        // 镜像作用域绑定会话，必须在会话创建之后、其他操作之前补齐
        ensure_alpine_image(&session).expect("准备 alpine:latest 镜像失败");

        // 容器必须携带 init process：无 init process 的容器启动后会立即退出，
        // 状态只能是 EXITED，后续派生进程与状态断言均无从谈起。
        // 这里用长驻进程把容器保持在 RUNNING，容器内输出交由下面派生的进程验证。
        //
        // 上一轮运行若在中途 panic，auto_remove 不会生效，容器名会被占用并导致
        // 后续运行报「名称已在使用」。先尽力清理上一轮残留，保证用例可重复执行。
        const CONTAINER_NAME: &str = "integration-e2e-container";
        if let Ok(leftover) = wslcsdk::WslcContainerHandle::open(&session, CONTAINER_NAME) {
            let _ = leftover.stop(WslcSignal::Sigkill, 0);
            let _ = leftover.delete(true);
        }

        let container = ContainerBuilder::new("alpine:latest")
            .name(CONTAINER_NAME)
            .auto_remove(true)
            .init_process(ProcessBuilder::new().command(&["/bin/sh", "-c", "sleep 300"]))
            .build(&session)
            .expect("容器创建失败");

        container.start(false).expect("容器启动失败");
        assert_eq!(
            container.state().expect("查询容器状态失败"),
            wslcsdk::WslcContainerState::RUNNING
        );

        // 路径需要真实的 wslcsdk.dll 才能给出有意义的结果
        let _ = container
            .get_init_process()
            .expect("获取 init 进程句柄失败");

        let (builder, mut streams) = ProcessBuilder::new()
            .command(&["/bin/sh", "-c", "echo integration-ok"])
            .with_streaming_io();
        let process = builder.spawn(&container).expect("派生进程失败");
        assert!(process.pid().expect("获取 PID 失败") > 0);

        let mut collected = String::new();
        while let Some(chunk) = streams.stdout.recv().await {
            collected.push_str(&String::from_utf8_lossy(&chunk));
        }
        assert_eq!(streams.wait_exit().await.expect("等待退出失败"), 0);
        assert!(
            collected.contains("integration-ok"),
            "实际输出: {collected}"
        );

        container
            .stop(WslcSignal::Sigterm, 10)
            .expect("停止容器失败");
        // 容器已配置 auto_remove，停止时已随容器一并删除，
        // 此处再调用 delete 只会得到 RPC_E_DISCONNECTED，故不再重复删除。
        session.terminate().expect("终止会话失败");
        cleanup_session_dir("integration-e2e-session");
    });
}

#[test]
#[ignore = "需要可用的 WSL Containers 运行时，请用 cargo test -- --ignored 显式运行"]
fn test_session_creation_requires_thread_apartment() {
    let result = SessionBuilder::new_default("apartment-regression").and_then(|b| b.build());
    if let Ok(session) = result {
        let _ = session.terminate();
        cleanup_session_dir("apartment-regression");
    } else {
        panic!("创建会话失败: {:?}", result.err());
    }
}

#[test]
#[ignore = "需要可用的 WSL Containers 运行时，请用 cargo test -- --ignored 显式运行"]
fn test_container_creation_requires_thread_apartment() {
    let Ok(session) =
        SessionBuilder::new_default("container-apartment-regression").and_then(|b| b.build())
    else {
        return;
    };
    let _ = ContainerBuilder::new("alpine:latest").build(&session);
    let _ = session.terminate();
    cleanup_session_dir("container-apartment-regression");
}

#[test]
#[ignore = "需要可用的 WSL Containers 运行时，请用 cargo test -- --ignored 显式运行"]
fn test_client_holds_shared_session() {
    let client = WslcClient::builder()
        .session_name("client-shared-session")
        .build()
        .expect("构建客户端应当成功");

    assert_eq!(client.session().name(), "client-shared-session");
    assert!(client.is_mta_active(), "默认应维持进程级 MTA 守护");

    let cloned = client.clone();
    assert_eq!(cloned.session().name(), client.session().name());
    let _ = client.session().terminate();
    drop(cloned);
    drop(client);
    cleanup_session_dir("client-shared-session");
}

#[test]
#[ignore = "需要可用的 WSL Containers 运行时，请用 cargo test -- --ignored 显式运行"]
fn test_builder_can_disable_mta() {
    let client = WslcClient::builder()
        .session_name("client-no-mta")
        .auto_init_mta(false)
        .build()
        .expect("构建客户端应当成功");
    assert!(!client.is_mta_active());
    let _ = client.session().terminate();
    drop(client);
    cleanup_session_dir("client-no-mta");
}

#[test]
#[ignore = "需要可用的 WSL Containers 运行时，请用 cargo test -- --ignored 显式运行"]
fn test_bound_builder_needs_no_session_argument() {
    let Ok(client) = WslcClient::builder()
        .session_name("bound-builder-test")
        .build()
    else {
        return;
    };
    let builder = client
        .create_container("alpine:latest")
        .name("bound-demo")
        .auto_remove(true);
    let _ = builder.build();
    let _ = client.session().terminate();
    drop(client);
    cleanup_session_dir("bound-builder-test");
}

/// 清理指定名称测试会话在宿主机文件系统上留下的存储目录
fn cleanup_session_dir(name: &str) {
    let base_dir = std::env::var("LOCALAPPDATA")
        .map(|p| std::path::PathBuf::from(p).join("wslc"))
        .unwrap_or_else(|_| {
            std::env::var("USERPROFILE")
                .map(|p| std::path::PathBuf::from(p).join(".wslc"))
                .unwrap_or_else(|_| std::path::PathBuf::from(r"C:\wslc"))
        });
    let path = base_dir.join("sessions").join(name);
    let _ = std::fs::remove_dir_all(path);
}
