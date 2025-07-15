use core::time::Duration;

use crate::DError;

pub trait Kernel {
    fn sleep(duration: Duration);
}

pub(crate) fn sleep(duration: Duration) {
    unsafe extern "Rust" {
        fn _eth_intel_sleep(duration: Duration);
    }
    unsafe {
        _eth_intel_sleep(duration);
    }
}

#[macro_export]
macro_rules! set_impl {
    ($t: ty) => {
        #[unsafe(no_mangle)]
        unsafe fn _eth_intel_sleep(duration: core::time::Duration) {
            <$t as $crate::osal::Kernel>::sleep(duration)
        }
    };
}

pub(crate) fn wait_for<F: FnMut() -> bool>(
    mut f: F,
    interval: Duration,
    try_count: Option<usize>,
) -> Result<(), DError> {
    for _ in 0..try_count.unwrap_or(usize::MAX) {
        if f() {
            return Ok(());
        }

        sleep(interval);
    }
    Err(DError::Timeout)
}
