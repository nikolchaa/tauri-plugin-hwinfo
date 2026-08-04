//! macOS backend.
//!
//! `sysctl -a` covers the CPU exhaustively for the cost of one process.
//! Everything else comes from `system_profiler -json`, the only supported way
//! to reach the hardware inventory without private frameworks. Each data type
//! is requested separately - a full report takes seconds - and memoised on the
//! scan context, because several sections read the same one.
//!
//! Apple silicon genuinely reports less than an Intel Mac did: there are no
//! DIMM slots to enumerate and no discrete GPU device. Fields are `null` there
//! because the hardware has no such thing, not because a probe failed.

use std::collections::HashMap;

use serde_json::Value;

use super::util::{edid_vendor_code, pnp_vendor, run};
use super::{CpuNative, DisplayNative, MemoryNative, NetNative, OsNative};
use crate::models::*;
use crate::scan::gpu::blank_gpu;
use crate::scan::{clean, clean_opt, Ctx};

// ---------------------------------------------------------------------------
// Shared plumbing
// ---------------------------------------------------------------------------

/// One `system_profiler` data type, parsed. The payload is always
/// `{"SPFooDataType": [ ... ]}`.
fn profiler(ctx: &mut Ctx, data_type: &str, probe: &str) -> Vec<Value> {
    let output = ctx.cached(data_type, || {
        run(
            "system_profiler",
            &["-json", "-detailLevel", "mini", data_type],
        )
    });

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            ctx.warn(format!("{probe}: {e}"));
            return Vec::new();
        }
    };

    match serde_json::from_str::<Value>(&output) {
        Ok(parsed) => parsed
            .get(data_type)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        Err(e) => {
            ctx.warn(format!(
                "{probe}: could not parse system_profiler output ({e})"
            ));
            Vec::new()
        }
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    clean_opt(value.get(key).and_then(Value::as_str))
}

/// A hex or decimal identifier as `system_profiler` writes it: `"0x1002"` or
/// `"1552"`.
fn id_value(value: &Value, key: &str) -> Option<u32> {
    let raw = text(value, key)?;
    match raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => raw.parse().ok(),
    }
}

