//! The data contract.
//!
//! See `docs/CONTRACT.md` for the flat field reference, including which detail
//! level and scan mode each field needs and which platforms populate it.
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
    /// Whether this mode permits per-unit identifiers.
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
    /// Processor packages.
    Cpu,
    /// Display adapters.
    Gpu,
    /// Memory totals and modules.
    Memory,
    /// Drives and mounted volumes.
    Storage,
    /// Network interfaces.
    Network,
    /// Attached monitors.
    Display,
    /// Batteries and power supplies.
    Battery,
    /// Motherboard, firmware and chassis.
    Board,
    /// Operating system and kernel.
    Os,
}

impl Section {
    /// Every section, in the order a full scan collects them.
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
    /// How deep into *identity* the scan may reach. Defaults to
    /// [`ScanMode::Safe`].
    pub mode: ScanMode,
    /// How much *effort* the scan may spend. Defaults to
    /// [`DetailLevel::Summary`].
    pub detail: DetailLevel,
    /// Restrict a full-system scan to these sections. `None` means all of them.
    /// Ignored by the single-section commands.
    pub sections: Option<Vec<Section>>,
}

/// Provenance for a scan: what ran, how long it took, and what it could not read.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanMeta {
    /// Contract version of this payload. Tracks the crate's major version, so
    /// it changes only when the shape does. `1` for this release.
    pub version: u32,
    /// The mode the scan ran in, echoed back.
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
    /// What ran, how long it took, and what it could not read.
    pub scan: ScanMeta,
    /// One entry per physical processor package.
    pub cpu: Option<Vec<Cpu>>,
    /// Every display adapter, including integrated and software ones.
    pub gpu: Option<Vec<Gpu>>,
    /// Memory totals and, at [`DetailLevel::Full`], the physical DIMMs.
    pub memory: Option<Memory>,
    /// Mounted volumes and, at [`DetailLevel::Full`], the drives behind them.
    pub storage: Option<Storage>,
    /// Network interfaces and their traffic counters.
    pub network: Option<Vec<NetworkInterface>>,
    /// Attached monitors.
    pub display: Option<Vec<Display>>,
    /// Batteries and power supplies. Empty on a desktop.
    pub battery: Option<Vec<Battery>>,
    /// Motherboard, firmware, chassis and whole-machine identity.
    pub board: Option<Board>,
    /// Operating system and kernel.
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
    ///
    /// Comes from SMBIOS, so it needs root on Linux and
    /// [`DetailLevel::Full`].
    pub socket: Option<String>,
    /// CPUID family, e.g. `25` for Zen 3.
    pub family: Option<u32>,
    /// CPUID model within the family. Not the marketing model number.
    pub model_id: Option<u32>,
    /// CPUID stepping — the silicon revision.
    pub stepping: Option<u32>,
    /// Running microcode revision, e.g. `"0x0A201025"`.
    pub microcode: Option<String>,
    /// Cache sizes for this package.
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
    ///
    /// Requires [`DetailLevel::Full`]: measuring load costs a sampling
    /// interval, since it is a delta between two observations.
    pub usage_percent: Option<f32>,
    /// Package temperature in °C.
    ///
    /// Requires [`DetailLevel::Full`], and a sensor the process is allowed to
    /// read — on Windows that means running elevated.
    pub temperature_c: Option<f32>,
    /// Per-logical-processor detail.
    ///
    /// Requires [`DetailLevel::Full`]; empty otherwise, because this array
    /// scales with core count and dominates the payload on large machines.
    pub cores: Vec<CpuCore>,
    /// *Identifying.* Processor ID / serial.
    pub serial: Option<String>,
}

/// Cache sizes for one package, in kibibytes.
///
/// L1 and L2 are summed across the cores that have them; L3 is normally shared
/// and reported once.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuCache {
    /// Level 1 data cache.
    pub l1d_kb: Option<u32>,
    /// Level 1 instruction cache.
    pub l1i_kb: Option<u32>,
    /// Level 2 cache.
    pub l2_kb: Option<u32>,
    /// Level 3 cache, usually shared by the whole package.
    pub l3_kb: Option<u32>,
}

