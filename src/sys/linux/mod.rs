//! Linux backend.
//!
//! Everything here comes from `/proc`, `/sys` and `/etc`: no helper binaries
//! are required, and the few optional ones (`lspci`, `dmidecode`,
//! `systemd-detect-virt`) only ever add detail.
//!
//! Several SMBIOS files are mode `0400`. Rather than reporting zeros for the
//! memory modules and serial numbers behind them, the collectors say so in the
//! scan warnings.

use std::collections::HashMap;
use std::path::Path;

use super::util::{read_trimmed, read_u64, run, value_after_colon};
use super::{CpuNative, DisplayNative, MemoryNative, NetNative, OsNative};
use crate::models::*;
use crate::scan::gpu::blank_gpu;
use crate::scan::{clean, to_mb, Ctx};

mod edid;

const DMI: &str = "/sys/class/dmi/id";
const CPU_ROOT: &str = "/sys/devices/system/cpu";

// ---------------------------------------------------------------------------
// CPU
// ---------------------------------------------------------------------------

pub fn cpu(ctx: &mut Ctx) -> Vec<CpuNative> {
    let Some(cpuinfo) = read_trimmed("/proc/cpuinfo") else {
        ctx.warn("cpu: /proc/cpuinfo is unreadable");
        return Vec::new();
    };

    // `/proc/cpuinfo` is one blank-line-separated block per logical processor.
    // Blocks sharing a `physical id` belong to the same package.
    let mut packages: Vec<Package> = Vec::new();
    let mut current: HashMap<String, String> = HashMap::new();

    for line in cpuinfo.lines() {
        if line.trim().is_empty() {
            flush_package(&mut current, &mut packages);
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            current.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    flush_package(&mut current, &mut packages);

    if packages.is_empty() {
        ctx.warn("cpu: /proc/cpuinfo contained no processor blocks");
        return Vec::new();
    }

    let cache = cache_sizes();
    // Another root-only `dmidecode` call, for one cosmetic string.
    let sockets = if ctx.wants(DetailLevel::Full) {
        dmidecode_sockets(ctx)
    } else {
        Vec::new()
    };

    packages
        .into_iter()
        .enumerate()
        .map(|(index, Package { block, .. })| {
            let flags = block.get("flags").map(String::as_str).unwrap_or_default();

            CpuNative {
                manufacturer: block.get("vendor_id").and_then(clean),
                model: block.get("model name").and_then(clean),
                socket: sockets.get(index).cloned(),
                physical_cores: block.get("cpu cores").and_then(|v| v.parse().ok()),
                threads: block.get("siblings").and_then(|v| v.parse().ok()),
                base_frequency: read_khz(&format!("{CPU_ROOT}/cpu0/cpufreq/base_frequency")),
                max_frequency: max_frequency(),
                current_frequency: block
                    .get("cpu MHz")
                    .and_then(|v| v.parse::<f64>().ok())
                    .map(|v| v.round() as u32),
                l1d_kb: cache.l1d_kb,
                l1i_kb: cache.l1i_kb,
                l2_kb: cache.l2_kb,
                l3_kb: cache.l3_kb,
                virtualization: Some(
                    flags.split_whitespace().any(|f| f == "vmx" || f == "svm"),
                ),
                microcode: block.get("microcode").and_then(clean),
                temperature_c: None,
                // Requires root, and x86 processor serial numbers have been
                // disabled in hardware since the Pentium III.
                serial: None,
            }
        })
        .collect()
}

/// One physical processor package, represented by the first `/proc/cpuinfo`
/// block that claimed its `physical id`.
struct Package {
    id: Option<String>,
    block: HashMap<String, String>,
}

/// Close off the block just parsed, keeping it only if it introduces a package
/// we have not seen. Processors with no `physical id` - Arm, many VMs - all
/// collapse into a single package, which is the right answer there.
fn flush_package(block: &mut HashMap<String, String>, packages: &mut Vec<Package>) {
    if block.is_empty() {
        return;
    }
    let id = block.get("physical id").cloned();
    if packages.iter().any(|p| p.id == id) {
        block.clear();
    } else {
        packages.push(Package {
            id,
            block: std::mem::take(block),
        });
    }
}

/// Highest `cpuinfo_max_freq` across all policies, in MHz.
fn max_frequency() -> Option<u32> {
    let entries = std::fs::read_dir(CPU_ROOT).ok()?;
    entries
        .flatten()
        .filter_map(|entry| {
            read_khz(&format!(
                "{}/cpufreq/cpuinfo_max_freq",
                entry.path().display()
            ))
        })
        .max()
}

fn read_khz(path: &str) -> Option<u32> {
    read_u64(path).map(|khz| (khz / 1000) as u32)
}

/// Sum the caches at each level from `/sys/devices/system/cpu/cpu0/cache`.
fn cache_sizes() -> CpuCache {
    let mut cache = CpuCache::default();
    let Ok(entries) = std::fs::read_dir(format!("{CPU_ROOT}/cpu0/cache")) else {
        return cache;
    };

    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.join("level").exists() {
            continue;
        }
        let level = read_trimmed(dir.join("level")).and_then(|v| v.parse::<u32>().ok());
        let kind = read_trimmed(dir.join("type")).unwrap_or_default();
        let Some(size_kb) = read_trimmed(dir.join("size")).and_then(|s| parse_size_kb(&s)) else {
            continue;
        };

        // cpu0's view is per-core for L1/L2 and shared for L3, which is exactly
        // what a caller wants to see for a single package.
        let slot = match (level, kind.as_str()) {
            (Some(1), "Data") => &mut cache.l1d_kb,
            (Some(1), "Instruction") => &mut cache.l1i_kb,
            (Some(2), _) => &mut cache.l2_kb,
            (Some(3), _) => &mut cache.l3_kb,
            _ => continue,
        };
        *slot = Some(slot.unwrap_or(0) + size_kb);
    }

    cache
}

/// `"32K"`, `"12288K"`, `"24M"` → kibibytes.
fn parse_size_kb(raw: &str) -> Option<u32> {
    let raw = raw.trim();
    let (digits, unit) = raw.split_at(raw.find(|c: char| !c.is_ascii_digit())?);
    let value: u32 = digits.parse().ok()?;
    Some(match unit.trim().to_ascii_uppercase().as_str() {
        "K" | "KB" | "KIB" => value,
        "M" | "MB" | "MIB" => value * 1024,
        "G" | "GB" | "GIB" => value * 1024 * 1024,
        _ => return None,
    })
}

/// Socket designations, in package order. Only available with root.
fn dmidecode_sockets(ctx: &mut Ctx) -> Vec<String> {
    match run("dmidecode", &["-t", "processor"]) {
        Ok(output) => output
            .lines()
            .filter(|l| l.trim_start().starts_with("Socket Designation:"))
            .filter_map(value_after_colon)
            .filter_map(clean)
            .collect(),
        Err(e) => {
            ctx.warn(format!("cpu: socket designation unavailable ({e})"));
            Vec::new()
        }
    }
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

pub fn gpus(ctx: &mut Ctx) -> Vec<Gpu> {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        ctx.warn("gpu: /sys/class/drm is unreadable");
        return Vec::new();
    };

    let names = lspci_names();
    let mut cards: Vec<_> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            // `cardN` is the device; `cardN-HDMI-A-1` is a connector on it.
            name.starts_with("card") && !name.contains('-')
        })
        .collect();
    cards.sort_by_key(|e| e.file_name());

    cards
        .into_iter()
        .filter_map(|entry| {
            let device = entry.path().join("device");
            let vendor_id = read_hex(device.join("vendor"))?;
            let device_id = read_hex(device.join("device"))?;

            let slot = uevent_value(&device.join("uevent"), "PCI_SLOT_NAME");
            let driver = uevent_value(&device.join("uevent"), "DRIVER");

            let manufacturer = super::pci_vendor_name(vendor_id)
                .map(str::to_string)
                .unwrap_or_else(|| format!("Vendor 0x{vendor_id:04X}"));

            // Without `lspci` there is no model string in sysfs at all; the
            // Vulkan merge usually supplies one, and the identifiers are always
            // present either way.
            let model = slot
                .as_deref()
                .and_then(|s| names.get(s).cloned())
                .unwrap_or_else(|| format!("Device 0x{device_id:04X}"));

            let mut gpu = blank_gpu(manufacturer, model);
            gpu.vendor_id = Some(vendor_id);
            gpu.vendor_id_hex = Some(format!("0x{vendor_id:04X}"));
            gpu.device_id = Some(device_id);
            gpu.device_id_hex = Some(format!("0x{device_id:04X}"));
            gpu.revision = read_hex(device.join("revision"));
            gpu.subsystem_id = read_hex(device.join("subsystem_device"))
                .zip(read_hex(device.join("subsystem_vendor")))
                .map(|(dev, ven)| format!("0x{dev:04X}{ven:04X}"));
            gpu.pci_bus = slot;
            // amdgpu and i915 publish the VRAM budget; the NVIDIA proprietary
            // driver does not, and `nvidia-smi` fills that gap later.
            gpu.vram_mb = read_u64(device.join("mem_info_vram_total"))
                .map(to_mb)
                .filter(|&mb| mb > 0);
            // The kernel module publishes its own version for out-of-tree
            // drivers; in-tree ones have none, and Vulkan supplies it instead.
            gpu.driver_version = driver
                .as_deref()
                .and_then(|name| read_trimmed(format!("/sys/module/{name}/version")))
                .and_then(clean);

            Some(gpu)
        })
        .collect()
}

