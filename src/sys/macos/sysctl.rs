//! `sysctl` through libc rather than by spawning the `sysctl` binary.
//!
//! The CPU section runs at every detail level, so a process spawn there was
//! the most-paid cost on macOS. `sysctlbyname` is the same interface the tool
//! itself uses, and these keys have been stable for the life of the platform.

use std::ffi::CString;

use crate::scan::clean;

/// Read a `sysctl` key as a NUL-terminated string.
pub fn string(name: &str) -> Option<String> {
    let key = CString::new(name).ok()?;
    let mut size: libc::size_t = 0;

    // A null buffer asks only for the size.
    let status = unsafe {
        libc::sysctlbyname(
            key.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 || size == 0 {
        return None;
    }

    let mut buffer = vec![0u8; size];
    let status = unsafe {
        libc::sysctlbyname(
            key.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return None;
    }

    // The value includes its terminating NUL; the size does not always exclude it.
    buffer.truncate(size);
    let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    clean(String::from_utf8_lossy(&buffer[..end]))
}

/// Read a `sysctl` key as an unsigned integer.
///
/// Handles both the 32- and 64-bit forms, since the kernel picks per key and
/// asking for the wrong width silently truncates.
pub fn integer(name: &str) -> Option<u64> {
    let key = CString::new(name).ok()?;
    let mut size: libc::size_t = 0;

    let status = unsafe {
        libc::sysctlbyname(
            key.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return None;
    }

    match size {
        4 => {
            let mut value: u32 = 0;
            let mut size = std::mem::size_of::<u32>();
            let status = unsafe {
                libc::sysctlbyname(
                    key.as_ptr(),
                    std::ptr::from_mut(&mut value).cast(),
                    &mut size,
                    std::ptr::null_mut(),
                    0,
                )
            };
            (status == 0).then_some(u64::from(value))
        }
        8 => {
            let mut value: u64 = 0;
            let mut size = std::mem::size_of::<u64>();
            let status = unsafe {
                libc::sysctlbyname(
                    key.as_ptr(),
                    std::ptr::from_mut(&mut value).cast(),
                    &mut size,
                    std::ptr::null_mut(),
                    0,
                )
            };
            (status == 0).then_some(value)
        }
        _ => None,
    }
}

/// Whether a boolean-ish key is set to a non-zero value.
pub fn flag(name: &str) -> Option<bool> {
    integer(name).map(|v| v != 0)
}
