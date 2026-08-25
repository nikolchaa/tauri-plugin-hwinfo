# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [1.0.0-alpha.1]

A complete rewrite. The data contract is different in every section; see
[Migrating](#migrating-from-0x) below and the full
[contract reference](docs/CONTRACT.md).

### Added

**Whole-system scans and section selection**

- `getSystemInfo()` returns every section in one call, with `scan` metadata:
  mode, detail level, duration, timestamp, and warnings.
- `sections` restricts a scan to the subsystems you need. Unrequested sections
  are never collected, not merely filtered out of the response.
- `scan.warnings` explains every `null`: which probe failed, and why —
  privileges, a missing tool, an absent device.

**Detail levels** — `detail: "summary" | "capabilities" | "full"`

- `summary` (default) reads only what is essentially free: CPUID, registry and
  `/sys`, firmware tables, totals. No helper processes, no device probes, no
  sampling delay.
- `capabilities` adds device probes — Vulkan, CUDA, HIP, OpenCL — answering
  "what can this machine run".
- `full` adds itemised inventory and live state: per-core breakdowns, memory
  modules, physical disks, adapter models, CPU load, Direct3D feature levels.
- `scan.detail` is echoed back, so a `null` from _"you didn't ask"_ is always
  distinguishable from a `null` from _"it failed"_.

**Safe and unsafe scan modes** — `mode: "safe" | "unsafe"`

- Safe (default) returns hardware capabilities only. Serial numbers, MAC
  addresses, IP addresses, SMBIOS UUID, machine ID, hostname and username are
  all `null`.
- Unsafe returns them, and requires the host application to opt in with
  `Builder::new().allow_unsafe_scan(true)`. A frontend cannot enable it alone,
  and a request without the opt-in fails rather than silently downgrading.

**New sections** — `storage`, `network`, `display`, `battery`, `board`

- Physical drives and mounted volumes, network interfaces with traffic
  counters, monitors with EDID identity and physical dimensions, battery health
  against design capacity, and motherboard/BIOS/chassis/system identity.

**Compute runtime detection**

- HIP/ROCm: `hip`, `rocmVersion`, `gfxArchitecture`. On Linux this reads the
  `amdkfd` kernel topology — no `rocminfo` or `rocm-smi` process, and no
  dependency on ROCm's userspace being on `PATH`.
- Vulkan enumeration through the loader, contributing driver versions, device
  classes and true heap sizes on all platforms.
- CUDA version and compute capability.
- Direct3D feature levels on Windows.

**Other**

- `scan_blocking()` runs a scan without a Tauri application, for CLI tools and
  tests.
- `cargo run --example dump` prints a full scan as JSON.
- A runnable demo app under `examples/demo`.
- Cargo features `vulkan`, `battery` and `opencl`, all default-on and all
  loaded at runtime — nothing links against Vulkan or OpenCL at build time.

### Changed

- **Breaking:** `getCpuInfo()` returns `Cpu[]`, one entry per physical package,
  instead of a single object.
- **Breaking:** `getGpuInfo()` returns `Gpu[]` — every adapter, not just the
  first one enumerated.
- **Breaking:** `getRamInfo()` is now `getMemoryInfo()` and returns totals, swap,
  slot counts and a DIMM inventory instead of `{ sizeMb }`.
- **Breaking:** `gpu.supportsCuda` / `supportsVulkan` are now `gpu.api.cuda` /
  `gpu.api.vulkan`, with versions alongside.
- **Breaking:** permission identifiers are now `hwinfo:allow-get-*-info`, or
  `hwinfo:default` for all of them.
- **Breaking:** unknown values are `null` rather than `0` or `"Unknown"`.
  Check for `null` where you previously checked for a zero value.
- `maxFrequency` now reports the highest clock either advertised _or observed_.
  No OS reliably publishes a turbo ceiling — Windows and CPUID both hand back
  the base clock — and `baseFrequency` carries the nominal value separately.

### Performance

- Windows reaches WMI through COM directly instead of spawning PowerShell:
  roughly two orders of magnitude faster, and no console window flashes in a
  packaged app.
- Linux uses no helper binaries at all. SMBIOS is parsed from
  `/sys/firmware/dmi/entries` instead of shelling to `dmidecode`, device names
  come from the `pci.ids` database `lspci` itself reads, and virtualisation and
  container detection run off `/proc` and `/sys` instead of
  `systemd-detect-virt`.
- macOS uses `sysctlbyname` instead of spawning `sysctl` and `sw_vers`, Core
  Graphics for displays, and `hw.model` for machine identity. A safe summary
  scan spawns no processes at all, where it previously spawned four.
- WMI connections are reused per namespace for the life of the thread rather
  than reopened per query.
- `Win32_NetworkAdapter`, the slowest class the Windows backend touches, moved
  to the `full` tier and gained a provider-side filter.

On a Core i7-13620H laptop with hybrid Intel/NVIDIA graphics, `capabilities`
costs roughly 1.5× a `summary` scan and `full` roughly 4×. Version 0.2.3 had no
equivalent to `summary` — it always did the equivalent of `full`.

Absolute times are not worth quoting: they swing several-fold with whether the
platform's data providers are already warm. A `summary` scan on the same
machine ranged from ~250 ms to ~2 s across runs, purely from WMI cold-start.
On a laptop with switchable graphics, `full` is additionally dominated by
waking the discrete GPU to read its Direct3D feature level.

### Fixed

- `mobile.rs` did not compile: it assigned `"Unavailable".into()` to a `u32`
  and constructed `GpuInfo` without all its fields. Mobile now runs the
  portable collectors and reports what is missing, instead of returning
  placeholder values.
- The `windows` crate was an unconditional dependency, pulling Windows bindings
  into Linux and macOS builds. It is now target-gated.
- GPU VRAM above 4 GiB was wrong on Windows: `Win32_VideoController::AdapterRAM`
  is a `uint32` and wraps. VRAM now comes from DXGI.
- Only the first GPU was ever enumerated. All adapters are now returned.
- `supportsCuda` was inferred from the model name containing `"RTX"` or
  `"GTX"`, and `supportsVulkan` from a DLL existing on disk. Both are now real
  probes.
- `opencl` was hardcoded — `true` on macOS, `false` everywhere else. It now
  loads the ICD loader and calls `clGetPlatformIDs`.
- Memory form factors were decoded on Windows with the SMBIOS table rather than
  the CIM one WMI actually uses, labelling every laptop's SODIMMs as `RIMM`.
- Firmware placeholder strings (`"To Be Filled By O.E.M."`, `"Default string"`,
  `"No Asset Tag"`, `"Tag 12345"`, runs of zeroes) are now normalised to `null`
  instead of surfacing as data.
- `starship-battery` was excluded from iOS, which its Darwin backend supports.
  It is now enabled there; only Android, which has no backend, is excluded.

### Documentation

- Every public item is documented, enforced by `#![warn(missing_docs)]`.
- [`docs/CONTRACT.md`](docs/CONTRACT.md) is a complete field reference: type,
  required detail level and scan mode, and per-platform availability.
- The README covers detail levels, scan modes, platform behaviour and
  migration.

### Known limitations

- Only Windows has been executed against real hardware. The Linux and macOS
  backends type-check and their pure parsing is unit-tested, but they have not
  been run.
- `gpu.api.hipVersion` and `openglVersion` are reserved and always `null`.
- `gfxArchitecture` is Linux-only; Windows has no unprivileged equivalent of
  the `amdkfd` topology.
- CPU temperature needs elevation on Windows and is frequently unavailable
  regardless, because it reports an ACPI thermal zone rather than the die.
- Shipping to the iOS App Store needs `sysinfo`'s `apple-app-store` feature,
  which is not yet exposed here.

### Migrating from 0.x

| Before                                | Now                                              |
| ------------------------------------- | ------------------------------------------------ |
| `getCpuInfo() → CpuInfo`              | `getCpuInfo() → Cpu[]`                           |
| `getGpuInfo() → GpuInfo`              | `getGpuInfo() → Gpu[]`                           |
| `getRamInfo() → { sizeMb }`           | `getMemoryInfo() → Memory`                       |
| `getOsInfo() → { name, version }`     | `getOsInfo() → Os`                               |
| `gpu.supportsCuda` / `supportsVulkan` | `gpu.api.cuda` / `gpu.api.vulkan`                |
| `hwinfo:allow-cpu-info`               | `hwinfo:allow-get-cpu-info`, or `hwinfo:default` |
| —                                     | `getSystemInfo()` for everything at once         |

The default `detail: "summary"` deliberately omits per-core, per-DIMM and
per-disk detail along with the GPU API probes. Pass `{ detail: "full" }` for
everything the plugin can find.

## [0.2.3]

Fixes for GPU detail output on Linux.

## [0.2.2]

Updated the CPU info retrieval method for Windows.
