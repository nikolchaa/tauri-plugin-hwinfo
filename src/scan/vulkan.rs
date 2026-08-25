//! Vulkan-based GPU enumeration.
//!
//! The Vulkan loader is opened at runtime, so a machine without Vulkan simply
//! produces no results and a warning - nothing links against it at build time.
//!
//! This is the only source that reports driver versions, device classes and
//! true heap sizes identically on Windows, Linux and macOS (via MoltenVK), so
//! it is worth the FFI.

use std::ffi::{c_char, CStr};

use ash::vk;

use super::Ctx;
use crate::models::GpuKind;

/// One `VkPhysicalDevice`, flattened.
#[derive(Debug, Clone)]
pub struct VulkanDevice {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub kind: GpuKind,
    /// Highest API version this device supports, e.g. `"1.3.280"`.
    pub api_version: String,
    /// Driver version decoded with the vendor's own packing.
    pub driver_version: Option<String>,
    /// e.g. `"NVIDIA"`, `"radv"`, `"MoltenVK"`.
    pub driver_name: Option<String>,
    /// Free-form driver detail, e.g. `"Mesa 24.0.3"`.
    pub driver_info: Option<String>,
    /// Sum of the device-local heaps.
    pub device_local_mb: Option<u64>,
    /// `"0000:0a:00.0"`.
    pub pci_bus: Option<String>,
    pub uuid: Option<String>,
}

pub fn probe(ctx: &mut Ctx) -> Vec<VulkanDevice> {
    match unsafe { enumerate() } {
        Ok(devices) => {
            if devices.is_empty() {
                ctx.warn("vulkan: loader present but reported no physical devices");
            }
            devices
        }
        Err(e) => {
            ctx.warn(format!("vulkan: {e}"));
            Vec::new()
        }
    }
}

unsafe fn enumerate() -> Result<Vec<VulkanDevice>, String> {
    let entry = ash::Entry::load().map_err(|e| format!("loader unavailable: {e}"))?;

    // Ask for the highest version the loader admits to, capped at 1.3 - asking
    // for more than the loader supports fails instance creation outright.
    let loader_version = entry
        .try_enumerate_instance_version()
        .ok()
        .flatten()
        .unwrap_or(vk::API_VERSION_1_0);
    let api_version = loader_version.min(vk::API_VERSION_1_3);

    let instance_extensions = entry
        .enumerate_instance_extension_properties(None)
        .unwrap_or_default();
    let has_instance_ext = |name: &CStr| {
        instance_extensions
            .iter()
            .any(|e| e.extension_name_as_c_str().is_ok_and(|n| n == name))
    };

    // MoltenVK is a non-conformant implementation and stays hidden unless the
    // portability flag is set.
    let portability = has_instance_ext(vk::KHR_PORTABILITY_ENUMERATION_NAME);
    let mut extension_names: Vec<*const c_char> = Vec::new();
    let mut flags = vk::InstanceCreateFlags::empty();
    if portability {
        extension_names.push(vk::KHR_PORTABILITY_ENUMERATION_NAME.as_ptr());
        flags |= vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR;
    }

    let app_name = c"tauri-plugin-hwinfo";
    let app_info = vk::ApplicationInfo::default()
        .application_name(app_name)
        .engine_name(app_name)
        .api_version(api_version);
    let create_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .flags(flags)
        .enabled_extension_names(&extension_names);

    let instance = entry
        .create_instance(&create_info, None)
        .map_err(|e| format!("could not create instance: {e}"))?;

    let result = collect_devices(&instance, api_version);
    instance.destroy_instance(None);
    result
}

