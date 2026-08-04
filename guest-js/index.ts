import { invoke } from "@tauri-apps/api/core";

/**
 * How deep a scan is allowed to reach.
 *
 * `"safe"` reports hardware capabilities only. `"unsafe"` additionally returns
 * per-unit identifiers - serial numbers, MAC addresses, UUIDs, hostname - which
 * together form a stable device fingerprint.
 *
 * `"unsafe"` must be enabled on the Rust side with
 * `Builder::new().allow_unsafe_scan(true)`; otherwise the call rejects.
 */
export type ScanMode = "safe" | "unsafe";

/**
 * How much work a scan is allowed to do. Orthogonal to {@link ScanMode}, which
 * governs *identity* rather than *effort*.
 *
 * - `"summary"` - whatever can be read essentially for free: CPUID, the
 *   registry and `/sys`, firmware tables, and process-wide totals. No helper
 *   processes, no device probes, no sampling delay. The default.
 * - `"capabilities"` - adds probes that ask devices what they support: Vulkan
 *   enumeration and CUDA via `nvidia-smi`. Answers "what can this machine run".
 * - `"full"` - adds itemised inventory and live state: per-core breakdowns,
 *   memory modules, physical disks, adapter models, CPU load and Direct3D
 *   feature levels.
 */
export type DetailLevel = "summary" | "capabilities" | "full";

/** A selectable part of {@link SystemInfo}. */
export type Section =
  | "cpu"
  | "gpu"
  | "memory"
  | "storage"
  | "network"
  | "display"
  | "battery"
  | "board"
  | "os";

export interface ScanOptions {
  /** Defaults to `"safe"`. */
  mode?: ScanMode;
  /** Defaults to `"summary"`. */
  detail?: DetailLevel;
  /**
   * Restrict a full-system scan to these sections. Omit for all of them.
   * Ignored by the single-section functions.
   */
  sections?: Section[];
}

/** What ran, how long it took, and what it could not read. */
export interface ScanMeta {
  /** Contract version of this payload. `2` for this release. */
  version: number;
  mode: ScanMode;
  /**
   * Echoed back so you can tell the two kinds of `null` apart: a field omitted
   * because of the detail level, versus one that failed to read and is
   * explained in {@link ScanMeta.warnings}.
   */
  detail: DetailLevel;
  /** Sections actually collected. */
  sections: Section[];
  durationMs: number;
  /** Unix timestamp in seconds. */
  timestamp: number;
  /**
   * Non-fatal problems: probes that were unavailable, needed privileges, or
   * returned nothing. A `null` field is usually explained here.
   */
  warnings: string[];
}

/**
 * Everything, or the subset named in {@link ScanOptions.sections}.
 * Sections that were not requested are `null`.
 */
export interface SystemInfo {
  scan: ScanMeta;
  cpu: Cpu[] | null;
  gpu: Gpu[] | null;
  memory: Memory | null;
  storage: Storage | null;
  network: NetworkInterface[] | null;
  display: Display[] | null;
  battery: Battery[] | null;
  board: Board | null;
  os: Os | null;
}

// ---------------------------------------------------------------------------
// CPU
// ---------------------------------------------------------------------------

/** One physical processor package. */
export interface Cpu {
  /** CPUID vendor string, e.g. `"AuthenticAMD"`. */
  manufacturer: string;
  /** e.g. `"AMD Ryzen 9 5900X 12-Core Processor"`. */
  model: string;
  /** e.g. `"x86_64"`, `"aarch64"`. */
  architecture: string;
  physicalCores: number | null;
  /** Logical processors (hardware threads). */
  threads: number;
  /** Base (nominal) clock in MHz. */
  baseFrequency: number | null;
  /**
   * Highest clock in MHz either advertised by firmware or observed during the
   * scan. No OS reliably publishes the turbo ceiling, so a core caught
   * boosting can raise this above the advertised figure.
   */
  maxFrequency: number;
  currentFrequency: number | null;
  /** e.g. `"AM4"`, `"LGA1700"`. */
  socket: string | null;
  family: number | null;
  modelId: number | null;
  stepping: number | null;
  microcode: string | null;
  cache: CpuCache;
  /** Upper-case and sorted, e.g. `["AVX", "AVX2", "SSE4.2"]`. */
  features: string[];
  /** Whether hardware virtualisation (VT-x / AMD-V) is exposed. */
  virtualization: boolean | null;
  /** Hypervisor vendor when running in a VM, e.g. `"KVM"`. */
  hypervisor: string | null;
  /** Whether SMT / Hyper-Threading is active. */
  simultaneousMultithreading: boolean | null;
  /**
   * Package-wide load, 0–100. Requires `detail: "full"` - measuring load costs
   * a sampling interval, the largest fixed cost in this section.
   */
  usagePercent: number | null;
  /** Requires `detail: "full"`. */
  temperatureC: number | null;
  /**
   * Per-logical-processor detail. Requires `detail: "full"` - this array
   * scales with core count and dominates the payload on large machines.
   * Empty otherwise.
   */
  cores: CpuCore[];
  /** Identifying. `null` unless the scan ran in `"unsafe"` mode. */
  serial: string | null;
}