/// One logical processor (hardware thread).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuCore {
    /// Platform label for this logical processor, e.g. `"cpu0"`.
    pub id: String,
    /// Load on this processor at the moment of the scan, 0–100.
    pub usage_percent: Option<f32>,
    /// Clock at the moment of the scan, in MHz.
    pub frequency: Option<u32>,
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

/// What class of device an adapter is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GpuKind {
    /// A separate card or module with its own memory.
    Discrete,
    /// Part of the CPU package, sharing system memory.
    Integrated,
    /// Presented by a hypervisor.
    Virtual,
    /// A software rasteriser presenting itself as a GPU (llvmpipe, WARP).
    Cpu,
    /// Not established — most often because the scan ran below
    /// [`DetailLevel::Capabilities`], where Vulkan is not probed.
    #[default]
    Unknown,
}

/// One display adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gpu {
    /// e.g. `"Advanced Micro Devices, Inc."`.
    pub manufacturer: String,
    /// e.g. `"AMD Radeon RX 6950 XT"`.
    pub model: String,
    /// Discrete, integrated, virtual or software.
    pub kind: GpuKind,
    /// PCI vendor ID as an integer, e.g. `4098`.
    pub vendor_id: Option<u32>,
    /// PCI vendor ID as `"0x1002"`.
    pub vendor_id_hex: Option<String>,
    /// PCI device ID as an integer.
    pub device_id: Option<u32>,
    /// PCI device ID as `"0x73AF"`.
    pub device_id_hex: Option<String>,
    /// Subsystem (board partner) ID, e.g. `"0x87CF1043"`.
    pub subsystem_id: Option<String>,
    /// Silicon revision.
    pub revision: Option<u32>,
    /// Memory reserved exclusively for this adapter.
    pub vram_mb: Option<u64>,
    /// System memory the adapter may borrow.
    pub shared_memory_mb: Option<u64>,
    /// Driver version as the platform reports it, e.g. `"32.0.12033.1030"`.
    pub driver_version: Option<String>,
    /// ISO-8601 date, e.g. `"2024-03-11"`.
    pub driver_date: Option<String>,
    /// PCI address, e.g. `"0000:0a:00.0"`.
    pub pci_bus: Option<String>,
    /// Current output mode, when the adapter drives a display.
    pub current_resolution: Option<Resolution>,
    /// Which graphics and compute APIs this adapter supports.
    pub api: GpuApiSupport,
    /// *Identifying.* Adapter UUID / LUID.
    pub uuid: Option<String>,
}

/// Which graphics and compute APIs an adapter supports.
///
/// These are live device probes rather than table lookups, so they need
/// [`DetailLevel::Capabilities`] or higher. Below that the booleans read
/// `false` meaning *not probed*, not *not supported* — check
/// [`ScanMeta::detail`] before treating a `false` as an answer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuApiSupport {
    /// Whether a Vulkan device backed by this adapter was found.
    pub vulkan: bool,
    /// Highest Vulkan API version the device supports, e.g. `"1.3.280"`.
    pub vulkan_version: Option<String>,
    /// Driver as Vulkan reports it, e.g. `"AMD proprietary driver"`.
    pub vulkan_driver: Option<String>,
    /// Whether the CUDA stack is installed and can see this adapter.
    pub cuda: bool,
    /// e.g. `"12.4"`.
    pub cuda_version: Option<String>,
    /// CUDA compute capability, e.g. `"8.6"`.
    pub compute_capability: Option<String>,
    /// Whether AMD's HIP runtime is installed and this adapter is visible to it.
    pub hip: bool,
    /// HIP runtime version, e.g. `"6.2.41134"`.
    pub hip_version: Option<String>,
    /// ROCm release the runtime belongs to, e.g. `"6.2.4"`.
    pub rocm_version: Option<String>,
    /// AMD GPU target architecture, e.g. `"gfx1100"`, `"gfx90a"`.
    ///
    /// This is the field that decides whether a given ROCm build will run: the
    /// officially supported list is narrow, and everything else depends on
    /// `HSA_OVERRIDE_GFX_VERSION`. The HIP analogue of
    /// [`GpuApiSupport::compute_capability`].
    pub gfx_architecture: Option<String>,
    /// Highest Direct3D feature level, e.g. `"12_1"`. Windows only.
    pub directx_feature_level: Option<String>,
    /// Whether Metal is available. True on every Mac, false everywhere else —
    /// this follows from the target rather than from a probe.
    pub metal: bool,
    /// Whether a working OpenCL platform is installed.
    ///
    /// Determined by loading the ICD loader and calling `clGetPlatformIDs` —
    /// the loader library exists on most systems whether or not any driver
    /// registered a platform behind it, so its presence alone proves nothing.
    pub opencl: bool,
    /// Highest OpenCL version any installed platform reports, e.g. `"3.0"`.
    pub opencl_version: Option<String>,
    /// Highest OpenGL version reported, e.g. `"4.6"`.
    pub opengl_version: Option<String>,
}