/// PCI slot → device description, from `lspci` when it is installed.
fn lspci_names() -> HashMap<String, String> {
    let Ok(output) = run("lspci", &["-D", "-mm"]) else {
        return HashMap::new();
    };

    output
        .lines()
        .filter_map(|line| {
            let (slot, rest) = line.split_once(' ')?;
            // `0000:01:00.0 "VGA compatible controller" "NVIDIA" "AD107M" ...`
            let fields: Vec<&str> = rest.split('"').filter(|f| !f.trim().is_empty()).collect();
            let class = fields.first()?;
            if !class.contains("VGA")
                && !class.contains("3D controller")
                && !class.contains("Display controller")
            {
                return None;
            }
            let vendor = fields.get(1)?.trim();
            let device = fields.get(2)?.trim();
            Some((slot.to_string(), format!("{vendor} {device}")))
        })
        .collect()
}

/// Read a sysfs file holding `0x1002`.
fn read_hex(path: impl AsRef<Path>) -> Option<u32> {
    let raw = read_trimmed(path)?;
    u32::from_str_radix(raw.trim_start_matches("0x"), 16).ok()
}

fn uevent_value(path: &Path, key: &str) -> Option<String> {
    let contents = read_trimmed(path)?;
    contents
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .and_then(clean)
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

pub fn memory(ctx: &mut Ctx) -> MemoryNative {
    // `dmidecode` is a process spawn that needs root, so for most callers this
    // costs a failed exec and returns nothing. Not worth paying for below the
    // tier that asked for itemised inventory.
    if !ctx.wants(DetailLevel::Full) {
        return MemoryNative::default();
    }

    let output = match run("dmidecode", &["-t", "memory"]) {
        Ok(o) => o,
        Err(e) => {
            ctx.warn(format!(
                "memory modules: per-DIMM detail needs SMBIOS access, which is root-only on Linux \
                 ({e})"
            ));
            return MemoryNative::default();
        }
    };

    let mut modules = Vec::new();
    let mut slots_total = None;
    let mut current: Option<HashMap<String, String>> = None;

    for line in output.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("Memory Device") && !trimmed.contains("Mapped") {
            if let Some(block) = current.take() {
                push_module(&mut modules, block);
            }
            current = Some(HashMap::new());
            continue;
        }
        if trimmed.starts_with("Physical Memory Array") {
            if let Some(block) = current.take() {
                push_module(&mut modules, block);
            }
            continue;
        }
        if let Some(devices) = trimmed.strip_prefix("Number Of Devices:") {
            slots_total = devices.trim().parse().ok();
            continue;
        }
        if let (Some(block), Some((key, value))) = (current.as_mut(), trimmed.split_once(':')) {
            block.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    if let Some(block) = current.take() {
        push_module(&mut modules, block);
    }

    MemoryNative {
        slots_used: Some(modules.len() as u32),
        slots_total,
        modules,
    }
}

/// Turn one `dmidecode` "Memory Device" block into a module, skipping empty
/// slots - `dmidecode` lists those with `Size: No Module Installed`.
fn push_module(modules: &mut Vec<MemoryModule>, block: HashMap<String, String>) {
    let get = |key: &str| block.get(key).and_then(clean);
    let capacity_mb = block.get("Size").and_then(|s| parse_dmi_size_mb(s));
    if capacity_mb.is_none() {
        return;
    }

    modules.push(MemoryModule {
        slot: get("Locator"),
        bank: get("Bank Locator"),
        manufacturer: get("Manufacturer"),
        part_number: get("Part Number"),
        capacity_mb,
        speed_mts: get("Speed").as_deref().and_then(parse_leading_u32),
        configured_speed_mts: get("Configured Memory Speed")
            .as_deref()
            .and_then(parse_leading_u32),
        memory_type: get("Type"),
        form_factor: get("Form Factor"),
        voltage_mv: get("Configured Voltage")
            .as_deref()
            .and_then(parse_volts_mv),
        rank: get("Rank").as_deref().and_then(parse_leading_u32),
        data_width_bits: get("Data Width").as_deref().and_then(parse_leading_u32),
        total_width_bits: get("Total Width").as_deref().and_then(parse_leading_u32),
        serial: get("Serial Number"),
    });
}

/// `"32 GB"`, `"16384 MB"` → mebibytes.
fn parse_dmi_size_mb(raw: &str) -> Option<u64> {
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

/// `"4800 MT/s"` → `4800`.
fn parse_leading_u32(raw: &str) -> Option<u32> {
    raw.split_whitespace().next()?.parse().ok()
}

/// `"1.1 V"` → `1100`.
fn parse_volts_mv(raw: &str) -> Option<u32> {
    let volts: f64 = raw.split_whitespace().next()?.parse().ok()?;
    Some((volts * 1000.0).round() as u32)
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

pub fn disks(ctx: &mut Ctx) -> Vec<Disk> {
    if !ctx.wants(DetailLevel::Full) {
        return Vec::new();
    }

    let Ok(entries) = std::fs::read_dir("/sys/block") else {
        ctx.warn("disks: /sys/block is unreadable");
        return Vec::new();
    };

    let mut disks: Vec<Disk> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // Virtual and pseudo devices are not hardware.
            !["loop", "ram", "dm-", "md", "zram", "sr", "fd"]
                .iter()
                .any(|prefix| name.starts_with(prefix))
        })
        .map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let device = path.join("device");
            // `/sys/block/X` links into the device tree; the bus shows up in
            // the resolved path.
            let bus = std::fs::canonicalize(&path)
                .ok()
                .map(|p| p.display().to_string())
                .and_then(|p| bus_from_path(&p))
                .map(str::to_string);

            Disk {
                model: read_trimmed(device.join("model"))
                    .and_then(clean)
                    .or_else(|| read_trimmed(path.join("device/nvme/model")).and_then(clean)),
                vendor: read_trimmed(device.join("vendor")).and_then(clean),
                // `rotational` is the kernel's own SSD/HDD verdict.
                kind: match read_trimmed(path.join("queue/rotational")).as_deref() {
                    Some("0") => DiskKind::Ssd,
                    Some("1") => DiskKind::Hdd,
                    _ => DiskKind::Unknown,
                },
                bus,
                // `size` is always in 512-byte sectors regardless of the
                // drive's own logical block size.
                size_mb: read_u64(path.join("size")).map(|sectors| sectors * 512 / 1024 / 1024),
                firmware_revision: read_trimmed(device.join("firmware_rev"))
                    .or_else(|| read_trimmed(device.join("rev")))
                    .and_then(clean),
                is_removable: read_trimmed(path.join("removable")).map(|v| v == "1"),
                partition_table: None,
                partition_count: partition_count(&path, &name),
                serial: read_trimmed(device.join("serial")).and_then(clean),
                device: format!("/dev/{name}"),
            }
        })
        .collect();

    disks.sort_by(|a, b| a.device.cmp(&b.device));
    disks
}