export interface CpuCache {
  l1dKb: number | null;
  l1iKb: number | null;
  l2Kb: number | null;
  l3Kb: number | null;
}

export interface CpuCore {
  /** Platform label, e.g. `"cpu0"`. */
  id: string;
  usagePercent: number | null;
  frequency: number | null;
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

export type GpuKind =
  | "discrete"
  | "integrated"
  | "virtual"
  /** A software rasteriser presenting itself as a GPU (llvmpipe, WARP). */
  | "cpu"
  | "unknown";

export interface Gpu {
  /** e.g. `"Advanced Micro Devices, Inc."`. */
  manufacturer: string;
  /** e.g. `"AMD Radeon RX 6950 XT"`. */
  model: string;
  kind: GpuKind;
  /** PCI vendor ID as an integer, e.g. `4098`. */
  vendorId: number | null;
  /** PCI vendor ID as `"0x1002"`. */
  vendorIdHex: string | null;
  deviceId: number | null;
  deviceIdHex: string | null;
  subsystemId: string | null;
  revision: number | null;
  /** Memory reserved exclusively for this adapter. */
  vramMb: number | null;
  /** System memory the adapter may borrow. */
  sharedMemoryMb: number | null;
  driverVersion: string | null;
  /** ISO-8601 date, e.g. `"2024-03-11"`. */
  driverDate: string | null;
  /** PCI address, e.g. `"0000:0a:00.0"`. */
  pciBus: string | null;
  /** Current output mode, when the adapter drives a display. */
  currentResolution: Resolution | null;
  api: GpuApiSupport;
  /** Identifying. `null` unless the scan ran in `"unsafe"` mode. */
  uuid: string | null;
}

/**
 * Which graphics and compute APIs the adapter supports.
 *
 * The Vulkan and CUDA fields require `detail: "capabilities"` or higher - they
 * are live device probes, not table lookups. Without them `vulkan` and `cuda`
 * read `false`, which means *not probed*, not *not supported*; check
 * `scan.detail` to tell the difference.
 */
export interface GpuApiSupport {
  vulkan: boolean;
  /** e.g. `"1.3.280"`. */
  vulkanVersion: string | null;
  /** e.g. `"AMD proprietary driver"`. */
  vulkanDriver: string | null;
  cuda: boolean;
  /** e.g. `"12.4"`. */
  cudaVersion: string | null;
  /** e.g. `"8.6"`. */
  computeCapability: string | null;
  /**
   * Highest Direct3D feature level, e.g. `"12_1"`. Windows only, and requires
   * `detail: "full"` - probing it creates a real D3D11 device, which on a
   * hybrid laptop wakes a sleeping discrete GPU and can take seconds.
   */
  directxFeatureLevel: string | null;
  metal: boolean;
  opencl: boolean;
  openglVersion: string | null;
}

export interface Resolution {
  width: number;
  height: number;
  refreshRateHz: number | null;
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

export interface Memory {
  totalMb: number;
  availableMb: number;
  usedMb: number;
  swapTotalMb: number;
  swapUsedMb: number;
  /**
   * Physical DIMMs. Requires `detail: "full"`; empty otherwise.
   *
   * Also empty when SMBIOS is unreadable - per-DIMM detail needs root on
   * Linux, and Apple silicon has no DIMMs at all.
   */
  modules: MemoryModule[];
  /** Available at every detail level: a much cheaper query than `modules`. */
  slotsTotal: number | null;
  slotsUsed: number | null;
}

export interface MemoryModule {
  /** Device locator, e.g. `"DIMM 0"`. */
  slot: string | null;
  /** Bank locator, e.g. `"P0 CHANNEL A"`. */
  bank: string | null;
  manufacturer: string | null;
  partNumber: string | null;
  capacityMb: number | null;
  /** Rated speed in MT/s. */
  speedMts: number | null;
  /** Speed the module is actually running at, in MT/s. */
  configuredSpeedMts: number | null;
  /** e.g. `"DDR4"`, `"DDR5"`, `"LPDDR5"`. */
  memoryType: string | null;
  /** e.g. `"DIMM"`, `"SODIMM"`. */
  formFactor: string | null;
  voltageMv: number | null;
  rank: number | null;
  dataWidthBits: number | null;
  totalWidthBits: number | null;
  /** Identifying. `null` unless the scan ran in `"unsafe"` mode. */
  serial: string | null;
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/**
 * Physical drives and mounted volumes, reported separately because the mapping
 * between them is not reliably recoverable on every platform.
 */
export interface Storage {
  /** Requires `detail: "full"`; empty otherwise. */
  disks: Disk[];
  /** Available at every detail level. */
  volumes: Volume[];
}

export type DiskKind = "hdd" | "ssd" | "unknown";

export interface Disk {
  /** e.g. `"/dev/nvme0n1"`, `"\\\\.\\PHYSICALDRIVE0"`. */
  device: string;
  model: string | null;
  vendor: string | null;
  kind: DiskKind;
  /** e.g. `"NVMe"`, `"SATA"`, `"USB"`. */
  bus: string | null;
  sizeMb: number | null;
  firmwareRevision: string | null;
  isRemovable: boolean | null;
  /** e.g. `"GPT"`, `"MBR"`. */
  partitionTable: string | null;
  partitionCount: number | null;
  /** Identifying. `null` unless the scan ran in `"unsafe"` mode. */
  serial: string | null;
}

export interface Volume {
  /** e.g. `"C:\\"`, `"/"`, `"/home"`. */
  mountPoint: string;
  name: string | null;
  /** e.g. `"NTFS"`, `"ext4"`, `"apfs"`. */
  fileSystem: string | null;
  totalMb: number;
  availableMb: number;
  usedMb: number;
  isRemovable: boolean;
  isReadOnly: boolean;
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

export type InterfaceState =
  | "up"
  | "down"
  | "dormant"
  | "testing"
  | "lowerLayerDown"
  | "notPresent"
  | "unknown";

export interface NetworkInterface {
  name: string;
  /**
   * e.g. `"Intel(R) Wi-Fi 6 AX200"`. Requires `detail: "full"` - on Windows
   * this is the single slowest query in the whole scan.
   */
  description: string | null;
  state: InterfaceState;
  mtu: number | null;
  /** Link speed in Mb/s. Requires `detail: "full"`. */
  speedMbps: number | null;
  /** Bytes since boot. */
  totalReceived: number;
  totalTransmitted: number;
  packetsReceived: number;
  packetsTransmitted: number;
  errorsReceived: number;
  errorsTransmitted: number;
  /** Identifying. `null` unless the scan ran in `"unsafe"` mode. */
  macAddress: string | null;
  /**
   * Identifying. CIDR notation, e.g. `["192.168.1.42/24"]`.
   * `null` unless the scan ran in `"unsafe"` mode.
   */
  ipNetworks: string[] | null;
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

export interface Display {
  /** Adapter-assigned name, e.g. `"\\\\.\\DISPLAY1"`, `"eDP-1"`. */
  name: string | null;
  /** Decoded from EDID, e.g. `"Dell Inc."`. */
  manufacturer: string | null;
  /** Product name from EDID, e.g. `"DELL U2720Q"`. */
  model: string | null;
  /** Current mode. */
  resolution: Resolution;
  /** Native panel resolution, when it differs from the current mode. */
  nativeResolution: Resolution | null;
  /** DPI scaling applied by the OS, e.g. `1.5`. */
  scaleFactor: number;
  positionX: number;
  positionY: number;
  isPrimary: boolean;
  isInternal: boolean | null;
  bitsPerPixel: number | null;
  physicalWidthMm: number | null;
  physicalHeightMm: number | null;
  /** Derived from the physical dimensions. */
  diagonalInches: number | null;
  manufactureYear: number | null;
  /** Identifying. `null` unless the scan ran in `"unsafe"` mode. */
  serial: string | null;
}

// ---------------------------------------------------------------------------
// Battery
// ---------------------------------------------------------------------------

export type BatteryState =
  | "charging"
  | "discharging"
  | "empty"
  | "full"
  | "unknown";

export interface Battery {
  vendor: string | null;
  model: string | null;
  /** e.g. `"lithium-ion"`. */
  technology: string | null;
  state: BatteryState;
  /** Current charge, 0–100. */
  chargePercent: number;
  /**
   * Remaining capacity relative to design capacity, 0–100.
   * Below ~80 usually means the pack is worn.
   */
  healthPercent: number | null;
  energyWh: number | null;
  energyFullWh: number | null;
  energyFullDesignWh: number | null;
  /** Positive while charging, negative while discharging. */
  energyRateW: number | null;
  voltageV: number | null;
  temperatureC: number | null;
  cycleCount: number | null;
  secondsToFull: number | null;
  secondsToEmpty: number | null;
  /** Identifying. `null` unless the scan ran in `"unsafe"` mode. */
  serial: string | null;
}

// ---------------------------------------------------------------------------
// Board
// ---------------------------------------------------------------------------

export interface Board {
  manufacturer: string | null;
  /** Board model, e.g. `"X570 AORUS ELITE"`. */
  product: string | null;
  version: string | null;
  /** Identifying. `null` unless the scan ran in `"unsafe"` mode. */
  serial: string | null;
  /** Identifying. `null` unless the scan ran in `"unsafe"` mode. */
  assetTag: string | null;
  bios: Bios;
  chassis: Chassis;
  system: SystemIdentity;
}

export interface Bios {
  vendor: string | null;
  version: string | null;
  /** ISO-8601 date, e.g. `"2024-01-30"`. */
  releaseDate: string | null;
  mode: "UEFI" | "Legacy" | null;
  secureBootEnabled: boolean | null;
  /** SMBIOS specification version, e.g. `"3.4"`. */
  smbiosVersion: string | null;
}

export interface Chassis {
  manufacturer: string | null;
  /** e.g. `"Desktop"`, `"Notebook"`, `"Mini PC"`. */
  kind: string | null;
  version: string | null;
  /** Identifying. `null` unless the scan ran in `"unsafe"` mode. */
  serial: string | null;
}

/** Whole-machine identity as the vendor stamped it. */
export interface SystemIdentity {
  manufacturer: string | null;
  /** e.g. `"MacBookPro18,3"`, `"XPS 15 9520"`. */
  product: string | null;
  version: string | null;
  family: string | null;
  /** Identifying. `null` unless the scan ran in `"unsafe"` mode. */
  sku: string | null;
  /** Identifying. SMBIOS system UUID. `null` unless in `"unsafe"` mode. */
  uuid: string | null;
  /** Identifying. `null` unless the scan ran in `"unsafe"` mode. */
  serial: string | null;
}

// ---------------------------------------------------------------------------
// OS
// ---------------------------------------------------------------------------

export interface Os {
  /** e.g. `"Windows"`, `"Ubuntu"`, `"Darwin"`. */
  name: string;
  family: "windows" | "linux" | "macos" | string;
  /** Marketing version, e.g. `"11"`, `"24.04"`, `"14.4.1"`. */
  version: string;
  longVersion: string | null;
  kernelVersion: string | null;
  /** e.g. `"22631.3447"`. */
  build: string | null;
  /** e.g. `"Professional"`. */
  edition: string | null;
  /** e.g. `"noble"`, `"24H2"`. */
  codename: string | null;
  /** `/etc/os-release` ID on Linux, e.g. `"ubuntu"`. */
  distributionId: string | null;
  architecture: string;
  uptimeSecs: number;
  /** Unix timestamp of the last boot. */
  bootTimeSecs: number;
  /** Detected hypervisor when running virtualised, e.g. `"KVM"`. */
  virtualization: string | null;
  /** Identifying. `null` unless the scan ran in `"unsafe"` mode. */
  hostname: string | null;
  /** Identifying. `null` unless the scan ran in `"unsafe"` mode. */
  machineId: string | null;
  /** Identifying. `null` unless the scan ran in `"unsafe"` mode. */
  user: string | null;
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/**
 * Scan every section, or just the ones named in `options.sections`.
 *
 * This is the only call that returns {@link ScanMeta}, so it is the one to use
 * when you want the warnings explaining any `null` fields.
 */
export async function getSystemInfo(
  options?: ScanOptions,
): Promise<SystemInfo> {
  return await invoke("plugin:hwinfo|get_system_info", { options });
}

export async function getCpuInfo(options?: ScanOptions): Promise<Cpu[]> {
  return await invoke("plugin:hwinfo|get_cpu_info", { options });
}

export async function getGpuInfo(options?: ScanOptions): Promise<Gpu[]> {
  return await invoke("plugin:hwinfo|get_gpu_info", { options });
}

export async function getMemoryInfo(options?: ScanOptions): Promise<Memory> {
  return await invoke("plugin:hwinfo|get_memory_info", { options });
}

export async function getStorageInfo(options?: ScanOptions): Promise<Storage> {
  return await invoke("plugin:hwinfo|get_storage_info", { options });
}

export async function getNetworkInfo(
  options?: ScanOptions,
): Promise<NetworkInterface[]> {
  return await invoke("plugin:hwinfo|get_network_info", { options });
}

export async function getDisplayInfo(
  options?: ScanOptions,
): Promise<Display[]> {
  return await invoke("plugin:hwinfo|get_display_info", { options });
}

export async function getBatteryInfo(
  options?: ScanOptions,
): Promise<Battery[]> {
  return await invoke("plugin:hwinfo|get_battery_info", { options });
}

export async function getBoardInfo(options?: ScanOptions): Promise<Board> {
  return await invoke("plugin:hwinfo|get_board_info", { options });
}

export async function getOsInfo(options?: ScanOptions): Promise<Os> {
  return await invoke("plugin:hwinfo|get_os_info", { options });
}