/// A display mode: pixel dimensions and, where known, refresh rate.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolution {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Refresh rate in hertz. `null` for panels with no variable rate.
    pub refresh_rate_hz: Option<f64>,
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

/// System memory totals and, at [`DetailLevel::Full`], the physical modules.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    /// Total physical memory installed.
    pub total_mb: u64,
    /// Memory available to start a new application without swapping.
    pub available_mb: u64,
    /// Memory currently in use.
    pub used_mb: u64,
    /// Total swap or page file.
    pub swap_total_mb: u64,
    /// Swap currently in use.
    pub swap_used_mb: u64,
    /// Physical DIMMs.
    ///
    /// Requires [`DetailLevel::Full`]. Also empty when SMBIOS is unreadable —
    /// root-only on Linux — or when the hardware has no modules to enumerate,
    /// as on Apple silicon. See [`ScanMeta::warnings`].
    pub modules: Vec<MemoryModule>,
    /// Memory slots on the board, occupied or not.
    ///
    /// Available at every detail level: a much cheaper lookup than
    /// [`Memory::modules`], and the answer to "can this machine be upgraded".
    pub slots_total: Option<u32>,
    /// Slots with a module in them.
    pub slots_used: Option<u32>,
}

/// One physical memory module.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryModule {
    /// Device locator, e.g. `"DIMM 0"`.
    pub slot: Option<String>,
    /// Bank locator, e.g. `"P0 CHANNEL A"`.
    pub bank: Option<String>,
    /// Module vendor, e.g. `"Kingston"`.
    pub manufacturer: Option<String>,
    /// Vendor part number.
    pub part_number: Option<String>,
    /// Capacity of this module.
    pub capacity_mb: Option<u64>,
    /// Rated speed in MT/s.
    pub speed_mts: Option<u32>,
    /// Speed the module is actually running at, in MT/s.
    ///
    /// Lower than [`MemoryModule::speed_mts`] when the board or an unset XMP
    /// profile is holding it back.
    pub configured_speed_mts: Option<u32>,
    /// e.g. `"DDR4"`, `"DDR5"`, `"LPDDR5"`.
    pub memory_type: Option<String>,
    /// e.g. `"DIMM"`, `"SODIMM"`.
    pub form_factor: Option<String>,
    /// Configured operating voltage in millivolts.
    pub voltage_mv: Option<u32>,
    /// Rank count — 1 for single-rank, 2 for dual-rank.
    pub rank: Option<u32>,
    /// Data bus width, normally 64.
    pub data_width_bits: Option<u32>,
    /// Total bus width including ECC bits — 72 on an ECC module.
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
    /// Physical drives.
    ///
    /// Requires [`DetailLevel::Full`]; empty otherwise.
    pub disks: Vec<Disk>,
    /// Mounted filesystems. Available at every detail level.
    pub volumes: Vec<Volume>,
}

/// Whether a drive is rotational.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskKind {
    /// A spinning disk.
    Hdd,
    /// Solid state.
    Ssd,
    /// The platform did not say.
    #[default]
    Unknown,
}

/// One physical drive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Disk {
    /// Device path or identifier, e.g. `"/dev/nvme0n1"`, `"\\\\.\\PHYSICALDRIVE0"`.
    pub device: String,
    /// Drive model, e.g. `"Samsung SSD 990 PRO 2TB"`.
    pub model: Option<String>,
    /// Vendor string, where the platform distinguishes it from the model.
    pub vendor: Option<String>,
    /// Rotational or solid state.
    pub kind: DiskKind,
    /// Interface, e.g. `"NVMe"`, `"SATA"`, `"USB"`.
    pub bus: Option<String>,
    /// Raw capacity — larger than the sum of its volumes.
    pub size_mb: Option<u64>,
    /// Drive firmware revision.
    pub firmware_revision: Option<String>,
    /// Whether the medium can be ejected.
    pub is_removable: Option<bool>,
    /// Partition table type, e.g. `"GPT"`, `"MBR"`.
    pub partition_table: Option<String>,
    /// Number of partitions on the drive.
    pub partition_count: Option<u32>,
    /// *Identifying.*
    pub serial: Option<String>,
}

