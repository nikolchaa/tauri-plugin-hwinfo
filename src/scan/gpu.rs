//! GPU collection.
//!
//! The platform backend enumerates adapters the way the OS sees them (DXGI,
//! `/sys/class/drm`, `system_profiler`). Vulkan results are then merged onto
//! those by PCI vendor + device ID, contributing driver versions, device class
//! and heap sizes. Vulkan devices that match no native adapter are appended, so
//! software rasterisers and virtual GPUs still show up.

use std::path::Path;

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
    apply_hip(ctx, &mut gpus);
    apply_opencl(ctx, &mut gpus);

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

    let query_args = [
        "--query-gpu=name,driver_version,memory.total,compute_cap,pci.bus_id",
        "--format=csv,noheader,nounits",
    ];
    let output = match run_nvidia_smi(&query_args) {
        Ok(o) => o,
        Err(e) => {
            // The driver can be fully functional without `nvidia-smi` being
            // installed — minimal containers ship exactly the kernel module
            // and `libcuda`. Answer what a file read can answer rather than
            // reporting nothing at all.
            let driver_api_present = find_libcuda();
            ctx.warn(if driver_api_present {
                format!("cuda: NVIDIA driver library is present but `nvidia-smi` is unavailable ({e}); version details omitted")
            } else {
                format!("cuda: an NVIDIA adapter is present but CUDA details are unavailable ({e})")
            });
            if driver_api_present {
                for gpu in gpus.iter_mut().filter(|g| g.vendor_id == Some(0x10DE)) {
                    gpu.api.cuda = true;
                }
            }
            return;
        }
    };

    // `nvidia-smi` lists adapters in PCI bus order; match on bus ID where we
    // have one, and fall back to the order NVIDIA adapters appear in.
    let rows: Vec<(Option<String>, Vec<String>)> = output
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let fields: Vec<String> = l.split(',').map(|f| f.trim().to_string()).collect();
            let bus = fields.get(4).and_then(|b| canonical_pci_address(b));
            (bus, fields)
        })
        .collect();

    let cuda_version = nvidia_cuda_version();

    for (nvidia_index, gpu) in gpus
        .iter_mut()
        .filter(|g| g.vendor_id == Some(0x10DE))
        .enumerate()
    {
        let row = gpu
            .pci_bus
            .as_deref()
            .and_then(canonical_pci_address)
            .and_then(|bus| rows.iter().find(|r| r.0.as_deref() == Some(bus.as_str())))
            .or_else(|| rows.get(nvidia_index));

        let Some((_, fields)) = row else { continue };

        gpu.api.cuda = true;
        gpu.api.cuda_version = cuda_version.clone();
        gpu.api.compute_capability = fields.get(3).and_then(clean);
        if gpu.driver_version.is_none() {
            gpu.driver_version = fields.get(1).and_then(clean);
        }
        if gpu.vram_mb.is_none() {
            gpu.vram_mb = fields.get(2).and_then(|v| v.trim().parse::<u64>().ok());
        }
    }
}

/// Run `nvidia-smi`, falling back to the one place it hides off `PATH`.
///
/// WSL2 installs the real binary under `/usr/lib/wsl/lib`, which interop
/// normally adds to `PATH` — but not for every process environment.
fn run_nvidia_smi(args: &[&str]) -> Result<String, String> {
    const WSL_SMI: &str = "/usr/lib/wsl/lib/nvidia-smi";
    sys::util::run("nvidia-smi", args).or_else(|e| {
        if Path::new(WSL_SMI).exists() {
            sys::util::run(WSL_SMI, args)
        } else {
            Err(e)
        }
    })
}

/// Whether any CUDA driver library is installed, wherever the distro put it.
fn find_libcuda() -> bool {
    let mut dirs: Vec<std::path::PathBuf> = vec![
        "/usr/lib64".into(),
        "/usr/lib".into(),
        "/usr/local/cuda/lib64".into(),
        "/usr/lib/wsl/lib".into(),
    ];
    // Debian multiarch triplets vary by architecture.
    if let Ok(entries) = std::fs::read_dir("/usr/lib") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().ends_with("-linux-gnu") {
                dirs.push(entry.path());
            }
        }
    }

    dirs.iter().any(|dir| {
        ["libcuda.so.1", "libcuda.so"]
            .iter()
            .any(|lib| dir.join(lib).exists())
    })
}

