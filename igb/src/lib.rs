#![no_std]

use core::time::Duration;

pub use crate::err::DError;

extern crate alloc;

mod err;

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
        #[no_mangle]
        unsafe fn _eth_intel_sleep(duration: core::time::Duration) {
            <$t as $crate::Kernel>::sleep(duration)
        }
    };
}

fn wait_for<F: Fn() -> bool>(
    f: F,
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
