use serde::{ser::Serializer, Serialize};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// An `unsafe` scan was requested but the host application did not opt in
    /// via `Builder::allow_unsafe_scan(true)`.
    #[error("unsafe scan mode is disabled; enable it with tauri_plugin_hwinfo::Builder::new().allow_unsafe_scan(true)")]
    UnsafeScanDisabled,

    /// The scan task panicked or the runtime shut down before it finished.
    #[error("hardware scan failed to complete: {0}")]
    ScanFailed(String),

    #[error(transparent)]
    Tauri(#[from] tauri::Error),

    #[cfg(mobile)]
    #[error(transparent)]
    PluginInvoke(#[from] tauri::plugin::mobile::PluginInvokeError),
}

impl Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}