/// Canonical form of a PCI address, so differently padded spellings match.
///
/// The same adapter appears as `0000:09:00.0` in sysfs and Vulkan but
/// `00000000:09:00.0` in modern `nvidia-smi` output, which pads the domain to
/// eight digits. Parsing and reformatting beats string surgery: every field is
/// validated along the way, and unparseable values simply never match.
pub(crate) fn canonical_pci_address(raw: &str) -> Option<String> {
    let lowered = raw.trim().to_ascii_lowercase();
    let mut fields = lowered.split([':', '.']);
    let domain = u16::from_str_radix(fields.next()?, 16).ok()?;
    let bus = u8::from_str_radix(fields.next()?, 16).ok()?;
    let device = u8::from_str_radix(fields.next()?, 16).ok()?;
    let function = u8::from_str_radix(fields.next()?, 16).ok()?;
    Some(format!("{domain:04x}:{bus:02x}:{device:02x}.{function}"))
}

/// Mark AMD adapters with what the HIP/ROCm stack knows about them.
fn apply_hip(ctx: &mut Ctx, gpus: &mut [Gpu]) {
    if !ctx.wants(DetailLevel::Capabilities) {
        return;
    }
    // 0x1002 is the graphics vendor ID; 0x1022 is the chipset one, which some
    // integrated parts report.
    if !gpus
        .iter()
        .any(|g| matches!(g.vendor_id, Some(0x1002 | 0x1022)))
    {
        return;
    }

    let hip = super::compute::hip(ctx);

    for gpu in gpus
        .iter_mut()
        .filter(|g| matches!(g.vendor_id, Some(0x1002 | 0x1022)))
    {
        // Match on PCI address where the kernel gave us one; otherwise fall
        // back to the single-device case, which covers almost every desktop.
        let device = gpu
            .pci_bus
            .as_ref()
            .and_then(|bus| {
                hip.devices.iter().find(|d| {
                    d.pci_bus
                        .as_deref()
                        .is_some_and(|b| b.eq_ignore_ascii_case(bus))
                })
            })
            .or_else(|| (hip.devices.len() == 1).then(|| &hip.devices[0]));

        // An adapter the kernel driver does not expose as a compute agent
        // cannot run HIP even when the runtime is installed.
        let visible = device.is_some() || (hip.devices.is_empty() && hip.runtime_present);

        gpu.api.hip = hip.runtime_present && visible;
        gpu.api.hip_version = hip.hip_version.clone();
        gpu.api.rocm_version = hip.rocm_version.clone();
        gpu.api.gfx_architecture = device.and_then(|d| d.gfx_architecture.clone());

        // The kernel driver's name is often better than what sysfs alone gave.
        if let Some(name) = device.and_then(|d| d.name.as_ref()) {
            if gpu.model.starts_with("Device 0x") {
                gpu.model = name.clone();
            }
        }
    }
}

/// OpenCL is a system-wide capability rather than a per-adapter one — the ICD
/// loader reports platforms, not devices — so every adapter gets the same
/// answer.
fn apply_opencl(ctx: &mut Ctx, gpus: &mut [Gpu]) {
    if !ctx.wants(DetailLevel::Capabilities) || gpus.is_empty() {
        return;
    }

    let opencl = super::compute::opencl(ctx);
    for gpu in gpus.iter_mut() {
        gpu.api.opencl = opencl.available;
        gpu.api.opencl_version = opencl.version.clone();
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
        api: GpuApiSupport {
            // Metal is the one API whose availability follows from the target
            // rather than a probe: every Mac that can run a Tauri app has it,
            // and nothing else ever does.
            metal: cfg!(target_os = "macos"),
            ..GpuApiSupport::default()
        },
        uuid: None,
    }
}

#[cfg(test)]
mod tests {
    use super::canonical_pci_address;

    #[test]
    fn pci_addresses_match_across_paddings() {
        // sysfs and Vulkan spell it with a four-digit domain; modern
        // nvidia-smi pads to eight.
        let sysfs = canonical_pci_address("0000:09:00.0").unwrap();
        let smi = canonical_pci_address("00000000:09:00.0").unwrap();
        assert_eq!(sysfs, smi);

        // Hex letters compare case-insensitively across paddings.
        assert_eq!(
            canonical_pci_address("0000:2A:10.3"),
            canonical_pci_address("00000000:02a:010.3")
        );
    }

    #[test]
    fn pci_address_garbage_never_matches() {
        assert!(canonical_pci_address("nvidia-smi").is_none());
        assert!(canonical_pci_address("").is_none());
        assert!(canonical_pci_address("0000:09:00").is_none());
    }
}
