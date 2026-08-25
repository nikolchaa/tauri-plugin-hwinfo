# Contract reference — v1

Every field the plugin can return, what populates it, and when it is `null`.

The canonical definitions live in [`guest-js/index.ts`](../guest-js/index.ts)
and the Rust types in [`src/models.rs`](../src/models.rs); both carry the same
documentation inline. This page is the flat, searchable view.

## Reading the tables

| Column      | Meaning                                                                                                     |
| ----------- | ----------------------------------------------------------------------------------------------------------- |
| **Detail**  | Minimum `detail` level. `—` means available at every level, including `summary`.                              |
| **Mode**    | `unsafe` marks *identifying* fields, `null` unless the scan ran in unsafe mode **and** the app opted in.       |
| **Windows / Linux / macOS** | ✅ populated · ⚠️ conditional (see notes) · ❌ never populated on that platform.                |

Two rules hold everywhere:

1. **`null` never means zero.** A field that could not be read is `null`, and
   the reason is in `scan.warnings`. A field omitted because of the detail level
   is also `null` — `scan.detail` is echoed back so you can tell which.
2. **Sizes are mebibytes** (`*Mb`), **clocks are MHz**, **memory speeds are
   MT/s**, **dates are ISO-8601**.

---

## `scan` — `ScanMeta`

Always present.

| Field        | Type              | Notes                                                        |
| ------------ | ----------------- | ------------------------------------------------------------ |
| `version`    | `number`          | Contract version. `1` for this release.                      |
| `mode`       | `"safe"\|"unsafe"` | Echoed back.                                                 |
| `detail`     | `"summary"\|"capabilities"\|"full"` | Echoed back. Tells you why a field is `null`. |
| `sections`   | `Section[]`       | Sections actually collected.                                 |
| `durationMs` | `number`          | Wall-clock duration.                                         |
| `timestamp`  | `number`          | Unix seconds at completion.                                  |
| `warnings`   | `string[]`        | Probes that failed, needed privileges, or returned nothing.  |

## `cpu[]` — `Cpu`

One entry per physical package.

| Field | Type | Detail | Mode | Win | Linux | macOS | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `manufacturer` | `string` | — | | ✅ | ✅ | ✅ | CPUID vendor, e.g. `"AuthenticAMD"`. Apple silicon reports `"Apple Inc."`. |
| `model` | `string` | — | | ✅ | ✅ | ✅ | Marketing name. |
| `architecture` | `string` | — | | ✅ | ✅ | ✅ | `"x86_64"`, `"aarch64"`. |
| `physicalCores` | `number?` | — | | ✅ | ✅ | ✅ | |
| `threads` | `number` | — | | ✅ | ✅ | ✅ | Logical processors. |
| `baseFrequency` | `number?` | — | | ✅ | ⚠️ | ⚠️ | Linux needs `intel_pstate`. Absent on Apple silicon. |
| `maxFrequency` | `number` | — | | ✅ | ✅ | ⚠️ | **Highest advertised *or observed*.** No OS publishes a turbo ceiling. |
| `currentFrequency` | `number?` | — | | ✅ | ✅ | ⚠️ | |
| `socket` | `string?` | `full` | | ✅ | ⚠️ | ❌ | Linux: SMBIOS, root-only. Macs have no socket. |
| `family` / `modelId` / `stepping` | `number?` | — | | ✅ | ✅ | ✅ | CPUID. x86 only. |
| `microcode` | `string?` | — | | ✅ | ✅ | ⚠️ | Intel Macs only. |
| `cache.l1dKb` / `l1iKb` / `l2Kb` / `l3Kb` | `number?` | — | | ✅ | ✅ | ✅ | KiB. L1/L2 summed per package. |
| `features[]` | `string[]` | — | | ✅ | ✅ | ✅ | Upper-case, sorted. Empty on non-x86. |
| `virtualization` | `boolean?` | — | | ✅ | ✅ | ⚠️ | `null` on Apple silicon: absence is not a negative. |
| `hypervisor` | `string?` | — | | ✅ | ✅ | ✅ | CPUID vendor when virtualised. |
| `simultaneousMultithreading` | `boolean?` | — | | ✅ | ✅ | ✅ | |
| `usagePercent` | `number?` | `full` | | ✅ | ✅ | ✅ | Costs a sampling interval. |
| `temperatureC` | `number?` | `full` | | ⚠️ | ✅ | ⚠️ | Windows needs elevation and an ACPI zone. |
| `cores[]` | `CpuCore[]` | `full` | | ✅ | ✅ | ✅ | `id`, `usagePercent`, `frequency`. Empty below `full`. |
| `serial` | `string?` | — | `unsafe` | ✅ | ❌ | ❌ | Disabled in hardware since the Pentium III on Linux/macOS. |

