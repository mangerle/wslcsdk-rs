//! Windows COM 多线程套间 (MTA) RAII 守卫与进程级上下文管理
//!
//! WSLC 底层依赖 COM/RPC 通信。本模块确保调用线程具备合法的 COM 上下文，
//! 并在离开作用域时按规范执行配对的 CoUninitialize，防止发生 CO_E_NOTINITIALIZED 错误。
//! 线程内的 COM 初始化状态由引用计数托管，因此无论守卫以何种顺序析构，
//! CoUninitialize 都恰好执行一次；同时提供进程级 `init_process_mta` 消除高频系统调用开销。

use crate::error::WslcError;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use windows_sys::Win32::System::Com::{
    CO_MTA_USAGE_COOKIE, COINIT_MULTITHREADED, CoDecrementMTAUsage, CoIncrementMTAUsage,
    CoInitializeEx, CoUninitialize,
};
use windows_sys::core::HRESULT;

/// COM 已经初始化为单线程套间 (STA) 时的错误码
const RPC_E_CHANGED_MODE: HRESULT = 0x8001_0106_u32 as HRESULT;

thread_local! {
    /// 当前线程已建立的 COM 初始化弱引用，强引用由各 [`ComGuard`] 持有
    ///
    /// 此处必须使用 `Weak` 而非 `Rc`：若线程局部变量自身持有强引用，引用计数将
    /// 永远无法归零，配对的 `CoUninitialize` 也就永不执行。
    static THREAD_MTA: RefCell<Weak<ComInitialization>> = const { RefCell::new(Weak::new()) };
    /// 当前线程是否已就 STA 降级告警过，避免重复初始化路径上的日志轰炸
    static THREAD_STA_DEGRADED_WARNED: Cell<bool> = const { Cell::new(false) };
}

/// 线程 COM 初始化标记，析构时配对调用 `CoUninitialize`
///
/// 该类型的析构时机完全由 `Rc` 的引用计数决定，因此无论各 [`ComGuard`] 以何种
/// 顺序析构 (包括外层守卫先于内层守卫析构的乱序情形)，`CoUninitialize` 都恰好
/// 执行一次，不会出现计数失衡导致的 COM 引用泄漏。
#[derive(Debug)]
struct ComInitialization;

impl Drop for ComInitialization {
    fn drop(&mut self) {
        // SAFETY: 与本类型所记录的 CoInitializeEx 严格配对，由 Rc 引用计数保证
        // 恰好执行一次，不会出现计数失衡导致的套间泄漏。
        unsafe {
            CoUninitialize();
        }
    }
}

/// 进程级 COM MTA 生命周期守卫
///
/// 通过 Windows 原生 `CoIncrementMTAUsage` 在进程全局维持 MTA 套间存活，
/// 使得进程内的所有工作线程默认具备合法的 COM 通信环境，彻底消除高频线程初始化开销。
#[derive(Debug)]
pub struct ProcessMtaGuard {
    cookie: CO_MTA_USAGE_COOKIE,
}

impl Drop for ProcessMtaGuard {
    fn drop(&mut self) {
        if !self.cookie.is_null() {
            // SAFETY: cookie 由同类型的构造路径经 CoIncrementMTAUsage 签发，
            // 此处已判空，且 Drop 需独占 &mut self，保证不会重复递减。
            unsafe {
                CoDecrementMTAUsage(self.cookie);
            }
            self.cookie = std::ptr::null_mut();
        }
    }
}

// SAFETY: 本类型仅持有一个由 `CoIncrementMTAUsage` 签发的进程级不透明 cookie，
// 既非 `Rc` 等引用计数对象，也无任何线程局部状态。`CoDecrementMTAUsage` 是
// 进程级操作，可从任意线程调用。
//
// 注意本类型**刻意不实现 `Clone`**：`CoIncrementMTAUsage` 与
// `CoDecrementMTAUsage` 必须严格一一配对，该 API 并不对同一 cookie 的重复递减
// 保持幂等——多减一次就会提前释放进程级 MTA，使仍在存活的句柄失去套间绑定
// 而崩溃。需要多于一份进程级租约时，必须重新调用 `CoIncrementMTAUsage` 签发
// 独立 cookie，不可复制既有的。
unsafe impl Send for ProcessMtaGuard {}

// SAFETY: 全部字段均为 `Send`，且无任何方法会在 `&self` 上修改内部状态
// （`Drop` 需独占 `&mut self`），故可安全地跨线程共享引用。
unsafe impl Sync for ProcessMtaGuard {}