fn bus_from_path(path: &str) -> Option<&'static str> {
    Some(match () {
        _ if path.contains("/nvme") => "NVMe",
        _ if path.contains("/usb") => "USB",
        _ if path.contains("/ata") => "SATA",
        _ if path.contains("/mmc") => "MMC",
        _ if path.contains("/virtio") => "VirtIO",
        _ if path.contains("/scsi") => "SCSI",
        _ => return None,
    })
}

/// Partitions appear as child directories prefixed with the disk's own name.
fn partition_count(path: &Path, name: &str) -> Option<u32> {
    let entries = std::fs::read_dir(path).ok()?;
    Some(
        entries
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(name))
            .count() as u32,
    )
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

pub fn network(ctx: &mut Ctx) -> HashMap<String, NetNative> {
    // Cheap here, but gated all the same so `description` and `speedMbps`
    // appear at the same tier on every platform. A contract that varies by OS
    // is worse than one that occasionally leaves free data on the table.
    if !ctx.wants(DetailLevel::Full) {
        return HashMap::new();
    }

    let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
        return HashMap::new();
    };

    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            (
                name,
                NetNative {
                    // The kernel module name is the closest thing sysfs has to
                    // a human-readable description.
                    description: uevent_value(&path.join("device/uevent"), "DRIVER"),
                    // `speed` is meaningless - and returns an error on read -
                    // for interfaces that are down or have no carrier.
                    speed_mbps: read_trimmed(path.join("speed"))
                        .and_then(|v| v.parse::<i64>().ok())
                        .filter(|&v| v > 0)
                        .map(|v| v as u64),
                },
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Displays
// ---------------------------------------------------------------------------

pub fn displays(ctx: &mut Ctx) -> Vec<DisplayNative> {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        ctx.warn("display: /sys/class/drm is unreadable");
        return Vec::new();
    };

    let mut connectors: Vec<_> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("card") && name.contains('-')
        })
        .collect();
    connectors.sort_by_key(|e| e.file_name());

    connectors
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if read_trimmed(path.join("status")).as_deref() != Some("connected") {
                return None;
            }

            let raw_name = entry.file_name().to_string_lossy().into_owned();
            // `card1-eDP-1` → `eDP-1`, which is how the compositor and Tauri
            // name the same output.
            let connector = raw_name
                .split_once('-')
                .map(|(_, rest)| rest.to_string())
                .unwrap_or(raw_name);

            let mut display = DisplayNative {
                is_internal: Some(
                    ["eDP", "LVDS", "DSI"]
                        .iter()
                        .any(|kind| connector.starts_with(kind)),
                ),
                name: Some(connector),
                ..Default::default()
            };

            // The first entry in `modes` is the preferred (native) mode.
            if let Some(modes) = read_trimmed(path.join("modes")) {
                if let Some((w, h)) = modes.lines().next().and_then(parse_mode) {
                    display.native_width = Some(w);
                    display.native_height = Some(h);
                }
            }

            if let Ok(bytes) = std::fs::read(path.join("edid")) {
                edid::apply(&bytes, &mut display);
            }

            Some(display)
        })
        .collect()
}