## `gpu[]` — `Gpu`

One entry per adapter, including integrated and software ones.

| Field | Type | Detail | Mode | Win | Linux | macOS | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `manufacturer` / `model` | `string` | — | | ✅ | ⚠️ | ✅ | Linux needs `pci.ids` or Vulkan for a name. |
| `kind` | `GpuKind` | ⚠️ | | ✅ | ✅ | ✅ | `discrete`/`integrated`/`virtual`/`cpu`/`unknown`. Accurate only at `capabilities`+, where Vulkan corrects the heap-size guess. |
| `vendorId` / `vendorIdHex` | `number?` / `string?` | — | | ✅ | ✅ | ⚠️ | |
| `deviceId` / `deviceIdHex` | `number?` / `string?` | — | | ✅ | ✅ | ⚠️ | |
| `subsystemId` / `revision` | `string?` / `number?` | — | | ✅ | ✅ | ⚠️ | |
| `vramMb` | `number?` | — | | ✅ | ⚠️ | ⚠️ | Windows uses DXGI, not the broken WMI `AdapterRAM`. Linux: amdgpu/i915 only until Vulkan or `nvidia-smi` fills in. |
| `sharedMemoryMb` | `number?` | — | | ✅ | ⚠️ | ⚠️ | Linux: amdgpu's GTT aperture. |
| `driverVersion` | `string?` | — | | ✅ | ⚠️ | ❌ | Linux: out-of-tree modules only, or via Vulkan. |
| `driverDate` | `string?` | — | | ✅ | ❌ | ❌ | |
| `pciBus` | `string?` | — | | ⚠️ | ✅ | ⚠️ | Windows needs `VK_EXT_pci_bus_info`. |
| `currentResolution` | `Resolution?` | — | | ✅ | ❌ | ❌ | |
| `uuid` | `string?` | `capabilities` | `unsafe` | ✅ | ✅ | ⚠️ | From Vulkan. |

Adapters are enumerated from DRM nodes; any display-class PCI device no driver
has bound (vfio-pci passthrough, missing driver) is still listed with
identifiers only, on every distro that mounts sysfs.

### `gpu[].api` — `GpuApiSupport`

**All of these require `detail: "capabilities"`.** Below that the booleans read
`false` meaning *not probed*, not *not supported*.

