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

use super::util::{read_trimmed, read_u64};
use super::{CpuNative, DisplayNative, MemoryNative, NetNative, OsNative};
use crate::models::*;
use crate::scan::gpu::blank_gpu;
use crate::scan::{clean, to_mb, Ctx};

mod dmi;
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
    // SMBIOS type 4 describes each package directly, rather than being
    // inferred from the per-thread blocks in /proc/cpuinfo. It is root-only,
    // so it enriches rather than replaces.
    let firmware = if ctx.wants(DetailLevel::Full) {
        firmware_processors(ctx)
    } else {
        Vec::new()
    };

    packages
        .into_iter()
        .enumerate()
        .map(|(index, Package { block, .. })| {
            let flags = block.get("flags").map(String::as_str).unwrap_or_default();
            let firmware = firmware.get(index);

            CpuNative {
                manufacturer: block.get("vendor_id").and_then(clean),
                model: block.get("model name").and_then(clean),
                socket: firmware.and_then(|p| p.socket.clone()),
                physical_cores: block
                    .get("cpu cores")
                    .and_then(|v| v.parse().ok())
                    .or_else(|| firmware.and_then(|p| p.core_count)),
                threads: block
                    .get("siblings")
                    .and_then(|v| v.parse().ok())
                    .or_else(|| firmware.and_then(|p| p.thread_count)),
                base_frequency: read_khz(&format!("{CPU_ROOT}/cpu0/cpufreq/base_frequency")),
                // cpufreq knows the governor's ceiling; firmware knows the
                // rated turbo, which is usually the higher and truer figure.
                max_frequency: max_frequency()
                    .into_iter()
                    .chain(firmware.and_then(|p| p.max_speed_mhz))
                    .max(),
                current_frequency: block
                    .get("cpu MHz")
                    .and_then(|v| v.parse::<f64>().ok())
                    .map(|v| v.round() as u32),
                l1d_kb: cache.l1d_kb,
                l1i_kb: cache.l1i_kb,
                l2_kb: cache.l2_kb,
                l3_kb: cache.l3_kb,
                virtualization: Some(flags.split_whitespace().any(|f| f == "vmx" || f == "svm")),
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

/// Firmware's own view of each package, in package order.
fn firmware_processors(ctx: &mut Ctx) -> Vec<dmi::Processor> {
    let processors = dmi::processors();
    if processors.is_empty() && !dmi::readable() {
        ctx.warn(
            "cpu: socket designation and rated clock come from SMBIOS, which is readable only by \
             root on Linux",
        );
    }
    processors
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

pub fn gpus(ctx: &mut Ctx) -> Vec<Gpu> {
    let mut gpus = drm_gpus(ctx);

    // DRM nodes only exist for adapters a kernel graphics driver has bound.
    // Adapters bound to vfio-pci for passthrough, or with no driver loaded at
    // all, are still hardware a caller wants to see — sweep the PCI bus for
    // display-class devices and add whatever the DRM sweep missed.
    let known: std::collections::HashSet<String> =
        gpus.iter().filter_map(|g| g.pci_bus.clone()).collect();
    let mut extra = pci_class_gpus(&known);
    if !extra.is_empty() {
        ctx.warn(format!(
            "gpu: {} adapter(s) have no kernel graphics driver bound; detail is limited",
            extra.len()
        ));
    }
    gpus.append(&mut extra);

    gpus.sort_by(|a, b| a.pci_bus.cmp(&b.pci_bus));
    gpus
}

/// Adapters the kernel's DRM subsystem has claimed.
fn drm_gpus(ctx: &mut Ctx) -> Vec<Gpu> {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        ctx.warn("gpu: /sys/class/drm is unreadable");
        return Vec::new();
    };

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

            // sysfs carries no model string, only identifiers. The PCI ID
            // database resolves them; failing that, the Vulkan merge usually
            // supplies a name, and the identifiers are present regardless.
            let model = pci_device_name(vendor_id, device_id)
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
            // amdgpu publishes both its own VRAM and the GTT aperture —
            // system memory the adapter can map for itself, which is the
            // honest answer to "shared memory". i915 publishes neither;
            // the NVIDIA proprietary driver omits VRAM, and `nvidia-smi`
            // fills that gap later.
            gpu.vram_mb = read_u64(device.join("mem_info_vram_total"))
                .map(to_mb)
                .filter(|&mb| mb > 0);
            gpu.shared_memory_mb = read_u64(device.join("mem_info_gtt_total"))
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

/// Look up a device name in the system's PCI ID database.
///
/// This is the same file `lspci` reads, so parsing it directly gives the same
/// answer without a process spawn — and works on minimal images where
/// `pciutils` was never installed but `hwdata` was.
///
/// The format is indentation-structured:
///
/// ```text
/// 10de  NVIDIA Corporation
/// \t2820  AD107M [GeForce RTX 4060 Max-Q]
/// ```
fn pci_device_name(vendor_id: u32, device_id: u32) -> Option<String> {
    const DATABASES: [&str; 5] = [
        "/usr/share/hwdata/pci.ids",
        "/usr/share/misc/pci.ids",
        "/usr/share/pci.ids",
        "/var/lib/pciutils/pci.ids",
        // NixOS exposes packages only through the current system profile.
        "/run/current-system/sw/share/hwdata/pci.ids",
    ];

    let contents = DATABASES
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())?;

    let vendor_prefix = format!("{vendor_id:04x}");
    let device_prefix = format!("{device_id:04x}");
    let mut in_vendor = false;

    for line in contents.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        // Sub-vendor entries are indented twice and are not wanted here.
        if line.starts_with("\t\t") {
            continue;
        }

        if let Some(entry) = line.strip_prefix('\t') {
            if !in_vendor {
                continue;
            }
            if let Some(name) = entry.strip_prefix(&device_prefix) {
                return clean(name);
            }
            continue;
        }

        // A new unindented line ends the vendor block we were in.
        if in_vendor {
            return None;
        }
        in_vendor = line.starts_with(&vendor_prefix);
    }

    None
}

/// Sweep the PCI bus for display-class adapters the DRM sweep missed.
///
/// PCI base class `0x03` covers VGA (`0x0300`), 3D (`0x0302`, the class
/// NVIDIA datacenter cards use) and other display controllers (`0x0380`,
/// Intel's discrete Arc line). Everything already found through DRM is
/// skipped via its slot name.
fn pci_class_gpus(known: &std::collections::HashSet<String>) -> Vec<Gpu> {
    let Ok(entries) = std::fs::read_dir("/sys/bus/pci/devices") else {
        return Vec::new();
    };

    let mut gpus: Vec<Gpu> = entries
        .flatten()
        .filter(|entry| {
            known.iter().all(|slot| {
                !entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(slot)
            })
        })
        .filter_map(|entry| {
            let path = entry.path();
            let class = read_hex(path.join("class"))?;
            if (class >> 16) != 0x03 {
                return None;
            }

            let vendor_id = read_hex(path.join("vendor"))?;
            let device_id = read_hex(path.join("device"))?;

            let manufacturer = super::pci_vendor_name(vendor_id)
                .map(str::to_string)
                .unwrap_or_else(|| format!("Vendor 0x{vendor_id:04X}"));
            let model = pci_device_name(vendor_id, device_id)
                .unwrap_or_else(|| format!("Device 0x{device_id:04X}"));

            let mut gpu = blank_gpu(manufacturer, model);
            gpu.vendor_id = Some(vendor_id);
            gpu.vendor_id_hex = Some(format!("0x{vendor_id:04X}"));
            gpu.device_id = Some(device_id);
            gpu.device_id_hex = Some(format!("0x{device_id:04X}"));
            gpu.revision = read_hex(path.join("revision"));
            gpu.subsystem_id = read_hex(path.join("subsystem_device"))
                .zip(read_hex(path.join("subsystem_vendor")))
                .map(|(dev, ven)| format!("0x{dev:04X}{ven:04X}"));
            // The directory name *is* the PCI address.
            gpu.pci_bus = Some(entry.file_name().to_string_lossy().into_owned());
            // Only a bound driver publishes version files; an unbound adapter
            // reports identifiers and nothing else, which still beats being
            // invisible.
            gpu.driver_version = uevent_value(&path.join("uevent"), "DRIVER")
                .and_then(|name| read_trimmed(format!("/sys/module/{name}/version")))
                .and_then(clean);

            Some(gpu)
        })
        .collect();

    gpus.sort_by(|a, b| a.pci_bus.cmp(&b.pci_bus));
    gpus
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
// HIP / ROCm
// ---------------------------------------------------------------------------

const KFD_NODES: &str = "/sys/class/kfd/kfd/topology/nodes";

/// Read the HIP/ROCm situation from the kernel driver's own topology.
///
/// `amdkfd` publishes one node per agent, and every property needed here is a
/// plain unprivileged sysfs read — no `rocminfo`, no `rocm-smi`, and no
/// dependency on ROCm's userspace being on `PATH`.
pub fn hip(ctx: &mut Ctx) -> crate::scan::compute::Hip {
    use crate::scan::compute::{gfx_name, Hip, HipDevice};

    let library = find_hip_library();

    let mut hip = Hip {
        // AMD's own installs stamp a release file; distro-packaged ROCm does
        // not use /opt at all, so fall back to what the runtime library's own
        // soname says (the two agree on major.minor).
        rocm_version: rocm_version().or_else(|| {
            library
                .as_ref()
                .and_then(|lib| lib.version.as_deref())
                .and_then(hip_version_to_rocm)
        }),
        hip_version: library.as_ref().and_then(|lib| lib.version.clone()),
        // The runtime library is what a HIP program actually loads; the kernel
        // driver alone is not enough to run anything. Distro packages ship it
        // as a versioned soname (`libamdhip64.so.6`) with the unversioned
        // symlink only in the -dev package, so glob rather than test one name.
        runtime_present: library.is_some(),
        ..Default::default()
    };

    let Ok(entries) = std::fs::read_dir(KFD_NODES) else {
        // No amdkfd at all: either no AMD GPU or the driver is not loaded.
        return hip;
    };

    let mut nodes: Vec<_> = entries.flatten().collect();
    nodes.sort_by_key(|e| e.file_name());

    for node in nodes {
        let path = node.path();
        let Some(properties) = read_trimmed(path.join("properties")) else {
            continue;
        };

        let property = |key: &str| {
            properties.lines().find_map(|line| {
                let (name, value) = line.split_once(char::is_whitespace)?;
                (name == key).then(|| parse_u64_any(value.trim()))?
            })
        };

        // CPU nodes appear in the same topology and have no SIMDs.
        let Some(gfx_architecture) = property("gfx_target_version").and_then(gfx_name) else {
            continue;
        };

        hip.devices.push(HipDevice {
            pci_bus: property("location_id").map(|id| {
                let domain = property("domain").unwrap_or(0);
                pci_address(domain, id)
            }),
            gfx_architecture: Some(gfx_architecture),
            name: read_trimmed(path.join("name")).and_then(clean),
        });
    }

    if !hip.devices.is_empty() && !hip.runtime_present {
        ctx.warn(
            "hip: an AMD compute agent is present but no HIP runtime library was found; install \
             ROCm to use it",
        );
    }

    hip
}

/// `location_id` packs the PCI bus, device and function the same way the
/// kernel's `PCI_DEVFN` macro does.
fn pci_address(domain: u64, location_id: u64) -> String {
    let bus = (location_id >> 8) & 0xFF;
    let device = (location_id >> 3) & 0x1F;
    let function = location_id & 0x7;
    format!("{domain:04x}:{bus:02x}:{device:02x}.{function}")
}

/// ROCm stamps its release into a plain text file at install time.
///
/// AMD's own packages put it under `/opt/rocm`; some installs keep parallel
/// versioned trees (`/opt/rocm-6.2.4`) without updating the symlink, so both
/// spellings are searched.
fn rocm_version() -> Option<String> {
    let mut candidates = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/opt") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // The symlink itself plus any versioned tree next to it.
            if name == "rocm"
                || (name.starts_with("rocm-")
                    && name[5..].starts_with(|c: char| c.is_ascii_digit()))
            {
                candidates.push(entry.path().join(".info"));
            }
        }
    }

    candidates
        .iter()
        .flat_map(|dir| [dir.join("version"), dir.join("version-dev")])
        .find_map(|path| read_trimmed(&path))
        // The file reads like "6.2.4-123"; the build suffix is noise here.
        .and_then(|raw| clean(raw.split('-').next().unwrap_or(&raw)))
}

/// The HIP runtime library as found on disk.
#[derive(Debug, Clone)]
struct HipLibrary {
    /// From the soname, e.g. `"7.1.52802"`.
    version: Option<String>,
}

/// Find `libamdhip64.so*` wherever the distro put it.
///
/// AMD's own installer uses `/opt/rocm/lib`. Distro packagers move it to the
/// platform libdir — `/usr/lib64` on Fedora/RHEL, `/usr/lib/<triplet>` on
/// Debian multiarch — and strip the unversioned symlink unless the matching
/// `-dev`/`-devel` package is installed. The runtime is fully usable through
/// the soname alone, so a versioned hit counts as present.
fn find_hip_library() -> Option<HipLibrary> {
    let mut dirs: Vec<std::path::PathBuf> = vec![
        "/opt/rocm/lib".into(),
        "/usr/lib64".into(),
        "/usr/lib".into(),
        "/usr/local/lib".into(),
    ];

    // Debian multiarch triplets vary by architecture; glob rather than
    // hardcode x86_64.
    if let Ok(entries) = std::fs::read_dir("/usr/lib") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with("-linux-gnu") || name.ends_with("-linux-musl") {
                dirs.push(entry.path());
            }
        }
    }

    // Parallel versioned install trees keep their own lib dir.
    if let Ok(entries) = std::fs::read_dir("/opt") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name != "rocm" && name.starts_with("rocm-") {
                dirs.push(entry.path().join("lib"));
            }
        }
    }

    let mut best: Option<(u64, HipLibrary)> = None;
    for dir in &dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(version) = hip_version_from_filename(&name.to_string_lossy()) else {
                continue;
            };
            let major = version
                .split('.')
                .next()
                .and_then(|m| m.parse::<u64>().ok())
                .unwrap_or(0);
            let better = match &best {
                Some((seen_major, seen)) => {
                    // Prefer the newest major; within one, the fullest
                    // version string (`libamdhip64.so.7.1.52802` beats its
                    // own `.so.7` soname symlink).
                    major > *seen_major
                        || (major == *seen_major
                            && seen
                                .version
                                .as_deref()
                                .is_none_or(|v| version.len() > v.len()))
                }
                None => true,
            };
            if better {
                best = Some((
                    major,
                    HipLibrary {
                        version: Some(version),
                    },
                ));
            }
        }
    }

    if let Some((_, lib)) = best {
        return Some(lib);
    }

    // Every directory listing failed, yet the unversioned dev symlink may
    // still be directly reachable.
    dirs.iter()
        .find(|dir| dir.join("libamdhip64.so").exists())
        .map(|_| HipLibrary { version: None })
}

