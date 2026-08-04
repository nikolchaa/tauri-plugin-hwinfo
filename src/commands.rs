//! Commands exposed to the frontend.
//!
//! `get_system_info` returns everything (or the sections asked for) together
//! with the scan metadata. The per-section commands return their payload
//! directly, which is what most callers want; the warnings that explain any
//! `null` fields are only carried by `get_system_info`.

use tauri::{command, AppHandle, Runtime};

use crate::models::*;
use crate::{Error, HwinfoExt, Result};

#[command]
pub(crate) async fn get_system_info<R: Runtime>(
    app: AppHandle<R>,
    options: Option<ScanOptions>,
) -> Result<SystemInfo> {
    app.hwinfo().scan(options.unwrap_or_default()).await
}

/// Run a scan restricted to one section and pull that section out.
///
/// The section is always present because we just asked for it; the error arm
/// only guards against a future refactor of the dispatch table.
macro_rules! section_command {
    ($name:ident, $section:expr, $field:ident, $ty:ty) => {
        #[command]
        pub(crate) async fn $name<R: Runtime>(
            app: AppHandle<R>,
            options: Option<ScanOptions>,
        ) -> Result<$ty> {
            let mut options = options.unwrap_or_default();
            options.sections = Some(vec![$section]);
            let info = app.hwinfo().scan(options).await?;
            info.$field.ok_or_else(|| {
                Error::ScanFailed(format!("the {:?} section was not collected", $section))
            })
        }
    };
}

section_command!(get_cpu_info, Section::Cpu, cpu, Vec<Cpu>);
section_command!(get_gpu_info, Section::Gpu, gpu, Vec<Gpu>);
section_command!(get_memory_info, Section::Memory, memory, Memory);
section_command!(get_storage_info, Section::Storage, storage, Storage);
section_command!(
    get_network_info,
    Section::Network,
    network,
    Vec<NetworkInterface>
);
section_command!(get_display_info, Section::Display, display, Vec<Display>);
section_command!(get_battery_info, Section::Battery, battery, Vec<Battery>);
section_command!(get_board_info, Section::Board, board, Board);
section_command!(get_os_info, Section::Os, os, Os);
