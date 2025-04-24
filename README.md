# 🧠 tauri-plugin-hwinfo

![License](https://img.shields.io/github/license/nikolchaa/tauri-plugin-hwinfo?color=blue)
![Crates.io](https://img.shields.io/crates/v/tauri-plugin-hwinfo?color=blue)
![Crates.io Downloads](https://img.shields.io/crates/d/tauri-plugin-hwinfo?color=blue)
![npm](https://img.shields.io/npm/v/tauri-plugin-hwinfo?color=blue)
![npm Downloads](https://img.shields.io/npm/dt/tauri-plugin-hwinfo?color=blue)

A cross-platform Tauri plugin to fetch detailed system hardware information from the user's device, including CPU, RAM, GPU, and OS metadata — all accessible through both Rust and JavaScript/TypeScript APIs.

> ⚠️ **Platform Support**: Desktop-only. Mobile returns placeholder values.
>
> ⚠️ **Testing**: Only Windows is tested and confirmed working so far.

## 🔧 Features

- ✅ CPU Info (manufacturer, model, threads, max frequency)
- ✅ RAM Info (total memory in MB)
- ✅ GPU Info (model, manufacturer, VRAM in MB, CUDA support, Vulkan support)
- ✅ OS Info (OS name and version)
- ✅ Full Tauri v2 permissions support
- ✅ JS/TS bindings via `@tauri-apps/api/core::invoke`

## 📦 Installation

### From Crates.io (Rust)

```sh
cargo add tauri-plugin-hwinfo
```

### From GitHub (bleeding edge)

```toml
[dependencies]
tauri-plugin-hwinfo = { git = "https://github.com/nikolchaa/tauri-plugin-hwinfo" }
```

## 🛠️ Usage (Rust Backend)

### Option 1: Auto-bind commands via invoke_handler (if calling from JS/TS manually)

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_hwinfo::init())
        .invoke_handler(tauri::generate_handler![
            tauri_plugin_hwinfo::get_cpu_info,
            tauri_plugin_hwinfo::get_gpu_info,
            tauri_plugin_hwinfo::get_ram_info,
            tauri_plugin_hwinfo::get_os_info
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

### Option 2: Plugin-only setup (if using only the frontend API)

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_hwinfo::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

> Use Option 1 if you want to manually expose commands to JS via `invoke()`.
>
> Use Option 2 if you're only using `tauri-plugin-hwinfo`'s built-in TS API.

Add this to your `src-tauri/capabilities/default.json`:

```json
{
  "permissions": [
    "hwinfo:allow-cpu-info",
    "hwinfo:allow-gpu-info",
    "hwinfo:allow-ram-info",
    "hwinfo:allow-os-info"
  ]
}
```

## 📜 Output Format

```json
// CPU Info:
{
  "manufacturer": "AuthenticAMD",
  "model": "AMD Ryzen 9 5900X 12-Core Processor",
  "maxFrequency": 3701,
  "threads": 24
}

// RAM Info:
{
  "sizeMb": 32686
}

// GPU Info:
{
  "manufacturer": "Advanced Micro Devices, Inc.",
  "model": "AMD Radeon RX 6950 XT",
  "vramMb": 16311,
  "supportsCuda": false,
  "supportsVulkan": true
}

// OS Info:
{
  "name": "Windows",
  "version": "10.0.26100"
}
```

## 📌 Frontend API (JS/TS)

Install:

```sh
npm i tauri-plugin-hwinfo
```

Usage:

```ts
import {
  getCpuInfo,
  getRamInfo,
  getGpuInfo,
  getOsInfo,
} from "tauri-plugin-hwinfo";

async function logSystemInfo() {
  const cpu = await getCpuInfo();
  const ram = await getRamInfo();
  const gpu = await getGpuInfo();
  const os = await getOsInfo();

  console.log("CPU Info:", cpu);
  console.log("RAM Info:", ram);
  console.log("GPU Info:", gpu);
  console.log("OS Info:", os);
}
```