| Field | Type | Win | Linux | macOS | Notes |
| --- | --- | --- | --- | --- | --- |
| `vulkan` | `boolean` | ✅ | ✅ | ⚠️ | macOS needs MoltenVK installed. |
| `vulkanVersion` | `string?` | ✅ | ✅ | ⚠️ | e.g. `"1.3.280"`. |
| `vulkanDriver` | `string?` | ✅ | ✅ | ⚠️ | e.g. `"AMD proprietary driver (2.0.324)"`. |
| `cuda` | `boolean` | ✅ | ✅ | ❌ | Needs `nvidia-smi`; without it, falls back to detecting `libcuda.so`. WSL2 supported. |
| `cudaVersion` | `string?` | ✅ | ✅ | ❌ | e.g. `"12.4"`. Needs `nvidia-smi`. |
| `computeCapability` | `string?` | ✅ | ✅ | ❌ | e.g. `"8.6"`. Needs `nvidia-smi`. |
| `hip` | `boolean` | ✅ | ✅ | ❌ | ROCm does not target macOS. Linux finds the runtime in `/opt/rocm`, `/usr/lib64` and multiarch dirs. |
| `hipVersion` | `string?` | ❌ | ✅ | ❌ | Linux: the runtime library's own soname (`libamdhip64.so.6.2.41134`). |
| `rocmVersion` | `string?` | ⚠️ | ✅ | ❌ | Windows: from the `HIP_PATH` install path. Linux: `/opt/rocm*/.info/version`, else derived from the HIP runtime version. |
| `gfxArchitecture` | `string?` | ❌ | ✅ | ❌ | e.g. `"gfx1100"`. From `amdkfd` topology; Windows has no equivalent. |
| `directxFeatureLevel` | `string?` | ✅ **(`full`)** | ❌ | ❌ | e.g. `"12_1"`. Creates a D3D11 device — wakes a sleeping dGPU. |
| `metal` | `boolean` | ❌ | ❌ | ✅ | Follows from the target, not a probe. |
| `opencl` | `boolean` | ✅ | ✅ | ✅ | Real `clGetPlatformIDs` call. |
| `openclVersion` | `string?` | ✅ | ✅ | ✅ | e.g. `"3.0"`. |
| `openglVersion` | `string?` | ❌ | ❌ | ❌ | Reserved; not yet probed on any platform. |

## `memory` — `Memory`

| Field | Type | Detail | Mode | Win | Linux | macOS | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `totalMb` / `availableMb` / `usedMb` | `number` | — | | ✅ | ✅ | ✅ | |
| `swapTotalMb` / `swapUsedMb` | `number` | — | | ✅ | ✅ | ✅ | Page file on Windows. |
| `slotsTotal` | `number?` | — | | ✅ | ⚠️ | ❌ | Linux: SMBIOS, root-only. Cheaper than `modules`, so offered at every level. |
| `slotsUsed` | `number?` | `full` | | ✅ | ⚠️ | ❌ | |
| `modules[]` | `MemoryModule[]` | `full` | | ✅ | ⚠️ | ⚠️ | Linux: root-only. Apple silicon has no DIMMs. |

### `memory.modules[]` — `MemoryModule`

All require `detail: "full"`.

| Field | Type | Notes |
| --- | --- | --- |
| `slot` / `bank` | `string?` | Device and bank locator. |
| `manufacturer` / `partNumber` | `string?` | |
| `capacityMb` | `number?` | |
| `speedMts` | `number?` | Rated. |
| `configuredSpeedMts` | `number?` | Actual — lower when XMP/EXPO is off. |
| `memoryType` | `string?` | `"DDR5"`, `"LPDDR5"`. |
| `formFactor` | `string?` | `"DIMM"`, `"SODIMM"`. |
| `voltageMv` / `rank` | `number?` | |
| `dataWidthBits` / `totalWidthBits` | `number?` | 64 vs 72 indicates ECC. |
| `serial` | `string?` | *Identifying.* |

## `storage` — `Storage`

| Field | Type | Detail | Notes |
| --- | --- | --- | --- |
| `volumes[]` | `Volume[]` | — | Mounted filesystems. Always available. |
| `disks[]` | `Disk[]` | `full` | Physical drives. Empty below `full`. |

### `storage.volumes[]` — `Volume`

| Field | Type | Notes |
| --- | --- | --- |
| `mountPoint` | `string` | `"C:\\"`, `"/"`. |
| `name` | `string?` | Underlying device. |
| `fileSystem` | `string?` | `"NTFS"`, `"ext4"`, `"apfs"`. |
| `totalMb` / `availableMb` / `usedMb` | `number` | |
| `isRemovable` / `isReadOnly` | `boolean` | |

### `storage.disks[]` — `Disk`

