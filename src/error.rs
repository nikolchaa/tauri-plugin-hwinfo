use serde::{ser::Serializer, Serialize};

/// A [`std::result::Result`] with this crate's [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// Something that stopped a scan from running at all.
///
/// A probe that merely *failed* does not produce an error — it leaves its
/// fields `null` and records the reason in
/// [`ScanMeta::warnings`](crate::ScanMeta::warnings), so a partial answer is
/// always preferred to none.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A filesystem or process error while reading a platform data source.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// An `unsafe` scan was requested but the host application did not opt in
    /// via `Builder::allow_unsafe_scan(true)`.
    #[error("unsafe scan mode is disabled; enable it with tauri_plugin_hwinfo::Builder::new().allow_unsafe_scan(true)")]
    UnsafeScanDisabled,

    /// The scan task panicked or the runtime shut down before it finished.
    #[error("hardware scan failed to complete: {0}")]
    ScanFailed(String),

    /// An error from the Tauri runtime itself.
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
