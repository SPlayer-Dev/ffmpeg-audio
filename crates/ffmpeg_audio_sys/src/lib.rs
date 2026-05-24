mod bindings;
mod consts;

pub use bindings::*;
pub use consts::*;

/// Returns a negative error code from a POSIX error code, to return from library functions.
#[must_use]
pub const fn averror(e: i32) -> i32 {
    // 这实际上是不必要的，AVERROR 的目的只是为了确保错误码为负数
    // 我们实际上可以直接判断传入的错误码是不是负数然后进行转换
    // 不过为了匹配 FFmpeg 原有的行为，这里还是使用 EDOM 进行判断
    if libc::EDOM > 0 { -e } else { e }
}

/// Returns a POSIX error code from a library function error return value.
#[must_use]
pub const fn avunerror(e: i32) -> i32 {
    if libc::EDOM > 0 { -e } else { e }
}