All require `detail: "full"`.

| Field | Type | Win | Linux | macOS | Notes |
| --- | --- | --- | --- | --- | --- |
| `device` | `string` | ✅ | ✅ | ✅ | `"\\\\.\\PHYSICALDRIVE0"`, `"/dev/nvme0n1"`. |
| `model` / `vendor` | `string?` | ✅ | ✅ | ✅ | |
| `kind` | `DiskKind` | ✅ | ✅ | ✅ | `hdd`/`ssd`/`unknown`. |
| `bus` | `string?` | ✅ | ✅ | ✅ | `"NVMe"`, `"SATA"`, `"USB"`. |
| `sizeMb` | `number?` | ✅ | ✅ | ✅ | Raw capacity. |
| `firmwareRevision` | `string?` | ✅ | ✅ | ✅ | |
| `isRemovable` | `boolean?` | ✅ | ✅ | ✅ | |
| `partitionTable` | `string?` | ❌ | ❌ | ✅ | `"GPT"`, `"MBR"`. |
| `partitionCount` | `number?` | ✅ | ✅ | ✅ | |
| `serial` | `string?` | ✅ | ⚠️ | ✅ | *Identifying.* |

## `network[]` — `NetworkInterface`

| Field | Type | Detail | Mode | Notes |
| --- | --- | --- | --- | --- |
| `name` | `string` | — | | Interface key, e.g. `"eth0"`, `"Wi-Fi"`. |
| `state` | `InterfaceState` | — | | `up`/`down`/`dormant`/`testing`/`lowerLayerDown`/`notPresent`/`unknown`. |
| `mtu` | `number?` | — | | |
| `totalReceived` / `totalTransmitted` | `number` | — | | Bytes since boot. |
| `packetsReceived` / `packetsTransmitted` | `number` | — | | |
| `errorsReceived` / `errorsTransmitted` | `number` | — | | |
| `description` | `string?` | `full` | | Adapter model. Windows' slowest single query. |
| `speedMbps` | `number?` | `full` | | Link speed. |
| `macAddress` | `string?` | — | `unsafe` | |
| `ipNetworks` | `string[]?` | — | `unsafe` | CIDR, e.g. `["192.168.1.42/24"]`. |

## `display[]` — `Display`

| Field | Type | Mode | Win | Linux | macOS | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `name` | `string?` | | ✅ | ✅ | ⚠️ | `"\\\\.\\DISPLAY1"`, `"eDP-1"`. |
| `manufacturer` / `model` | `string?` | | ✅ | ✅ | ⚠️ | Decoded from EDID. |
| `resolution` | `Resolution` | | ✅ | ✅ | ✅ | Current mode. |
| `nativeResolution` | `Resolution?` | | ✅ | ✅ | ✅ | Only when it differs. |
| `scaleFactor` | `number` | | ✅ | ✅ | ✅ | From the Tauri runtime. |
| `positionX` / `positionY` | `number` | | ✅ | ✅ | ✅ | |
| `isPrimary` | `boolean` | | ✅ | ✅ | ✅ | |
| `isInternal` | `boolean?` | | ⚠️ | ✅ | ✅ | |
| `bitsPerPixel` | `number?` | | ✅ | ❌ | ❌ | |
| `physicalWidthMm` / `physicalHeightMm` | `number?` | | ✅ | ✅ | ✅ | |
| `diagonalInches` | `number?` | | ✅ | ✅ | ✅ | Derived. |
| `manufactureYear` | `number?` | | ✅ | ✅ | ❌ | |
| `serial` | `string?` | `unsafe` | ✅ | ✅ | ✅ | EDID serial. |

> The display section needs a windowing runtime, so
> [`scan_blocking`](../src/lib.rs) returns it empty.

## `battery[]` — `Battery`

Empty on a desktop. Requires the `battery` cargo feature.