/// `"libamdhip64.so.7.1.52802"` → `"7.1.52802"`.
///
/// The runtime's soname carries its full version; every distro package keeps
/// at least the major (`libamdhip64.so.N`) even without the dev symlink.
fn hip_version_from_filename(name: &str) -> Option<String> {
    let rest = name.strip_prefix("libamdhip64.so")?;
    let rest = rest.strip_prefix('.')?;
    if rest.is_empty() || !rest.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    clean(rest)
}

/// HIP 6.2.41134 and ROCm 6.2.4 are the same release under different schemes:
/// the runtime patch number is an internal build, so only major.minor carries.
fn hip_version_to_rocm(hip_version: &str) -> Option<String> {
    let mut parts = hip_version.split('.');
    let major = parts.next()?;
    let minor = parts.next()?;
    Some(format!("{major}.{minor}"))
}

/// Parse a decimal or `0x`-prefixed hexadecimal integer.
fn parse_u64_any(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    if let Some(hex) = raw.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).ok()
    } else {
        raw.parse().ok()
    }
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

pub fn memory(ctx: &mut Ctx) -> MemoryNative {
    // Slot occupancy comes from a much smaller structure than the per-module
    // inventory, so it is offered at every level — as far as SMBIOS allows,
    // which on Linux means root either way.
    let slots_total = dmi::memory_slots();
    let empty = |slots_total| MemoryNative {
        modules: Vec::new(),
        slots_total,
        slots_used: None,
    };

    if !ctx.wants(DetailLevel::Full) {
        return empty(slots_total);
    }

    let devices = dmi::memory_devices();
    if devices.is_empty() {
        if !dmi::readable() {
            ctx.warn(
                "memory modules: the SMBIOS tables under /sys/firmware/dmi are readable only by \
                 root, so per-DIMM detail was omitted",
            );
        }
        return empty(slots_total);
    }

    let modules: Vec<MemoryModule> = devices
        .into_iter()
        .map(|d| MemoryModule {
            slot: d.locator,
            bank: d.bank_locator,
            manufacturer: d.manufacturer,
            part_number: d.part_number,
            capacity_mb: d.capacity_mb,
            speed_mts: d.speed_mts,
            configured_speed_mts: d.configured_speed_mts,
            memory_type: d.memory_type.map(str::to_string),
            form_factor: d.form_factor.map(str::to_string),
            voltage_mv: d.voltage_mv,
            rank: d.rank,
            data_width_bits: d.data_width_bits,
            total_width_bits: d.total_width_bits,
            serial: d.serial,
        })
        .collect();

    MemoryNative {
        slots_used: Some(modules.len() as u32),
        slots_total,
        modules,
    }
}