/// One mounted filesystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    /// e.g. `"C:\\"`, `"/"`, `"/home"`.
    pub mount_point: String,
    /// Underlying device name as the OS reports it.
    pub name: Option<String>,
    /// e.g. `"NTFS"`, `"ext4"`, `"apfs"`.
    pub file_system: Option<String>,
    /// Capacity of the filesystem.
    pub total_mb: u64,
    /// Free space usable by the current user.
    pub available_mb: u64,
    /// Space in use.
    pub used_mb: u64,
    /// Whether the volume sits on removable media.
    pub is_removable: bool,
    /// Whether the volume is mounted read-only.
    pub is_read_only: bool,
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

/// One network interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterface {
    /// Interface name as the OS keys it, e.g. `"eth0"`, `"Wi-Fi"`.
    pub name: String,
    /// Friendly description, e.g. `"Intel(R) Wi-Fi 6 AX200"`.
    ///
    /// Requires [`DetailLevel::Full`] — on Windows this is the slowest single
    /// query in the whole scan.
    pub description: Option<String>,
    /// `"up"`, `"down"`, `"dormant"`, `"testing"`, `"lowerLayerDown"`,
    /// `"notPresent"`, `"unknown"`.
    pub state: String,
    /// Maximum transmission unit in bytes.
    pub mtu: Option<u64>,
    /// Link speed in Mb/s. Requires [`DetailLevel::Full`].
    pub speed_mbps: Option<u64>,
    /// Bytes received since boot.
    pub total_received: u64,
    /// Bytes transmitted since boot.
    pub total_transmitted: u64,
    /// Packets received since boot.
    pub packets_received: u64,
    /// Packets transmitted since boot.
    pub packets_transmitted: u64,
    /// Receive errors since boot.
    pub errors_received: u64,
    /// Transmit errors since boot.
    pub errors_transmitted: u64,
    /// *Identifying.* e.g. `"a4:bb:6d:00:11:22"`.
    pub mac_address: Option<String>,
    /// *Identifying.* CIDR notation, e.g. `["192.168.1.42/24"]`.
    pub ip_networks: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

/// One attached monitor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Display {
    /// Adapter-assigned name, e.g. `"\\\\.\\DISPLAY1"`, `"eDP-1"`.
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
    /// Left edge in the virtual desktop, in pixels.
    pub position_x: i32,
    /// Top edge in the virtual desktop, in pixels.
    pub position_y: i32,
    /// Whether this is the primary display.
    pub is_primary: bool,
    /// Whether this is a built-in panel rather than an external monitor.
    pub is_internal: Option<bool>,
    /// Colour depth of the current mode.
    pub bits_per_pixel: Option<u32>,
    /// Physical panel width in millimetres.
    pub physical_width_mm: Option<u32>,
    /// Physical panel height in millimetres.
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

/// What a battery is currently doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BatteryState {
    /// Taking charge from an external supply.
    Charging,
    /// Running down.
    Discharging,
    /// Fully depleted.
    Empty,
    /// Fully charged.
    Full,
    /// The platform did not say.
    #[default]
    Unknown,
}

