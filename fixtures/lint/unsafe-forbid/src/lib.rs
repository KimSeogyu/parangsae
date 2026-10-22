//! Deliberate `unsafe`. `cargo check` must fail with `unsafe_code` forbidden.

pub fn exhibit() -> i64 {
    unsafe { forged_clock() }
}

unsafe fn forged_clock() -> i64 {
    0
}
