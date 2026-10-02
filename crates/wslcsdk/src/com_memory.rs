//! 由 COM 堆分配的原始指针的 RAII 托管
//!
//! WSLC 的多个接口会通过 `CoTaskMemAlloc` 族函数返回内存，官方要求调用方
//! 自行调用 `CoTaskMemFree` 释放。这类手工释放极易在新增调用点时被遗漏，
//! 造成进程内存泄漏，且泄漏点远离报错位置，极难排查。
//!
//! 本模块以 [`ComAnsiString`]、[`ComWideString`] 与 [`ComArray`] 统一托管这类指针：
//! 所有权在构造时转移给对应类型，无论后续走成功路径还是错误路径，
//! 析构时都会恰好释放一次。

use core::ffi::c_void;
use core::ptr;
use core::slice;
use std::ffi::CStr;
use windows_sys::Win32::System::Com::CoTaskMemFree;

/// 由 COM 堆分配、需交由 `CoTaskMemFree` 释放的 ANSI 字符串
///
/// 典型来源为官方各类 `_Outptr_result_z_ PSTR*` 输出参数。构造时接管所有权，
/// 析构时自动释放；`as_str` 可在释放前借用其内容。
pub struct ComAnsiString(*mut i8);

impl ComAnsiString {
    /// 接管一块由 COM 堆分配的、以 NUL 结尾的 ANSI 字符串
    ///
    /// # Safety
    ///
    /// 调用方必须保证 `ptr` 指向一块由 `CoTaskMemAlloc` 族函数分配、以 NUL
    /// 结尾、且**所有权已完整移交**给本对象的有效内存。本对象析构时会释放它，
    /// 因此调用方不得再对该指针调用 `CoTaskMemFree` 或重复使用。
    pub unsafe fn from_raw(ptr: *mut i8) -> Option<Self> {
        if ptr.is_null() { None } else { Some(Self(ptr)) }
    }

    /// 借用字符串内容
    ///
    /// # Errors
    ///
    /// 当内容不是合法 UTF-8 时返回 [`std::str::Utf8Error`]。
    /// 刻意不提供会静默返回空串的版本：ANSI 字符串来自官方接口（如
    /// `WslcInspectContainer` 的 JSON 输出），一旦含非 UTF-8 字节，静默降级为空串
    /// 会让后续的 JSON 解析报出「语法错误」这类**指向错误位置**的误导性诊断。
    pub fn as_str(&self) -> Result<&str, std::str::Utf8Error> {
        // SAFETY: 构造时已保证指针非空且以 NUL 结尾，内存生命周期由本对象托管
        unsafe { CStr::from_ptr(self.0) }.to_str()
    }

    /// 以 UTF-8 借用字符串内容；非法序列按替换字符处理
    ///
    /// 适用于诊断与日志等「宁可显示乱码也不愿失败」的场合。
    pub fn to_string_lossy(&self) -> String {
        // SAFETY: 构造时已保证指针非空且以 NUL 结尾，内存生命周期由本对象托管
        unsafe { CStr::from_ptr(self.0) }
            .to_string_lossy()
            .into_owned()
    }
}

impl Drop for ComAnsiString {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: 指针非空且所有权已移交本对象，由 Drop 保证恰好释放一次
            unsafe { CoTaskMemFree(self.0.cast()) };
            self.0 = ptr::null_mut();
        }
    }
}

impl std::fmt::Debug for ComAnsiString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 诊断场景下容错优先，用有损转换避免 Debug 自身 panic
        f.debug_tuple("ComAnsiString")
            .field(&self.to_string_lossy())
            .finish()
    }
}

/// 由 COM 堆分配、需交由 `CoTaskMemFree` 释放的 UTF-16 宽字符串
///
/// 典型来源为官方各类 `_Outptr_opt_result_z_ PWSTR*` 错误消息输出参数。
pub struct ComWideString(*mut u16);

impl ComWideString {
    /// 接管一块由 COM 堆分配的、以 NUL 结尾的 UTF-16 宽字符串
    ///
    /// # Safety
    ///
    /// 调用方必须保证 `ptr` 指向一块由 `CoTaskMemAlloc` 族函数分配、以 NUL
    /// 结尾、且**所有权已完整移交**给本对象的有效内存。本对象析构时会释放它，
    /// 因此调用方不得再对该指针调用 `CoTaskMemFree` 或重复使用。
    pub unsafe fn from_raw(ptr: *mut u16) -> Option<Self> {
        if ptr.is_null() { None } else { Some(Self(ptr)) }
    }