/// `"2560x1440"` → `(2560, 1440)`.
fn parse_mode(raw: &str) -> Option<(u32, u32)> {
    let (w, h) = raw.trim().split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

// ---------------------------------------------------------------------------
// Board / firmware
// ---------------------------------------------------------------------------

pub fn board(ctx: &mut Ctx) -> Board {
    let dmi = |name: &str| read_trimmed(format!("{DMI}/{name}")).and_then(clean);

    // Serial-bearing DMI files are mode 0400. Say so once rather than emitting
    // several identical warnings.
    if ctx.mode.is_unsafe() && read_trimmed(format!("{DMI}/product_serial")).is_none() {
        ctx.warn(
            "board: DMI serial numbers and the system UUID are root-only on Linux and were left \
             null",
        );
    }

    Board {
        manufacturer: dmi("board_vendor"),
        product: dmi("board_name"),
        version: dmi("board_version"),
        serial: dmi("board_serial"),
        asset_tag: dmi("board_asset_tag").or_else(|| dmi("chassis_asset_tag")),
        bios: Bios {
            vendor: dmi("bios_vendor"),
            version: dmi("bios_version"),
            release_date: dmi("bios_date").as_deref().and_then(parse_us_date),
            mode: Some(if Path::new("/sys/firmware/efi").exists() {
                "UEFI".into()
            } else {
                "Legacy".into()
            }),
            secure_boot_enabled: secure_boot(),
            // `/sys/class/dmi/id` exposes the BIOS release, not the SMBIOS
            // specification version; the latter needs the raw entry point,
            // which is root-only.
            smbios_version: None,
        },
        chassis: Chassis {
            manufacturer: dmi("chassis_vendor"),
            kind: dmi("chassis_type")
                .and_then(|v| v.parse::<u64>().ok())
                .and_then(chassis_type)
                .map(str::to_string),
            version: dmi("chassis_version"),
            serial: dmi("chassis_serial"),
        },
        system: SystemIdentity {
            manufacturer: dmi("sys_vendor"),
            product: dmi("product_name"),
            version: dmi("product_version"),
            family: dmi("product_family"),
            sku: dmi("product_sku"),
            uuid: dmi("product_uuid"),
            serial: dmi("product_serial"),
        },
    }
}

/// `"01/30/2024"` → `"2024-01-30"`.
fn parse_us_date(raw: &str) -> Option<String> {
    let mut parts = raw.split('/');
    let month = parts.next()?;
    let day = parts.next()?;
    let year = parts.next()?;
    (month.len() == 2 && day.len() == 2 && year.len() == 4)
        .then(|| format!("{year}-{month}-{day}"))
}

/// The EFI `SecureBoot` variable: a four-byte attribute header then one byte of
/// value.
fn secure_boot() -> Option<bool> {
    const VAR: &str =
        "/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c";
    let bytes = std::fs::read(VAR).ok()?;
    bytes.get(4).map(|&v| v == 1)
}

/// SMBIOS 7.4.1 chassis type codes, as exposed by `/sys/class/dmi/id`.
fn chassis_type(code: u64) -> Option<&'static str> {
    Some(match code {
        1 => "Other",
        3 => "Desktop",
        4 => "Low Profile Desktop",
        5 => "Pizza Box",
        6 => "Mini Tower",
        7 => "Tower",
        8 => "Portable",
        9 => "Laptop",
        10 => "Notebook",
        11 => "Hand Held",
        12 => "Docking Station",
        13 => "All in One",
        14 => "Sub Notebook",
        15 => "Space-saving",
        16 => "Lunch Box",
        17 => "Main System Chassis",
        18 => "Expansion Chassis",
        21 => "Peripheral Chassis",
        22 => "Storage Chassis",
        23 => "Rack Mount Chassis",
        24 => "Sealed-case PC",
        28 => "Blade",
        30 => "Tablet",
        31 => "Convertible",
        32 => "Detachable",
        35 => "Mini PC",
        36 => "Stick PC",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// OS
// ---------------------------------------------------------------------------

pub fn os(_ctx: &mut Ctx) -> OsNative {
    let release = os_release();

    OsNative {
        build: None,
        edition: release.get("VARIANT").cloned(),
        codename: release
            .get("VERSION_CODENAME")
            .or_else(|| release.get("UBUNTU_CODENAME"))
            .cloned(),
        virtualization: virtualization(),
        machine_id: read_trimmed("/etc/machine-id")
            .or_else(|| read_trimmed("/var/lib/dbus/machine-id")),
        user: std::env::var("USER").ok().and_then(clean),
    }
}

/// `/etc/os-release` as key/value pairs, with the shell quoting stripped.
fn os_release() -> HashMap<String, String> {
    let Some(contents) = read_trimmed("/etc/os-release")
        .or_else(|| read_trimmed("/usr/lib/os-release"))
    else {
        return HashMap::new();
    };

    contents
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            let value = value.trim().trim_matches('"').trim_matches('\'');
            Some((key.trim().to_string(), clean(value)?))
        })
        .collect()
}

fn virtualization() -> Option<String> {
    // `systemd-detect-virt` is the authority where it exists, and prints
    // "none" on bare metal.
    if let Ok(output) = run("systemd-detect-virt", &[]) {
        let detected = output.trim();
        if !detected.is_empty() && detected != "none" {
            return clean(detected);
        }
        return None;
    }

    // Otherwise fall back to the identity the hypervisor stamps into DMI.
    let haystack = [
        read_trimmed(format!("{DMI}/sys_vendor")),
        read_trimmed(format!("{DMI}/product_name")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase();

    Some(
        match () {
            _ if haystack.contains("vmware") => "VMware",
            _ if haystack.contains("virtualbox") || haystack.contains("innotek") => "VirtualBox",
            _ if haystack.contains("kvm") => "KVM",
            _ if haystack.contains("qemu") => "QEMU",
            _ if haystack.contains("xen") => "Xen",
            _ if haystack.contains("parallels") => "Parallels",
            _ if haystack.contains("microsoft corporation") => "Microsoft Hyper-V",
            _ => return None,
        }
        .to_string(),
    )
}
