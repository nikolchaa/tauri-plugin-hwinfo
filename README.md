# 🧠 tauri-plugin-hwinfo

A cross-platform Tauri plugin to fetch detailed system hardware information from the user's device, including CPU, RAM, GPU, and OS metadata — all accessible through both Rust and JavaScript/TypeScript APIs.

> ⚠️ **Platform Support**: Desktop-only. Mobile support returns placeholder values.  
> ⚠️ **Testing**: Only Windows is tested and confirmed working so far.

---

## 🔧 Features

- ✅ CPU Info (manufacturer, model, threads, max frequency)
- ✅ RAM Info (total memory in MB)
- ✅ GPU Info (model, manufacturer, VRAM in MB)
- ✅ OS Info (OS name and version)
- ✅ Full Tauri v2 permissions support
- ✅ JS/TS bindings via `@tauri-apps/api/core::invoke`

---

## 📦 Installation

### From Crates.io (Rust)

```toml
[dependencies]
tauri-plugin-hwinfo = "0.1.0"
```

> 🔖 Replace with the latest version from [crates.io](https://crates.io/crates/tauri-plugin-hwinfo)

### From GitHub (pre-release testing)

```toml
[dependencies]
tauri-plugin-hwinfo = { git = "https://github.com/nikolchaa/tauri-plugin-hwinfo", tag = "0.1.0" }
```

---

## 🧰 Usage (Rust Backend)

```rust
use tauri_plugin_hwinfo::init;

fn main() {
    tauri::Builder::default()
        .plugin(init())
        .run(tauri::generate_context!())
        .expect("failed to run app");
}
```

⚠️ Add the following permissions to your `src-tauri/capabilities/default.json`

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

---

## 📜 Frontend API (JS/TS)

Install via NPM (once published), or link locally if using manually.

```ts
import {
  getCpuInfo,
  getRamInfo,
  getGpuInfo,
  getOsInfo,
} from "tauri-plugin-hwinfo";

async function showCpuInfo() {
  const cpu = await getCpuInfo();
  console.log(cpu.model);
}
```