unsafe fn collect_devices(
    instance: &ash::Instance,
    instance_api_version: u32,
) -> Result<Vec<VulkanDevice>, String> {
    let physical = instance
        .enumerate_physical_devices()
        .map_err(|e| format!("could not enumerate physical devices: {e}"))?;

    let properties2_available = instance_api_version >= vk::API_VERSION_1_1;

    Ok(physical
        .into_iter()
        .map(|device| {
            let device_extensions = instance
                .enumerate_device_extension_properties(device)
                .unwrap_or_default();
            let has_device_ext = |name: &CStr| {
                device_extensions
                    .iter()
                    .any(|e| e.extension_name_as_c_str().is_ok_and(|n| n == name))
            };

            let mut driver_props = vk::PhysicalDeviceDriverProperties::default();
            let mut id_props = vk::PhysicalDeviceIDProperties::default();
            let mut pci_props = vk::PhysicalDevicePCIBusInfoPropertiesEXT::default();

            // Only chain structures the device actually understands; the spec
            // forbids unsupported entries in a pNext chain.
            let want_driver = has_device_ext(vk::KHR_DRIVER_PROPERTIES_NAME);
            let want_pci = has_device_ext(vk::EXT_PCI_BUS_INFO_NAME);
            let want_id = properties2_available;

            let props = if properties2_available {
                let mut props2 = vk::PhysicalDeviceProperties2::default();
                if want_driver {
                    props2 = props2.push_next(&mut driver_props);
                }
                if want_id {
                    props2 = props2.push_next(&mut id_props);
                }
                if want_pci {
                    props2 = props2.push_next(&mut pci_props);
                }
                instance.get_physical_device_properties2(device, &mut props2);
                props2.properties
            } else {
                instance.get_physical_device_properties(device)
            };

            let memory = instance.get_physical_device_memory_properties(device);
            let device_local: u64 = memory.memory_heaps[..memory.memory_heap_count as usize]
                .iter()
                .filter(|h| h.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
                .map(|h| h.size)
                .sum();

            VulkanDevice {
                name: cstr_array(&props.device_name),
                vendor_id: props.vendor_id,
                device_id: props.device_id,
                kind: match props.device_type {
                    vk::PhysicalDeviceType::DISCRETE_GPU => GpuKind::Discrete,
                    vk::PhysicalDeviceType::INTEGRATED_GPU => GpuKind::Integrated,
                    vk::PhysicalDeviceType::VIRTUAL_GPU => GpuKind::Virtual,
                    vk::PhysicalDeviceType::CPU => GpuKind::Cpu,
                    _ => GpuKind::Unknown,
                },
                api_version: format_version(props.api_version),
                driver_version: decode_driver_version(props.vendor_id, props.driver_version),
                driver_name: want_driver
                    .then(|| cstr_array(&driver_props.driver_name))
                    .and_then(super::clean),
                driver_info: want_driver
                    .then(|| cstr_array(&driver_props.driver_info))
                    .and_then(super::clean),
                device_local_mb: (device_local > 0).then(|| super::to_mb(device_local)),
                pci_bus: want_pci.then(|| {
                    format!(
                        "{:04x}:{:02x}:{:02x}.{}",
                        pci_props.pci_domain,
                        pci_props.pci_bus,
                        pci_props.pci_device,
                        pci_props.pci_function
                    )
                }),
                uuid: want_id.then(|| format_uuid(&id_props.device_uuid)),
            }
        })
        .collect())
}

fn cstr_array(buf: &[c_char]) -> String {
    // SAFETY: Vulkan guarantees these arrays are NUL-terminated.
    let bytes = unsafe { CStr::from_ptr(buf.as_ptr()) };
    bytes.to_string_lossy().trim().to_string()
}

fn format_version(v: u32) -> String {
    format!(
        "{}.{}.{}",
        vk::api_version_major(v),
        vk::api_version_minor(v),
        vk::api_version_patch(v)
    )
}

/// `VkPhysicalDeviceProperties::driverVersion` is vendor-defined. Two vendors
/// pack it differently from `VK_MAKE_VERSION`, and decoding them wrongly
/// produces numbers that look nothing like the driver the user installed.
fn decode_driver_version(vendor_id: u32, raw: u32) -> Option<String> {
    if raw == 0 {
        return None;
    }
    Some(match vendor_id {
        // NVIDIA: 10 | 8 | 8 | 6 bits.
        0x10DE => format!(
            "{}.{}.{}.{}",
            (raw >> 22) & 0x3FF,
            (raw >> 14) & 0x0FF,
            (raw >> 6) & 0x0FF,
            raw & 0x03F
        ),
        // Intel on Windows: 14 | 18 bits. Elsewhere Intel uses the standard packing.
        0x8086 if cfg!(windows) => format!("{}.{}", raw >> 14, raw & 0x3FFF),
        _ => format_version(raw),
    })
}

fn format_uuid(bytes: &[u8; 16]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}
