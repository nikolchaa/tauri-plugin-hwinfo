use tauri::{AppHandle, command, Runtime};

use crate::Result;
use crate::HwinfoExt;
use crate::models::*;

#[command]
pub(crate) async fn get_cpu_info<R: Runtime>(app: AppHandle<R>) -> Result<CpuInfo> {
    app.hwinfo().cpu_info().await
}

#[command]
pub(crate) async fn get_ram_info<R: Runtime>(app: AppHandle<R>) -> Result<RamInfo> {
    app.hwinfo().ram_info().await
}

#[command]
pub(crate) async fn get_gpu_info<R: Runtime>(app: AppHandle<R>) -> Result<GpuInfo> {
    app.hwinfo().gpu_info().await
}

#[command]
pub(crate) async fn get_os_info<R: Runtime>(app: AppHandle<R>) -> Result<OsInfo> {
    app.hwinfo().os_info().await
}
