//! Deep hardware and system inspection for Tauri applications.
//!
//! ```ignore
//! tauri::Builder::default()
//!     .plugin(tauri_plugin_hwinfo::init())
//!     .run(tauri::generate_context!())
//!     .expect("error while running tauri application");
//! ```
//!
//! Scans default to [`ScanMode::Safe`], which reports hardware capabilities but
//! no per-unit identifiers. To allow [`ScanMode::Unsafe`] - serial numbers, MAC
//! addresses, UUIDs, hostname - the host application has to opt in:
//!
//! ```ignore
//! tauri::Builder::default()
//!     .plugin(tauri_plugin_hwinfo::Builder::new().allow_unsafe_scan(true).build())
//!     .run(tauri::generate_context!())
//!     .expect("error while running tauri application");
//! ```

use tauri::{
    plugin::{Builder as PluginBuilder, TauriPlugin},
    AppHandle, Manager, Runtime,
};

mod commands;
mod error;
mod models;
mod scan;
mod sys;

pub use error::{Error, Result};
pub use models::*;

use scan::{Ctx, MonitorHint};

/// Configures the plugin before it is registered.
#[derive(Debug, Clone, Default)]
pub struct Builder {
    allow_unsafe_scan: bool,
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Permit [`ScanMode::Unsafe`] scans.
    ///
    /// Off by default. While off, any request for unsafe mode fails with
    /// [`Error::UnsafeScanDisabled`] rather than silently downgrading, so a
    /// frontend cannot quietly harvest device identifiers.
    pub fn allow_unsafe_scan(mut self, allow: bool) -> Self {
        self.allow_unsafe_scan = allow;
        self
    }

    pub fn build<R: Runtime>(self) -> TauriPlugin<R> {
        PluginBuilder::new("hwinfo")
            .invoke_handler(tauri::generate_handler![
                commands::get_system_info,
                commands::get_cpu_info,
                commands::get_gpu_info,
                commands::get_memory_info,
                commands::get_storage_info,
                commands::get_network_info,
                commands::get_display_info,
                commands::get_battery_info,
                commands::get_board_info,
                commands::get_os_info,
            ])
            .setup(move |app, _api| {
                app.manage(Hwinfo {
                    app: app.clone(),
                    allow_unsafe_scan: self.allow_unsafe_scan,
                });
                Ok(())
            })
            .build()
    }
}

/// Initialise the plugin with default settings - safe mode only.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new().build()
}

/// Run a scan without a Tauri application.
///
/// Useful for CLI tools, build scripts and tests. The display section comes
/// back empty because monitor enumeration needs a windowing runtime; every
/// other section is identical to what the plugin returns.
///
/// This blocks the calling thread for the duration of the scan, which includes
/// a short sampling interval when the CPU section is requested.
///
/// The [`Builder::allow_unsafe_scan`] gate does not apply here - it exists to
/// stop a *frontend* from requesting identifiers, and Rust callers are already
/// on the trusted side of that boundary.
pub fn scan_blocking(options: ScanOptions) -> SystemInfo {
    let sections = options
        .sections
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| Section::ALL.to_vec());
    scan::run(Ctx::new(options.mode, options.detail), &sections)
}

/// Access to the hwinfo APIs from Rust.
pub struct Hwinfo<R: Runtime> {
    app: AppHandle<R>,
    allow_unsafe_scan: bool,
}

/// Extension trait giving [`tauri::App`], [`tauri::AppHandle`] and
/// [`tauri::Window`] access to the hwinfo APIs.
pub trait HwinfoExt<R: Runtime> {
    fn hwinfo(&self) -> &Hwinfo<R>;
}

impl<R: Runtime, T: Manager<R>> HwinfoExt<R> for T {
    fn hwinfo(&self) -> &Hwinfo<R> {
        self.state::<Hwinfo<R>>().inner()
    }
}

impl<R: Runtime> Hwinfo<R> {
    /// Scan the sections named in `options`, or all of them when none are
    /// named.
    pub async fn scan(&self, options: ScanOptions) -> Result<SystemInfo> {
        if options.mode.is_unsafe() && !self.allow_unsafe_scan {
            return Err(Error::UnsafeScanDisabled);
        }

        let sections = options
            .sections
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| Section::ALL.to_vec());

        // Probing hardware means blocking syscalls, COM round-trips and reads
        // from `/sys`; none of that belongs on an async worker.
        let monitors = self.monitors(&sections);
        let (mode, detail) = (options.mode, options.detail);

        tauri::async_runtime::spawn_blocking(move || {
            scan::run(Ctx::new(mode, detail).with_monitors(monitors), &sections)
        })
        .await
        .map_err(|e| Error::ScanFailed(e.to_string()))
    }

    /// Ask the runtime for the monitor list, which only it can answer.
    fn monitors(&self, sections: &[Section]) -> Vec<MonitorHint> {
        if !sections.contains(&Section::Display) {
            return Vec::new();
        }

        #[cfg(desktop)]
        {
            let primary = self.app.primary_monitor().ok().flatten();
            self.app
                .available_monitors()
                .unwrap_or_default()
                .into_iter()
                .map(|m| {
                    let is_primary = primary.as_ref().is_some_and(|p| {
                        p.name() == m.name() && p.position() == m.position()
                    });
                    MonitorHint {
                        name: m.name().cloned(),
                        width: m.size().width,
                        height: m.size().height,
                        position_x: m.position().x,
                        position_y: m.position().y,
                        scale_factor: m.scale_factor(),
                        is_primary,
                    }
                })
                .collect()
        }

        #[cfg(not(desktop))]
        {
            Vec::new()
        }
    }
}
