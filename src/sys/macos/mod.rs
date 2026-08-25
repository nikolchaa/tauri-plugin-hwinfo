//! macOS backend.
//!
//! The CPU comes from `sysctlbyname`, displays from Core Graphics, and the
//! machine's identity from `hw.model` - all direct calls, so the sections that
//! run at every detail level spawn no processes at all.
//!
//! `system_profiler -json` remains for the itemised inventory that has no
//! public API short of private frameworks: memory modules, physical disks and
//! the GPU list. Each data type is requested separately - a full report takes
//! seconds - and memoised on the scan context, because several sections read
//! the same one.
//!
//! Apple silicon genuinely reports less than an Intel Mac did: there are no
//! DIMM slots to enumerate and no discrete GPU device. Fields are `null` there
//! because the hardware has no such thing, not because a probe failed.

use std::collections::HashMap;

use serde_json::Value;

use super::util::run;
use super::{CpuNative, DisplayNative, MemoryNative, NetNative, OsNative};
use crate::models::*;
use crate::scan::gpu::blank_gpu;
use crate::scan::{clean, clean_opt, to_mb, Ctx};

mod display;
mod sysctl;

// ---------------------------------------------------------------------------
// Shared plumbing
// ---------------------------------------------------------------------------

/// One `system_profiler` data type, parsed. The payload is always
/// `{"SPFooDataType": [ ... ]}`.
fn profiler(ctx: &mut Ctx, data_type: &str, probe: &str) -> Vec<Value> {
    profiler_at(ctx, data_type, probe, "mini")
}