/// All `sysctl` keys. Memoised, since both the CPU and OS collectors want them.
fn sysctl(ctx: &mut Ctx) -> HashMap<String, String> {
    let output = match ctx.cached("sysctl", || run("sysctl", &["-a"])) {
        Ok(o) => o,
        Err(e) => {
            ctx.warn(format!("sysctl: {e}"));
            return HashMap::new();
        }
    };

    output
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

/// `"8 GB"`, `"1536 MB"` → mebibytes.
fn parse_size_mb(raw: &str) -> Option<u64> {
    let mut parts = raw.split_whitespace();
    let value: u64 = parts.next()?.parse().ok()?;
    Some(match parts.next()?.to_ascii_uppercase().as_str() {
        "KB" => value / 1024,
        "MB" => value,
        "GB" => value * 1024,
        "TB" => value * 1024 * 1024,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// CPU
// ---------------------------------------------------------------------------

pub fn cpu(ctx: &mut Ctx) -> Vec<CpuNative> {
    let keys = sysctl(ctx);
    let get = |key: &str| keys.get(key).and_then(clean);
    let num = |key: &str| keys.get(key).and_then(|v| v.trim().parse::<u64>().ok());
    // Intel Macs report clocks in Hz; Apple silicon omits them entirely,
    // because its cores run at published fixed frequencies the kernel does not
    // expose.
    let mhz = |key: &str| num(key).map(|hz| (hz / 1_000_000) as u32);

    let model = get("machdep.cpu.brand_string");
    if model.is_none() {
        ctx.warn("cpu: sysctl reported no processor brand string");
    }

    vec![CpuNative {
        // Apple silicon has no CPUID vendor string to report.
        manufacturer: get("machdep.cpu.vendor").or_else(|| {
            model
                .as_deref()
                .filter(|m| m.starts_with("Apple"))
                .map(|_| "Apple Inc.".to_string())
        }),
        model,
        socket: None,
        physical_cores: num("hw.physicalcpu").map(|v| v as u32),
        threads: num("hw.logicalcpu").map(|v| v as u32),
        base_frequency: mhz("hw.cpufrequency"),
        max_frequency: mhz("hw.cpufrequency_max"),
        current_frequency: mhz("hw.cpufrequency"),
        l1d_kb: num("hw.l1dcachesize").map(|b| (b / 1024) as u32),
        l1i_kb: num("hw.l1icachesize").map(|b| (b / 1024) as u32),
        l2_kb: num("hw.l2cachesize").map(|b| (b / 1024) as u32),
        l3_kb: num("hw.l3cachesize").map(|b| (b / 1024) as u32),
        // VMX appears as a feature flag on Intel Macs. Apple silicon
        // virtualises through the Hypervisor framework and advertises nothing
        // here, so absence is not a negative answer.
        virtualization: keys.get("machdep.cpu.features").map(|features| {
            features
                .split_whitespace()
                .any(|f| f.eq_ignore_ascii_case("VMX"))
        }),
        microcode: get("machdep.cpu.microcode_version"),
        temperature_c: None,
        serial: None,
    }]
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

pub fn gpus(ctx: &mut Ctx) -> Vec<Gpu> {
    profiler(ctx, "SPDisplaysDataType", "gpu")
        .iter()
        .map(|entry| {
            let model = text(entry, "sppci_model").unwrap_or_else(|| "Unknown".into());
            let vendor_id = id_value(entry, "spdisplays_vendor-id");

            let manufacturer = vendor_id
                .and_then(super::pci_vendor_name)
                .map(str::to_string)
                .or_else(|| {
                    // Otherwise it arrives as `sppci_vendor_Apple`.
                    text(entry, "spdisplays_vendor")?
                        .strip_prefix("sppci_vendor_")
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "Apple Inc.".into());

            let mut gpu = blank_gpu(manufacturer, model);
            gpu.vendor_id = vendor_id;
            gpu.vendor_id_hex = vendor_id.map(|v| format!("0x{v:04X}"));
            gpu.device_id = id_value(entry, "spdisplays_device-id");
            gpu.device_id_hex = gpu.device_id.map(|v| format!("0x{v:04X}"));
            gpu.revision = id_value(entry, "spdisplays_revision-id");
            gpu.pci_bus = text(entry, "sppci_bus");
            // A dedicated `spdisplays_vram` figure means a discrete card;
            // integrated and Apple silicon GPUs report a shared budget.
            let dedicated = text(entry, "spdisplays_vram")
                .as_deref()
                .and_then(parse_size_mb);
            gpu.kind = if dedicated.is_some() {
                GpuKind::Discrete
            } else {
                GpuKind::Integrated
            };
            gpu.vram_mb = dedicated;
            gpu.shared_memory_mb = text(entry, "spdisplays_vram_shared")
                .as_deref()
                .and_then(parse_size_mb);
            // Every Mac that can run a Tauri app supports both.
            gpu.api.metal = true;
            gpu.api.opencl = true;

            gpu
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

pub fn memory(ctx: &mut Ctx) -> MemoryNative {
    // Every `system_profiler` data type is a slow process spawn.
    if !ctx.wants(DetailLevel::Full) {
        return MemoryNative::default();
    }

    let entries = profiler(ctx, "SPMemoryDataType", "memory modules");

    let mut modules = Vec::new();
    for item in entries
        .iter()
        .filter_map(|e| e.get("_items").and_then(Value::as_array))
        .flatten()
    {
        let Some(capacity_mb) = text(item, "dimm_size").as_deref().and_then(parse_size_mb) else {
            continue;
        };
        modules.push(MemoryModule {
            slot: text(item, "_name"),
            bank: None,
            manufacturer: text(item, "dimm_manufacturer"),
            part_number: text(item, "dimm_part_number"),
            capacity_mb: Some(capacity_mb),
            speed_mts: text(item, "dimm_speed")
                .as_deref()
                .and_then(|v| v.split_whitespace().next())
                .and_then(|v| v.parse().ok()),
            configured_speed_mts: None,
            memory_type: text(item, "dimm_type"),
            form_factor: None,
            voltage_mv: None,
            rank: None,
            data_width_bits: None,
            total_width_bits: None,
            serial: text(item, "dimm_serial_number"),
        });
    }

    if modules.is_empty() && !entries.is_empty() {
        ctx.warn(
            "memory modules: this Mac has memory packaged on the SoC and reports no per-DIMM \
             detail",
        );
    }

    MemoryNative {
        slots_used: (!modules.is_empty()).then_some(modules.len() as u32),
        slots_total: None,
        modules,
    }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

pub fn disks(ctx: &mut Ctx) -> Vec<Disk> {
    if !ctx.wants(DetailLevel::Full) {
        return Vec::new();
    }

    // NVMe and SATA drives live under separate data types.
    let mut controllers = profiler(ctx, "SPNVMeDataType", "disks");
    controllers.extend(profiler(ctx, "SPSerialATADataType", "disks"));

    controllers
        .iter()
        .flat_map(|controller| {
            let bus = text(controller, "_name");
            controller
                .get("_items")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(move |item| (bus.clone(), item))
        })
        .map(|(controller_name, item)| {
            let is_nvme = controller_name
                .as_deref()
                .is_some_and(|b| b.to_ascii_uppercase().contains("NVME"));
            let medium = text(&item, "spnvme_medium_type")
                .or_else(|| text(&item, "spsata_medium_type"));

            Disk {
                device: text(&item, "bsd_name")
                    .map(|name| format!("/dev/{name}"))
                    .or_else(|| text(&item, "_name"))
                    .unwrap_or_else(|| "unknown".into()),
                model: text(&item, "device_model").or_else(|| text(&item, "_name")),
                vendor: text(&item, "device_manufacturer"),
                kind: match medium.as_deref() {
                    Some(m) if m.to_ascii_lowercase().contains("rotational") => DiskKind::Hdd,
                    Some(_) => DiskKind::Ssd,
                    // Nothing that speaks NVMe spins.
                    None if is_nvme => DiskKind::Ssd,
                    None => DiskKind::Unknown,
                },
                bus: if is_nvme {
                    Some("NVMe".into())
                } else {
                    controller_name.map(|_| "SATA".into())
                },
                size_mb: text(&item, "size").as_deref().and_then(parse_size_mb),
                firmware_revision: text(&item, "device_revision"),
                is_removable: text(&item, "removable_media")
                    .map(|v| v.eq_ignore_ascii_case("yes")),
                partition_table: text(&item, "spnvme_partition_map_type")
                    .or_else(|| text(&item, "spsata_partition_map_type")),
                partition_count: item
                    .get("volumes")
                    .and_then(Value::as_array)
                    .map(|v| v.len() as u32),
                serial: text(&item, "device_serial"),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

pub fn network(ctx: &mut Ctx) -> HashMap<String, NetNative> {
    if !ctx.wants(DetailLevel::Full) {
        return HashMap::new();
    }

    profiler(ctx, "SPNetworkDataType", "network")
        .iter()
        .filter_map(|entry| {
            // `sysinfo` keys interfaces by BSD name (`en0`); `system_profiler`
            // leads with the service name ("Wi-Fi"), which makes the better
            // description.
            let key = text(entry, "interface")?;
            Some((
                key,
                NetNative {
                    description: text(entry, "_name"),
                    speed_mbps: None,
                },
            ))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Displays
// ---------------------------------------------------------------------------

pub fn displays(ctx: &mut Ctx) -> Vec<DisplayNative> {
    profiler(ctx, "SPDisplaysDataType", "display")
        .iter()
        .flat_map(|gpu| {
            gpu.get("spdisplays_ndrvs")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .map(|screen| {
            let (width, height, refresh_rate_hz) = text(&screen, "_spdisplays_resolution")
                .or_else(|| text(&screen, "spdisplays_resolution"))
                .as_deref()
                .map(parse_resolution)
                .unwrap_or_default();

            let (native_width, native_height, _) = text(&screen, "_spdisplays_pixels")
                .as_deref()
                .map(parse_resolution)
                .unwrap_or_default();

            DisplayNative {
                name: text(&screen, "_name"),
                manufacturer: id_value(&screen, "_spdisplays_display-vendor-id")
                    .and_then(|id| edid_vendor_code(id as u16))
                    .map(|code| pnp_vendor(&code).map(str::to_string).unwrap_or(code)),
                model: text(&screen, "_spdisplays_display-product-name")
                    .or_else(|| text(&screen, "_name")),
                width,
                height,
                refresh_rate_hz,
                native_width,
                native_height,
                position_x: None,
                position_y: None,
                is_primary: text(&screen, "spdisplays_main").map(|v| v.contains("yes")),
                is_internal: Some(
                    text(&screen, "spdisplays_connection_type")
                        .is_some_and(|c| c.contains("internal")),
                ),
                bits_per_pixel: text(&screen, "spdisplays_depth").and_then(|d| color_depth(&d)),
                // The physical dimensions are not in `system_profiler` output;
                // only the diagonal, and only for built-in panels.
                physical_width_mm: None,
                physical_height_mm: None,
                manufacture_year: None,
                serial: text(&screen, "_spdisplays_display-serial-number"),
            }
        })
        .collect()
}

/// `"3024 x 1964 @ 120.00Hz"` → `(3024, 1964, 120.0)`.
fn parse_resolution(raw: &str) -> (Option<u32>, Option<u32>, Option<f64>) {
    let (dimensions, refresh) = match raw.split_once('@') {
        Some((d, r)) => (d, Some(r)),
        None => (raw, None),
    };

    let mut parts = dimensions.split('x');
    let width = parts.next().and_then(|v| v.trim().parse().ok());
    let height = parts.next().and_then(|v| v.trim().parse().ok());
    let hz = refresh.and_then(|r| {
        r.trim()
            .trim_end_matches(['H', 'h', 'Z', 'z'])
            .trim()
            .parse()
            .ok()
    });

    (width, height, hz)
}

/// macOS spells the depth out: `"CGSThirtyTwoBitColor"`.
fn color_depth(raw: &str) -> Option<u32> {
    Some(match () {
        _ if raw.contains("ThirtyTwo") => 32,
        _ if raw.contains("TwentyFour") => 24,
        _ if raw.contains("Sixteen") => 16,
        _ if raw.contains("Eight") => 8,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Board / firmware
// ---------------------------------------------------------------------------

pub fn board(ctx: &mut Ctx) -> Board {
    let hardware = profiler(ctx, "SPHardwareDataType", "board");
    let entry = hardware.first().cloned().unwrap_or(Value::Null);

    Board {
        // A Mac has no logic-board identity separate from the machine itself.
        manufacturer: Some("Apple Inc.".into()),
        product: text(&entry, "machine_model").or_else(|| text(&entry, "machine_name")),
        version: None,
        serial: text(&entry, "serial_number"),
        asset_tag: None,
        bios: Bios {
            vendor: Some("Apple Inc.".into()),
            version: text(&entry, "boot_rom_version"),
            release_date: None,
            // Intel Macs booted an EFI-derived ROM; Apple silicon uses iBoot,
            // which is likewise not a legacy BIOS.
            mode: Some("UEFI".into()),
            secure_boot_enabled: None,
            smbios_version: None,
        },
        chassis: Chassis {
            manufacturer: Some("Apple Inc.".into()),
            kind: text(&entry, "machine_name").map(|name| {
                if name.to_ascii_lowercase().contains("book") {
                    "Notebook".into()
                } else {
                    "Desktop".into()
                }
            }),
            version: None,
            serial: None,
        },
        system: SystemIdentity {
            manufacturer: Some("Apple Inc.".into()),
            product: text(&entry, "machine_model"),
            version: None,
            family: text(&entry, "machine_name"),
            sku: None,
            uuid: text(&entry, "platform_UUID"),
            serial: text(&entry, "serial_number"),
        },
    }
}

// ---------------------------------------------------------------------------
// OS
// ---------------------------------------------------------------------------

pub fn os(ctx: &mut Ctx) -> OsNative {
    let virtualization = sysctl(ctx)
        .get("kern.hv_vmm_present")
        .filter(|v| v.trim() == "1")
        // The sysctl says a hypervisor is present but never which one.
        .map(|_| "hypervisor".to_string());

    let machine_id = profiler(ctx, "SPHardwareDataType", "os")
        .first()
        .and_then(|entry| text(entry, "platform_UUID"));

    OsNative {
        build: ctx
            .cached("sw_vers", || run("sw_vers", &["-buildVersion"]))
            .ok()
            .and_then(clean),
        edition: None,
        codename: None,
        virtualization,
        // The hardware UUID is the closest macOS equivalent of a machine ID.
        machine_id,
        user: std::env::var("USER").ok().and_then(clean),
    }
}
