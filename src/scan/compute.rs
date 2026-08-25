//! Detection of GPU compute runtimes: HIP/ROCm and OpenCL.
//!
//! Neither probe spawns a process. HIP is read from the kernel driver's own
//! sysfs topology on Linux and from the SDK's install layout on Windows;
//! OpenCL is answered by the ICD loader itself.
//!
//! Deliberately *not* done: loading `libamdhip64` and calling
//! `hipGetDeviceProperties`. `hipDeviceProp_t` changed layout between ROCm 5
//! and 6 — hence the `hipDeviceProp_tR0600` alias in AMD's own headers — and a
//! hand-written struct that is right for one release is memory corruption on
//! the other. The kernel topology carries the same architecture information
//! with no ABI to get wrong.

use super::Ctx;
use crate::sys;

// `clean` is only reached from the OpenCL version parsing.
#[cfg(feature = "opencl")]
use super::clean;

/// What the HIP/ROCm stack reports about one adapter.
#[derive(Debug, Clone, Default)]
pub struct HipDevice {
    /// PCI address, used to line this up with an enumerated GPU.
    pub pci_bus: Option<String>,
    /// e.g. `"gfx1100"`.
    pub gfx_architecture: Option<String>,
    /// Marketing name as the kernel driver knows it.
    pub name: Option<String>,
}

/// The HIP/ROCm installation as a whole, plus the devices it can see.
#[derive(Debug, Clone, Default)]
pub struct Hip {
    pub runtime_present: bool,
    pub hip_version: Option<String>,
    pub rocm_version: Option<String>,
    pub devices: Vec<HipDevice>,
}

pub fn hip(ctx: &mut Ctx) -> Hip {
    sys::hip(ctx)
}

/// Decode a KFD `gfx_target_version` into the target name compilers use.
///
/// The value packs `major * 10000 + minor * 100 + step`, and the name renders
/// the major in decimal but the minor and step in hex — which is why `gfx90a`
/// comes out of `90010` rather than looking like a typo. This mirrors the
/// arithmetic in AMD's own `rocm_agent_enumerator`.
///
/// Only Linux exposes the source value, but the decoding lives here with the
/// rest of the HIP knowledge so it stays testable everywhere.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn gfx_name(target_version: u64) -> Option<String> {
    if target_version == 0 {
        return None;
    }
    let major = (target_version / 10000) % 100;
    let minor = (target_version / 100) % 100;
    let step = target_version % 100;
    Some(format!("gfx{major}{minor:x}{step:x}"))
}

// ---------------------------------------------------------------------------
// OpenCL
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct OpenCl {
    pub available: bool,
    /// Highest version string any platform reported.
    pub version: Option<String>,
}

/// Ask the OpenCL ICD loader whether any platform is actually registered.
///
/// Only two symbols are called, both with stable scalar signatures that have
/// not changed since OpenCL 1.0. Nothing links against OpenCL at build time.
#[cfg(feature = "opencl")]
pub fn opencl(ctx: &mut Ctx) -> OpenCl {
    use std::ffi::c_void;

    // Candidate loader names, most specific first.
    const NAMES: &[&str] = if cfg!(windows) {
        &["OpenCL.dll"]
    } else if cfg!(target_os = "macos") {
        &["/System/Library/Frameworks/OpenCL.framework/OpenCL"]
    } else {
        &["libOpenCL.so.1", "libOpenCL.so"]
    };

    type ClGetPlatformIDs = unsafe extern "C" fn(u32, *mut *mut c_void, *mut u32) -> i32;
    type ClGetPlatformInfo =
        unsafe extern "C" fn(*mut c_void, u32, usize, *mut c_void, *mut usize) -> i32;
    const CL_PLATFORM_VERSION: u32 = 0x0901;

    let library = NAMES
        .iter()
        .find_map(|&name| unsafe { libloading::Library::new(name) }.ok());

    let Some(library) = library else {
        ctx.warn("opencl: no ICD loader found");
        return OpenCl::default();
    };

    unsafe {
        let Ok(get_ids) = library.get::<ClGetPlatformIDs>(b"clGetPlatformIDs\0") else {
            ctx.warn("opencl: loader is missing clGetPlatformIDs");
            return OpenCl::default();
        };

        let mut count: u32 = 0;
        if get_ids(0, std::ptr::null_mut(), &mut count) != 0 || count == 0 {
            // A loader with no registered platforms behind it: OpenCL is
            // installed but nothing can run.
            return OpenCl::default();
        }

        let mut platforms: Vec<*mut c_void> = vec![std::ptr::null_mut(); count as usize];
        if get_ids(count, platforms.as_mut_ptr(), &mut count) != 0 {
            return OpenCl {
                available: true,
                version: None,
            };
        }

        let version = library
            .get::<ClGetPlatformInfo>(b"clGetPlatformInfo\0")
            .ok()
            .and_then(|get_info| {
                platforms
                    .iter()
                    .take(count as usize)
                    .filter_map(|&platform| {
                        let mut buffer = [0u8; 128];
                        let mut written: usize = 0;
                        let status = get_info(
                            platform,
                            CL_PLATFORM_VERSION,
                            buffer.len(),
                            buffer.as_mut_ptr().cast(),
                            &mut written,
                        );
                        (status == 0)
                            .then(|| {
                                let text = String::from_utf8_lossy(
                                    &buffer[..written.saturating_sub(1).min(buffer.len())],
                                );
                                // "OpenCL 3.0 CUDA 12.4.131" -> "3.0"
                                clean(text.split_whitespace().nth(1).unwrap_or(text.trim()))
                            })
                            .flatten()
                    })
                    .max()
            });

        OpenCl {
            available: true,
            version,
        }
    }
}

#[cfg(not(feature = "opencl"))]
pub fn opencl(ctx: &mut Ctx) -> OpenCl {
    ctx.warn("opencl: the `opencl` cargo feature is disabled");
    OpenCl::default()
}

#[cfg(test)]
mod tests {
    use super::gfx_name;

    #[test]
    fn decodes_gfx_target_versions() {
        // The packing is major * 10000 + minor * 100 + step, so the digits of
        // the encoded value do not line up with the digits of the name:
        // gfx1030 is 100300, not 103000.
        assert_eq!(gfx_name(100300).as_deref(), Some("gfx1030"));
        assert_eq!(gfx_name(110000).as_deref(), Some("gfx1100"));
        assert_eq!(gfx_name(110003).as_deref(), Some("gfx1103"));
        // Vega: a single-digit major.
        assert_eq!(gfx_name(90006).as_deref(), Some("gfx906"));
        // CDNA2, where the step renders in hex — the case a decimal-only
        // decoder silently gets wrong as "gfx9010".
        assert_eq!(gfx_name(90010).as_deref(), Some("gfx90a"));
        // A CPU node in the same topology reports zero.
        assert_eq!(gfx_name(0), None);
    }
}
