use std::process::Command;

use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};
use sysinfo::System;

use crate::models::*;
use crate::Result;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

pub fn init<R: Runtime, C: DeserializeOwned>(
  app: &AppHandle<R>,
  _api: PluginApi<R, C>,
) -> Result<Hwinfo<R>> {
  Ok(Hwinfo(app.clone()))
}

/// Access to the hwinfo APIs.
pub struct Hwinfo<R: Runtime>(AppHandle<R>);

impl<R: Runtime> Hwinfo<R> {
  pub async fn cpu_info(&self) -> Result<CpuInfo> {
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("lscpu").output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);

        let mut manufacturer = String::new();
        let mut model = String::new();
        let mut max_freq = 0;
        let mut threads = 0;

        for line in stdout.lines() {
            if line.starts_with("Vendor ID:") {
                manufacturer = line.split(':').nth(1).unwrap_or("").trim().to_string();
            }
            if line.starts_with("Model name:") {
                model = line.split(':').nth(1).unwrap_or("").trim().to_string();
            }
            if line.starts_with("CPU MHz:") {
                max_freq = line
                    .split(':')
                    .nth(1)
                    .unwrap_or("")
                    .trim()
                    .parse::<f32>()
                    .unwrap_or(0.0)
                    .round() as u32;
            }
            if line.starts_with("CPU(s):") {
                threads = line
                    .split(':')
                    .nth(1)
                    .unwrap_or("")
                    .trim()
                    .parse()
                    .unwrap_or(0);
            }
        }