/// 为整个进程初始化并保持 COM 多线程套间 (MTA)
///
/// 推荐在长生命周期服务或后台应用入口处调用并持有返回的 `ProcessMtaGuard`。
pub fn init_process_mta() -> Result<ProcessMtaGuard, WslcError> {
    let mut cookie: CO_MTA_USAGE_COOKIE = std::ptr::null_mut();
    // SAFETY: 出参 cookie 为合法的可写指针，
    // 入参为进程级 API，无需额外前置条件。
    let hr = unsafe { CoIncrementMTAUsage(&mut cookie) };
    if hr < 0 || cookie.is_null() {
        Err(WslcError::from_hresult(
            hr,
            "进程级 CoIncrementMTAUsage 初始化失败",
        ))
    } else {
        static FIRST_LOGGED: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !FIRST_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log::info!("进程级 COM 多线程套间 (MTA) 首次初始化成功");
        } else {
            log::debug!("增加进程级 COM 多线程套间 (MTA) 引用计数");
        }
        Ok(ProcessMtaGuard { cookie })
    }
}

/// 句柄托管所需的进程级 MTA 租约
///
/// # 为何句柄必须持有进程级租约而非线程级 [`ComGuard`]
///
/// WSLC 的会话、容器、进程句柄都是 RPC 通道上的对象，其**有效性绑定于建立它
/// 时的那个 COM 套间**。而 [`try_initialize_mta`] 返回的 [`ComGuard`] 仅在词法
/// 作用域内存活——`SessionBuilder::build` 一返回它就被析构，随即触发
/// `CoUninitialize`，此时句柄虽仍存活，其在原套间上的 RPC 绑定却已断开。待
/// 句柄析构时再于 `Drop` 中重建套间只是新开一个套间，救不回已断开的绑定，
/// `WslcReleaseSession` 将在此崩溃。
///
/// 本类型走 `CoIncrementMTAUsage` / `CoDecrementMTAUsage` 路径：这两个 API
/// 维护的是**进程级** MTA 使用计数，与具体线程的套间状态无关。因此只要句柄
/// 持有本租约，MTA 就不会在句柄存活期间被注销，句柄始终有效。
///
/// # 为何本类型不实现 `Clone`
///
/// `CoIncrementMTAUsage` 与 `CoDecrementMTAUsage` 必须严格一一配对，该 API
/// **并不对同一 cookie 的重复递减保持幂等**：多减一次即提前释放进程级 MTA，
/// 使仍在存活的句柄失去套间绑定而崩溃——那正是本类型要消灭的缺陷本身。
///
/// 三个持有方（会话、容器、进程包装）均包在 `Arc` 中，克隆的是 `Arc` 本身
/// 而不会克隆其内部字段，故本类型无需具备克隆能力。若将来确实需要多份租约，
/// 必须重新调用 [`Self::acquire`] 签发独立 cookie，**不可**实现会复制 cookie
/// 的 `Clone`。该约束由 `assert_not_clone!` 在编译期守护。
#[derive(Debug)]
pub(crate) struct HandleMtaLease {
    _mta: ProcessMtaGuard,
}

impl HandleMtaLease {
    /// 为即将被创建的句柄取得一份进程级 MTA 租约
    ///
    /// # Errors
    ///
    /// 当 `CoIncrementMTAUsage` 失败时返回错误。此处刻意不做降级：缺少租约
    /// 意味着句柄在存活期内随时可能因套间注销而失效，属不可接受的风险，
    /// 宁可在创建阶段直接失败。
    pub(crate) fn acquire() -> Result<Self, WslcError> {
        Ok(Self {
            _mta: init_process_mta()?,
        })
    }

    /// 取得租约后包装一个裸句柄，租约获取失败时先回收裸句柄再返回错误
    ///
    /// # 为何本方法必须存在
    ///
    /// 「句柄创建成功 → 取租约 → 取租约失败则释放裸句柄」这一顺序约束，
    /// 在会话、容器、进程三处各写了一遍，且都附了长篇注释解释「为何不能把
    /// `acquire()?` 写进结构体字面量」。三份拷贝意味着该约束要在三个地方
    /// 各自被想起一次，漏掉任一处即句柄泄漏。
    ///
    /// 顺序之所以不可颠倒：若把 `acquire()?` 写在结构体字面量内部，租约
    /// 获取失败时 `?` 会提前返回，而 `raw` 已由官方成功创建却尚未被任何
    /// RAII 对象接管，其释放函数永不执行。
    ///
    /// # Safety
    ///
    /// `raw` 必须是本次由官方创建成功、所有权尚未移交任何 RAII 对象的有效
    /// 句柄；`release` 必须是与之匹配的官方释放函数。本方法保证二者恰好
    /// 配对一次：成功路径下所有权转交 `wrap`，失败路径下由 `release` 回收。
    pub(crate) unsafe fn wrap_raw<T, H>(
        raw: H,
        release: unsafe fn(H),
        wrap: impl FnOnce(H, Self) -> T,
    ) -> Result<T, WslcError>
    where
        H: Copy,
    {
        match Self::acquire() {
            Ok(lease) => Ok(wrap(raw, lease)),
            Err(e) => {
                // 失败路径：裸句柄所有权尚未移交，由本处负责回收
                // SAFETY: 调用方保证 raw 有效、release 与之匹配，
                // 且本分支下无其他持有者，释放恰好执行一次。
                unsafe { release(raw) };
                Err(e)
            }
        }
    }