| Field | Type | Mode | Notes |
| --- | --- | --- | --- |
| `vendor` / `model` / `technology` | `string?` | | `"lithium-ion"`. |
| `state` | `BatteryState` | | `charging`/`discharging`/`empty`/`full`/`unknown`. |
| `chargePercent` | `number` | | 0–100. |
| `healthPercent` | `number?` | | Against design capacity. Below ~80 means worn. |
| `energyWh` / `energyFullWh` / `energyFullDesignWh` | `number?` | | |
| `energyRateW` | `number?` | | Positive charging, negative discharging. |
| `voltageV` / `temperatureC` | `number?` | | |
| `cycleCount` | `number?` | | |
| `secondsToFull` / `secondsToEmpty` | `number?` | | |
| `serial` | `string?` | `unsafe` | |

## `board` — `Board`

| Field | Type | Mode | Win | Linux | macOS | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `manufacturer` / `product` / `version` | `string?` | | ✅ | ✅ | ✅ | |
| `serial` / `assetTag` | `string?` | `unsafe` | ✅ | ⚠️ | ⚠️ | Linux: root-only. |
| `bios.vendor` / `.version` | `string?` | | ✅ | ✅ | ✅ | |
| `bios.releaseDate` | `string?` | | ✅ | ✅ | ❌ | |
| `bios.mode` | `string?` | | ✅ | ✅ | ✅ | `"UEFI"` or `"Legacy"`. |
| `bios.secureBootEnabled` | `boolean?` | | ✅ | ⚠️ | ❌ | Linux: efivars, usually root. |
| `bios.smbiosVersion` | `string?` | | ✅ | ❌ | ❌ | |
| `chassis.manufacturer` / `.kind` / `.version` | `string?` | | ✅ | ✅ | ⚠️ | `"Desktop"`, `"Notebook"`. |
| `chassis.serial` | `string?` | `unsafe` | ✅ | ⚠️ | ❌ | |
| `system.manufacturer` / `.product` / `.version` / `.family` | `string?` | | ✅ | ✅ | ✅ | |
| `system.sku` / `.uuid` / `.serial` | `string?` | `unsafe` | ✅ | ⚠️ | ⚠️ | macOS UUID/serial need a `system_profiler` call, so unsafe mode only. |

## `os` — `Os`

| Field | Type | Mode | Win | Linux | macOS | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `name` / `family` / `version` | `string` | | ✅ | ✅ | ✅ | `family` is `windows`/`linux`/`macos`. |
| `longVersion` | `string?` | | ✅ | ✅ | ✅ | |
| `kernelVersion` | `string?` | | ✅ | ✅ | ✅ | |
| `build` | `string?` | | ✅ | ❌ | ✅ | `"26100.1742"`, `"23F79"`. |
| `edition` | `string?` | | ✅ | ⚠️ | ❌ | |
| `codename` | `string?` | | ✅ | ✅ | ❌ | `"24H2"`, `"noble"`. |
| `distributionId` | `string?` | | ✅ | ✅ | ✅ | |
| `architecture` | `string` | | ✅ | ✅ | ✅ | |
| `uptimeSecs` / `bootTimeSecs` | `number` | | ✅ | ✅ | ✅ | |
| `virtualization` | `string?` | | ✅ | ✅ | ⚠️ | Linux also detects Docker/Podman/LXC and WSL. |
| `hostname` | `string?` | `unsafe` | ✅ | ✅ | ✅ | |
| `machineId` | `string?` | `unsafe` | ✅ | ✅ | ✅ | `MachineGuid` / `/etc/machine-id` / hardware UUID. |
| `user` | `string?` | `unsafe` | ✅ | ✅ | ✅ | |

---

## Mobile

Android and iOS fall back to the portable collectors: `cpu` (without CPUID
extras on Arm), `memory` totals, `storage.volumes`, `network`, `os`, and
`battery` on iOS. Everything platform-native is `null`, with a warning naming
what is missing. Shipping to the iOS App Store additionally needs `sysinfo`'s
`apple-app-store` feature, which is **not** wired up here yet.
