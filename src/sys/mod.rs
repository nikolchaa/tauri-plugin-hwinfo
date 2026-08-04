//! Platform seam.
//!
//! Each platform module implements the same set of functions. The portable
//! collectors in [`crate::scan`] gather what `sysinfo` and friends can give on
//! every OS, then merge these platform-native extras on top.

use crate::models::*;
use crate::scan::Ctx;

// Aliased rather than loaded through `#[path]` so each backend can have its own
// submodules without the nested-path resolution surprises that `#[path]` brings.
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use self::linux as imp;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use self::macos as imp;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use self::windows as imp;

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod unsupported;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
use self::unsupported as imp;

pub(crate) mod util;

/// Per-package CPU facts the portable path cannot reach.
#[derive(Debug, Clone, Default)]
pub struct CpuNative {
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub socket: Option<String>,
    pub physical_cores: Option<u32>,
    pub threads: Option<u32>,
    pub base_frequency: Option<u32>,
    pub max_frequency: Option<u32>,
    pub current_frequency: Option<u32>,
    pub l1d_kb: Option<u32>,
    pub l1i_kb: Option<u32>,
    pub l2_kb: Option<u32>,
    pub l3_kb: Option<u32>,
    pub virtualization: Option<bool>,
    pub microcode: Option<String>,
    pub temperature_c: Option<f32>,
    /// Identifying; the caller redacts it in safe mode.
    pub serial: Option<String>,
}

/// Per-interface network facts `sysinfo` does not expose.
#[derive(Debug, Clone, Default)]
pub struct NetNative {
    pub description: Option<String>,
    pub speed_mbps: Option<u64>,
}

/// A display as the windowing system sees it, before merging with Tauri's
/// monitor list.
#[derive(Debug, Clone, Default)]
pub struct DisplayNative {
    /// Adapter device name used to match against Tauri's monitor name.
    pub name: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub refresh_rate_hz: Option<f64>,
    pub native_width: Option<u32>,
    pub native_height: Option<u32>,
    pub position_x: Option<i32>,
    pub position_y: Option<i32>,
    pub is_primary: Option<bool>,
    pub is_internal: Option<bool>,
    pub bits_per_pixel: Option<u32>,
    pub physical_width_mm: Option<u32>,
    pub physical_height_mm: Option<u32>,
    pub manufacture_year: Option<u32>,
    /// Identifying; the caller redacts it in safe mode.
    pub serial: Option<String>,
}

/// Physical DIMM inventory plus slot occupancy.
#[derive(Debug, Clone, Default)]
pub struct MemoryNative {
    pub modules: Vec<MemoryModule>,
    pub slots_total: Option<u32>,
    pub slots_used: Option<u32>,
}

/// OS facts beyond what `sysinfo` and `os_info` report.
#[derive(Debug, Clone, Default)]
pub struct OsNative {
    pub build: Option<String>,
    pub edition: Option<String>,
    pub codename: Option<String>,
    pub virtualization: Option<String>,
    /// Identifying; the caller redacts it in safe mode.
    pub machine_id: Option<String>,
    /// Identifying; the caller redacts it in safe mode.
    pub user: Option<String>,
}

pub fn cpu(ctx: &mut Ctx) -> Vec<CpuNative> {
    imp::cpu(ctx)
}

/// GPUs discovered through the platform's own device enumeration. Vulkan
/// results are merged onto these by PCI vendor/device ID.
pub fn gpus(ctx: &mut Ctx) -> Vec<Gpu> {
    imp::gpus(ctx)
}

pub fn memory(ctx: &mut Ctx) -> MemoryNative {
    imp::memory(ctx)
}

pub fn disks(ctx: &mut Ctx) -> Vec<Disk> {
    imp::disks(ctx)
}

/// Keyed by interface name, matching `sysinfo`'s network keys.
pub fn network(ctx: &mut Ctx) -> std::collections::HashMap<String, NetNative> {
    imp::network(ctx)
}

pub fn displays(ctx: &mut Ctx) -> Vec<DisplayNative> {
    imp::displays(ctx)
}

pub fn board(ctx: &mut Ctx) -> Board {
    imp::board(ctx)
}

pub fn os(ctx: &mut Ctx) -> OsNative {
    imp::os(ctx)
}

/// Resolve a PCI vendor ID to the name the vendor uses in its own drivers.
pub fn pci_vendor_name(vendor_id: u32) -> Option<&'static str> {
    Some(match vendor_id {
        0x1002 | 0x1022 => "Advanced Micro Devices, Inc.",
        0x10DE => "NVIDIA Corporation",
        0x8086 => "Intel Corporation",
        0x106B => "Apple Inc.",
        0x13B5 => "Arm Limited",
        0x5143 => "Qualcomm Technologies, Inc.",
        0x14E4 => "Broadcom Inc.",
        0x1AF4 | 0x1B36 => "Red Hat, Inc.",
        0x15AD => "VMware, Inc.",
        0x1414 => "Microsoft Corporation",
        0x1D17 => "Zhaoxin",
        0x1EB1 => "VeriSilicon",
        0x10005 => "Mesa",
        _ => return None,
    })
}
