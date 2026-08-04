//! Windows backend.
//!
//! WMI is reached through COM directly rather than by shelling out to
//! PowerShell: it is roughly two orders of magnitude faster and never flashes a
//! console window in a packaged app.
//!
//! Where WMI is known to lie, a native API is used instead - most notably
//! `Win32_VideoController::AdapterRAM`, a `uint32` that silently wraps for any
//! adapter with 4 GiB or more of VRAM. DXGI reports the real figure.

use std::cell::RefCell;
use std::collections::HashMap;

use wmi::{Variant, WMIConnection};

use super::util::pnp_vendor;
use super::{CpuNative, DisplayNative, MemoryNative, NetNative, OsNative};
use crate::models::*;
use crate::scan::{clean, to_mb, Ctx};

mod dxgi;
mod gdi;
mod registry;

type Row = HashMap<String, Variant>;

// ---------------------------------------------------------------------------
// WMI plumbing
// ---------------------------------------------------------------------------

thread_local! {
    /// One WMI connection per namespace, reused for the life of the thread.
    ///
    /// Opening a connection means `CoCreateInstance` + `ConnectServer` +
    /// `CoSetProxyBlanket` - tens of milliseconds each, and a full scan issues
    /// a dozen queries across two namespaces. `WMIConnection` is `!Send`
    /// precisely because it must stay on the thread that created it, which is
    /// what a thread-local guarantees.
    static CONNECTIONS: RefCell<HashMap<String, WMIConnection>> = RefCell::new(HashMap::new());
}

/// Run `f` with a connection to `namespace`, opening one if this thread has
/// none yet.
fn with_connection<T>(
    namespace: &str,
    f: impl FnOnce(&WMIConnection) -> T,
) -> Result<T, wmi::WMIError> {
    CONNECTIONS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.contains_key(namespace) {
            cache.insert(
                namespace.to_string(),
                WMIConnection::with_namespace_path(namespace)?,
            );
        }
        Ok(f(&cache[namespace]))
    })
}

/// Run a query, warning once per failed probe.
///
/// `columns` is empty for `SELECT *`. Name them explicitly for any class that
/// carries an embedded object - `SELECT *` on those fails to deserialise
/// wholesale, taking the useful scalar columns down with it.
fn query(ctx: &mut Ctx, namespace: &str, class: &str, columns: &[&str], probe: &str) -> Vec<Row> {
    query_where(ctx, namespace, class, columns, None, probe)
}

/// `query`, with a provider-side `WHERE` clause. Filtering in WMI rather than
/// in Rust matters for classes whose provider does real work per row.
fn query_where(
    ctx: &mut Ctx,
    namespace: &str,
    class: &str,
    columns: &[&str],
    filter: Option<&str>,
    probe: &str,
) -> Vec<Row> {
    let projection = if columns.is_empty() {
        "*".to_string()
    } else {
        columns.join(", ")
    };

    let query = match filter {
        Some(clause) => format!("SELECT {projection} FROM {class} WHERE {clause}"),
        None => format!("SELECT {projection} FROM {class}"),
    };
    let result = with_connection(namespace, |connection| connection.raw_query(query));

    match result {
        Ok(Ok(rows)) => rows,
        Ok(Err(e)) => {
            ctx.warn(format!("{probe}: WMI query for `{class}` failed ({e})"));
            Vec::new()
        }
        Err(e) => {
            ctx.warn(format!("{probe}: could not connect to WMI `{namespace}` ({e})"));
            Vec::new()
        }
    }
}

trait RowExt {
    fn text(&self, key: &str) -> Option<String>;
    fn number(&self, key: &str) -> Option<u64>;
    fn flag(&self, key: &str) -> Option<bool>;
    fn numbers(&self, key: &str) -> Vec<u64>;
    /// A `uint16[]` property holding a NUL-padded string, as the monitor EDID
    /// classes use.
    fn wide_string(&self, key: &str) -> Option<String>;
}

impl RowExt for Row {
    fn text(&self, key: &str) -> Option<String> {
        match self.get(key)? {
            Variant::String(s) => clean(s),
            other => clean(scalar_to_string(other)?),
        }
    }

