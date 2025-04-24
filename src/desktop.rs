use std::process::Command;

use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};
use sysinfo::System;

use crate::models::*;
use crate::Result;

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
        });
    }

    #[cfg(target_os = "windows")]
    {
        let output = Command::new("wmic")
            .args([
                "cpu",
                "get",
                "Name,NumberOfLogicalProcessors,MaxClockSpeed,Manufacturer",
            ])
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();

        if lines.len() >= 2 {
            let values = lines[1];
            let fields: Vec<&str> = values.split_whitespace().collect();

            let manufacturer = fields.get(0).unwrap_or(&"Unknown").to_string();
            let max_freq = fields.get(1).unwrap_or(&"0").parse().unwrap_or(0);

            // Name can be multiple words; slice between 2 and last (excluding thread count)
            let model_slice = &fields[2..fields.len().saturating_sub(1)];
            let model = model_slice.join(" ");

            let threads = fields.last().unwrap_or(&"0").parse().unwrap_or(0);

            return Ok(CpuInfo {
                manufacturer,
                model,
                max_frequency: max_freq,
                threads,
            });
        }

        return Ok(CpuInfo {
            manufacturer: "Unknown".into(),
            model: "Unknown".into(),
            max_frequency: 0,
            threads: 0,
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
        });
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
      let mut manufacturer = String::from("Unknown");
      let mut model = String::from("Unknown");
      let mut vram_mb = 0;

      // Get GPU model from lspci
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
            } else {
              break;
            }
          }
        }
      }

      // Get VRAM size from glxinfo
      if let Ok(output) = Command::new("glxinfo").arg("-B").output() {
          let stdout = String::from_utf8_lossy(&output.stdout);
          for line in stdout.lines() {
              if line.contains("Video memory") || line.contains("Video memory:") {
                  if let Some(num_part) = line.split_whitespace().last() {
                      vram_mb = num_part.parse::<u64>().unwrap_or(0);
                  }
              }
          }
      }

      let supports_cuda = Command::new("which")
        .arg("nvidia-smi")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

      let supports_vulkan = Command::new("which")
        .arg("vulkaninfo")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

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
        sys.total_memory() / 1024 / 1024; // from B to KB to MB
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