// (memory module decoding now lives in `dmi`)

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
    (month.len() == 2 && day.len() == 2 && year.len() == 4).then(|| format!("{year}-{month}-{day}"))
}

/// The EFI `SecureBoot` variable: a four-byte attribute header then one byte of
/// value.
fn secure_boot() -> Option<bool> {
    const VAR: &str = "/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c";
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
    let Some(contents) =
        read_trimmed("/etc/os-release").or_else(|| read_trimmed("/usr/lib/os-release"))
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

/// Detect virtualisation and containerisation from the filesystem alone.
///
/// This is what `systemd-detect-virt` does internally, minus the process
/// spawn and the dependency on systemd being installed — which matters,
/// because the containers most likely to be running this are the ones least
/// likely to have it.
fn virtualization() -> Option<String> {
    // Containers first: a container on a VM should report the container, since
    // that is the boundary the application actually lives inside.
    if let Some(kind) = container() {
        return Some(kind.to_string());
    }

    // WSL is a VM but identifies itself only through the kernel release.
    if let Some(release) = read_trimmed("/proc/sys/kernel/osrelease") {
        let lowered = release.to_ascii_lowercase();
        if lowered.contains("microsoft") || lowered.contains("wsl") {
            return Some("WSL".into());
        }
    }

    // Xen publishes its own directory rather than appearing in DMI.
    if Path::new("/proc/xen").exists() || read_trimmed("/sys/hypervisor/type").is_some() {
        return Some("Xen".into());
    }

    // Everything else stamps an identity into DMI, which is world-readable
    // unlike the tables behind it.
    let haystack = [
        read_trimmed(format!("{DMI}/sys_vendor")),
        read_trimmed(format!("{DMI}/product_name")),
        read_trimmed(format!("{DMI}/board_vendor")),
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
            _ if haystack.contains("qemu") || haystack.contains("bochs") => "QEMU",
            _ if haystack.contains("xen") => "Xen",
            _ if haystack.contains("parallels") => "Parallels",
            _ if haystack.contains("bhyve") => "bhyve",
            _ if haystack.contains("amazon ec2") => "Amazon EC2",
            _ if haystack.contains("google") => "Google Compute Engine",
            _ if haystack.contains("microsoft corporation") => "Microsoft Hyper-V",
            _ => return None,
        }
        .to_string(),
    )
}