    /// 以 UTF-8 借用字符串内容；非法序列按替换字符处理
    pub fn to_string_lossy(&self) -> String {
        // SAFETY: 构造时已保证指针非空且以 NUL 结尾，内存生命周期由本对象托管
        unsafe {
            let mut len = 0usize;
            while *self.0.add(len) != 0 {
                len += 1;
            }
            String::from_utf16_lossy(slice::from_raw_parts(self.0, len))
        }
    }
}

impl Drop for ComWideString {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: 指针非空且所有权已移交本对象，由 Drop 保证恰好释放一次
            unsafe { CoTaskMemFree(self.0.cast()) };
            self.0 = ptr::null_mut();
        }
    }
}

impl std::fmt::Debug for ComWideString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ComWideString")
            .field(&self.to_string_lossy())
            .finish()
    }
}

/// 由 COM 堆分配、需交由 `CoTaskMemFree` 释放的 POD 数组
///
/// 典型来源为 [`WslcListSessionImages`](wslcsdk_sys::WslcListSessionImages)
/// 返回的镜像数组。数组元素为平凡类型，按 `len * size_of::<T>()` 释放。
///
/// # Safety
///
/// `T` 必须为 POD 类型（不含析构函数），否则按字节释放将导致内存损坏。
pub struct ComArray<T> {
    ptr: *mut T,
    len: usize,
}

// SAFETY: 本类型接管 POD 数组的原始内存，状态由 `ptr` 与 `len` 构成；
// 当元素类型 `T` 满足 `Send` 时，其数组内存可在线程间安全转移所有权。
unsafe impl<T: Send> Send for ComArray<T> {}

// SAFETY: `as_slice` 只以 `&self` 借用内存并返回 `&[T]`，不修改任何内部字段；
// 内存释放仅在 `Drop` 时独占执行。当元素类型 `T` 满足 `Sync` 时，可安全跨线程共享引用。
unsafe impl<T: Sync> Sync for ComArray<T> {}

impl<T> ComArray<T> {
    /// 接管一块由 COM 堆分配的数组
    ///
    /// # Safety
    ///
    /// 调用方必须保证 `ptr` 指向一块由 `CoTaskMemAlloc` 族函数分配、长度为
    /// `len`、且**所有权已完整移交**给本对象的有效内存。
    pub unsafe fn from_raw(ptr: *mut T, len: usize) -> Option<Self> {
        if ptr.is_null() {
            // 长度为 0 时 COM 允许返回空指针，视为空数组而非错误
            Some(Self {
                ptr: ptr::null_mut(),
                len: 0,
            })
        } else {
            Some(Self { ptr, len })
        }
    }

