
#[inline(always)]
pub fn barrier() {
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

/// Calling this function will cause a linker error when building the firmware,
/// unless the compiler optimises it away completely.
///
/// This is used to build assertions that are evaluated at compile time but
/// aren't officially Rust const code.
///
/// For test builds, this is converted to a run-time panic.
#[inline(always)]
pub fn unreachable() -> ! {
    #[cfg(target_os = "none")]
    unsafe {
        // This will cause a compiler error if not removed by the optimizer.
        unsafe extern "C" {fn nowayjose();}
        nowayjose();
    }
    panic!();
}

/// Cause a build time error if the condition fails and the code path is not
/// optimized out.  For test builds this is converted to a run-time check.
#[macro_export]
macro_rules! link_assert {
    ($e:expr) => { if !$e {$crate::utils::unreachable()} }
}