    /// 构造一个不具备套间保障的降级租约
    ///
    /// 仅供 `unsafe` 构造器在无法取得租约时兜底，以维持其既有签名。此状态下
    /// 句柄在析构时可能崩溃，调用方须在文档中告知该风险。
    pub(crate) fn degraded() -> Self {
        // 空 cookie 意味着析构时不会调用 CoDecrementMTAUsage，
        // 因此不会产生任何不平衡的计数
        Self {
            _mta: ProcessMtaGuard {
                cookie: std::ptr::null_mut(),
            },
        }
    }
}

/// COM 多线程套间 RAII 生命周期守卫
///
/// 内部持有 `Rc`，编译器据此自动保证本类型为 `!Send + !Sync`，
/// 无法被移动到其他线程，无需任何手工断言。
#[derive(Debug)]
pub struct ComGuard {
    _init: Rc<ComInitialization>,
}

/// 尝试为当前线程初始化 COM MTA 上下文
///
/// 若当前线程已处于 MTA，返回共享同一初始化状态的守卫；
/// 若当前线程已被初始化为 STA（如某些 GUI 线程），则返回 `None`（已有 COM 环境且不可改变）；
/// 若初始化失败则返回错误。
pub fn try_initialize_mta() -> Result<Option<ComGuard>, WslcError> {
    // 当前线程已建立 COM 初始化时，仅需克隆强引用以延长其生命周期
    if let Some(existing) = THREAD_MTA.with(|cell| cell.borrow().upgrade()) {
        return Ok(Some(ComGuard { _init: existing }));
    }

    // SAFETY: 首参数为 null 表示使用当前线程的默认套间模型，
    // 由 COM 自行分配，调用方无需提供 COM 对象。
    let hr = unsafe { CoInitializeEx(std::ptr::null_mut(), COINIT_MULTITHREADED as u32) };
    match hr {
        // S_OK (0) 或 S_FALSE (1): 初始化成功或已初始化为 MTA
        0 | 1 => {
            let init = Rc::new(ComInitialization);
            // 线程局部仅保存弱引用，强引用交由本守卫持有，最后一个守卫析构时即触发配对注销
            THREAD_MTA.with(|cell| *cell.borrow_mut() = Rc::downgrade(&init));
            Ok(Some(ComGuard { _init: init }))
        }
        // 当前线程已被外部宿主 (如 UI 框架) 初始化为 STA。既有套间模型无法更改，
        // 只能复用该 COM 环境继续尝试调用；WSLC 依赖 MTA 通信，后续调用存在失败风险，
        // 故在此明确告警，且每个线程仅告警一次以免日志轰炸。
        RPC_E_CHANGED_MODE => {
            THREAD_STA_DEGRADED_WARNED.with(|warned| {
                if !warned.get() {
                    warned.set(true);
                    log::warn!(
                        "当前线程已被外部宿主初始化为 STA 单线程套间，无法切换为 WSLC 所需的 MTA，\
                         将复用既有 COM 环境继续执行；若后续调用失败，请将相关操作移至独立线程执行"
                    );
                }
            });
            Ok(None)
        }
        code => Err(WslcError::from_hresult(code, "CoInitializeEx 初始化失败")),
    }
}

