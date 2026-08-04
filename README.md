# 🧠 tauri-plugin-hwinfo

![License](https://img.shields.io/github/license/nikolchaa/tauri-plugin-hwinfo?color=blue)
![Crates.io](https://img.shields.io/crates/v/tauri-plugin-hwinfo?color=blue)
![Crates.io Downloads](https://img.shields.io/crates/d/tauri-plugin-hwinfo?color=blue)
![npm](https://img.shields.io/npm/v/tauri-plugin-hwinfo?color=blue)
![npm Downloads](https://img.shields.io/npm/dt/tauri-plugin-hwinfo?color=blue)

Deep hardware and system inspection for Tauri v2 apps - CPU, GPU, memory,
storage, network, displays, batteries, motherboard and OS - from both Rust and
TypeScript.

Get the whole machine in one call, or just the section you need.

📖 **[Full contract reference](docs/CONTRACT.md)** — every field, its type, the
detail level and scan mode it needs, and which platforms populate it.
📋 **[Changelog](CHANGELOG.md)** — what changed in 1.0, and why.

```ts
import { getSystemInfo, getCpuInfo } from "tauri-plugin-hwinfo";

const everything = await getSystemInfo();
const cpus = await getCpuInfo();
```

## 🔧 What it reports

| Section     | Highlights                                                                                                                                                                                              |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cpu[]`     | Per package: vendor, model, socket, physical cores, threads, base/max/current clock, L1–L3 cache, family/model/stepping, microcode, ISA extensions, virtualisation, hypervisor, per-core load and clock |
| `gpu[]`     | Every adapter: vendor and device IDs, discrete/integrated/virtual/software class, VRAM and shared memory, driver version and date, PCI address, plus Vulkan / CUDA / HIP-ROCm / Direct3D / Metal / OpenCL support with versions and `gfx` architecture |
| `memory`    | Totals and swap, plus a physical DIMM inventory: slot, bank, manufacturer, part number, capacity, rated and configured speed, DDR generation, form factor, voltage, widths                              |
| `storage`   | Physical disks (model, SSD/HDD, bus, size, firmware, partitions) and mounted volumes (filesystem, capacity, free space)                                                                                 |
| `network[]` | Per interface: description, operational state, MTU, link speed, byte/packet/error counters                                                                                                              |
| `display[]` | Per monitor: EDID manufacturer and model, current and native resolution, refresh rate, DPI scale, position, physical size and diagonal, year of manufacture                                             |
| `battery[]` | State, charge, health against design capacity, energy and rate, voltage, cycle count, time to full/empty                                                                                                |
| `board`     | Motherboard, BIOS (vendor, version, date, UEFI/Legacy, Secure Boot), chassis type, and the vendor's whole-machine identity                                                                              |
| `os`        | Name, version, build, edition, codename, kernel, architecture, uptime, boot time, detected hypervisor                                                                                                   |

Anything that could not be read is `null` - never `0` or `"Unknown"` - and the
reason lands in `scan.warnings`. Every field is catalogued in the
[contract reference](docs/CONTRACT.md).

## ⚡ Detail levels

Most callers don't need everything, and the expensive parts of a scan are not
spread evenly. `detail` picks how hard the plugin works:

| Level          | Adds                                                                                                                                                      | Cost\*  |
| -------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| `summary`      | Whatever is essentially free: CPUID, registry and `/sys`, firmware tables, totals. No helper processes, no device probes, no sampling delay. **Default.** | 1×     |
| `capabilities` | Device probes: Vulkan, CUDA, HIP, OpenCL. Answers _what can this machine run_.                                                                            | ~1.5×  |
| `full`         | Itemised inventory and live state: per-core breakdowns, memory modules, physical disks, adapter models, CPU load, Direct3D feature levels.                | ~4×    |

\* Relative to `summary`, measured on a Core i7-13620H laptop with hybrid
Intel/NVIDIA graphics. The ratio is the stable part: absolute times swing
several-fold depending on whether the platform's data providers are already
warm — on Windows a `summary` scan ranged from ~250 ms to ~2 s across runs on
the same machine, purely from WMI cold-start. Measure on your own hardware
rather than trusting a single number, and combine with `sections` to cut it
further.

```ts
// "Can this machine run local inference?" - the capability question.
const { gpu, cpu } = await getSystemInfo({
  detail: "capabilities",
  sections: ["cpu", "gpu"],
});
const backend =
  gpu!.find((g) => g.api.cuda) ?? // NVIDIA
  gpu!.find((g) => g.api.hip) ?? // AMD, via ROCm
  gpu!.find((g) => g.api.metal) ?? // Apple
  gpu!.find((g) => g.api.vulkan); // universal fallback

// ROCm's officially supported list is narrow; the gfx target is what decides.
const gfx = backend?.api.gfxArchitecture; // "gfx1100"
const hasAvx2 = cpu![0].features.includes("AVX2");
```

What each level gates, precisely:

| Field                                                   | Requires       |
| ------------------------------------------------------- | -------------- |
| `gpu[].api` probes: `vulkan*`, `cuda*`, `hip*`, `gfxArchitecture`, `opencl*` | `capabilities` |
| `cpu[].cores`, `usagePercent`, `temperatureC`           | `full`         |
| `memory.modules` (but **not** `slotsTotal`/`slotsUsed`) | `full`         |
| `storage.disks` (but **not** `volumes`)                 | `full`         |
| `network[].description`, `speedMbps`                    | `full`         |
| `gpu[].api.directxFeatureLevel`                         | `full`         |

Two things worth knowing:

- `scan.detail` is echoed back in the response, so a `null` from _"you didn't
  ask"_ is always distinguishable from a `null` from _"it failed"_ - the latter
  is the one explained in `scan.warnings`.
- Below `capabilities`, the `gpu[].api` booleans read `false` meaning **not
  probed**, not _not supported_. Check `scan.detail` before treating a `false`
  as a real answer.

## 🔐 Safe and unsafe scans

Scans run in **safe** mode by default: hardware capabilities only. Fields marked
_identifying_ - serial numbers, MAC addresses, IP addresses, SMBIOS UUID,
machine ID, hostname, username - come back `null`.

**Unsafe** mode returns them. Together they are a stable device fingerprint, so
the host application has to opt in explicitly; a frontend cannot enable it on
its own.

```rust
tauri::Builder::default()
    .plugin(
        tauri_plugin_hwinfo::Builder::new()
            .allow_unsafe_scan(true)
            .build(),
    )
```

```ts
const info = await getSystemInfo({ mode: "unsafe" });
```

Without that opt-in, an `"unsafe"` request rejects rather than silently
downgrading, so you always know which you got.

> Some identifiers need privileges the app may not have. On Linux, SMBIOS
> serials and the per-DIMM inventory are root-only; `scan.warnings` says so
> rather than leaving you guessing.

## 📦 Installation

```sh
cargo add tauri-plugin-hwinfo
npm i tauri-plugin-hwinfo
```

Register the plugin:

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_hwinfo::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

Add to `src-tauri/capabilities/default.json`:

```json
{
  "permissions": ["hwinfo:default"]
}
```

`hwinfo:default` allows every section. To narrow it, list only what you need:

```json
{
  "permissions": ["hwinfo:allow-get-cpu-info", "hwinfo:allow-get-gpu-info"]
}
```

## 📌 Frontend API

```ts
import {
  getSystemInfo,
  getCpuInfo,
  getGpuInfo,
  getMemoryInfo,
  getStorageInfo,
  getNetworkInfo,
  getDisplayInfo,
  getBatteryInfo,
  getBoardInfo,
  getOsInfo,
} from "tauri-plugin-hwinfo";

// Everything.
const info = await getSystemInfo();

// Just two sections, in one round trip.
const partial = await getSystemInfo({ sections: ["cpu", "gpu"] });

// One section, unwrapped.
const gpus = await getGpuInfo();
```

Every function takes the same options:

```ts
interface ScanOptions {
  mode?: "safe" | "unsafe"; // default "safe"   - how deep into identity
  detail?: "summary" | "capabilities" | "full"; // default "summary" - how hard to work
  sections?: Section[]; // getSystemInfo only   - which subsystems
}
```

The three are orthogonal, and between them cover most of what callers need to
trade off.

`getSystemInfo` is the only call that returns `scan` metadata - mode, duration,
and the warnings explaining any `null` fields. The per-section helpers return
their payload directly.

## 📜 Output shape

```jsonc
{
  "scan": {
    "version": 1,
    "mode": "safe",
    "detail": "full",
    "sections": ["cpu", "gpu", "memory", "..."],
    "durationMs": 412,
    "timestamp": 1785801392,
    "warnings": [],
  },
  "cpu": [
    {
      "manufacturer": "AuthenticAMD",
      "model": "AMD Ryzen 9 5900X 12-Core Processor",
      "architecture": "x86_64",
      "physicalCores": 12,
      "threads": 24,
      "baseFrequency": 3700,
      "maxFrequency": 4950,
      "currentFrequency": 4421,
      "socket": "AM4",
      "family": 25,
      "modelId": 33,
      "stepping": 0,
      "microcode": "0x0A201025",
      "cache": { "l1dKb": 384, "l1iKb": 384, "l2Kb": 6144, "l3Kb": 65536 },
      "features": ["AES-NI", "AMD-V", "AVX", "AVX2", "SHA", "..."],
      "virtualization": true,
      "hypervisor": null,
      "simultaneousMultithreading": true,
      "usagePercent": 8.4,
      "temperatureC": 42.5,
      "cores": [{ "id": "cpu0", "usagePercent": 12.1, "frequency": 4421 }],
      "serial": null,
    },
  ],
  "gpu": [
    {
      "manufacturer": "Advanced Micro Devices, Inc.",
      "model": "AMD Radeon RX 6950 XT",
      "kind": "discrete",
      "vendorId": 4098,
      "vendorIdHex": "0x1002",
      "deviceId": 29615,
      "deviceIdHex": "0x73AF",
      "vramMb": 16368,
      "sharedMemoryMb": 32619,
      "driverVersion": "32.0.12033.1030",
      "driverDate": "2025-04-08",
      "pciBus": "0000:0a:00.0",
      "api": {
        "vulkan": true,
        "vulkanVersion": "1.4.312",
        "vulkanDriver": "AMD proprietary driver (2.0.324)",
        "cuda": false,
        "directxFeatureLevel": "12_1",
        "metal": false,
        "opencl": false,
      },
      "uuid": null,
    },
  ],
  // memory, storage, network, display, battery, board, os …
}
```

Sections you did not ask for are `null`.

## 🦀 Rust API

```rust
use tauri_plugin_hwinfo::{DetailLevel, HwinfoExt, ScanMode, ScanOptions, Section};

let info = app.hwinfo().scan(ScanOptions {
    mode: ScanMode::Safe,
    detail: DetailLevel::Capabilities,
    sections: Some(vec![Section::Cpu, Section::Gpu]),
}).await?;
```

There is also a Tauri-free entry point for CLI tools and tests. It returns
everything except displays, which need a windowing runtime:

```rust
let info = tauri_plugin_hwinfo::scan_blocking(ScanOptions::default());
```

Try it against your own machine:

```bash
cargo run --example dump -- full unsafe
```

## ⚙️ Cargo features

| Feature   | Default | Effect                                                                                                                                                                                                         |
| --------- | ------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `vulkan`  | on      | Enumerates GPUs through the Vulkan loader, adding driver versions, device classes and true heap sizes on all platforms. Loaded at runtime - no SDK needed to build, and absent Vulkan just produces a warning. |
| `battery` | on      | Battery and power-supply reporting.                                                                                                                                                                            |
| `opencl`  | on      | Asks the OpenCL ICD loader whether any platform is actually registered. Loaded at runtime; nothing links against OpenCL at build time.                                                                          |

## 🖥️ Platform notes

- **Windows** - WMI is reached through COM directly, so no PowerShell is spawned
  and no console flashes. VRAM comes from DXGI rather than
  `Win32_VideoController::AdapterRAM`, which is a `uint32` and wraps for any
  adapter with 4 GiB or more.
- **Linux** - No helper binaries at all. SMBIOS is parsed directly out of
  `/sys/firmware/dmi/entries` rather than by shelling to `dmidecode`, device
  names come from the same `pci.ids` database `lspci` reads, virtualisation and
  container detection run off `/proc` and `/sys`, and HIP/ROCm comes from the
  `amdkfd` topology. The SMBIOS tables are mode `0400`, so DMI serials and the
  per-DIMM inventory still need root - and say so in `scan.warnings`.
- **macOS** - `sysctlbyname` for the CPU, Core Graphics for displays, `hw.model`
  for the machine identity: the sections that run at every detail level spawn
  nothing. `system_profiler` remains only for `full`-tier inventory that has no
  public API - memory modules, physical disks, the GPU list - and for the
  hardware UUID in unsafe mode. Apple silicon has no DIMM slots and no discrete
  GPU, so those fields are `null` because the hardware has no such thing.
- **Mobile** - Android and iOS get the portable subset (CPU, memory, storage,
  network, OS) with a warning naming what is missing, instead of the placeholder
  values v1 returned. Shipping to the iOS App Store additionally needs
  `sysinfo`'s `apple-app-store` feature, which is not wired up here yet.

`maxFrequency` deserves a note: no OS reliably publishes a turbo ceiling -
Windows and CPUID both hand back the base clock - so it reports the highest
figure either advertised or actually observed during the scan.

## ⬆️ Migrating from 0.x

The contract changed. The old flat, single-device shapes are gone.

| Before                                | Now                                                        |
| ------------------------------------- | ---------------------------------------------------------- |
| `getCpuInfo() → CpuInfo`              | `getCpuInfo() → Cpu[]` - one entry per physical package    |
| `getGpuInfo() → GpuInfo`              | `getGpuInfo() → Gpu[]` - every adapter, not just the first |
| `getRamInfo() → { sizeMb }`           | `getMemoryInfo() → Memory` with `totalMb` and `modules[]`  |
| `getOsInfo() → { name, version }`     | `getOsInfo() → Os` with build, edition, kernel and more    |
| `gpu.supportsCuda` / `supportsVulkan` | `gpu.api.cuda` / `gpu.api.vulkan`, with versions           |
| `hwinfo:allow-cpu-info`               | `hwinfo:allow-get-cpu-info`, or just `hwinfo:default`      |
| -                                     | `getSystemInfo()` for everything at once                   |

Fields are now `null` when unknown rather than `0` or `"Unknown"`, so check for
`null` where you previously checked for a zero value.

Note the default `detail: "summary"` - it is deliberately cheap, and omits
per-core, per-DIMM and per-disk detail along with the GPU API probes. Pass
`{ detail: "full" }` if you want everything the plugin can find.

## 📄 License

MIT
