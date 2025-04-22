use tauri::{plugin::PluginApi, AppHandle, Runtime};
use crate::models::*;
use crate::Result;

pub fn init<R: Runtime, C>(_app: &AppHandle<R>, _api: PluginApi<R, C>) -> Result<Hwinfo<R>> {
  Ok(Hwinfo)
}

pub struct Hwinfo;

impl Hwinfo {
  pub async fn cpu_info(&self) -> Result<CpuInfo> {
    Ok(CpuInfo {
      manufacturer: "Unavailable".into(),
      model: "Unavailable".into(),
      max_frequency: "Unavailable".into(),
      threads: 0,
    })
  }

  pub async fn ram_info(&self) -> Result<RamInfo> {
    Ok(RamInfo {
      size_mb: 0,
    })
  }

  pub async fn gpu_info(&self) -> Result<GpuInfo> {
    Ok(GpuInfo {
      manufacturer: "Unavailable".into(),
      model: "Unavailable".into(),
      vram_mb: 0,
    })
  }

  pub async fn os_info(&self) -> Result<OsInfo> {
    Ok(OsInfo {
      name: "Unavailable".into(),
      version: "Unavailable".into(),
    })
  }
}