        return Ok(CpuInfo {
            manufacturer,
            model,
            max_frequency: max_freq,
            threads,
        })
    }

    #[cfg(target_os = "windows")]
    {
        let output = Command::new("powershell")
        .args([
            "-Command",
            "Get-CimInstance Win32_Processor | Select-Object Name, NumberOfLogicalProcessors, MaxClockSpeed, Manufacturer | ConvertTo-Json -Compress",
        ])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()?;

      let stdout = String::from_utf8_lossy(&output.stdout);
      let json: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

      return Ok(CpuInfo {
      manufacturer: json["Manufacturer"]
          .as_str()
          .unwrap_or("Unknown")
          .trim()
          .to_string(),
      model: json["Name"]
          .as_str()
          .unwrap_or("Unknown")
          .trim()
          .to_string(),
      max_frequency: json["MaxClockSpeed"]
          .as_u64()
          .unwrap_or(0) as u32,
      threads: json["NumberOfLogicalProcessors"]
          .as_u64()
          .unwrap_or(0) as usize,
      });
    }

    #[cfg(target_os = "macos")]
    {
        let output = Command::new("system_profiler")
            .arg("SPHardwareDataType")
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);

        let mut manufacturer = "Apple".to_string();
        let mut model = String::new();
        let mut max_freq = 0;
        let mut threads = 0;

        for line in stdout.lines() {
            if line.contains("Chip:") || line.contains("Processor Name:") {
                model = line.split(':').nth(1).unwrap_or("").trim().to_string();
            }
            if line.contains("Total Number of Cores:") || line.contains("Total Number of Threads:") {
                threads = line
                    .split(':')
                    .nth(1)
                    .unwrap_or("")
                    .trim()
                    .parse()
                    .unwrap_or(0);
            }
        }

        return Ok(CpuInfo {
            manufacturer,
            model,
            max_frequency: max_freq,
            threads,
        })
    }

    #[allow(unreachable_code)]
    Ok(CpuInfo {
        manufacturer: "Unknown".into(),
        model: "Unknown".into(),
        max_frequency: 0,
        threads: 0,
    })
}

  pub async fn ram_info(&self) -> Result<RamInfo> {
    let mut sys = System::new_all();
    sys.refresh_memory();
    let size_mb = sys.total_memory() / 1024 / 1024; // from B to KB to MB
    Ok(RamInfo { size_mb })
  }

  pub async fn gpu_info(&self) -> Result<GpuInfo> {

    #[cfg(target_os = "windows")]
    {
        use std::path::Path;
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory, DXGI_ADAPTER_DESC, IDXGIAdapter, IDXGIFactory};

        let factory: IDXGIFactory = unsafe { CreateDXGIFactory().unwrap() };
        let adapter: IDXGIAdapter = unsafe { factory.EnumAdapters(0).unwrap() };

        let mut desc = DXGI_ADAPTER_DESC::default();
        unsafe { adapter.GetDesc(&mut desc).unwrap() };

        let model = String::from_utf16_lossy(&desc.Description)
            .trim_end_matches(char::from(0))
            .to_string();

        let vram_mb = (desc.DedicatedVideoMemory / 1024 / 1024) as u64;

        let manufacturer = match desc.VendorId {
            0x10DE => "NVIDIA Corporation".to_string(),
            0x1002 => "Advanced Micro Devices, Inc.".to_string(),
            0x8086 => "Intel Corporation".to_string(),
            _ => "Unknown".to_string(),
        };

        let supports_cuda = manufacturer.contains("NVIDIA")
            && (model.contains("RTX") || model.contains("GTX"));
        let supports_vulkan = Path::new("C:\\Windows\\System32\\vulkan-1.dll").exists()
            || Path::new("C:\\Windows\\SysWOW64\\vulkan-1.dll").exists();

        return Ok(GpuInfo {
            manufacturer,
            model,
            vram_mb,
            supports_cuda,
            supports_vulkan,
        });
    }

    #[cfg(target_os = "linux")]
    {
        let mut manufacturer = "Unknown".to_string();
        let mut model = "Unknown".to_string();
        let mut vram_mb = 0;

        // Extract model and vendor using lspci
        if let Ok(output) = Command::new("lspci").arg("-mm").output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.to_lowercase().contains("vga compatible controller") {
                    let parts: Vec<&str> = line.split('"').collect();
                    if let Some(name) = parts.get(5) {
                        model = name.to_string();
                        if name.contains("AMD") || name.contains("Advanced Micro Devices") || name.contains("Radeon") {
                            manufacturer = "Advanced Micro Devices, Inc.".into();
                        } else if name.contains("NVIDIA") {
                            manufacturer = "NVIDIA Corporation".into();
                        } else if name.contains("Intel") {
                            manufacturer = "Intel Corporation".into();
                        }
                    }
                }
            }
        }

        // Detect AMD VRAM from sysfs
        if vram_mb == 0 && manufacturer.contains("AMD") {
            if let Ok(content) = std::fs::read_to_string("/sys/class/drm/card0/device/mem_info_vram_total") {
                if let Ok(bytes) = content.trim().parse::<u64>() {
                    vram_mb = bytes / 1024 / 1024;
                }
            }
        }

        // Detect NVIDIA VRAM from nvidia-smi
        if vram_mb == 0 && manufacturer.contains("NVIDIA") {
            if let Ok(output) = Command::new("nvidia-smi")
                .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
                .output()
            {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if let Some(line) = stdout.lines().next() {
                        vram_mb = line.trim().parse::<u64>().unwrap_or(0);
                    }
                }
            }
        }

        // Try Intel and general fallback via lshw
        if vram_mb == 0 {
            if let Ok(output) = Command::new("lshw").args(["-C", "display"]).output() {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    for line in stdout.lines() {
                        if line.to_lowercase().contains("size:") && line.contains("MB") {
                            if let Some(size) = line.split(':').nth(1) {
                                if let Some(num) = size.trim().split_whitespace().next() {
                                    vram_mb = num.parse::<u64>().unwrap_or(0);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Final fallback: glxinfo parsing (surprisingly reliable)
        if vram_mb == 0 {
            if let Ok(output) = Command::new("glxinfo").arg("-B").output() {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    for line in stdout.lines() {
                        if line.contains("Video memory:") {
                            if let Some(value) = line.split(':').nth(1) {
                                let mem = value.trim().replace("MB", "").trim().to_string();
                                vram_mb = mem.parse::<u64>().unwrap_or(0);
                                break;
                            }
                        }
                    }
                }
            }
        }

        // CUDA support: only if nvidia-smi succeeds
        let supports_cuda = Command::new("nvidia-smi")
            .arg("--query-gpu=name")
            .arg("--format=csv,noheader")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        // Vulkan support: ldconfig + ICD presence check
        let supports_vulkan = {
            let ldconfig_ok = Command::new("sh")
                .arg("-c")
                .arg("ldconfig -p | grep -q libvulkan.so")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);

            let icd_present = std::path::Path::new("/etc/vulkan/icd.d").exists()
                || std::path::Path::new("/usr/share/vulkan/icd.d").exists();

            ldconfig_ok && icd_present
        };

        return Ok(GpuInfo {
            manufacturer,
            model,
            vram_mb,
            supports_cuda,
            supports_vulkan,
        });
    }



    #[cfg(target_os = "macos")]
    {
      let output = Command::new("system_profiler")
        .arg("SPDisplaysDataType")
        .output()?;

      let stdout = String::from_utf8_lossy(&output.stdout);

      let mut manufacturer = "Apple Inc.".to_string();
      let mut model = String::new();
      let mut vram_mb = 0;

      for line in stdout.lines() {
        if line.contains("Chipset Model:") {
          model = line
            .split(':')
            .nth(1)
            .unwrap_or("")
            .replace("(Metal)", "")
            .trim()
            .to_string();
        }

        if line.trim_start().starts_with("VRAM") {
          if let Some((_, value)) = line.split_once(':') {
            let clean = value
              .trim()
              .replace("GB", "")
              .replace("MB", "")
              .trim()
              .to_string();

            if clean.contains('.') {
              vram_mb = (clean.parse::<f32>().unwrap_or(0.0) * 1024.0) as u64;
            } else {
              vram_mb = clean.parse::<u64>().unwrap_or(0);
            }
          }
        }
      }

      if vram_mb == 0 {
        let mut sys = System::new_all();
        sys.refresh_memory();
        vram_mb = sys.total_memory() / 1024 / 1024;
      }

      return Ok(GpuInfo {
        manufacturer,
        model,
        vram_mb,
        supports_cuda: false,
        supports_vulkan: false,
      });
    }

    #[allow(unreachable_code)]
    Ok(GpuInfo {
      manufacturer: "Unknown".into(),
      model: "Unknown".into(),
      vram_mb: 0,
      supports_cuda: false,
      supports_vulkan: false,
    })
  }

  pub async fn os_info(&self) -> Result<OsInfo> {
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    {
      let info = os_info::get();
      let name = info.os_type().to_string();
      let version = info.version().to_string();

      return Ok(OsInfo { name, version });
    }

    #[allow(unreachable_code)]
    {
      return Ok(OsInfo {
        name: "Unknown".into(),
        version: "Unknown".into(),
      });
    }
  }
}
