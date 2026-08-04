//! The v2 data contract.
//!
//! Every field is either always populated or an `Option` that serialises to
//! `null`. Nothing is silently replaced with `0` or `"Unknown"` - if a value
//! could not be read, it is `null` and the reason lands in
//! [`ScanMeta::warnings`].
//!
//! Fields marked *identifying* are only populated in [`ScanMode::Unsafe`].

use serde::{Deserialize, Serialize};

/// How deep the scan is allowed to reach.
///
/// `Safe` returns hardware capabilities only: models, capacities, speeds,
/// feature flags. `Unsafe` additionally returns per-unit identifiers -
/// serial numbers, MAC addresses, UUIDs, hostname, logged-in user - which
/// together form a stable device fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanMode {
    /// No per-unit identifiers. The default.
    #[default]
    Safe,
    /// Includes serial numbers, MACs, UUIDs and other identifiers.
    ///
    /// Must be enabled on the Rust side via
    /// [`crate::Builder::allow_unsafe_scan`], otherwise requests for it fail
    /// with [`crate::Error::UnsafeScanDisabled`].
    Unsafe,
}

impl ScanMode {
    pub fn is_unsafe(self) -> bool {
        self == ScanMode::Unsafe
    }

    /// Returns `value` in unsafe mode, `None` otherwise.
    pub(crate) fn redact<T>(self, value: Option<T>) -> Option<T> {
        if self.is_unsafe() {
            value
        } else {
            None
        }
    }
}

/// How much work a scan is allowed to do.
///
/// Orthogonal to [`ScanMode`], which governs *identity* rather than *effort*.
/// The variants are ordered, so a collector asks `detail >= Capabilities`
/// rather than matching every level.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum DetailLevel {
    /// Whatever can be read essentially for free: `CPUID`, the registry and
    /// `/sys`, firmware tables, and process-wide totals. No helper processes,
    /// no device probes, no sampling delay. The default.
    #[default]
    Summary,
    /// Adds probes that ask devices what they support - Vulkan enumeration and
    /// CUDA via `nvidia-smi`. Answers "what can this machine run".
    Capabilities,
    /// Adds itemised inventory and live state: per-core breakdowns, memory
    /// modules, physical disks, CPU load and Direct3D feature levels.
    Full,
}

/// A selectable part of [`SystemInfo`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Section {
    Cpu,
    Gpu,
    Memory,
    Storage,
    Network,
    Display,
    Battery,
    Board,
    Os,
}

impl Section {
    pub const ALL: [Section; 9] = [
        Section::Cpu,
        Section::Gpu,
        Section::Memory,
        Section::Storage,
        Section::Network,
        Section::Display,
        Section::Battery,
        Section::Board,
        Section::Os,
    ];
}

/// Options accepted by every command.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScanOptions {
    pub mode: ScanMode,
    pub detail: DetailLevel,
    /// Restrict a full-system scan to these sections. `None` means all of them.
    /// Ignored by the single-section commands.
    pub sections: Option<Vec<Section>>,
}

/// Provenance for a scan: what ran, how long it took, and what it could not read.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanMeta {
    /// Contract version of this payload.
    pub version: u32,
    pub mode: ScanMode,
    /// Echoed back so a caller holding a payload can tell the two kinds of
    /// `null` apart: a field omitted because of the detail level, versus one
    /// that failed to read and is explained in [`ScanMeta::warnings`].
    pub detail: DetailLevel,
    /// Sections actually collected.
    pub sections: Vec<Section>,
    /// Wall-clock duration of the scan.
    pub duration_ms: u64,
    /// Unix timestamp (seconds) the scan completed at.
    pub timestamp: u64,
    /// Non-fatal problems: probes that were unavailable, needed privileges, or
    /// returned nothing. A field being `null` is usually explained here.
    pub warnings: Vec<String>,
}

/// Everything, or the subset requested via [`ScanOptions::sections`].
///
/// Sections that were not requested are `null`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub scan: ScanMeta,
    pub cpu: Option<Vec<Cpu>>,
    pub gpu: Option<Vec<Gpu>>,
    pub memory: Option<Memory>,
    pub storage: Option<Storage>,
    pub network: Option<Vec<NetworkInterface>>,
    pub display: Option<Vec<Display>>,
    pub battery: Option<Vec<Battery>>,
    pub board: Option<Board>,
    pub os: Option<Os>,
}

// ---------------------------------------------------------------------------
// CPU
// ---------------------------------------------------------------------------

