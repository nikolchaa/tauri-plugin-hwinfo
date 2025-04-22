use tauri::{
  plugin::{Builder, TauriPlugin},
  Manager, Runtime,
};

pub use models::*;

#[cfg(desktop)]
pub mod desktop;
#[cfg(mobile)]
mod mobile;

mod commands;
mod error;
mod models;

pub use error::{Error, Result};

#[cfg(desktop)]
use desktop::Hwinfo;
#[cfg(mobile)]
use mobile::Hwinfo;

/// Extensions to [`tauri::App`], [`tauri::AppHandle`] and [`tauri::Window`] to access the hwinfo APIs.
pub trait HwinfoExt<R: Runtime> {
  fn hwinfo(&self) -> &Hwinfo<R>;
}

impl<R: Runtime, T: Manager<R>> crate::HwinfoExt<R> for T {
  fn hwinfo(&self) -> &Hwinfo<R> {
    self.state::<Hwinfo<R>>().inner()
  }
}

/// Initializes the plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
  Builder::new("hwinfo")
    .invoke_handler(tauri::generate_handler![
      commands::get_cpu_info,
      commands::get_ram_info,
      commands::get_gpu_info,
      commands::get_os_info
    ])
    .setup(|app, api| {
      #[cfg(mobile)]
      let hwinfo = mobile::init(app, api)?;
      #[cfg(desktop)]
      let hwinfo = desktop::init(app, api)?;
      app.manage(hwinfo);
      Ok(())
    })
    .build()
}