/// Same, at an explicit detail level.
///
/// The itemised inventory runs at [`DetailLevel::Full`] anyway, and the
/// `mini` level strips per-device fields from several reports — the NVMe and
/// disc reports lose exactly the identifiers the drive parser matches on —
/// so those callers ask for `full`.
fn profiler_at(ctx: &mut Ctx, data_type: &str, probe: &str, level: &str) -> Vec<Value> {
    let output = ctx.cached(data_type, || {
        run(
            "system_profiler",
            &["-json", "-detailLevel", level, data_type],
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
    // Intel Macs report clocks in Hz; Apple silicon omits them entirely,
    // because its cores run at published fixed frequencies the kernel does not
    // expose.
    let mhz = |key: &str| sysctl::integer(key).map(|hz| (hz / 1_000_000) as u32);
    let kb = |key: &str| sysctl::integer(key).map(|bytes| (bytes / 1024) as u32);

    let model = sysctl::string("machdep.cpu.brand_string");
    if model.is_none() {
        ctx.warn("cpu: sysctl reported no processor brand string");
    }

    vec![CpuNative {
        // Apple silicon has no CPUID vendor string to report.
        manufacturer: sysctl::string("machdep.cpu.vendor").or_else(|| {
            model
                .as_deref()
                .filter(|m| m.starts_with("Apple"))
                .map(|_| "Apple Inc.".to_string())
        }),
        model,
        socket: None,
        physical_cores: sysctl::integer("hw.physicalcpu").map(|v| v as u32),
        threads: sysctl::integer("hw.logicalcpu").map(|v| v as u32),
        base_frequency: mhz("hw.cpufrequency"),
        max_frequency: mhz("hw.cpufrequency_max"),
        current_frequency: mhz("hw.cpufrequency"),
        l1d_kb: kb("hw.l1dcachesize"),
        l1i_kb: kb("hw.l1icachesize"),
        l2_kb: kb("hw.l2cachesize"),
        l3_kb: kb("hw.l3cachesize"),
        // Intel Macs list VMX among the CPU features. Apple silicon
        // virtualises through the Hypervisor framework and advertises nothing
        // here, so absence is not a negative answer — hence the `Option`
        // rather than a bare false.
        virtualization: sysctl::string("machdep.cpu.features").map(|features| {
            features
                .split_whitespace()
                .any(|f| f.eq_ignore_ascii_case("VMX"))
        }),
        microcode: sysctl::string("machdep.cpu.microcode_version"),
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

    // NVMe and SATA drives live under separate data types. The `full` detail
    // level matters here: `mini` strips the device identifiers these reports
    // exist to provide.
    let nvme = profiler_at(ctx, "SPNVMeDataType", "disks", "full");
    let sata = profiler_at(ctx, "SPSerialATADataType", "disks", "full");

    // The NVMe report lists drives flat at the top level; the SATA report
    // nests them beneath their controllers. Accepting both shapes everywhere
    // costs nothing and survives Apple reshuffling the nesting again.
    let mut disks: Vec<Disk> = sp_entries(&nvme, Some("NVMe"), true);
    disks.extend(sp_entries(&sata, Some("SATA"), false));

    // Virtualised Macs present their disks through neither NVMe nor SATA, and
    // USB card readers never appear there either; the generic disc report is
    // the catch-all both fall back to.
    if disks.is_empty() {
        let disc = profiler_at(ctx, "SPDiscDataType", "disks", "full");
        disks = sp_entries(&disc, None, false);
    }

    if disks.is_empty() {
        ctx.warn(
            "disks: no drives could be parsed from system_profiler; the report layout may have \
             changed",
        );
    }

    disks
}

/// Collect drives from one `system_profiler` report.
///
/// Entries either carry their drives in an `_items` array (the SATA shape) or
/// are themselves the drive (the NVMe shape). `bus` labels every drive found;
/// `is_nvme` only settles SSD-vs-HDD for entries that spell no medium type.
fn sp_entries(entries: &[Value], bus: Option<&str>, is_nvme: bool) -> Vec<Disk> {
    const DRIVE_KEYS: [&str; 4] = ["bsd_name", "device_model", "capacity_in_bytes", "size"];

    entries
        .iter()
        .flat_map(|entry| {
            let bus = bus.map(str::to_string);
            match entry.get("_items").and_then(Value::as_array) {
                Some(items) => items
                    .iter()
                    .map(move |item| sp_drive(bus.clone(), is_nvme, item))
                    .collect::<Vec<Disk>>(),
                // A flat entry is a drive when it carries device keys at all;
                // anything else is a header we would mislabel.
                None => {
                    if DRIVE_KEYS
                        .iter()
                        .any(|key| entry.get(key).is_some_and(|v| !v.is_null()))
                    {
                        vec![sp_drive(bus, is_nvme, entry)]
                    } else {
                        Vec::new()
                    }
                }
            }
        })
        .collect()
}

/// One drive out of a `system_profiler` `_items` array.
///
/// The NVMe, SATA and generic-disc reports share most key names; where they
/// differ, the missing fields simply stay `null`.
fn sp_drive(bus: Option<String>, is_nvme: bool, item: &Value) -> Disk {
    let medium = text(item, "spnvme_medium_type").or_else(|| text(item, "spsata_medium_type"));

    Disk {
        device: text(item, "bsd_name")
            .map(|name| format!("/dev/{name}"))
            .or_else(|| text(item, "_name"))
            .unwrap_or_else(|| "unknown".into()),
        model: text(item, "device_model").or_else(|| text(item, "_name")),
        vendor: text(item, "device_manufacturer"),
        kind: match medium.as_deref() {
            Some(m) if m.to_ascii_lowercase().contains("rotational") => DiskKind::Hdd,
            Some(_) => DiskKind::Ssd,
            // Nothing that speaks NVMe spins.
            None if is_nvme => DiskKind::Ssd,
            None => DiskKind::Unknown,
        },
        bus,
        // The reports either spell a human-readable size ("500 GB") or raw
        // bytes; builds have drifted between the two spellings, and the
        // byte fields arrive sometimes as numbers and sometimes as strings.
        size_mb: ["size", "capacity"]
            .iter()
            .find_map(|key| text(item, key))
            .as_deref()
            .and_then(parse_size_mb)
            .or_else(|| {
                ["size_in_bytes", "capacity_in_bytes"]
                    .iter()
                    .find_map(|key| {
                        item.get(key).and_then(|value| match value {
                            Value::Number(n) => n.as_u64(),
                            Value::String(s) => s.trim().parse().ok(),
                            _ => None,
                        })
                    })
                    .map(to_mb)
            }),
        firmware_revision: text(item, "device_revision"),
        is_removable: text(item, "removable_media").map(|v| v.eq_ignore_ascii_case("yes")),
        partition_table: text(item, "spnvme_partition_map_type")
            .or_else(|| text(item, "spsata_partition_map_type")),
        partition_count: item
            .get("volumes")
            .and_then(Value::as_array)
            .map(|v| v.len() as u32),
        serial: text(item, "device_serial"),
    }
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
    display::displays(ctx)
}

// (resolution and colour depth now come typed from Core Graphics)

// ---------------------------------------------------------------------------
// Board / firmware
// ---------------------------------------------------------------------------

pub fn board(ctx: &mut Ctx) -> Board {
    // `hw.model` gives the model identifier ("MacBookPro18,3") for free.
    // Everything else in the hardware report is either an identifier or
    // firmware detail, so the spawn is only worth it when one of those was
    // actually requested.
    let model = sysctl::string("hw.model");
    let wants_report = ctx.mode.is_unsafe() || ctx.wants(DetailLevel::Full);
    let entry = if wants_report {
        profiler(ctx, "SPHardwareDataType", "board")
            .first()
            .cloned()
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };

    Board {
        // A Mac has no logic-board identity separate from the machine itself.
        manufacturer: Some("Apple Inc.".into()),
        product: model
            .clone()
            .or_else(|| text(&entry, "machine_model"))
            .or_else(|| text(&entry, "machine_name")),
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
            // The model identifier names the form factor directly:
            // "MacBookPro18,3" against "Macmini9,1".
            kind: model
                .as_deref()
                .or_else(|| entry.get("machine_name")?.as_str())
                .map(|name| {
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
            product: model.clone().or_else(|| text(&entry, "machine_model")),
            version: None,
            family: text(&entry, "machine_name"),
            sku: None,
            uuid: text(&entry, "platform_UUID"),
            serial: text(&entry, "serial_number"),
        },
    }
}

/// ROCm has never supported macOS, so this is a definite negative rather than
/// an unread probe.
pub fn hip(_ctx: &mut Ctx) -> crate::scan::compute::Hip {
    crate::scan::compute::Hip::default()
}

// ---------------------------------------------------------------------------
// OS
// ---------------------------------------------------------------------------

pub fn os(ctx: &mut Ctx) -> OsNative {
    // The hardware UUID is the closest macOS equivalent of a machine ID, and
    // it is only reachable through `system_profiler`. Since it is an
    // identifier, the spawn is skipped entirely unless the caller asked for
    // identifiers — which keeps a safe-mode scan free of processes.
    let machine_id = ctx.mode.is_unsafe().then(|| {
        profiler(ctx, "SPHardwareDataType", "os")
            .first()
            .and_then(|entry| text(entry, "platform_UUID"))
    });

    OsNative {
        // `kern.osversion` is the build identifier `sw_vers -buildVersion`
        // prints, without the process.
        build: sysctl::string("kern.osversion"),
        edition: None,
        codename: None,
        // The sysctl reports that a hypervisor is present but never which one.
        virtualization: sysctl::flag("kern.hv_vmm_present")
            .unwrap_or(false)
            .then(|| "hypervisor".to_string()),
        machine_id: machine_id.flatten(),
        user: std::env::var("USER").ok().and_then(clean),
    }
}
