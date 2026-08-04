//! Adapter enumeration through DXGI.
//!
//! DXGI is used in preference to `Win32_VideoController` because the latter's
//! `AdapterRAM` is a `uint32`: any adapter with 4 GiB or more of VRAM reports a
//! wrapped, wrong value. DXGI also enumerates every adapter, including the
//! integrated one on a laptop with switchable graphics.

use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1,
    D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_12_0, D3D_FEATURE_LEVEL_12_1,
};
use windows::Win32::Graphics::Direct3D11::{D3D11CreateDevice, D3D11_SDK_VERSION};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
    DXGI_ERROR_NOT_FOUND,
};

use crate::scan::gpu::blank_gpu;
use crate::scan::{to_mb, Ctx};
use crate::models::DetailLevel;
use crate::sys::util::from_wide;
use crate::sys::pci_vendor_name;
use crate::models::*;

pub fn adapters(ctx: &mut Ctx) -> Vec<Gpu> {
    let factory: IDXGIFactory1 = match unsafe { CreateDXGIFactory1() } {
        Ok(f) => f,
        Err(e) => {
            ctx.warn(format!("gpu: DXGI factory creation failed ({e})"));
            return Vec::new();
        }
    };

    let mut gpus = Vec::new();

    for index in 0.. {
        let adapter: IDXGIAdapter1 = match unsafe { factory.EnumAdapters1(index) } {
            Ok(a) => a,
            Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(e) => {
                ctx.warn(format!("gpu: DXGI adapter {index} could not be read ({e})"));
                break;
            }
        };

        let desc = match unsafe { adapter.GetDesc1() } {
            Ok(d) => d,
            Err(e) => {
                ctx.warn(format!(
                    "gpu: DXGI adapter {index} description unavailable ({e})"
                ));
                continue;
            }
        };

        let model = from_wide(&desc.Description);
        let is_software = desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0
            || model.contains("Basic Render");

        let mut gpu = blank_gpu(
            pci_vendor_name(desc.VendorId)
                .map(str::to_string)
                .unwrap_or_else(|| format!("Vendor 0x{:04X}", desc.VendorId)),
            model,
        );

        gpu.vendor_id = Some(desc.VendorId);
        gpu.vendor_id_hex = Some(format!("0x{:04X}", desc.VendorId));
        gpu.device_id = Some(desc.DeviceId);
        gpu.device_id_hex = Some(format!("0x{:04X}", desc.DeviceId));
        gpu.subsystem_id = Some(format!("0x{:08X}", desc.SubSysId));
        gpu.revision = Some(desc.Revision);
        gpu.vram_mb = Some(to_mb(desc.DedicatedVideoMemory as u64));
        gpu.shared_memory_mb = Some(to_mb(desc.SharedSystemMemory as u64));
        // A first guess only; Vulkan overrides it with the driver's own answer
        // when a Vulkan device matches. Integrated GPUs either carve out no
        // dedicated memory at all or reserve a token amount - Intel's iGPUs
        // report 128 MiB - while any real discrete card has far more.
        const DISCRETE_VRAM_FLOOR: usize = 512 * 1024 * 1024;
        gpu.kind = if is_software {
            GpuKind::Cpu
        } else if desc.DedicatedVideoMemory < DISCRETE_VRAM_FLOOR {
            GpuKind::Integrated
        } else {
            GpuKind::Discrete
        };
        // Probing the feature level means creating a real D3D11 device, which
        // on a hybrid laptop wakes a sleeping discrete GPU and can take
        // seconds. It is the most expensive thing in the whole scan.
        if ctx.wants(DetailLevel::Full) {
            gpu.api.directx_feature_level = feature_level(&adapter);
        }

        gpus.push(gpu);
    }

    gpus
}

/// Highest Direct3D feature level the adapter supports.
///
/// The device is created and dropped immediately; `D3D11CreateDevice` reports
/// the level it settled on even when we ask for no device object back.
fn feature_level(adapter: &IDXGIAdapter1) -> Option<String> {
    const LEVELS: [D3D_FEATURE_LEVEL; 6] = [
        D3D_FEATURE_LEVEL_12_1,
        D3D_FEATURE_LEVEL_12_0,
        D3D_FEATURE_LEVEL_11_1,
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_1,
        D3D_FEATURE_LEVEL_10_0,
    ];

    let base: IDXGIAdapter = adapter.cast().ok()?;
    let mut chosen = D3D_FEATURE_LEVEL::default();

    unsafe {
        D3D11CreateDevice(
            &base,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            Default::default(),
            Some(&LEVELS),
            D3D11_SDK_VERSION,
            None,
            Some(&mut chosen),
            None,
        )
        .ok()?;
    }

    // windows-rs models feature levels as newtype constants, not enum variants.
    LEVELS
        .iter()
        .zip(["12_1", "12_0", "11_1", "11_0", "10_1", "10_0"])
        .find(|(level, _)| **level == chosen)
        .map(|(_, name)| name.to_string())
}