/// Identify the container runtime, if this process is inside one.
fn container() -> Option<&'static str> {
    if Path::new("/.dockerenv").exists() {
        return Some("Docker");
    }
    if Path::new("/run/.containerenv").exists() {
        return Some("Podman");
    }

    // systemd-nspawn and LXC advertise themselves to PID 1's environment.
    if let Ok(environ) = std::fs::read("/proc/1/environ") {
        let text = String::from_utf8_lossy(&environ);
        for entry in text.split('\0') {
            if let Some(value) = entry.strip_prefix("container=") {
                return Some(match value {
                    "lxc" | "lxc-libvirt" => "LXC",
                    "podman" => "Podman",
                    "docker" => "Docker",
                    _ => "container",
                });
            }
        }
    }

    // Older runtimes leave their name only in PID 1's cgroup path.
    let cgroup = read_trimmed("/proc/1/cgroup")?;
    if cgroup.contains("/docker") {
        Some("Docker")
    } else if cgroup.contains("/lxc") {
        Some("LXC")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{hip_version_from_filename, hip_version_to_rocm, parse_u64_any, pci_address};

    #[test]
    fn hip_runtime_versions_come_from_the_soname() {
        // Fedora ships the full version in the real file name.
        assert_eq!(
            hip_version_from_filename("libamdhip64.so.7.1.52802"),
            Some("7.1.52802".into())
        );
        // Debian's runtime package keeps only the soname major.
        assert_eq!(
            hip_version_from_filename("libamdhip64.so.6"),
            Some("6".into())
        );
        assert_eq!(
            hip_version_from_filename("libamdhip64.so"),
            None,
            "the unversioned dev symlink carries no version"
        );
        assert_eq!(hip_version_from_filename("libhsa-runtime64.so.1"), None);
    }

    #[test]
    fn hip_major_minor_maps_to_the_rocm_release() {
        // The runtime patch number is an internal build (41134), not the
        // ROCm release patch (4); only major.minor is shared between them.
        assert_eq!(hip_version_to_rocm("7.1.52802"), Some("7.1".into()));
        assert_eq!(hip_version_to_rocm("6.2.41134"), Some("6.2".into()));
        assert_eq!(hip_version_to_rocm("6"), None);
    }

    #[test]
    fn kfd_properties_parse_decimal_and_hex() {
        // Mainline kernels print `location_id` in decimal.
        assert_eq!(parse_u64_any("2304"), Some(2304));
        // ...but be tolerant of the hex spelling some tooling uses.
        assert_eq!(parse_u64_any("0x900"), Some(0x900));
        assert_eq!(parse_u64_any("nope"), None);
    }

    #[test]
    fn kfd_location_id_decodes_to_a_pci_address() {
        // 2304 = 0x900 → bus 09, device 00, function 0, the address of the
        // RX 6950 XT this was captured from.
        assert_eq!(pci_address(0, 2304), "0000:09:00.0");
        assert_eq!(
            pci_address(0x0001, ((0x2a << 8) | (0x10 << 3) | 0x3) as u64),
            "0001:2a:10.3"
        );
    }
}