    fn number(&self, key: &str) -> Option<u64> {
        match self.get(key)? {
            Variant::UI1(v) => Some(*v as u64),
            Variant::UI2(v) => Some(*v as u64),
            Variant::UI4(v) => Some(*v as u64),
            Variant::UI8(v) => Some(*v),
            Variant::I1(v) => u64::try_from(*v).ok(),
            Variant::I2(v) => u64::try_from(*v).ok(),
            Variant::I4(v) => u64::try_from(*v).ok(),
            Variant::I8(v) => u64::try_from(*v).ok(),
            // CIM_UINT64 frequently arrives as a decimal string.
            Variant::String(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    fn flag(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            Variant::Bool(v) => Some(*v),
            _ => None,
        }
    }

    fn numbers(&self, key: &str) -> Vec<u64> {
        match self.get(key) {
            Some(Variant::Array(items)) => items
                .iter()
                .filter_map(|v| match v {
                    Variant::UI1(n) => Some(*n as u64),
                    Variant::UI2(n) => Some(*n as u64),
                    Variant::UI4(n) => Some(*n as u64),
                    Variant::UI8(n) => Some(*n),
                    Variant::I4(n) => u64::try_from(*n).ok(),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn wide_string(&self, key: &str) -> Option<String> {
        let chars: Vec<u16> = self
            .numbers(key)
            .into_iter()
            .map(|n| n as u16)
            .take_while(|&c| c != 0)
            .collect();
        clean(String::from_utf16_lossy(&chars))
    }
}

fn scalar_to_string(v: &Variant) -> Option<String> {
    Some(match v {
        Variant::String(s) => s.clone(),
        Variant::UI1(n) => n.to_string(),
        Variant::UI2(n) => n.to_string(),
        Variant::UI4(n) => n.to_string(),
        Variant::UI8(n) => n.to_string(),
        Variant::I1(n) => n.to_string(),
        Variant::I2(n) => n.to_string(),
        Variant::I4(n) => n.to_string(),
        Variant::I8(n) => n.to_string(),
        Variant::R4(n) => n.to_string(),
        Variant::R8(n) => n.to_string(),
        Variant::Bool(b) => b.to_string(),
        _ => return None,
    })
}

/// WMI dates are `yyyymmddHHMMSS.ffffff±UUU`; callers only ever want the day.
fn wmi_date(raw: &str) -> Option<String> {
    if raw.len() < 8 || !raw[..8].bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(format!("{}-{}-{}", &raw[0..4], &raw[4..6], &raw[6..8]))
}

// ---------------------------------------------------------------------------
// CPU
// ---------------------------------------------------------------------------

pub fn cpu(ctx: &mut Ctx) -> Vec<CpuNative> {
    query(
        ctx,
        "ROOT\\CIMV2",
        "Win32_Processor",
        &[
            "Manufacturer",
            "Name",
            "SocketDesignation",
            "NumberOfCores",
            "NumberOfLogicalProcessors",
            "MaxClockSpeed",
            "CurrentClockSpeed",
            "L2CacheSize",
            "L3CacheSize",
            "VirtualizationFirmwareEnabled",
            "VMMonitorModeExtensions",
            "ProcessorId",
        ],
        "cpu",
    )
    .iter()
    .map(|row| CpuNative {
        manufacturer: row.text("Manufacturer"),
        model: row.text("Name"),
        socket: row.text("SocketDesignation"),
        physical_cores: row.number("NumberOfCores").map(|v| v as u32),
        threads: row.number("NumberOfLogicalProcessors").map(|v| v as u32),
        // `MaxClockSpeed` is a misnomer: Windows publishes the *base* clock
        // there, both on Intel and AMD. Report it as such and let `build` raise
        // `max_frequency` past it when a core is observed boosting.
        base_frequency: row.number("MaxClockSpeed").map(|v| v as u32),
        max_frequency: row.number("MaxClockSpeed").map(|v| v as u32),
        current_frequency: row.number("CurrentClockSpeed").map(|v| v as u32),
        // Win32_Processor reports L2/L3 in KiB but has no L1 field at all;
        // CPUID fills those in.
        l1d_kb: None,
        l1i_kb: None,
        l2_kb: row.number("L2CacheSize").map(|v| v as u32),
        l3_kb: row.number("L3CacheSize").map(|v| v as u32),
        // Either flag being set means the hardware exposes virtualisation.
        // `VirtualizationFirmwareEnabled` reads false on a host running
        // Hyper-V or VBS, where the extensions are held by the root partition.
        virtualization: match (
            row.flag("VirtualizationFirmwareEnabled"),
            row.flag("VMMonitorModeExtensions"),
        ) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(false) || b.unwrap_or(false)),
        },
        microcode: registry::microcode_revision(),
        temperature_c: None,
        serial: row.text("ProcessorId"),
    })
    .collect()
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

pub fn gpus(ctx: &mut Ctx) -> Vec<Gpu> {
    let mut gpus = dxgi::adapters(ctx);
    let controllers = query(
        ctx,
        "ROOT\\CIMV2",
        "Win32_VideoController",
        &[
            "PNPDeviceID",
            "DriverVersion",
            "DriverDate",
            "AdapterCompatibility",
            "CurrentHorizontalResolution",
            "CurrentVerticalResolution",
            "CurrentRefreshRate",
        ],
        "gpu",
    );

    for gpu in &mut gpus {
        // PNPDeviceID looks like `PCI\VEN_1002&DEV_73A5&SUBSYS_...`, which is
        // the only reliable way to tie a WMI controller to a DXGI adapter.
        let matched = controllers.iter().find(|row| {
            row.text("PNPDeviceID").is_some_and(|id| {
                let id = id.to_ascii_uppercase();
                gpu.vendor_id
                    .zip(gpu.device_id)
                    .is_some_and(|(v, d)| {
                        id.contains(&format!("VEN_{v:04X}")) && id.contains(&format!("DEV_{d:04X}"))
                    })
            })
        });

        let Some(row) = matched else { continue };

        gpu.driver_version = row.text("DriverVersion");
        gpu.driver_date = row.text("DriverDate").as_deref().and_then(wmi_date);
        if gpu.manufacturer == "Unknown" {
            gpu.manufacturer = row
                .text("AdapterCompatibility")
                .unwrap_or_else(|| "Unknown".into());
        }

        let width = row.number("CurrentHorizontalResolution");
        let height = row.number("CurrentVerticalResolution");
        if let (Some(w), Some(h)) = (width, height) {
            if w > 0 && h > 0 {
                gpu.current_resolution = Some(Resolution {
                    width: w as u32,
                    height: h as u32,
                    refresh_rate_hz: row.number("CurrentRefreshRate").map(|v| v as f64),
                });
            }
        }
    }

    gpus
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

pub fn memory(ctx: &mut Ctx) -> MemoryNative {
    // Slot occupancy comes from a separate, much cheaper class than the
    // per-module inventory, and "is there a free slot" is the most broadly
    // useful thing here - so it stays available at every detail level.
    let slots_total = query(
        ctx,
        "ROOT\\CIMV2",
        "Win32_PhysicalMemoryArray",
        &["MemoryDevices"],
        "memory slots",
    )
    .iter()
    .filter_map(|row| row.number("MemoryDevices"))
    .map(|v| v as u32)
    .max();

    if !ctx.wants(DetailLevel::Full) {
        return MemoryNative {
            modules: Vec::new(),
            slots_total,
            slots_used: None,
        };
    }

    let modules = query(
        ctx,
        "ROOT\\CIMV2",
        "Win32_PhysicalMemory",
        &[
            "DeviceLocator",
            "BankLabel",
            "Manufacturer",
            "PartNumber",
            "Capacity",
            "Speed",
            "ConfiguredClockSpeed",
            "SMBIOSMemoryType",
            "FormFactor",
            "ConfiguredVoltage",
            "DataWidth",
            "TotalWidth",
            "SerialNumber",
        ],
        "memory modules",
    )
    .iter()
        .map(|row| MemoryModule {
            slot: row.text("DeviceLocator"),
            bank: row.text("BankLabel"),
            manufacturer: row.text("Manufacturer"),
            part_number: row.text("PartNumber"),
            capacity_mb: row.number("Capacity").map(to_mb),
            speed_mts: row.number("Speed").map(|v| v as u32),
            configured_speed_mts: row.number("ConfiguredClockSpeed").map(|v| v as u32),
            memory_type: row
                .number("SMBIOSMemoryType")
                .and_then(smbios_memory_type)
                .map(str::to_string),
            form_factor: row
                .number("FormFactor")
                .and_then(form_factor)
                .map(str::to_string),
            voltage_mv: row.number("ConfiguredVoltage").map(|v| v as u32),
            rank: None,
            data_width_bits: row.number("DataWidth").map(|v| v as u32),
            total_width_bits: row.number("TotalWidth").map(|v| v as u32),
            serial: row.text("SerialNumber"),
        })
        .collect::<Vec<_>>();

    MemoryNative {
        slots_used: Some(modules.len() as u32),
        modules,
        slots_total,
    }
}

/// SMBIOS 7.18.2 memory type codes.
fn smbios_memory_type(code: u64) -> Option<&'static str> {
    Some(match code {
        2 => "DRAM",
        5 => "EDO",
        9 => "RAM",
        10 => "ROM",
        17 => "SDRAM",
        18 => "EDRAM",
        20 => "DDR",
        21 => "DDR2",
        22 => "DDR2 FB-DIMM",
        24 => "DDR3",
        25 => "FBD2",
        26 => "DDR4",
        27 => "LPDDR",
        28 => "LPDDR2",
        29 => "LPDDR3",
        30 => "LPDDR4",
        31 => "Logical non-volatile device",
        32 => "HBM",
        33 => "HBM2",
        34 => "DDR5",
        35 => "LPDDR5",
        36 => "HBM3",
        _ => return None,
    })
}

/// `Win32_PhysicalMemory::FormFactor` codes.
///
/// These are the CIM values, which are *not* the SMBIOS 7.18.1 codes the same
/// field uses on Linux - SODIMM is 12 here and 13 there, so a shared table
/// would label every laptop's memory as RIMM.
fn form_factor(code: u64) -> Option<&'static str> {
    Some(match code {
        2 => "SIP",
        3 => "DIP",
        4 => "ZIP",
        5 => "SOJ",
        6 => "Proprietary",
        7 => "SIMM",
        8 => "DIMM",
        9 => "TSOP",
        10 => "PGA",
        11 => "RIMM",
        12 => "SODIMM",
        13 => "SRIMM",
        14 => "SMD",
        15 => "SSMP",
        16 => "QFP",
        17 => "TQFP",
        18 => "SOIC",
        19 => "LCC",
        20 => "PLCC",
        21 => "BGA",
        22 => "FPBGA",
        23 => "LGA",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

pub fn disks(ctx: &mut Ctx) -> Vec<Disk> {
    // MSFT_PhysicalDisk knows SSD vs HDD and the real bus type; Win32_DiskDrive
    // only offers "Fixed hard disk media" for everything. Missing on Windows
    // editions without the Storage provider, hence the merge rather than a
    // dependency.
    if !ctx.wants(DetailLevel::Full) {
        return Vec::new();
    }

    let physical = query(
        ctx,
        "ROOT\\Microsoft\\Windows\\Storage",
        "MSFT_PhysicalDisk",
        &["DeviceId", "MediaType", "BusType", "Manufacturer"],
        "disk media type",
    );

    query(
        ctx,
        "ROOT\\CIMV2",
        "Win32_DiskDrive",
        &[
            "Index",
            "DeviceID",
            "Model",
            "Manufacturer",
            "InterfaceType",
            "Size",
            "FirmwareRevision",
            "MediaType",
            "Partitions",
            "SerialNumber",
        ],
        "disks",
    )
    .iter()
        .map(|row| {
            let index = row.number("Index");
            let extra = physical.iter().find(|p| {
                p.text("DeviceId")
                    .and_then(|d| d.parse::<u64>().ok())
                    .is_some_and(|d| Some(d) == index)
            });

            let kind = extra
                .and_then(|p| p.number("MediaType"))
                .map(|m| match m {
                    3 => DiskKind::Hdd,
                    4 => DiskKind::Ssd,
                    _ => DiskKind::Unknown,
                })
                .unwrap_or(DiskKind::Unknown);

            Disk {
                device: row
                    .text("DeviceID")
                    .unwrap_or_else(|| "\\\\.\\PHYSICALDRIVE?".into()),
                model: row.text("Model"),
                vendor: extra.and_then(|p| p.text("Manufacturer")).or_else(|| row.text("Manufacturer")),
                kind,
                bus: extra
                    .and_then(|p| p.number("BusType"))
                    .and_then(bus_type)
                    .map(str::to_string)
                    .or_else(|| row.text("InterfaceType")),
                size_mb: row.number("Size").map(to_mb),
                firmware_revision: row.text("FirmwareRevision"),
                is_removable: row
                    .text("MediaType")
                    .map(|m| m.to_ascii_lowercase().contains("removable")),
                partition_table: None,
                partition_count: row.number("Partitions").map(|v| v as u32),
                serial: row.text("SerialNumber"),
            }
        })
        .collect()
}

/// `MSFT_PhysicalDisk::BusType` values.
fn bus_type(code: u64) -> Option<&'static str> {
    Some(match code {
        1 => "SCSI",
        2 => "ATAPI",
        3 => "ATA",
        4 => "IEEE 1394",
        5 => "SSA",
        6 => "Fibre Channel",
        7 => "USB",
        8 => "RAID",
        9 => "iSCSI",
        10 => "SAS",
        11 => "SATA",
        12 => "SD",
        13 => "MMC",
        15 => "File-backed virtual",
        16 => "Storage spaces",
        17 => "NVMe",
        18 => "SCM",
        19 => "UFS",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

pub fn network(ctx: &mut Ctx) -> HashMap<String, NetNative> {
    // `Win32_NetworkAdapter` is the slowest class this backend touches - its
    // provider walks the whole network stack, and even filtered it costs
    // several hundred milliseconds. The adapter model string and link speed it
    // adds are inventory detail, so they wait for the full tier; the interface
    // list itself comes from `sysinfo` at every level.
    if !ctx.wants(DetailLevel::Full) {
        return HashMap::new();
    }

    query_where(
        ctx,
        "ROOT\\CIMV2",
        "Win32_NetworkAdapter",
        &[
            "NetConnectionID",
            "Name",
            "Description",
            "ProductName",
            "Speed",
        ],
        // Adapters with no connection name are hidden or virtual, and never
        // match a key `sysinfo` reports.
        Some("NetConnectionID IS NOT NULL"),
        "network",
    )
    .iter()
        .filter_map(|row| {
            // `sysinfo` keys interfaces by the connection name Windows shows in
            // Network Connections, which is NetConnectionID.
            let name = row.text("NetConnectionID").or_else(|| row.text("Name"))?;
            Some((
                name,
                NetNative {
                    description: row.text("Description").or_else(|| row.text("ProductName")),
                    speed_mbps: row
                        .number("Speed")
                        .filter(|&s| s > 0)
                        .map(|bits_per_sec| bits_per_sec / 1_000_000),
                },
            ))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Displays
// ---------------------------------------------------------------------------

pub fn displays(ctx: &mut Ctx) -> Vec<DisplayNative> {
    let found = gdi::displays(ctx);

    let ids = query(
        ctx,
        "ROOT\\WMI",
        "WmiMonitorID",
        &[
            "InstanceName",
            "ManufacturerName",
            "UserFriendlyName",
            "SerialNumberID",
            "YearOfManufacture",
        ],
        "monitor EDID",
    );
    let params = query(
        ctx,
        "ROOT\\WMI",
        "WmiMonitorBasicDisplayParams",
        &[
            "InstanceName",
            "MaxHorizontalImageSize",
            "MaxVerticalImageSize",
            "VideoInputType",
        ],
        "monitor size",
    );

    found
        .into_iter()
        .map(|entry| {
            let mut display = entry.display;

            // GDI hands back `\\?\DISPLAY#DEL41A8#5&1a2b&UID4353#{guid}`; WMI
            // keys the same monitor as `DISPLAY\DEL41A8\5&1a2b&UID4353_0`.
            let Some(key) = entry.monitor_key.as_deref() else {
                return display;
            };

            if let Some(row) = ids.iter().find(|r| instance_matches(r, key)) {
                display.manufacturer = row
                    .wide_string("ManufacturerName")
                    .map(|code| pnp_vendor(&code).map(str::to_string).unwrap_or(code));
                display.model = row.wide_string("UserFriendlyName");
                display.serial = row.wide_string("SerialNumberID");
                display.manufacture_year = row.number("YearOfManufacture").map(|v| v as u32);
            }

            if let Some(row) = params.iter().find(|r| instance_matches(r, key)) {
                // EDID stores physical size in whole centimetres.
                display.physical_width_mm = row
                    .number("MaxHorizontalImageSize")
                    .filter(|&v| v > 0)
                    .map(|cm| (cm * 10) as u32);
                display.physical_height_mm = row
                    .number("MaxVerticalImageSize")
                    .filter(|&v| v > 0)
                    .map(|cm| (cm * 10) as u32);
                // EDID marks the video input as digital or analog. Every
                // external panel worth the name is digital these days, so this
                // only reliably flags legacy VGA-attached monitors as external.
                if row.flag("VideoInputType") == Some(false) {
                    display.is_internal = Some(false);
                }
            }

            display
        })
        .collect()
}

fn instance_matches(row: &Row, monitor_key: &str) -> bool {
    row.text("InstanceName")
        .is_some_and(|instance| normalise_instance(&instance) == monitor_key)
}

/// Reduce a WMI `InstanceName` or a GDI device interface path to a common form.
fn normalise_instance(raw: &str) -> String {
    let upper = raw.to_ascii_uppercase().replace('#', "\\");
    let without_prefix = upper.trim_start_matches("\\\\?\\");
    // Drop the trailing interface GUID (GDI) or `_0` suffix (WMI).
    let core = without_prefix
        .split_once("\\{")
        .map(|(head, _)| head)
        .unwrap_or(without_prefix);
    core.trim_end_matches("_0").trim_end_matches('\\').to_string()
}

// ---------------------------------------------------------------------------
// Board / firmware
// ---------------------------------------------------------------------------

pub fn board(ctx: &mut Ctx) -> Board {
    let base = query(
        ctx,
        "ROOT\\CIMV2",
        "Win32_BaseBoard",
        &["Manufacturer", "Product", "Version", "SerialNumber", "Tag"],
        "board",
    );
    let bios_rows = query(
        ctx,
        "ROOT\\CIMV2",
        "Win32_BIOS",
        &[
            "Manufacturer",
            "SMBIOSBIOSVersion",
            "Version",
            "ReleaseDate",
            "SMBIOSMajorVersion",
            "SMBIOSMinorVersion",
            "SerialNumber",
        ],
        "bios",
    );
    let enclosure = query(
        ctx,
        "ROOT\\CIMV2",
        "Win32_SystemEnclosure",
        &[
            "Manufacturer",
            "ChassisTypes",
            "Version",
            "SerialNumber",
            "SMBIOSAssetTag",
        ],
        "chassis",
    );
    let computer = query(
        ctx,
        "ROOT\\CIMV2",
        "Win32_ComputerSystem",
        &[
            "Manufacturer",
            "Model",
            "SystemFamily",
            "SystemSKUNumber",
            "UserName",
        ],
        "system",
    );
    let product = query(
        ctx,
        "ROOT\\CIMV2",
        "Win32_ComputerSystemProduct",
        &["Version", "UUID", "IdentifyingNumber", "Name"],
        "system identity",
    );

    let board_row = base.first();
    let bios_row = bios_rows.first();
    let enclosure_row = enclosure.first();
    let computer_row = computer.first();
    let product_row = product.first();

    let bios = Bios {
        vendor: bios_row.and_then(|r| r.text("Manufacturer")),
        version: bios_row.and_then(|r| {
            r.text("SMBIOSBIOSVersion")
                .or_else(|| r.text("Version"))
        }),
        release_date: bios_row
            .and_then(|r| r.text("ReleaseDate"))
            .as_deref()
            .and_then(wmi_date),
        mode: registry::firmware_type(),
        secure_boot_enabled: registry::secure_boot_enabled(),
        smbios_version: bios_row.and_then(|r| {
            let major = r.number("SMBIOSMajorVersion")?;
            let minor = r.number("SMBIOSMinorVersion")?;
            Some(format!("{major}.{minor}"))
        }),
    };

    let chassis = Chassis {
        manufacturer: enclosure_row.and_then(|r| r.text("Manufacturer")),
        kind: enclosure_row
            .map(|r| r.numbers("ChassisTypes"))
            .and_then(|types| types.first().copied())
            .and_then(chassis_type)
            .map(str::to_string),
        version: enclosure_row.and_then(|r| r.text("Version")),
        serial: enclosure_row.and_then(|r| r.text("SerialNumber")),
    };

    let system = SystemIdentity {
        manufacturer: computer_row.and_then(|r| r.text("Manufacturer")),
        product: computer_row.and_then(|r| r.text("Model")),
        version: product_row.and_then(|r| r.text("Version")),
        family: computer_row.and_then(|r| r.text("SystemFamily")),
        sku: computer_row.and_then(|r| r.text("SystemSKUNumber")),
        uuid: product_row.and_then(|r| r.text("UUID")),
        serial: product_row
            .and_then(|r| r.text("IdentifyingNumber"))
            .or_else(|| bios_row.and_then(|r| r.text("SerialNumber"))),
    };

    Board {
        manufacturer: board_row.and_then(|r| r.text("Manufacturer")),
        product: board_row.and_then(|r| r.text("Product")),
        version: board_row.and_then(|r| r.text("Version")),
        serial: board_row.and_then(|r| r.text("SerialNumber")),
        asset_tag: board_row
            .and_then(|r| r.text("Tag"))
            .or_else(|| enclosure_row.and_then(|r| r.text("SMBIOSAssetTag"))),
        bios,
        chassis,
        system,
    }
}

/// SMBIOS 7.4.1 chassis type codes.
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

pub fn os(ctx: &mut Ctx) -> OsNative {
    let computer = query(
        ctx,
        "ROOT\\CIMV2",
        "Win32_ComputerSystem",
        &["Manufacturer", "Model", "UserName"],
        "os",
    );
    let computer_row = computer.first();

    // The registry carries the update-build revision and the marketing release
    // label ("24H2"), neither of which WMI exposes.
    let build = match (
        registry::current_version("CurrentBuild"),
        registry::current_version_u32("UBR"),
    ) {
        (Some(build), Some(ubr)) => Some(format!("{build}.{ubr}")),
        (Some(build), None) => Some(build),
        _ => None,
    };

    OsNative {
        build,
        edition: registry::current_version("EditionID"),
        codename: registry::current_version("DisplayVersion"),
        virtualization: computer_row.and_then(|r| {
            let model = r.text("Model")?;
            let manufacturer = r.text("Manufacturer").unwrap_or_default();
            detect_hypervisor(&manufacturer, &model).map(str::to_string)
        }),
        machine_id: registry::machine_guid(),
        user: computer_row.and_then(|r| r.text("UserName")),
    }
}

/// Recognise the machine as virtual from the identity the hypervisor stamps
/// into SMBIOS.
fn detect_hypervisor(manufacturer: &str, model: &str) -> Option<&'static str> {
    let haystack = format!("{manufacturer} {model}").to_ascii_lowercase();
    Some(match () {
        _ if haystack.contains("vmware") => "VMware",
        _ if haystack.contains("virtualbox") => "VirtualBox",
        _ if haystack.contains("kvm") => "KVM",
        _ if haystack.contains("qemu") => "QEMU",
        _ if haystack.contains("xen") => "Xen",
        _ if haystack.contains("parallels") => "Parallels",
        _ if haystack.contains("virtual machine") || haystack.contains("hyper-v") => {
            "Microsoft Hyper-V"
        }
        _ => return None,
    })
}