    /// 以切片借用数组内容
    pub fn as_slice(&self) -> &[T] {
        if self.ptr.is_null() {
            return &[];
        }
        // SAFETY: 构造时已保证指针有效且长度为 len，内存生命周期由本对象托管
        unsafe { slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl<T> Drop for ComArray<T> {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            // SAFETY: 指针非空且所有权已移交本对象，由 Drop 保证恰好释放一次。
            //
            // 此处按**字节**释放（而非逐元素析构），故要求 `T` 为 POD 类型——
            // 若`T` 含析构函数，按字节释放将直接损坏内存。该约束由
            // `from_raw` 的文档注释承载，属调用方责任：编译器无法在
            // 泛型层面校验「无析构函数」。
            //
            // `CoTaskMemFree` 只接收单一 void 指针，不感知元素类型与个数，
            // 故它无法替本类型做类型检查——这正是约束必须写在文档而非
            // 代码中的原因。
            unsafe { CoTaskMemFree(self.ptr.cast::<c_void>()) };
            self.ptr = ptr::null_mut();
            self.len = 0;
        }
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for ComArray<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ComArray").field(&self.as_slice()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;
    use core::ptr::copy_nonoverlapping;
    use windows_sys::Win32::System::Com::CoTaskMemAlloc;

    /// 分配一块由 COM 堆管理的 UTF-16 缓冲区并写入可识别内容
    fn alloc_wide(text: &str) -> *mut u16 {
        let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = units.len() * size_of::<u16>();
        // SAFETY: 按字节数正确分配，返回指针由测试自行管理
        let raw = unsafe { CoTaskMemAlloc(bytes) } as *mut u16;
        assert!(!raw.is_null(), "测试前提：COM 堆分配应当成功");
        // SAFETY: 目标缓冲区长度为 units.len()，容量匹配
        unsafe { copy_nonoverlapping(units.as_ptr(), raw, units.len()) };
        raw
    }

    #[test]
    fn test_com_wide_string_takes_ownership_and_frees() {
        let raw = alloc_wide("容器未找到");
        let content;
        {
            // SAFETY: 指针由本测试分配且所有权移交本对象，测试结束即释放
            let owned = unsafe { ComWideString::from_raw(raw) }.expect("非空指针应构造成功");
            content = owned.to_string_lossy();
            assert_eq!(content, "容器未找到");
            // 析构在此发生，COM 堆内存被自动回收
        }
        // 释放后不应崩溃，且内容已在析构前拷出
        assert_eq!(content, "容器未找到");
    }

    #[test]
    fn test_null_raw_yields_none() {
        // SAFETY: 空指针是合法的「无输出」情形
        assert!(unsafe { ComWideString::from_raw(ptr::null_mut()) }.is_none());
        // SAFETY: 空指针是合法的「无输出」情形
        assert!(unsafe { ComAnsiString::from_raw(ptr::null_mut()) }.is_none());
        // SAFETY: 空指针且长度为 0 是合法的空数组情形
        let empty =
            unsafe { ComArray::<u32>::from_raw(ptr::null_mut(), 0) }.expect("应构造为空数组");
        assert!(empty.as_slice().is_empty());
    }

    #[test]
    fn test_com_array_borrows_and_frees() {
        let values = [1u32, 2, 3, 4, 5];
        let bytes = values.len() * size_of::<u32>();
        // SAFETY: 按字节数正确分配 5 个 u32 的空间
        let raw = unsafe { CoTaskMemAlloc(bytes) } as *mut u32;
        assert!(!raw.is_null(), "测试前提：COM 堆分配应当成功");
        // SAFETY: 目标缓冲区容量与源数组一致
        unsafe { copy_nonoverlapping(values.as_ptr(), raw, values.len()) };

        {
            // SAFETY: 指针由本测试分配且所有权移交本对象，离开作用域即释放
            let owned =
                unsafe { ComArray::from_raw(raw, values.len()) }.expect("非空指针应构造成功");
            assert_eq!(owned.as_slice(), &values);
        }
        // 析构完成，内存已归还 COM 堆
    }

    #[test]
    fn test_com_ansi_string_roundtrip() {
        let text = b"mcr.microsoft.com\0";
        let bytes = text.len();
        // SAFETY: 按字节数正确分配，长度含结尾 NUL
        let raw = unsafe { CoTaskMemAlloc(bytes) } as *mut i8;
        assert!(!raw.is_null(), "测试前提：COM 堆分配应当成功");
        // SAFETY: 目标缓冲区容量与 text 长度一致
        unsafe { copy_nonoverlapping(text.as_ptr().cast::<i8>(), raw, bytes) };

        {
            // SAFETY: 指针由本测试分配且所有权移交本对象
            let owned = unsafe { ComAnsiString::from_raw(raw) }.expect("非空指针应构造成功");
            assert_eq!(
                owned.as_str().expect("测试载荷为合法 UTF-8"),
                "mcr.microsoft.com"
            );
            assert_eq!(owned.to_string_lossy(), "mcr.microsoft.com");
        }
    }

    /// 非 UTF-8 的 ANSI 内容必须被显式报告，而非静默降级为空串
    ///
    /// 该行为关系到诊断质量：若 `as_str` 在非法编码时返回空串，
    /// `WslcInspectContainer` 的后续 JSON 解析会报出「语法错误」，
    /// 把矛头指向 JSON 本身，而非真正的原因（官方返回了非法字节）。
    #[test]
    fn test_com_ansi_string_reports_non_utf8_instead_of_silently_empty() {
        // 0xFF 在 UTF-8 中永不合法，可稳定构造出非法序列
        let bytes: [u8; 4] = [b'{', 0xFF, b'}', 0x00];
        let n = bytes.len();
        // SAFETY: 按字节数正确分配，长度含结尾 NUL
        let raw = unsafe { CoTaskMemAlloc(n) } as *mut i8;
        assert!(!raw.is_null(), "测试前提：COM 堆分配应当成功");
        // SAFETY: 目标缓冲区容量与 bytes 长度一致
        unsafe { copy_nonoverlapping(bytes.as_ptr().cast::<i8>(), raw, n) };

        // SAFETY: 指针由本测试分配且所有权移交本对象
        let owned = unsafe { ComAnsiString::from_raw(raw) }.expect("非空指针应构造成功");

        // 核心断言：必须报错，而非返回空串
        assert!(
            owned.as_str().is_err(),
            "非 UTF-8 内容应返回 Err，而不是静默变成空串"
        );
        // 有损转换仍可保留可读结构（非法字节以 U+FFFD 呈现）
        let lossy = owned.to_string_lossy();
        assert!(
            lossy.starts_with('{') && lossy.ends_with('}'),
            "有损转换应保留原有结构，实际为: {lossy:?}"
        );
    }
}