/// One battery or power supply.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    /// Pack vendor.
    pub vendor: Option<String>,
    /// Pack model.
    pub model: Option<String>,
    /// e.g. `"lithium-ion"`.
    pub technology: Option<String>,
    /// Whether the pack is charging, discharging, full or empty.
    pub state: BatteryState,
    /// Current charge, 0–100.
    pub charge_percent: f32,
    /// Remaining capacity relative to design capacity, 0–100. Below ~80 usually
    /// means the pack is worn.
    pub health_percent: Option<f32>,
    /// Energy currently stored.
    pub energy_wh: Option<f32>,
    /// Energy the pack holds when full today.
    pub energy_full_wh: Option<f32>,
    /// Energy the pack held when full when new.
    pub energy_full_design_wh: Option<f32>,
    /// Positive while charging, negative while discharging.
    pub energy_rate_w: Option<f32>,
    /// Terminal voltage.
    pub voltage_v: Option<f32>,
    /// Pack temperature in °C, where a sensor exists.
    pub temperature_c: Option<f32>,
    /// Completed charge cycles.
    pub cycle_count: Option<u32>,
    /// Estimated seconds until full, while charging.
    pub seconds_to_full: Option<u64>,
    /// Estimated seconds until empty, while discharging.
    pub seconds_to_empty: Option<u64>,
    /// *Identifying.*
    pub serial: Option<String>,
}

// ---------------------------------------------------------------------------
// Board / firmware / chassis
// ---------------------------------------------------------------------------

/// Motherboard, firmware, chassis and whole-machine identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    /// Board vendor, e.g. `"GIGABYTE"`.
    pub manufacturer: Option<String>,
    /// Board model, e.g. `"X570 AORUS ELITE"`.
    pub product: Option<String>,
    /// Board revision.
    pub version: Option<String>,
    /// *Identifying.*
    pub serial: Option<String>,
    /// *Identifying.* Inventory tag, where an IT department set one.
    pub asset_tag: Option<String>,
    /// Firmware.
    pub bios: Bios,
    /// Enclosure.
    pub chassis: Chassis,
    /// Whole-machine identity.
    pub system: SystemIdentity,
}

/// System firmware.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bios {
    /// Firmware vendor, e.g. `"American Megatrends Inc."`.
    pub vendor: Option<String>,
    /// Firmware version, e.g. `"F34"`.
    pub version: Option<String>,
    /// ISO-8601 date, e.g. `"2024-01-30"`.
    pub release_date: Option<String>,
    /// `"UEFI"` or `"Legacy"`.
    pub mode: Option<String>,
    /// Whether Secure Boot is enabled. `null` on legacy-BIOS machines, where
    /// the concept does not exist.
    pub secure_boot_enabled: Option<bool>,
    /// SMBIOS specification version, e.g. `"3.4"`.
    pub smbios_version: Option<String>,
}

/// The enclosure the machine lives in.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chassis {
    /// Case vendor.
    pub manufacturer: Option<String>,
    /// e.g. `"Desktop"`, `"Notebook"`, `"Mini PC"`.
    pub kind: Option<String>,
    /// Case revision.
    pub version: Option<String>,
    /// *Identifying.*
    pub serial: Option<String>,
}

/// Whole-machine identity as the vendor stamped it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemIdentity {
    /// System vendor, e.g. `"Dell Inc."`, `"Apple Inc."`.
    pub manufacturer: Option<String>,
    /// e.g. `"MacBookPro18,3"`, `"XPS 15 9520"`.
    pub product: Option<String>,
    /// System revision.
    pub version: Option<String>,
    /// Product family, e.g. `"ThinkPad T14"`.
    pub family: Option<String>,
    /// *Identifying.* Vendor order code.
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
/// Operating system and kernel.
pub struct Os {
    /// e.g. `"Windows"`, `"Ubuntu"`, `"Darwin"`.
    pub name: String,
    /// `"windows"`, `"linux"`, `"macos"`, or the raw `target_os`.
    pub family: String,
    /// Marketing version, e.g. `"11"`, `"24.04"`, `"14.4.1"`.
    pub version: String,
    /// Full version string as the OS reports it, e.g. `"Windows 11 Pro"`.
    pub long_version: Option<String>,
    /// Kernel release, e.g. `"6.8.0-40-generic"`.
    pub kernel_version: Option<String>,
    /// Build identifier, e.g. `"26100.1742"` on Windows, `"23F79"` on macOS.
    pub build: Option<String>,
    /// e.g. `"Professional"`.
    pub edition: Option<String>,
    /// Release label or codename, e.g. `"24H2"`, `"noble"`, `"Sonoma"`.
    pub codename: Option<String>,
    /// `/etc/os-release` ID on Linux, e.g. `"ubuntu"`.
    pub distribution_id: Option<String>,
    /// CPU architecture the OS is built for, e.g. `"x86_64"`, `"aarch64"`.
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
