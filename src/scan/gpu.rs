//! GPU collection.
//!
//! The platform backend enumerates adapters the way the OS sees them (DXGI,
//! `/sys/class/drm`, `system_profiler`). Vulkan results are then merged onto
//! those by PCI vendor + device ID, contributing driver versions, device class
//! and heap sizes. Vulkan devices that match no native adapter are appended, so
//! software rasterisers and virtual GPUs still show up.

use super::{clean, Ctx};
use crate::models::*;
use crate::sys;

#[cfg(feature = "vulkan")]
use super::vulkan::VulkanDevice;
#[cfg(not(feature = "vulkan"))]
type VulkanDevice = ();

pub fn collect(ctx: &mut Ctx, vulkan: &[VulkanDevice]) -> Vec<Gpu> {
    let mut gpus = sys::gpus(ctx);

    #[cfg(feature = "vulkan")]
    merge_vulkan(&mut gpus, vulkan);
    #[cfg(not(feature = "vulkan"))]
    let _ = vulkan;

    nvidia_smi(ctx, &mut gpus);

    if gpus.is_empty() {
        ctx.warn("gpu: no display adapters could be enumerated");
    }

    if !ctx.mode.is_unsafe() {
        for gpu in &mut gpus {
            gpu.uuid = None;
        }
    }

    gpus
}

#[cfg(feature = "vulkan")]
fn merge_vulkan(gpus: &mut Vec<Gpu>, vulkan: &[VulkanDevice]) {
    let mut unmatched: Vec<&VulkanDevice> = Vec::new();

    for vk_device in vulkan {
        let target = gpus.iter_mut().find(|g| {
            g.vendor_id == Some(vk_device.vendor_id) && g.device_id == Some(vk_device.device_id)
        });

        let Some(gpu) = target else {
            unmatched.push(vk_device);
            continue;
        };

        apply_vulkan(gpu, vk_device);
    }

    for vk_device in unmatched {
        let mut gpu = blank_gpu(
            sys::pci_vendor_name(vk_device.vendor_id)
                .map(str::to_string)
                .unwrap_or_else(|| format!("Vendor 0x{:04X}", vk_device.vendor_id)),
            vk_device.name.clone(),
        );
        gpu.vendor_id = Some(vk_device.vendor_id);
        gpu.vendor_id_hex = Some(format!("0x{:04X}", vk_device.vendor_id));
        gpu.device_id = Some(vk_device.device_id);
        gpu.device_id_hex = Some(format!("0x{:04X}", vk_device.device_id));
        apply_vulkan(&mut gpu, vk_device);
        gpus.push(gpu);
    }
}

#[cfg(feature = "vulkan")]
fn apply_vulkan(gpu: &mut Gpu, vk_device: &VulkanDevice) {
    gpu.api.vulkan = true;
    gpu.api.vulkan_version = Some(vk_device.api_version.clone());
    gpu.api.vulkan_driver = vk_device
        .driver_name
        .clone()
        .zip(vk_device.driver_info.clone())
        .map(|(name, info)| format!("{name} ({info})"))
        .or_else(|| vk_device.driver_name.clone())
        .or_else(|| vk_device.driver_info.clone());

    // Vulkan asks the driver what class of device this is; the DXGI and sysfs
    // paths can only guess from heap sizes. The one thing Vulkan cannot see is
    // a software adapter that exposes no Vulkan device at all, so a `Cpu`
    // verdict from the platform stands.
    if vk_device.kind != GpuKind::Unknown && gpu.kind != GpuKind::Cpu {
        gpu.kind = vk_device.kind;
    }
    if gpu.driver_version.is_none() {
        gpu.driver_version = vk_device.driver_version.clone();
    }
    if gpu.pci_bus.is_none() {
        gpu.pci_bus = vk_device.pci_bus.clone();
    }
    if gpu.uuid.is_none() {
        gpu.uuid = vk_device.uuid.clone();
    }
    // Vulkan heap sizes are authoritative for discrete adapters; WMI's
    // `AdapterRAM` wraps at 4 GiB and several Linux paths report nothing.
    if gpu.vram_mb.is_none() || vk_device.kind == GpuKind::Discrete {
        gpu.vram_mb = vk_device.device_local_mb.or(gpu.vram_mb);
    }
    if gpu.model.is_empty() || gpu.model == "Unknown" {
        gpu.model = vk_device.name.clone();
    }
}

/// Ask `nvidia-smi` for the CUDA facts no other source exposes. Skipped
/// entirely when no NVIDIA adapter is present.
fn nvidia_smi(ctx: &mut Ctx, gpus: &mut [Gpu]) {
    // A process spawn that also wakes a sleeping dGPU, so it waits for the
    // tier where device probing is expected.
    if !ctx.wants(DetailLevel::Capabilities) {
        return;
    }
    if !gpus.iter().any(|g| g.vendor_id == Some(0x10DE)) {
        return;
    }

    let output = match sys::util::run(
        "nvidia-smi",
        &[
            "--query-gpu=name,driver_version,memory.total,compute_cap,pci.bus_id",
            "--format=csv,noheader,nounits",
        ],
    ) {
        Ok(o) => o,
        Err(e) => {
            ctx.warn(format!(
                "cuda: an NVIDIA adapter is present but CUDA details are unavailable ({e})"
            ));
            return;
        }
    };

    // `nvidia-smi` lists adapters in PCI bus order; match on bus ID where we
    // have one, and fall back to the order NVIDIA adapters appear in.
    let rows: Vec<Vec<String>> = output
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split(',').map(|f| f.trim().to_string()).collect())
        .collect();

    let cuda_version = nvidia_cuda_version();

    for (nvidia_index, gpu) in gpus
        .iter_mut()
        .filter(|g| g.vendor_id == Some(0x10DE))
        .enumerate()
    {
        let row = gpu
            .pci_bus
            .as_ref()
            .and_then(|bus| {
                rows.iter().find(|r| {
                    r.get(4)
                        .is_some_and(|b| b.eq_ignore_ascii_case(bus) || b.ends_with(bus))
                })
            })
            .or_else(|| rows.get(nvidia_index));

        let Some(row) = row else { continue };

        gpu.api.cuda = true;
        gpu.api.cuda_version = cuda_version.clone();
        gpu.api.compute_capability = row.get(3).and_then(clean);
        if gpu.driver_version.is_none() {
            gpu.driver_version = row.get(1).and_then(clean);
        }
        if gpu.vram_mb.is_none() {
            gpu.vram_mb = row.get(2).and_then(|v| v.trim().parse::<u64>().ok());
        }
    }
}

/// The CUDA runtime version is a header line rather than a queryable field.
fn nvidia_cuda_version() -> Option<String> {
    let output = sys::util::run("nvidia-smi", &[]).ok()?;
    let line = output.lines().find(|l| l.contains("CUDA Version"))?;
    let after = line.split("CUDA Version:").nth(1)?;
    clean(after.split_whitespace().next()?)
}

pub(crate) fn blank_gpu(manufacturer: String, model: String) -> Gpu {
    Gpu {
        manufacturer,
        model,
        kind: GpuKind::Unknown,
        vendor_id: None,
        vendor_id_hex: None,
        device_id: None,
        device_id_hex: None,
        subsystem_id: None,
        revision: None,
        vram_mb: None,
        shared_memory_mb: None,
        driver_version: None,
        driver_date: None,
        pci_bus: None,
        current_resolution: None,
        api: GpuApiSupport::default(),
        uuid: None,
    }
}