/// 在保证具备合法的 COM MTA 环境内执行操作
pub fn with_mta<F, R>(f: F) -> Result<R, WslcError>
where
    F: FnOnce() -> Result<R, WslcError>,
{
    let _guard = try_initialize_mta()?;
    f()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mta_guard_nesting_and_out_of_order_drop() {
        let outer = try_initialize_mta()
            .expect("COM MTA 初始化失败")
            .expect("预期返回守卫");
        let inner = try_initialize_mta()
            .expect("嵌套 COM MTA 初始化失败")
            .expect("预期返回守卫");

        // 关键场景：乱序析构，外层守卫先于内层守卫释放。
        // 引用计数实现下 CoUninitialize 只会在最后一个守卫析构时执行一次；
        // 原先的手工深度计数在此顺序下会归零却从不调用 CoUninitialize。
        drop(outer);
        drop(inner);

        // 守卫全部释放后，应当可以重新初始化并再次正常释放
        let reinit = try_initialize_mta()
            .expect("重新初始化失败")
            .expect("预期返回守卫");
        drop(reinit);
    }

    #[test]
    fn test_with_mta_returns_closure_result() {
        assert_eq!(with_mta(|| Ok(42)).expect("闭包执行失败"), 42);

        let err = with_mta(|| Err::<(), _>(WslcError::InvalidHandle));
        assert_eq!(err.unwrap_err(), WslcError::InvalidHandle);
    }

    #[test]
    fn test_process_mta_guard_is_reentrant() {
        let first = init_process_mta();
        assert!(first.is_ok());
        let second = init_process_mta();
        assert!(second.is_ok());
        drop(second);
        drop(first);
    }

    /// 回归验证：句柄租约必须在自身析构后才递减计数
    ///
    /// 这正是历史缺陷的成因——`build()` 中的线程级 `ComGuard` 随函数返回而
    /// 析构，导致句柄存活期间套间被注销。本用例以「租约先于句柄释放」的顺序
    /// 建立等价场景，确保 `CoIncrementMTAUsage` 的计数语义可用。
    #[test]
    fn test_handle_lease_keeps_mta_alive_independently_of_thread_guard() {
        // 先建立并随即释放线程级守卫，模拟 build() 返回时套间被注销的情形
        {
            let thread_guard = try_initialize_mta()
                .expect("线程级 MTA 初始化失败")
                .expect("预期返回守卫");
            // 守卫持有 Rc 强引用，套间此刻处于存活状态
            let _ = &thread_guard;
        } // 最后一个守卫析构，CoUninitialize 已执行

        // 句柄租约走进程级 API，不受线程级注销影响
        let lease = HandleMtaLease::acquire().expect("句柄租约获取失败");
        assert!(!lease._mta.cookie.is_null(), "租约应持有有效 cookie");

        // 租约析构后计数归零，可重新获取
        drop(lease);
        let again = HandleMtaLease::acquire().expect("重新获取租约失败");
        drop(again);
    }

    /// 编译期断言：`T` 未实现 `Clone`
    ///
    /// 原理是两个同名方法构成的**二选一**关系：
    /// - `CloneDetector` 仅为实现了 `Clone` 的类型提供实现；
    /// - `NotCloneFallback` 为任意类型提供兜底实现。
    ///
    /// 当目标类型**未实现** `Clone` 时，只有兜底实现可用，调用点编译通过；
    /// 一旦有人给它加上 `Clone`，两个实现同时可用，Rust 报
    /// `multiple applicable items in scope`（E0034）而**编译失败**。
    ///
    /// 这与「自动引用特化」方案的关键区别：后者依赖方法查找的回退行为，
    /// 在本类型上实测不成立；而此方案把违规变成硬编译错误，正是所需的语义。
    ///
    /// 之所以不能只留一个 `impl<T: Clone>`：那样未实现 `Clone` 时会因约束
    /// 不满足而报「trait not satisfied」，同样可用，但错误信息不指向真正原因；
    /// 双实现方案的报错直指「出现了二义性」，更易定位。
    ///
    /// # 为何两个 trait 都不会被判定为死代码
    ///
    /// `CloneDetector` 的方法**只在目标类型实现 `Clone` 时可达**——而那恰恰是
    /// 编译失败的情形，故单靠 `assert_not_clone!` 使用它会被静态分析视为死代码。
    /// 解决办法是由 `test_clone_detector_is_live_and_correct` 显式调用：拿一个
    /// 确实实现 `Clone` 的类型（`String`）作对照，既赋予其调用点，也验证探测器
    /// 本身未空转。`NotCloneFallback` 则由 `assert_not_clone!` 直接使用。
    trait CloneDetector {
        /// 该类型实现了 `Clone` 时可达
        fn clone_detected(&self) -> bool {
            true
        }
    }

    impl<T: Clone> CloneDetector for T {}

    /// 兜底实现：`T` 未实现 `Clone` 时可达
    trait NotCloneFallback {
        /// 恒为 false
        fn clone_detected(&self) -> bool {
            false
        }
    }

    impl<T> NotCloneFallback for T {}

    /// 探测机制的自验证：用一个**确实实现** `Clone` 的类型确认探测能返回 true
    ///
    /// 本用例同时解决两件事：
    /// - 让 `CloneDetector` 拥有真实调用点。其方法原本只在目标类型实现
    ///   `Clone` 时可达，而那正是编译失败的情形，故静态分析判定其为死代码；
    /// - 防止探测器**空转**。若两个实现都被误删或逻辑写反，本用例会先失败，
    ///   从而保证 `assert_not_clone!` 的判定结果可信。
    #[test]
    fn test_clone_detector_is_live_and_correct() {
        // `String: Clone`，两个 impl 对它都适用，故必须显式消歧——
        // 这恰恰印证了 `assert_not_clone!` 的判定依据：两个方法同时可用
        // 正是「目标类型实现了 Clone」的特征。
        let sample = "probe".to_string();

        // Clone 分支应返回 true
        assert!(
            CloneDetector::clone_detected(&sample),
            "`String` 实现了 Clone，探测应返回 true；否则探测器已失效，\
             `assert_not_clone!` 的判定不可信"
        );

        // 兜底分支恒返回 false，说明两个实现确实相互独立
        assert!(
            !NotCloneFallback::clone_detected(&sample),
            "兜底实现应对任意类型返回 false"
        );
    }

    /// 断言给定类型的值未实现 `Clone`
    ///
    /// 宏参数为「类型」与「构造该类型一个值」的表达式，二者成对给出。
    /// 一旦目标类型被加上 `Clone`，`probe` 内将同时匹配两个 impl 而报 E0034，
    /// 从而把违规挡在**编译期**——这正是本断言的效力所在。`probe` 与断言体本身
    /// 在运行时求值，但只要编译能通过，其结论就恒为「未实现 Clone」。
    macro_rules! assert_not_clone {
    ($($t:ty => $ctor:expr),+ $(,)?) => {$(
        // 每次展开各自成块，避免多次 `probe` 同名冲突
        {
            fn probe(value: &$t) -> bool {
                // 若 $t 实现了 Clone，此处将同时匹配两个 impl 而报 E0034
                value.clone_detected()
            }
            assert!(
                !probe(&$ctor),
                concat!(stringify!($t), " 绝不可实现 Clone：副本析构会多减一次 MTA 计数，\
                 提前拆掉套间并使存活句柄失效")
            );
        }
    )+};
}

    /// 降级租约不得影响 MTA 计数平衡
    #[test]
    fn test_degraded_lease_is_neutral() {
        let degraded = HandleMtaLease::degraded();
        assert!(degraded._mta.cookie.is_null());
        // 析构不得调用 CoDecrementMTAUsage（已在 Drop 中判空跳过）
        drop(degraded);
        // 计数未受影响，仍可正常获取
        let lease = HandleMtaLease::acquire().expect("获取租约失败");
        drop(lease);
    }

    /// 锁定「租约与进程级守卫均不可克隆」这一安全约束
    ///
    /// `CoIncrementMTAUsage` 与 `CoDecrementMTAUsage` 须严格一一配对，该 API
    /// 不对同一 cookie 的重复递减保持幂等：一旦持有 cookie 的类型可被克隆，
    /// 副本析构就会多减一次，提前释放进程级 MTA，使仍在存活的句柄失去套间
    /// 绑定而崩溃——正是租约机制要消灭的缺陷本身。
    ///
    /// 用例的**编译通过**即是断言：若日后有人给这两类加上 `Clone`，
    /// `assert_not_clone!` 展开处的 `probe` 会因方法二义性而报E0034。
    #[test]
    fn test_handles_are_not_cloneable() {
        assert_not_clone!(
            HandleMtaLease => HandleMtaLease::degraded(),
            ProcessMtaGuard => ProcessMtaGuard { cookie: std::ptr::null_mut() },
        );
    }

    /// 每次获取租约都独立签发新 cookie
    ///
    /// 这是「租约不可克隆」的伴生约定：由于不存在可复制的 cookie，需要多于
    /// 一份租约时只能重新调用 [`HandleMtaLease::acquire`]，每次签发独立
    /// cookie，因而计数始终不会失衡。此断言比「释放后能重新获取」更强——
    /// 后者无法排除两个租约共用同一 cookie 的可能。
    #[test]
    fn test_lease_acquires_independent_cookie_each_time() {
        let first = HandleMtaLease::acquire().expect("获取租约失败");
        let second = HandleMtaLease::acquire().expect("再次获取租约失败");
        assert_ne!(
            first._mta.cookie, second._mta.cookie,
            "每次获取租约都应签发独立 cookie，否则计数将被重复持有"
        );
        drop(first);
        drop(second);
    }
}