/// One physical processor package.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cpu {
    /// CPUID vendor string, e.g. `"AuthenticAMD"`.
    pub manufacturer: String,
    /// Marketing name, e.g. `"AMD Ryzen 9 5900X 12-Core Processor"`.
    pub model: String,
    /// Instruction set, e.g. `"x86_64"`, `"aarch64"`.
    pub architecture: String,
    /// Physical cores in this package. `null` when the platform cannot
    /// distinguish cores from threads.
    pub physical_cores: Option<u32>,
    /// Logical processors (hardware threads).
    pub threads: u32,
    /// Base (nominal) clock in MHz.
    pub base_frequency: Option<u32>,
    /// Highest clock in MHz either advertised by firmware or observed during
    /// the scan.
    ///
    /// No OS reliably publishes the turbo ceiling - Windows and CPUID both
    /// report the base clock on modern parts - so a core caught boosting can
    /// raise this above the advertised figure.
    pub max_frequency: u32,
    /// Clock at the moment of the scan, in MHz.
    pub current_frequency: Option<u32>,
    /// Socket designation, e.g. `"AM4"`, `"LGA1700"`.
    pub socket: Option<String>,
    pub family: Option<u32>,
    pub model_id: Option<u32>,
    pub stepping: Option<u32>,
    pub microcode: Option<String>,
    pub cache: CpuCache,
    /// Instruction-set extensions, upper-case and sorted, e.g. `["AVX", "AVX2", "SSE4.2"]`.
    pub features: Vec<String>,
    /// Whether hardware virtualisation (VT-x / AMD-V) is exposed.
    pub virtualization: Option<bool>,
    /// Hypervisor vendor string when running inside a VM, e.g. `"KVMKVMKVM"`.
    pub hypervisor: Option<String>,
    /// Whether SMT / Hyper-Threading is active.
    pub simultaneous_multithreading: Option<bool>,
    /// Package-wide load at the moment of the scan, 0–100.
    pub usage_percent: Option<f32>,
    /// Package temperature in °C.
    pub temperature_c: Option<f32>,
    /// Per-logical-processor detail.
    pub cores: Vec<CpuCore>,
    /// *Identifying.* Processor ID / serial.
    pub serial: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuCache {
    pub l1d_kb: Option<u32>,
    pub l1i_kb: Option<u32>,
    pub l2_kb: Option<u32>,
    pub l3_kb: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuCore {
    /// Platform label for this logical processor, e.g. `"cpu0"`.
    pub id: String,
    pub usage_percent: Option<f32>,
    pub frequency: Option<u32>,
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GpuKind {
    Discrete,
    Integrated,
    Virtual,
    /// A software rasteriser presenting itself as a GPU (llvmpipe, WARP).
    Cpu,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gpu {
    /// e.g. `"Advanced Micro Devices, Inc."`.
    pub manufacturer: String,
    /// e.g. `"AMD Radeon RX 6950 XT"`.
    pub model: String,
    pub kind: GpuKind,
    /// PCI vendor ID as an integer, e.g. `4098`.
    pub vendor_id: Option<u32>,
    /// PCI vendor ID as `"0x1002"`.
    pub vendor_id_hex: Option<String>,
    pub device_id: Option<u32>,
    pub device_id_hex: Option<String>,
    pub subsystem_id: Option<String>,
    pub revision: Option<u32>,
    /// Memory reserved exclusively for this adapter.
    pub vram_mb: Option<u64>,
    /// System memory the adapter may borrow.
    pub shared_memory_mb: Option<u64>,
    pub driver_version: Option<String>,
    /// ISO-8601 date, e.g. `"2024-03-11"`.
    pub driver_date: Option<String>,
    /// PCI address, e.g. `"0000:0a:00.0"`.
    pub pci_bus: Option<String>,
    /// Current output mode, when the adapter drives a display.
    pub current_resolution: Option<Resolution>,
    pub api: GpuApiSupport,
    /// *Identifying.* Adapter UUID / LUID.
    pub uuid: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuApiSupport {
    pub vulkan: bool,
    /// e.g. `"1.3.280"`.
    pub vulkan_version: Option<String>,
    /// Driver as Vulkan reports it, e.g. `"AMD proprietary driver"`.
    pub vulkan_driver: Option<String>,
    pub cuda: bool,
    /// e.g. `"12.4"`.
    pub cuda_version: Option<String>,
    /// e.g. `"8.6"`.
    pub compute_capability: Option<String>,
    /// Highest Direct3D feature level, e.g. `"12_1"`. Windows only.
    pub directx_feature_level: Option<String>,
    pub metal: bool,
    pub opencl: bool,
    /// e.g. `"4.6"`.
    pub opengl_version: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
    pub refresh_rate_hz: Option<f64>,
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub total_mb: u64,
    pub available_mb: u64,
    pub used_mb: u64,
    pub swap_total_mb: u64,
    pub swap_used_mb: u64,
    /// Physical DIMMs. Empty when SMBIOS is unreadable - see
    /// [`ScanMeta::warnings`].
    pub modules: Vec<MemoryModule>,
    pub slots_total: Option<u32>,
    pub slots_used: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryModule {
    /// Device locator, e.g. `"DIMM 0"`.
    pub slot: Option<String>,
    /// Bank locator, e.g. `"P0 CHANNEL A"`.
    pub bank: Option<String>,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub capacity_mb: Option<u64>,
    /// Rated speed in MT/s.
    pub speed_mts: Option<u32>,
    /// Speed the module is actually running at, in MT/s.
    pub configured_speed_mts: Option<u32>,
    /// e.g. `"DDR4"`, `"DDR5"`, `"LPDDR5"`.
    pub memory_type: Option<String>,
    /// e.g. `"DIMM"`, `"SODIMM"`.
    pub form_factor: Option<String>,
    pub voltage_mv: Option<u32>,
    pub rank: Option<u32>,
    pub data_width_bits: Option<u32>,
    pub total_width_bits: Option<u32>,
    /// *Identifying.*
    pub serial: Option<String>,
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Physical drives and the mounted volumes, reported separately because the
/// mapping between them is not reliably recoverable on every platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    pub disks: Vec<Disk>,
    pub volumes: Vec<Volume>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskKind {
    Hdd,
    Ssd,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Disk {
    /// Device path or identifier, e.g. `"/dev/nvme0n1"`, `"\\\\.\\PHYSICALDRIVE0"`.
    pub device: String,
    pub model: Option<String>,
    pub vendor: Option<String>,
    pub kind: DiskKind,
    /// Interface, e.g. `"NVMe"`, `"SATA"`, `"USB"`.
    pub bus: Option<String>,
    pub size_mb: Option<u64>,
    pub firmware_revision: Option<String>,
    pub is_removable: Option<bool>,
    /// Partition table type, e.g. `"GPT"`, `"MBR"`.
    pub partition_table: Option<String>,
    pub partition_count: Option<u32>,
    /// *Identifying.*
    pub serial: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    /// e.g. `"C:\\"`, `"/"`, `"/home"`.
    pub mount_point: String,
    /// Underlying device name as the OS reports it.
    pub name: Option<String>,
    /// e.g. `"NTFS"`, `"ext4"`, `"apfs"`.
    pub file_system: Option<String>,
    pub total_mb: u64,
    pub available_mb: u64,
    pub used_mb: u64,
    pub is_removable: bool,
    pub is_read_only: bool,
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterface {
    pub name: String,
    /// Friendly description, e.g. `"Intel(R) Wi-Fi 6 AX200"`.
    pub description: Option<String>,
    /// `"up"`, `"down"`, `"dormant"`, `"testing"`, `"lowerLayerDown"`,
    /// `"notPresent"`, `"unknown"`.
    pub state: String,
    pub mtu: Option<u64>,
    /// Link speed in Mb/s.
    pub speed_mbps: Option<u64>,
    /// Bytes received since boot.
    pub total_received: u64,
    /// Bytes transmitted since boot.
    pub total_transmitted: u64,
    pub packets_received: u64,
    pub packets_transmitted: u64,
    pub errors_received: u64,
    pub errors_transmitted: u64,
    /// *Identifying.* e.g. `"a4:bb:6d:00:11:22"`.
    pub mac_address: Option<String>,
    /// *Identifying.* CIDR notation, e.g. `["192.168.1.42/24"]`.
    pub ip_networks: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Display {
    /// Adapter-assigned name, e.g. `"\\\\.\\DISPLAY1"`.
    pub name: Option<String>,
    /// Manufacturer decoded from EDID, e.g. `"Dell Inc."`.
    pub manufacturer: Option<String>,
    /// Product name from EDID, e.g. `"DELL U2720Q"`.
    pub model: Option<String>,
    /// Current mode.
    pub resolution: Resolution,
    /// Native panel resolution, when it differs from the current mode.
    pub native_resolution: Option<Resolution>,
    /// DPI scaling applied by the OS, e.g. `1.5`.
    pub scale_factor: f64,
    /// Top-left corner in the virtual desktop.
    pub position_x: i32,
    pub position_y: i32,
    pub is_primary: bool,
    pub is_internal: Option<bool>,
    pub bits_per_pixel: Option<u32>,
    pub physical_width_mm: Option<u32>,
    pub physical_height_mm: Option<u32>,
    /// Diagonal size in inches, derived from the physical dimensions.
    pub diagonal_inches: Option<f64>,
    /// Year of manufacture from EDID.
    pub manufacture_year: Option<u32>,
    /// *Identifying.* EDID serial.
    pub serial: Option<String>,
}

// ---------------------------------------------------------------------------
// Battery
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BatteryState {
    Charging,
    Discharging,
    Empty,
    Full,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub vendor: Option<String>,
    pub model: Option<String>,
    /// e.g. `"lithium-ion"`.
    pub technology: Option<String>,
    pub state: BatteryState,
    /// Current charge, 0–100.
    pub charge_percent: f32,
    /// Remaining capacity relative to design capacity, 0–100. Below ~80 usually
    /// means the pack is worn.
    pub health_percent: Option<f32>,
    pub energy_wh: Option<f32>,
    pub energy_full_wh: Option<f32>,
    pub energy_full_design_wh: Option<f32>,
    /// Positive while charging, negative while discharging.
    pub energy_rate_w: Option<f32>,
    pub voltage_v: Option<f32>,
    pub temperature_c: Option<f32>,
    pub cycle_count: Option<u32>,
    pub seconds_to_full: Option<u64>,
    pub seconds_to_empty: Option<u64>,
    /// *Identifying.*
    pub serial: Option<String>,
}

// ---------------------------------------------------------------------------
// Board / firmware / chassis
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub manufacturer: Option<String>,
    /// Board model, e.g. `"X570 AORUS ELITE"`.
    pub product: Option<String>,
    pub version: Option<String>,
    /// *Identifying.*
    pub serial: Option<String>,
    /// *Identifying.*
    pub asset_tag: Option<String>,
    pub bios: Bios,
    pub chassis: Chassis,
    pub system: SystemIdentity,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bios {
    pub vendor: Option<String>,
    pub version: Option<String>,
    /// ISO-8601 date, e.g. `"2024-01-30"`.
    pub release_date: Option<String>,
    /// `"UEFI"` or `"Legacy"`.
    pub mode: Option<String>,
    pub secure_boot_enabled: Option<bool>,
    /// SMBIOS specification version, e.g. `"3.4"`.
    pub smbios_version: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chassis {
    pub manufacturer: Option<String>,
    /// e.g. `"Desktop"`, `"Notebook"`, `"Mini PC"`.
    pub kind: Option<String>,
    pub version: Option<String>,
    /// *Identifying.*
    pub serial: Option<String>,
}

/// Whole-machine identity as the vendor stamped it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemIdentity {
    pub manufacturer: Option<String>,
    /// e.g. `"MacBookPro18,3"`, `"XPS 15 9520"`.
    pub product: Option<String>,
    pub version: Option<String>,
    pub family: Option<String>,
    /// *Identifying.*
    pub sku: Option<String>,
    /// *Identifying.* SMBIOS system UUID.
    pub uuid: Option<String>,
    /// *Identifying.*
    pub serial: Option<String>,
}

// ---------------------------------------------------------------------------
// OS
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Os {
    /// e.g. `"Windows"`, `"Ubuntu"`, `"Darwin"`.
    pub name: String,
    /// `"windows"`, `"linux"`, `"macos"`, or the raw `target_os`.
    pub family: String,
    /// Marketing version, e.g. `"11"`, `"24.04"`, `"14.4.1"`.
    pub version: String,
    /// Full version string as the OS reports it.
    pub long_version: Option<String>,
    pub kernel_version: Option<String>,
    /// e.g. `"22631"`.
    pub build: Option<String>,
    /// e.g. `"Professional"`.
    pub edition: Option<String>,
    /// e.g. `"noble"`, `"Sonoma"`.
    pub codename: Option<String>,
    /// `/etc/os-release` ID on Linux, e.g. `"ubuntu"`.
    pub distribution_id: Option<String>,
    pub architecture: String,
    /// Seconds since boot.
    pub uptime_secs: u64,
    /// Unix timestamp of the last boot.
    pub boot_time_secs: u64,
    /// Detected hypervisor when running virtualised, e.g. `"KVM"`.
    pub virtualization: Option<String>,
    /// *Identifying.*
    pub hostname: Option<String>,
    /// *Identifying.* `/etc/machine-id`, Windows `MachineGuid`, or the macOS
    /// hardware UUID.
    pub machine_id: Option<String>,
    /// *Identifying.* The account the app is running as.
    pub user: Option<String>,
}
