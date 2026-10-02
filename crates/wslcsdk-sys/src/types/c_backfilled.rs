//! 由 C 侧回填取值的枚举类型
//!
//! 官方 C 库会将部分枚举取值直接写入调用方提供的内存（`out` 参数），
//! 或经由回调函数回传。此类取值若按 Rust `enum` 建模，一旦官方在后续版本
//! 新增枚举成员，读取该内存即构成未定义行为（编译期与运行期均不会报错）。
//!
//! 因此本模块统一以 `#[repr(transparent)]` Newtype 承载原始 `u32`：
//! ABI 布局与 C 侧完全一致，同时使任意取值都成为合法的 Rust 值。
//! 需要与官方已知取值比较时，使用各类型暴露的关联常量。

/// 生成一个承载 C 侧回填取值的 Newtype 类型
///
/// 生成结果包含：
/// - `#[repr(transparent)] pub struct $name(pub u32)`：与 C 侧布局逐字节一致；
/// - 每个官方已知取值对应的关联常量（`ScreamingSnakeCase`）；
/// - `name()` 返回官方已定义取值的可读名称，便于识别官方新增的未定义取值；
/// - 手工实现的 `Debug`：已知取值输出常量名，未知取值输出 `Unknown(原始值)`，
///   避免退化为 `类型名(数字)` 而丢失可读性；
/// - `u32` 与本类型之间的双向无损转换（`From` 实现，同时经由标准库的
///   一揽子实现自动获得不会失败的 `TryFrom<u32>`）。
macro_rules! c_backfilled_enum {
    (
        $(#[$meta:meta])*
        $name:ident { $($(#[$variant_meta:meta])* $variant:ident = $value:literal),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(Copy, Clone, Default, PartialEq, Eq)]
        pub struct $name(pub u32);

        #[allow(non_upper_case_globals)]
        impl $name {
            $(
                $(#[$variant_meta])*
                pub const $variant: Self = Self($value);
            )+

            /// 返回官方已定义取值的可读名称；官方尚未定义该取值时返回 `None`
            pub const fn name(self) -> Option<&'static str> {
                match self.0 {
                    $( $value => Some(stringify!($variant)), )+
                    _ => None,
                }
            }
        }

        impl core::fmt::Debug for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                match self.name() {
                    Some(name) => f.write_str(name),
                    None => write!(f, "Unknown({})", self.0),
                }
            }
        }

        impl From<u32> for $name {
            fn from(value: u32) -> Self {
                Self(value)
            }
        }

        impl From<$name> for u32 {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

c_backfilled_enum! {
    /// 会话退出原因
    WslcSessionTerminationReason {
        /// 未知原因
        Unknown = 0,
        /// 会话正常关机退出
        Shutdown = 1,
        /// 会话异常崩溃退出
        Crashed = 2,
    }
}

impl WslcSessionTerminationReason {
    pub const UNKNOWN: Self = Self::Unknown;
    pub const SHUTDOWN: Self = Self::Shutdown;
    pub const CRASHED: Self = Self::Crashed;
}

c_backfilled_enum! {
    /// 容器运行状态
    WslcContainerState {
        /// 无效状态
        Invalid = 0,
        /// 已创建但尚未启动
        Created = 1,
        /// 正在运行
        Running = 2,
        /// 已退出
        Exited = 3,
        /// 已被删除
        Deleted = 4,
    }
}

impl WslcContainerState {
    pub const INVALID: Self = Self::Invalid;
    pub const CREATED: Self = Self::Created;
    pub const RUNNING: Self = Self::Running;
    pub const EXITED: Self = Self::Exited;
    pub const DELETED: Self = Self::Deleted;
}

c_backfilled_enum! {
    /// 进程标准 IO 流类型
    ///
    /// 既作为 `WslcGetProcessIOHandle` 的入参，也会经由 `WslcStdIOCallback`
    /// 的 `ioHandle` 参数由 C 侧回传，故必须按回填取值处理。
    WslcProcessIOHandle {
        /// 标准输入
        Stdin = 0,
        /// 标准输出
        Stdout = 1,
        /// 标准错误
        Stderr = 2,
    }
}

impl WslcProcessIOHandle {
    pub const STDIN: Self = Self::Stdin;
    pub const STDOUT: Self = Self::Stdout;
    pub const STDERR: Self = Self::Stderr;
}

c_backfilled_enum! {
    /// 进程运行状态
    WslcProcessState {
        /// 未知状态
        Unknown = 0,
        /// 正在运行
        Running = 1,
        /// 已退出
        Exited = 2,
        /// 被信号终止
        Signalled = 3,
    }
}

impl WslcProcessState {
    pub const UNKNOWN: Self = Self::Unknown;
    pub const RUNNING: Self = Self::Running;
    pub const EXITED: Self = Self::Exited;
    pub const SIGNALLED: Self = Self::Signalled;
}

c_backfilled_enum! {
    /// 镜像拉取进度阶段状态
    WslcImageProgressStatus {
        /// 未知阶段
        Unknown = 0,
        /// 正在拉取镜像层
        Pulling = 1,
        /// 等待中
        Waiting = 2,
        /// 正在下载
        Downloading = 3,
        /// 正在校验摘要
        Verifying = 4,
        /// 正在解包
        Extracting = 5,
        /// 已完成
        Complete = 6,
    }
}

impl WslcImageProgressStatus {
    pub const UNKNOWN: Self = Self::Unknown;
    pub const PULLING: Self = Self::Pulling;
    pub const WAITING: Self = Self::Waiting;
    pub const DOWNLOADING: Self = Self::Downloading;
    pub const VERIFYING: Self = Self::Verifying;
    pub const EXTRACTING: Self = Self::Extracting;
    pub const COMPLETE: Self = Self::Complete;
}

c_backfilled_enum! {
    /// 身份认证令牌返回类型
    WslcIdentityTokenType {
        /// 未知类型
        Unknown = 0,
        /// 服务端返回了身份令牌
        Token = 1,
        /// 服务端未返回令牌，凭据已内嵌
        Credentials = 2,
    }
}

impl WslcIdentityTokenType {
    pub const UNKNOWN: Self = Self::Unknown;
    pub const TOKEN: Self = Self::Token;
    pub const CREDENTIALS: Self = Self::Credentials;
}
